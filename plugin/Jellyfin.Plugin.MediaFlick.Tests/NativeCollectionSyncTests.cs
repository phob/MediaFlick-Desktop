using Jellyfin.Plugin.MediaFlick.Services;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class NativeCollectionSyncTests
{
    private static CollectionsService.Mapping Mapping(int collectionId, string name)
        => new(collectionId, name, null, null, DateTimeOffset.UtcNow);

    [Fact]
    public void DesiredCollectionsGroupMembersAndKeepSortNameFiling()
    {
        var movieIds = new[] { 603, 11, 1892, 42 };
        var mappings = new Dictionary<int, CollectionsService.Mapping>
        {
            [603] = Mapping(10, "The Matrix Collection"),
            [11] = Mapping(10, "The Matrix Collection"),
            [1892] = Mapping(2, "Alien Collection"),
            // A movie with no collection produces no BoxSet.
            [42] = Mapping(0, string.Empty)
        };

        var desired = NativeCollectionSync.DesiredCollections(movieIds, mappings);

        Assert.Equal(2, desired.Count);
        Assert.Equal(2, desired[0].CollectionId);
        Assert.Equal("Alien Collection", desired[0].Name);
        Assert.Equal([1892], desired[0].Members);
        Assert.Equal(10, desired[1].CollectionId);
        Assert.Equal([603, 11], desired[1].Members);
    }

    [Fact]
    public void MembershipDiffAddsMissingAndRemovesStale()
    {
        var (add, remove) = NativeCollectionSync.MembershipDiff(
            new HashSet<int> { 603, 11, 999 },
            [603, 11, 1892]);

        Assert.Equal([1892], add);
        Assert.Equal([999], remove);
    }

    [Fact]
    public void MembershipDiffIsStableWhenEverythingAlreadyMatches()
    {
        var (add, remove) = NativeCollectionSync.MembershipDiff(
            new HashSet<int> { 603 },
            [603]);

        Assert.Empty(add);
        Assert.Empty(remove);
    }
}
