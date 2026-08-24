using System.Globalization;
using System.Net;
using System.Text.Json.Nodes;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal enum CuratedMediaKind
{
    Movie,
    Series
}

/// <summary>A TMDB identity whose namespace is explicit.</summary>
internal sealed record CuratedItem(CuratedMediaKind Kind, int TmdbId)
{
    internal string SeerrMediaType => Kind == CuratedMediaKind.Movie ? "movie" : "tv";
}

/// <summary>
/// Resolves one curated definition into its ordered TMDB identities.
///
/// Manual definitions carry movie ids inline. MDBList definitions can contain
/// movies and shows; the media kind remains part of the identity because TMDB
/// can assign the same numeric id in both namespaces.
/// </summary>
public sealed class CuratedCollectionResolver
{
    internal static readonly TimeSpan CacheLifetime = TimeSpan.FromHours(6);

    private readonly IMdbListTransport _transport;
    private readonly IRatingSecretStore _secrets;
    private readonly object _gate = new();
    private readonly Dictionary<string, (DateTimeOffset At, IReadOnlyList<CuratedItem> Items)> _cache =
        new(StringComparer.Ordinal);

    internal CuratedCollectionResolver(IMdbListTransport transport, IRatingSecretStore secrets)
    {
        _transport = transport;
        _secrets = secrets;
    }

    /// <summary>One curated definition's members in curation order.</summary>
    internal async Task<IReadOnlyList<CuratedItem>> ResolveAsync(
        string tmdbIds,
        string mdbListSource,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(mdbListSource))
        {
            return ParseTmdbIds(tmdbIds)
                .Select(static id => new CuratedItem(CuratedMediaKind.Movie, id))
                .ToArray();
        }

        if (!TryParseSource(mdbListSource, out var resource, out var cacheKey))
        {
            throw new GatewayException(
                StatusCodes.Status400BadRequest,
                "the MDBList source must look like 'username/listname', 'user/username/listname', or 'official/slug'");
        }

        var now = DateTimeOffset.UtcNow;
        lock (_gate)
        {
            if (_cache.TryGetValue(cacheKey, out var cached)
                && now - cached.At < CacheLifetime)
            {
                return cached.Items;
            }
        }

        var apiKey = _secrets.Get("mdblist");
        if (string.IsNullOrWhiteSpace(apiKey))
        {
            throw new GatewayException(
                StatusCodes.Status409Conflict,
                "an MDBList API key is required to resolve this curated collection; add one on the MediaFlick dashboard");
        }

        var response = await _transport.ListItemsAsync(apiKey, resource, cancellationToken)
            .ConfigureAwait(false);
        if (response.StatusCode == HttpStatusCode.Unauthorized
            || response.StatusCode == HttpStatusCode.Forbidden)
        {
            throw new GatewayException(
                StatusCodes.Status502BadGateway,
                "MDBList rejected the stored API key; check it on the MediaFlick dashboard");
        }
        if (response.StatusCode == HttpStatusCode.TooManyRequests)
        {
            throw new GatewayException(
                StatusCodes.Status429TooManyRequests,
                "MDBList quota is exhausted for now; the next sync will retry");
        }
        if (response.StatusCode is not HttpStatusCode.OK)
        {
            throw new GatewayException(
                StatusCodes.Status502BadGateway,
                $"MDBList could not serve that list (status {(int)response.StatusCode})");
        }

        var items = ExtractItems(response.Body);
        if (items.Count == 0 && ReportedItemCount(response.Body) > 0)
        {
            throw new GatewayException(
                StatusCodes.Status502BadGateway,
                "MDBList returned list items without usable TMDB identities");
        }

        lock (_gate)
        {
            _cache[cacheKey] = (now, items);
        }

