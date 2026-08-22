using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Services;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class CollectionsTests
{
    [Fact]
    public void CollectionPartsReuseTheSearchResultShapeAndDropAdultRows()
    {
        var source = JsonNode.Parse(
            """
            {
              "id":10,"name":"Star Wars Collection",
              "overview":"An epic space-opera theatrical film series.",
              "posterPath":"/22dj38IckjzEEUZwN1tPU5VJ1qq.jpg",
              "backdropPath":"/4z9ijhgEthfRHShoOvMaBlpciXS.jpg",
              "parts":[
                {"id":11,"title":"Star Wars","releaseDate":"1977-05-25",
                 "posterPath":"/6FfCtAuVAW8XJjZ7eWeLibRLWTw.jpg","voteAverage":8.2},
                {"id":1891,"title":"The Empire Strikes Back","adult":true},
                {"id":0,"title":"Malformed"}
              ]
            }
            """);

        var shaped = SeerrGateway.ShapeCollection(source);
        Assert.Equal(10, shaped["id"]?.GetValue<int>());
        Assert.Equal("Star Wars Collection", shaped["name"]?.GetValue<string>());
        var parts = Assert.IsType<JsonArray>(shaped["parts"]);
        var part = Assert.IsType<JsonObject>(Assert.Single(parts));
        Assert.Equal("movie", part["mediaType"]?.GetValue<string>());
        Assert.Equal(11, part["tmdbId"]?.GetValue<int>());
        Assert.Equal("unknown", part["status"]?.GetValue<string>());
        Assert.Null(part["libraryItemId"]);
    }

    [Fact]
    public void GroupCollectionsDeduplicateByNameOrderAndCountMovies()
    {
        var movieIds = new[] { 603, 11, 1891, 1892, 42 };
        var mappings = new Dictionary<int, CollectionsService.Mapping>
        {
            [603] = new(10, "The Matrix Collection", "/a.jpg", null, DateTimeOffset.UtcNow),
            [11] = new(10, "The Matrix Collection", "/a.jpg", null, DateTimeOffset.UtcNow),
            // A movie TMDB no longer knows maps to no collection at all.
            [1891] = new(0, string.Empty, null, null, DateTimeOffset.UtcNow),
            [1892] = new(2, "Alien Collection", null, "/b.jpg", DateTimeOffset.UtcNow)
        };

        var summary = CollectionsService.GroupCollections(movieIds, mappings, pending: 1);

        var collections = Assert.IsType<JsonArray>(summary["collections"]);
        Assert.Equal(2, collections.Count);
        Assert.Equal("Alien Collection", collections[0]?["name"]?.GetValue<string>());
        Assert.Equal("The Matrix Collection", collections[1]?["name"]?.GetValue<string>());
        Assert.Equal(2, collections[1]?["movieCount"]?.GetValue<int>());
        Assert.Equal(5, summary["libraryMovies"]?.GetValue<int>());
        Assert.Equal(3, summary["mappedMovies"]?.GetValue<int>());
        Assert.Equal(1, summary["pendingMovies"]?.GetValue<int>());
    }

    [Fact]
    public void GroupCollectionsIgnoresMappingsOutsideTheCurrentLibrary()
    {
        var mappings = new Dictionary<int, CollectionsService.Mapping>
        {
            [603] = new(10, "The Matrix Collection", "/a.jpg", null, DateTimeOffset.UtcNow),
            [1892] = new(2, "Alien Collection", null, "/b.jpg", DateTimeOffset.UtcNow)
        };

        var summary = CollectionsService.GroupCollections([603], mappings, pending: 0);

        var collections = Assert.IsType<JsonArray>(summary["collections"]);
        var collection = Assert.IsType<JsonObject>(Assert.Single(collections));
        Assert.Equal(10, collection["id"]?.GetValue<int>());
        Assert.Equal(1, collection["movieCount"]?.GetValue<int>());
    }

    [Fact]
    public void MovieCollectionShapeCarriesIdentityOnlyWhenOneExists()
    {
        var known = CollectionsService.MovieCollectionShape(603, 10, "The Matrix Collection");
        Assert.Equal(603, known["tmdbId"]?.GetValue<int>());
        Assert.Equal(10, known["collection"]?["id"]?.GetValue<int>());
        Assert.Equal("The Matrix Collection", known["collection"]?["name"]?.GetValue<string>());

        var unknown = CollectionsService.MovieCollectionShape(42, null, null);
        Assert.Null(unknown["collection"]);
    }

    [Fact]
    public void MappingCacheRoundTripsThroughTheDocumentFormat()
    {
        var cachedAt = DateTimeOffset.FromUnixTimeMilliseconds(1_775_000_000_000);
        var mappings = new Dictionary<int, CollectionsService.Mapping>
        {
            [603] = new(10, "The Matrix Collection", "/a.jpg", "/b.jpg", cachedAt),
            // A movie with no collection persists too: its empty marker is what
            // keeps it from costing a resolve again after a restart.
            [42] = new(0, string.Empty, null, null, cachedAt)
        };

        var reloaded = CollectionsService.ReadDocument(CollectionsService.WriteDocument(mappings));

        Assert.Equal(2, reloaded.Count);
        Assert.Equal(10, reloaded[603].CollectionId);
        Assert.Equal("The Matrix Collection", reloaded[603].Name);
        Assert.Equal(cachedAt, reloaded[603].CachedAt);
        Assert.Equal(0, reloaded[42].CollectionId);
    }

    [Theory]
    [InlineData("not json")]
    [InlineData("{}")]
    [InlineData("{\"version\":99,\"mappings\":{}}")]
    public void DamagedOrForeignCacheDocumentsDegradeToEmpty(string json)
    {
        Assert.Empty(CollectionsService.ReadDocument(json));
    }

    [Fact]
    public void CollectionsSortUnderTheirRealTitleNotTheLeadingArticle()
    {
        var movieIds = new[] { 1, 2, 3, 4, 5 };
        var mappings = new Dictionary<int, CollectionsService.Mapping>
        {
            [1] = new(5, "Zombie Collection", null, null, DateTimeOffset.UtcNow),
            [2] = new(1, "Alien Collection", null, null, DateTimeOffset.UtcNow),
            [3] = new(2, "The Matrix Collection", null, null, DateTimeOffset.UtcNow),
            // Article stripping must not eat into the following word.
            [4] = new(3, "An American Werewolf Collection", null, null, DateTimeOffset.UtcNow),
            [5] = new(4, "A Quiet Place Collection", null, null, DateTimeOffset.UtcNow)
        };

        var summary = CollectionsService.GroupCollections(movieIds, mappings, pending: 0);

        var names = Assert.IsType<JsonArray>(summary["collections"])
            .Select(node => node?["name"]?.GetValue<string>())
            .ToArray();
        Assert.Equal(
            [
                "Alien Collection",
                "An American Werewolf Collection",
                "The Matrix Collection",
                "A Quiet Place Collection",
                "Zombie Collection"
            ],
            names);
    }

    [Theory]
    [InlineData("The Matrix Collection", "Matrix Collection")]
    [InlineData("An American Werewolf Collection", "American Werewolf Collection")]
    [InlineData("A Quiet Place Collection", "Quiet Place Collection")]
    [InlineData("Alien Collection", "Alien Collection")]
    [InlineData("", "")]
    public void SortNameStripsOnlyALeadingEnglishArticle(string name, string expected)
    {
        Assert.Equal(expected, CollectionsService.SortName(name));
    }
}
