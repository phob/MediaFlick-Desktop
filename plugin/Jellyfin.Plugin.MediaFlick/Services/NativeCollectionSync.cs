using System.Globalization;
using Jellyfin.Data.Enums;
using Jellyfin.Database.Implementations.Entities;
using Jellyfin.Plugin.MediaFlick.Configuration;
using MediaBrowser.Controller.Collections;
using MediaBrowser.Controller.Entities;
using MediaBrowser.Controller.Entities.Movies;
using MediaBrowser.Controller.Library;

namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// Mirrors the library's TMDB collections into Jellyfin's own BoxSet feature.
///
/// [`CollectionsService`] stays the discovery engine — it is still the only
/// way to know which TMDB collections the library belongs to. This service
/// turns its resolved mappings into real server collections so every Jellyfin
/// client sees them, and keeps their membership in step with the library.
///
/// Adoption goes by the TMDB provider id, never by name: an existing BoxSet
/// whose `Tmdb` id matches is taken over and synced, whether it was created
/// by hand or by another automation. Turning the feature off stops syncing;
/// collections already created stay in the library.
/// </summary>
public sealed class NativeCollectionSync
{
    /// <summary>
    /// Seerr resolves are bounded per run so one pass stays short. The
    /// schedule converges the remaining mappings over later runs.
    /// </summary>
    internal const int ResolvesPerRun = 200;

    private readonly CollectionsService _collections;
    private readonly ICollectionManager _collectionManager;
    private readonly IUserManager _users;
    private readonly ILibraryManager _library;

    public NativeCollectionSync(
        CollectionsService collections,
        ICollectionManager collectionManager,
        IUserManager users,
        ILibraryManager library)
    {
        _collections = collections;
        _collectionManager = collectionManager;
        _users = users;
        _library = library;
    }

    public async Task SyncAsync(
        IProgress<double>? progress,
        CancellationToken cancellationToken)
    {
        var configuration = Plugin.Instance?.Configuration ?? new PluginConfiguration();
        if (!configuration.NativeCollections)
        {
            progress?.Report(100);
            return;
        }

        // Seerr resolves happen as some mapped Jellyfin user; a background
        // sync has no session, so the first user stands in for the lookup.
        var user = _users.GetFirstUser()
            ?? throw new InvalidOperationException("no Jellyfin user exists for Seerr resolution");

        var movies = _collections.LibraryMovies();
        var movieIds = movies.Select(movie => movie.TmdbId).Distinct().Order().ToArray();
        progress?.Report(5);
        var (mappings, _) = await _collections.EnsureMappingsAsync(
            user.Id,
            movieIds,
            ResolvesPerRun,
            cancellationToken).ConfigureAwait(false);
        progress?.Report(40);

        var desired = DesiredCollections(movieIds, mappings);
        if (desired.Count == 0)
        {
            progress?.Report(100);
            return;
        }

        var existing = BoxSetsByTmdbId();
        var done = 0;
        foreach (var wanted in desired)
        {
            cancellationToken.ThrowIfCancellationRequested();

            var boxset = existing.TryGetValue(wanted.CollectionId, out var found)
                ? found
                : await CreateCollectionAsync(wanted.Name, wanted.CollectionId, cancellationToken)
                    .ConfigureAwait(false);

            await SyncMembershipAsync(user, boxset, wanted.Members, movies, cancellationToken)
                .ConfigureAwait(false);
            await RenameAsync(boxset, wanted.Name, cancellationToken).ConfigureAwait(false);
            done += 1;
            progress?.Report(40 + done * 60 / desired.Count);
        }

        progress?.Report(100);
    }

    /// <summary>One TMDB collection that should exist as a BoxSet on the server.</summary>
    internal sealed record WantedCollection(int CollectionId, string Name, IReadOnlyList<int> Members);

    /// <summary>
    /// Groups resolved movie-to-collection mappings into the sets the server
    /// should carry. Pure so tests can pin grouping and membership identity.
    /// </summary>
    internal static IReadOnlyList<WantedCollection> DesiredCollections(
        IReadOnlyList<int> movieIds,
        IReadOnlyDictionary<int, CollectionsService.Mapping> mappings)
    {
        return movieIds
            .Select(movieId => (
                MovieId: movieId,
                Mapping: mappings.TryGetValue(movieId, out var mapping) && mapping.CollectionId > 0
                    ? mapping
                    : null))
            .Where(pair => pair.Mapping is not null)
            .GroupBy(pair => pair.Mapping!.CollectionId)
            .Select(group => new WantedCollection(
                group.Key,
                group.First().Mapping!.Name,
                group.Select(pair => pair.MovieId).ToArray()))
            .OrderBy(collection => CollectionsService.SortName(collection.Name), StringComparer.OrdinalIgnoreCase)
            .ToArray();
    }

