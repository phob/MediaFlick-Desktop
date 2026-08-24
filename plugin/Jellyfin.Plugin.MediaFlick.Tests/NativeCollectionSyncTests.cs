using System.Text.Json.Nodes;
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

    [Fact]
    public void CuratedTmdbIdParsingKeepsDefinitionOrderAndDropsJunk()
    {
        var ids = CuratedCollectionResolver.ParseTmdbIds(" 603, not-an-id, 11, 603, -5, 0 ");

        Assert.Equal([603, 11], ids);
    }

    [Theory]
    [InlineData("user/snoak/imdb-top-250-movies", "lists/snoak/imdb-top-250-movies/items")]
    [InlineData("snoak/imdb-top-250-movies", "lists/snoak/imdb-top-250-movies/items")]
    [InlineData("official/imdb-top-250", "lists/official/imdb-top-250/items")]
    [InlineData("USER/Snoak/List_1-x.y", "lists/Snoak/List_1-x.y/items")]
    public void MdbListSourcesParseIntoAllowlistedApiPaths(string raw, string expected)
    {
        Assert.True(CuratedCollectionResolver.TryParseSource(raw, out var resource, out _));
        Assert.Equal(expected, resource);
    }

    [Theory]
    [InlineData("")]
    [InlineData("imdb-top-250-movies")]
    [InlineData("user/a/b/c")]
    [InlineData("official/a/b")]
    [InlineData("user/snoak/../admin")]
    [InlineData("https://evil.example/list")]
    public void MdbListSourcePathsOutsideTheListsNamespaceAreRejected(string raw)
    {
        Assert.False(CuratedCollectionResolver.TryParseSource(raw, out _, out _));
    }

    [Fact]
    public void MdbListItemsKeepRankAndMediaNamespace()
    {
        var body = JsonNode.Parse(
            """
            {
              "movies": [
                {"rank":1,"ids":{"tmdb":603,"imdb":"tt0133093"}},
                {"rank":3,"ids":{"tmdb":11}},
                {"rank":4,"ids":{"tmdb":null}}
              ],
              "shows": [{"rank":2,"id":1396}]
            }
            """);

        var items = CuratedCollectionResolver.ExtractItems(body);

        Assert.Equal(
            [
                new CuratedItem(CuratedMediaKind.Movie, 603),
                new CuratedItem(CuratedMediaKind.Series, 1396),
                new CuratedItem(CuratedMediaKind.Movie, 11)
            ],
            items);
    }

    [Fact]
    public void CuratedMembershipKeepsMovieAndSeriesTmdbNamespacesSeparate()
    {
        var movie = new CuratedItem(CuratedMediaKind.Movie, 603);
        var oldSeries = new CuratedItem(CuratedMediaKind.Series, 603);
        var newSeries = new CuratedItem(CuratedMediaKind.Series, 1396);

        var (add, remove) = NativeCollectionSync.CuratedMembershipDiff(
            new HashSet<CuratedItem> { movie, oldSeries },
            [movie, newSeries]);

        Assert.Equal([newSeries], add);
        Assert.Equal([oldSeries], remove);
    }
}
