using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using Jellyfin.Data.Enums;
using Jellyfin.Plugin.MediaFlick.Models;
using MediaBrowser.Controller.Entities;
using MediaBrowser.Controller.Library;
using MediaBrowser.Model.Querying;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// The TMDB movie collections present in the local Jellyfin library.
///
/// TMDB has no "list collections" endpoint, so the collection set is derived:
/// every library movie's TMDB id is mapped to its collection through Seerr
/// (which caches TMDB), and identical mappings group into one entry. The
/// library itself stays the authority on membership; a mapping only records
/// where a movie belongs, never that it exists.
///
/// Mappings persist to the plugin data folder, so a server restart does not
/// throw away days of resolved identity and re-ask Seerr for every movie.
/// </summary>
public sealed class CollectionsService
{
    private const int DocumentVersion = 1;

    /// <summary>Mappings older than this are resolved again on later requests.</summary>
    private static readonly TimeSpan MappingLifetime = TimeSpan.FromDays(7);

    /// <summary>
    /// Upstream resolves are bounded per request so a cold or expired cache
    /// converges over several loads instead of stalling one response behind
    /// every movie in the library.
    /// </summary>
    internal const int MaxResolvesPerRequest = 100;

    /// <summary>Seerr resolves within one request run concurrently up to this.</summary>
    internal const int ResolveParallelism = 8;

    internal sealed record Mapping(
        int CollectionId,
        string Name,
        string? PosterPath,
        string? BackdropPath,
        DateTimeOffset CachedAt);

    private sealed class CacheDocument
    {
        public int Version { get; set; } = DocumentVersion;

        public Dictionary<string, CachedMapping> Mappings { get; set; } =
            new(StringComparer.Ordinal);
    }

    internal sealed record CachedMapping(
        int CollectionId,
        string Name,
        string? PosterPath,
        string? BackdropPath,
        long CachedAt);

    private readonly SeerrGateway _seerr;
    private readonly ILibraryManager _library;
    private readonly string? _cachePath;
    private readonly object _gate = new();
    private Dictionary<int, Mapping> _mappings;

    public CollectionsService(SeerrGateway seerr, ILibraryManager library, string? cachePath = null)
    {
        _seerr = seerr;
        _library = library;
        _cachePath = cachePath;
        _mappings = LoadMappings(cachePath);
    }

    public async Task<JsonNode> SummaryAsync(Guid jellyfinUserId, CancellationToken cancellationToken)
    {
        var movieIds = LibraryMovieTmdbIds();
        var (mappings, pending) = await EnsureMappingsAsync(
            jellyfinUserId,
            movieIds,
            MaxResolvesPerRequest,
            cancellationToken).ConfigureAwait(false);
        return GroupCollections(movieIds, mappings, pending);
    }

    /// <summary>The single collection one library-identified movie belongs to.</summary>
    public async Task<JsonNode> ForMovieAsync(
        Guid jellyfinUserId,
        int tmdbId,
        CancellationToken cancellationToken)
    {
        var (mappings, _) = await EnsureMappingsAsync(
            jellyfinUserId,
            new[] { tmdbId },
            1,
            cancellationToken).ConfigureAwait(false);
        mappings.TryGetValue(tmdbId, out var mapping);
        return MovieCollectionShape(tmdbId, mapping?.CollectionId, mapping?.Name);
    }

    /// <summary>
    /// One collection's live detail with its parts. Parts keep the shared
    /// search result shape; Desktop joins local ownership onto them itself.
    /// </summary>
    public async Task<JsonNode> DetailAsync(
        Guid jellyfinUserId,
        int collectionId,
        CancellationToken cancellationToken)
        => await _seerr.CollectionAsync(jellyfinUserId, collectionId, cancellationToken)
            .ConfigureAwait(false);