    /// <summary>Diffs one collection's current and wanted members by TMDB id.</summary>
    internal static (IReadOnlyList<int> Add, IReadOnlyList<int> Remove) MembershipDiff(
        IReadOnlySet<int> currentMembers,
        IReadOnlyList<int> wantedMembers)
    {
        var add = wantedMembers.Where(id => !currentMembers.Contains(id)).ToArray();
        var remove = currentMembers.Where(id => !wantedMembers.Contains(id)).ToArray();
        return (add, remove);
    }

    /// <summary>A BoxSet counts as carrying a TMDB collection when its provider id says so.</summary>
    internal static int? CollectionTmdbId(BoxSet boxset)
        => boxset.ProviderIds is { } ids
            && ids.TryGetValue("Tmdb", out var value)
            && int.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out var id)
            && id > 0
                ? id
                : null;

    private async Task RenameAsync(BoxSet boxset, string name, CancellationToken cancellationToken)
    {
        if (string.Equals(boxset.Name, name, StringComparison.Ordinal))
        {
            return;
        }

        // Upstream renames reach adopted and previously created sets alike;
        // membership identity stays keyed on the TMDB provider id.
        boxset.Name = name;
        boxset.SortName = CollectionsService.SortName(name);
        await _library.UpdateItemAsync(boxset, boxset, ItemUpdateType.MetadataEdit, cancellationToken)
            .ConfigureAwait(false);
    }
    private Dictionary<int, BoxSet> BoxSetsByTmdbId()
    {
        var query = new InternalItemsQuery
        {
            IncludeItemTypes = new[] { BaseItemKind.BoxSet },
            Recursive = true,
            DtoOptions = new MediaBrowser.Controller.Dto.DtoOptions
            {
                EnableImages = false
            }
        };
        var result = new Dictionary<int, BoxSet>();
        foreach (var item in _library.GetItemList(query).OfType<BoxSet>())
        {
            if (CollectionTmdbId(item) is { } tmdbId)
            {
                result.TryAdd(tmdbId, item);
            }
        }

        return result;
    }

    private async Task<BoxSet> CreateCollectionAsync(
        string name,
        int collectionId,
        CancellationToken cancellationToken)
    {        var boxset = await _collectionManager.CreateCollectionAsync(new CollectionCreationOptions
        {
            Name = name,
            ProviderIds = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
            {
                ["Tmdb"] = collectionId.ToString(CultureInfo.InvariantCulture)
            }
        }).ConfigureAwait(false);
        return boxset;
    }

    private async Task SyncMembershipAsync(
        User user,
        BoxSet boxset,
        IReadOnlyList<int> wantedMembers,
        IReadOnlyList<CollectionsService.LibraryMovie> movies,
        CancellationToken cancellationToken)
    {
        // Children answer through the folder API; each child's own provider
        // ids decide membership so duplicate copies of one film collapse.
        var currentMembers = (boxset as Folder)?.GetChildren(user, false, null)
            .Select(child => CollectionsService.ParseTmdbId(child))
            .OfType<int>()
            .ToHashSet() ?? new HashSet<int>();

        var (add, remove) = MembershipDiff(currentMembers, wantedMembers);
        if (add.Count > 0)
        {
            var addIds = MemberItemIds(movies, add);
            if (addIds.Count > 0)
            {
                await _collectionManager.AddToCollectionAsync(boxset.Id, addIds)
                    .ConfigureAwait(false);
            }
        }

        if (remove.Count == 0)
        {
            return;
        }

        cancellationToken.ThrowIfCancellationRequested();
        var removeIds = (boxset as Folder)?.GetChildren(user, false, null)
            .Where(child => CollectionsService.ParseTmdbId(child) is { } tmdbId
                && remove.Contains(tmdbId))
            .Select(child => child.Id)
            .ToArray() ?? [];
        if (removeIds.Length > 0)
        {
            await _collectionManager.RemoveFromCollectionAsync(boxset.Id, removeIds)
                .ConfigureAwait(false);
        }
    }

    private static IReadOnlyList<Guid> MemberItemIds(
        IReadOnlyList<CollectionsService.LibraryMovie> movies,
        IReadOnlyList<int> memberTmdbIds)
    {
        var wanted = memberTmdbIds.ToHashSet();
        var result = new List<Guid>();
        foreach (var movie in movies)
        {
            if (wanted.Contains(movie.TmdbId))
            {
                result.Add(movie.Item.Id);
                wanted.Remove(movie.TmdbId);
            }
        }

        return result;
    }
}