        return items;
    }

    /// <summary>
    /// Parses a validated source reference into an API path and cache key.
    /// MDBList serves user lists at `lists/{username}/{listname}/items`, so
    /// both the website path and an explicit `user/` prefix are accepted.
    /// </summary>
    internal static bool TryParseSource(string raw, out string resource, out string cacheKey)
    {
        resource = string.Empty;
        cacheKey = string.Empty;
        var segments = raw.Trim().Split(
            '/',
            StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (segments.Length == 2
            && segments[0].Equals("official", StringComparison.OrdinalIgnoreCase))
        {
            if (!IsSafeSegment(segments[1]))
            {
                return false;
            }

            resource = $"lists/official/{segments[1]}/items";
            cacheKey = $"official/{segments[1].ToLowerInvariant()}";
            return true;
        }

        if ((segments.Length == 3
                && segments[0].Equals("user", StringComparison.OrdinalIgnoreCase))
            || (segments.Length == 2
                && !segments[0].Equals("official", StringComparison.OrdinalIgnoreCase)))
        {
            var (username, listname) = segments.Length == 3
                ? (segments[1], segments[2])
                : (segments[0], segments[1]);
            if (!IsSafeSegment(username) || !IsSafeSegment(listname))
            {
                return false;
            }

            resource = $"lists/{username}/{listname}/items";
            cacheKey = $"user/{username.ToLowerInvariant()}/{listname.ToLowerInvariant()}";
            return true;
        }

        return false;

        static bool IsSafeSegment(string segment)
            => segment.Length > 0
                && segment.Length <= 100
                && segment.All(character =>
                    char.IsAsciiLetterOrDigit(character)
                    || character is '_' or '-' or '.');
    }

    internal static IReadOnlyList<int> ParseTmdbIds(string raw)
        => raw
            .Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Select(part => int.TryParse(
                part,
                NumberStyles.Integer,
                CultureInfo.InvariantCulture,
                out var id)
                    ? id
                    : 0)
            .Where(id => id > 0)
            .Distinct()
            .ToArray();

    /// <summary>
    /// Reads movies and shows with their TMDB namespace intact. MDBList splits
    /// the response into buckets, so rank is used to reconstruct mixed lists.
    /// </summary>
    internal static IReadOnlyList<CuratedItem> ExtractItems(JsonNode? body)
    {
        if (body is not JsonObject detail)
        {
            return [];
        }

        var ranked = new List<(CuratedItem Item, int Rank, int Bucket, int Position)>();
        AddBucket(detail["movies"] as JsonArray, CuratedMediaKind.Movie, 0, ranked);
        AddBucket(detail["shows"] as JsonArray, CuratedMediaKind.Series, 1, ranked);
        return ranked
            .OrderBy(static row => row.Rank)
            .ThenBy(static row => row.Bucket)
            .ThenBy(static row => row.Position)
            .Select(static row => row.Item)
            .Distinct()
            .ToArray();
    }

    private static void AddBucket(
        JsonArray? entries,
        CuratedMediaKind kind,
        int bucket,
        ICollection<(CuratedItem Item, int Rank, int Bucket, int Position)> result)
    {
        if (entries is null)
        {
            return;
        }

        var position = 0;
        foreach (var entry in entries.OfType<JsonObject>())
        {
            var tmdbId = PositiveInteger(entry["ids"]?["tmdb"])
                ?? PositiveInteger(entry["id"]);
            if (tmdbId is { } id)
            {
                result.Add((
                    new CuratedItem(kind, id),
                    PositiveInteger(entry["rank"]) ?? int.MaxValue,
                    bucket,
                    position));
            }

            position += 1;
        }
    }

    private static int ReportedItemCount(JsonNode? body)
        => body is JsonObject detail
            ? PositiveInteger(detail["pagination"]?["total"]) ?? 0
            : 0;

    private static int? PositiveInteger(JsonNode? node)
    {
        if (node is not JsonValue value)
        {
            return null;
        }

        if (value.TryGetValue<int>(out var integer) && integer > 0)
        {
            return integer;
        }
        if (value.TryGetValue<long>(out var longInteger)
            && longInteger is > 0 and <= int.MaxValue)
        {
            return (int)longInteger;
        }
        if (value.TryGetValue<string>(out var text)
            && int.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out integer)
            && integer > 0)
        {
            return integer;
        }

        return null;
    }
}