    /// <summary>Current movie TMDB ids from the authoritative server library.</summary>
    internal IReadOnlyList<int> LibraryMovieTmdbIds()
    {
        var query = new InternalItemsQuery
        {
            IncludeItemTypes = new[] { BaseItemKind.Movie },
            Recursive = true,
            DtoOptions = new MediaBrowser.Controller.Dto.DtoOptions
            {
                EnableImages = false
            }
        };
        return _library.GetItemList(query)
            .Select(item => ParseTmdbId(item))
            .OfType<int>()
            .Distinct()
            .Order()
            .ToArray();

        static int? ParseTmdbId(BaseItem item)
        {
            foreach (var key in (string[]) ["Tmdb", "tmdb"])
            {
                if (item.ProviderIds?.TryGetValue(key, out var value) is true
                    && int.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out var id)
                    && id > 0)
                {
                    return id;
                }
            }

            return null;
        }
    }

    /// <summary>
    /// Fills missing and stale mappings through Seerr — concurrently, bounded
    /// by [`ResolveParallelism`] — prunes mappings for movies that left the
    /// library, and reports how many resolves did not fit in this request's
    /// budget. A batch of fresh mappings is persisted before the answer.
    /// </summary>
    internal async Task<(Dictionary<int, Mapping> Mappings, int Pending)> EnsureMappingsAsync(
        Guid jellyfinUserId,
        IReadOnlyList<int> movieIds,
        int resolveBudget,
        CancellationToken cancellationToken)
    {
        var now = DateTimeOffset.UtcNow;
        HashSet<int> presentIds;
        Dictionary<int, Mapping> current;
        lock (_gate)
        {
            // Movies that left the library lose their mapping; the summary is
            // derived from current ids alone, so stale rows would only age.
            presentIds = new HashSet<int>(movieIds);
            _mappings = _mappings
                .Where(entry => presentIds.Contains(entry.Key))
                .ToDictionary(entry => entry.Key, entry => entry.Value);
            current = new Dictionary<int, Mapping>(_mappings);
        }

        var wanted = movieIds
            .Where(id => !current.TryGetValue(id, out var mapping)
                || now - mapping.CachedAt >= MappingLifetime)
            .Order()
            .Take(Math.Max(0, resolveBudget))
            .ToArray();
        if (wanted.Length > 0)
        {
            await ResolveMappingsAsync(jellyfinUserId, wanted, now, cancellationToken)
                .ConfigureAwait(false);
            PersistMappings();
        }

        Dictionary<int, Mapping> snapshot;
        lock (_gate)
        {
            snapshot = new Dictionary<int, Mapping>(_mappings);
        }

        var pending = movieIds.Count(id =>
            !snapshot.TryGetValue(id, out var mapping) || now - mapping.CachedAt >= MappingLifetime);
        return (snapshot, pending);
    }

    private async Task ResolveMappingsAsync(
        Guid jellyfinUserId,
        int[] tmdbIds,
        DateTimeOffset cachedAt,
        CancellationToken cancellationToken)
    {
        using var throttler = new SemaphoreSlim(Math.Max(1, ResolveParallelism));
        var resolves = tmdbIds.Select(async tmdbId =>
        {
            await throttler.WaitAsync(cancellationToken).ConfigureAwait(false);
            try
            {
                var answer = await _seerr.MovieCollectionAsync(jellyfinUserId, tmdbId, cancellationToken)
                    .ConfigureAwait(false);
                StoreMapping(tmdbId, answer["collection"] as JsonObject, cachedAt);
            }
            catch (GatewayException exception) when (
                exception.StatusCode == StatusCodes.Status404NotFound)
            {
                // A movie TMDB no longer knows (or an id Seerr never cached)
                // simply has no collection; remember that so it stops costing
                // a resolve per request.
                StoreMapping(tmdbId, null, cachedAt);
            }
            finally
            {
                throttler.Release();
            }
        });
        await Task.WhenAll(resolves).ConfigureAwait(false);
    }

    private void StoreMapping(int tmdbId, JsonObject? collection, DateTimeOffset cachedAt)
    {
        Mapping? mapping;
        if (collection is null)
        {
            // A movie TMDB no longer knows has no collection; remember that so
            // it stops costing a resolve per request.
            mapping = new Mapping(0, string.Empty, null, null, cachedAt);
        }
        else
        {
            var id = IntValue(collection, "id");
            if (id is null || id <= 0)
            {
                return;
            }

            mapping = new Mapping(
                id.Value,
                StringValue(collection, "name") ?? string.Empty,
                StringValue(collection, "posterPath"),
                StringValue(collection, "backdropPath"),
                cachedAt);
        }

        lock (_gate)
        {
            _mappings[tmdbId] = mapping;
        }
    }

    /// <summary>Parses a persisted mapping document. Any doubt returns empty.</summary>
    internal static Dictionary<int, Mapping> ReadDocument(string json)
    {
        try
        {
            var document = JsonSerializer.Deserialize<CacheDocument>(json, CompanionJson.CamelCase);
            if (document is not { Version: DocumentVersion })
            {
                return new Dictionary<int, Mapping>();
            }

            var result = new Dictionary<int, Mapping>();
            foreach (var entry in document.Mappings)
            {
                if (!int.TryParse(entry.Key, NumberStyles.Integer, CultureInfo.InvariantCulture, out var tmdbId)
                    || tmdbId <= 0
                    || entry.Value is null)
                {
                    continue;
                }

                var cached = entry.Value;
                result[tmdbId] = new Mapping(
                    cached.CollectionId,
                    cached.Name ?? string.Empty,
                    cached.PosterPath,
                    cached.BackdropPath,
                    DateTimeOffset.FromUnixTimeMilliseconds(cached.CachedAt));
            }

            return result;
        }
        catch (Exception exception) when (
            exception is JsonException or IOException or UnauthorizedAccessException
                or FormatException)
        {
            // A damaged cache must never block the feature: it rebuilds.
            return new Dictionary<int, Mapping>();
        }
    }

    internal static string WriteDocument(IReadOnlyDictionary<int, Mapping> mappings)
    {
        var document = new CacheDocument
        {
            Mappings = mappings.ToDictionary(
                entry => entry.Key.ToString(CultureInfo.InvariantCulture),
                entry => new CachedMapping(
                    entry.Value.CollectionId,
                    entry.Value.Name,
                    entry.Value.PosterPath,
                    entry.Value.BackdropPath,
                    entry.Value.CachedAt.ToUnixTimeMilliseconds()))
        };
        return JsonSerializer.Serialize(document, CompanionJson.CamelCase);
    }

    private static Dictionary<int, Mapping> LoadMappings(string? path)
    {
        try
        {
            return path is null || !File.Exists(path)
                ? new Dictionary<int, Mapping>()
                : ReadDocument(File.ReadAllText(path));
        }
        catch (IOException)
        {
            return new Dictionary<int, Mapping>();
        }
    }

    private void PersistMappings()
    {
        if (_cachePath is null)
        {
            return;
        }

        Dictionary<int, Mapping> snapshot;
        lock (_gate)
        {
            snapshot = new Dictionary<int, Mapping>(_mappings);
        }

        try
        {
            var directory = Path.GetDirectoryName(_cachePath);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory);
            }

            var temporary = _cachePath + ".tmp";
            File.WriteAllText(temporary, WriteDocument(snapshot));
            File.Move(temporary, _cachePath, true);
        }
        catch (IOException)
        {
            // The in-memory cache remains usable; persistence failure must
            // never make an optional derived view fail.
        }
        catch (UnauthorizedAccessException)
        {
            // Same graceful degradation for a read-only plugin data volume.
        }
    }

    /// <summary>
    /// Groups movie-to-collection mappings into the summary contract. Pure so
    /// tests can pin ordering, deduplication, and counts without a library.
    /// </summary>
    internal static JsonNode GroupCollections(
        IReadOnlyList<int> movieIds,
        IReadOnlyDictionary<int, Mapping> mappings,
        int pending)
    {
        var collections = mappings.Values
            .Where(mapping => mapping.CollectionId > 0)
            .GroupBy(mapping => mapping.CollectionId)
            .Select(group =>
            {
                var first = group.First();
                return (JsonObject)new JsonObject
                {
                    ["id"] = first.CollectionId,
                    ["name"] = first.Name,
                    ["posterPath"] = first.PosterPath,
                    ["backdropPath"] = first.BackdropPath,
                    ["movieCount"] = group.Count()
                };
            })
            .OrderBy(collection => SortName(StringValue(collection, "name") ?? string.Empty), StringComparer.OrdinalIgnoreCase)
            .ToArray();
        return new JsonObject
        {
            ["collections"] = new JsonArray(collections),
            ["libraryMovies"] = movieIds.Count,
            ["mappedMovies"] = movieIds.Count(movieId =>
                mappings.TryGetValue(movieId, out var mapping) && mapping.CollectionId > 0),
            ["pendingMovies"] = pending
        };
    }

    internal static JsonNode MovieCollectionShape(int tmdbId, int? collectionId, string? name)
        => new JsonObject
        {
            ["tmdbId"] = tmdbId,
            ["collection"] = collectionId is { } id and > 0
                ? new JsonObject { ["id"] = id, ["name"] = name }
                : null
        };

    /// <summary>TMDB names collections with English articles; file them under the real title.</summary>
    internal static string SortName(string name)
    {
        foreach (var article in (string[]) ["The ", "An ", "A "])
        {
            if (name.StartsWith(article, StringComparison.OrdinalIgnoreCase))
            {
                return name[article.Length..].TrimStart();
            }
        }

        return name;
    }

    private static int? IntValue(JsonObject obj, string name)
        => obj[name] is JsonValue value && value.TryGetValue<int>(out var parsed) ? parsed : null;

    private static string? StringValue(JsonObject obj, string name)
        => obj[name] is JsonValue value && value.TryGetValue<string>(out var parsed) ? parsed : null;
}
