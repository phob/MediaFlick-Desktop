using System.Net;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Api;
using Jellyfin.Plugin.MediaFlick.Models;
using Jellyfin.Plugin.MediaFlick.Services;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Http;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class CollectionProviderTests : IDisposable
{
    private readonly string _directory = Path.Combine(
        Path.GetTempPath(),
        "mediaflick-collection-provider-" + Guid.NewGuid().ToString("N"));
    private readonly MemorySecrets _secrets = new();
    private readonly FakeTmdb _tmdb = new();
    private readonly FakeMdbList _mdbList = new();
    private readonly RatingsCacheStore _health;
    private readonly CollectionProviderService _service;

    public CollectionProviderTests()
    {
        Directory.CreateDirectory(_directory);
        _health = new RatingsCacheStore(Path.Combine(_directory, "provider-cache.json"));
        _service = new CollectionProviderService(_tmdb, _mdbList, _secrets, _health);
    }

    [Fact]
    public async Task PreviewReturnsTwentyFourNormalizedTitlesButKeepsFullCounts()
    {
        ConfigureTmdb();
        _tmdb.Handler = (path, query) => path == "3/discover/movie"
            ? Ok(new JsonObject
            {
                ["total_results"] = 30,
                ["total_pages"] = 1,
                ["results"] = new JsonArray(Enumerable.Range(1, 30).Select(index =>
                    (JsonNode)new JsonObject
                    {
                        ["id"] = index,
                        ["title"] = "Movie " + index,
                        ["release_date"] = "2020-01-01",
                        ["poster_path"] = "/poster-" + index + ".jpg",
                        ["backdrop_path"] = "/backdrop-" + index + ".jpg",
                        ["adult"] = index == 30
                    }).ToArray())
            })
            : Ok(new JsonObject());

        var result = await _service.PreviewAsync(
            Request(new JsonObject
            {
                ["kind"] = "tmdbDiscover",
                ["parameters"] = new JsonObject { ["sortBy"] = "popularity.desc" }
            }),
            TestContext.Current.CancellationToken);

        Assert.Equal(24, result.Items.Count);
        Assert.Equal(30, result.Total);
        Assert.All(result.Items, item => Assert.False(item.Adult));
        Assert.Equal(Enumerable.Range(0, 24), result.Items.Select(item => item.SourceOrder));
        Assert.Equal("/poster-1.jpg", result.Items[0].PosterPath);
        Assert.Equal("/backdrop-1.jpg", result.Items[0].BackdropPath);
    }

    [Fact]
    public async Task PreviewStopsPagingAsSoonAsTwentyFourUsableTitlesExist()
    {
        ConfigureTmdb();
        var calls = 0;
        _tmdb.Handler = (path, query) =>
        {
            if (path == "3/configuration")
            {
                return Ok(new JsonObject());
            }
            Assert.Equal("3/discover/movie", path);
            calls += 1;
            var page = int.Parse(query["page"]);
            return Ok(new JsonObject
            {
                ["total_results"] = 10_000,
                ["total_pages"] = 500,
                ["results"] = new JsonArray(Enumerable.Range(1, 20).Select(index =>
                    (JsonNode)new JsonObject
                    {
                        ["id"] = (page - 1) * 20 + index,
                        ["title"] = $"Movie {(page - 1) * 20 + index}"
                    }).ToArray())
            });
        };

        var result = await _service.PreviewAsync(
            Request(new JsonObject
            {
                ["kind"] = "tmdbDiscover",
                ["parameters"] = new JsonObject()
            }),
            TestContext.Current.CancellationToken);

        Assert.Equal(2, calls);
        Assert.Equal(24, result.Items.Count);
        Assert.Equal(10_000, result.Total);
    }

    [Fact]
    public async Task ArtworkReturnsTheRequestedTmdbRendition()
    {
        var result = await _service.ArtworkAsync(
            "w342",
            "/matrix.jpg",
            TestContext.Current.CancellationToken);

        Assert.Equal("image/jpeg", result.ContentType);
        Assert.Equal([0xFF, 0xD8, 0xFF], result.Body);
        Assert.Equal(("w342", "/matrix.jpg"), _tmdb.ArtworkRequest);
    }

    [Theory]
    [InlineData("w92")]
    [InlineData("w154")]
    [InlineData("w185")]
    [InlineData("w300")]
    [InlineData("w342")]
    [InlineData("w500")]
    [InlineData("w780")]
    [InlineData("w1280")]
    public void ArtworkTransportAcceptsEveryDesktopRendition(string size)
    {
        Assert.True(TmdbHttpTransport.SafeArtwork(size, "/matrix.jpg"));
    }

    [Theory]
    [InlineData("w91")]
    [InlineData("../w300")]
    [InlineData("")]
    public void ArtworkTransportRejectsUnknownRenditions(string size)
    {
        Assert.False(TmdbHttpTransport.SafeArtwork(size, "/matrix.jpg"));
    }

    [Fact]
    public async Task PersistedHealthRequiresOneRealValidationAfterPluginStartup()
    {
        ConfigureTmdb();
        ConfigureMdbList();
        Assert.False(_service.TmdbReady);
        Assert.False(_service.MdbListReady);

        await _service.RefreshReadinessAsync(TestContext.Current.CancellationToken);

        Assert.True(_service.TmdbReady);
        Assert.True(_service.MdbListReady);
    }

    [Theory]
    [InlineData(HttpStatusCode.TooManyRequests)]
    [InlineData(HttpStatusCode.BadGateway)]
    [InlineData(HttpStatusCode.GatewayTimeout)]
    public async Task TransientReadinessFailuresDoNotInvalidateAPreviouslyValidMdbListKey(
        HttpStatusCode status)
    {
        ConfigureMdbList();
        _mdbList.Handler = resource => resource == "user"
            ? new(status, null, new(null, null, null), null)
            : new(HttpStatusCode.NotFound, null, new(null, null, null), null);

        await _service.RefreshReadinessAsync(TestContext.Current.CancellationToken);

        var health = _health.Health("mdblist");
        Assert.True(health.Valid);
        Assert.NotEqual("invalid", health.Validation);
    }

    [Theory]
    [InlineData(HttpStatusCode.Unauthorized)]
    [InlineData(HttpStatusCode.Forbidden)]
    public async Task CredentialRejectionsStillInvalidateMdbListReadiness(HttpStatusCode status)
    {
        ConfigureMdbList();
        _mdbList.Handler = resource => resource == "user"
            ? new(status, null, new(null, null, null), null)
            : new(HttpStatusCode.NotFound, null, new(null, null, null), null);

        await _service.RefreshReadinessAsync(TestContext.Current.CancellationToken);

        Assert.False(_health.Health("mdblist").Valid);
        Assert.Equal("invalid", _health.Health("mdblist").Validation);
    }

    [Fact]
    public async Task ProviderRetryTimingStopsRepeatedCollectionRequests()
    {
        ConfigureTmdb();
        var discoverCalls = 0;
        _tmdb.Handler = (path, query) =>
        {
            if (path == "3/discover/movie")
            {
                discoverCalls += 1;
                return new(
                    HttpStatusCode.TooManyRequests,
                    null,
                    DateTimeOffset.UtcNow.AddMinutes(5).ToUnixTimeSeconds());
            }
            return Ok(new JsonObject());
        };
        var request = Request(new JsonObject
        {
            ["kind"] = "tmdbDiscover",
            ["parameters"] = new JsonObject()
        });

        await Assert.ThrowsAsync<GatewayException>(() =>
            _service.ResultsAsync(request, TestContext.Current.CancellationToken));
        await Assert.ThrowsAsync<GatewayException>(() =>
            _service.ResultsAsync(request, TestContext.Current.CancellationToken));

        Assert.Equal(1, discoverCalls);
        Assert.False(_service.TmdbReady);
    }

    [Fact]
    public void DesktopAndCompanionShareTheNormalizedProviderFixture()
    {
        using var stream = typeof(CollectionProviderTests).Assembly.GetManifestResourceStream(
            "Jellyfin.Plugin.MediaFlick.Tests.Fixtures.provider-result-v1.json");
        Assert.NotNull(stream);
        var result = JsonSerializer.Deserialize<CollectionProviderResult>(
            stream,
            CompanionJson.CamelCase);

        Assert.NotNull(result);
        Assert.Equal("fixture-v1", result.SourceIdentity);
        Assert.Equal(["movie", "series"], result.Items.Select(item => item.MediaType));
        Assert.Equal([603L, 1396L], result.Items.Select(item => item.TmdbId));
    }

    [Fact]
    public async Task PublicUrlSelectorAndNumericIdCommitTheSameSourceIdentity()
    {
        ConfigureMdbList();
        _mdbList.Handler = resource => resource switch
        {
            "user" => new(
                HttpStatusCode.OK,
                new JsonObject(),
                new(null, null, null),
                null),
            "lists/alice/favorites" or "lists/42" => new(
                HttpStatusCode.OK,
                new JsonObject { ["id"] = 42, ["name"] = "Favorites", ["username"] = "alice" },
                new(null, null, null),
                null),
            "lists/42/items?unified=true&limit=1000&offset=0" => new(
                HttpStatusCode.OK,
                new JsonArray(new JsonObject
                {
                    ["id"] = 603,
                    ["title"] = "The Matrix",
                    ["mediatype"] = "movie"
                }),
                new(null, null, null),
                null),
            _ => new(HttpStatusCode.NotFound, null, new(null, null, null), null)
        };

        var fromUrl = await _service.ResultsAsync(
            Request(new JsonObject { ["kind"] = "mdbListPublicList", ["listId"] = "https://mdblist.com/lists/alice/favorites" }, "mixed"),
            TestContext.Current.CancellationToken);
        var fromId = await _service.ResultsAsync(
            Request(new JsonObject { ["kind"] = "mdbListPublicList", ["listId"] = "42" }, "mixed"),
            TestContext.Current.CancellationToken);

        Assert.Equal("42", fromUrl.SourceIdentity);
        Assert.Equal(fromUrl.SourceIdentity, fromId.SourceIdentity);
        Assert.Equal(603, Assert.Single(fromUrl.Items).TmdbId);
    }

    [Fact]
    public async Task DocumentedMdbListBucketsAndIntegerAdultFlagsAreNormalized()
    {
        ConfigureMdbList();
        _mdbList.Handler = resource => resource switch
        {
            "user" => new(HttpStatusCode.OK, new JsonObject(), new(null, null, null), null),
            "lists/42" => new(
                HttpStatusCode.OK,
                new JsonObject { ["id"] = 42, ["name"] = "Mixed" },
                new(null, null, null),
                null),
            "lists/42/items?unified=true&limit=1000&offset=0" => new(
                HttpStatusCode.OK,
                new JsonObject
                {
                    ["movies"] = new JsonArray(
                        new JsonObject { ["id"] = 603, ["title"] = "The Matrix", ["adult"] = 0 },
                        new JsonObject { ["id"] = 604, ["title"] = "Adult", ["adult"] = 1 }),
                    ["shows"] = new JsonArray(
                        new JsonObject { ["id"] = 1396, ["title"] = "Breaking Bad", ["adult"] = 0 },
                        new JsonObject { ["title"] = "Missing identity", ["adult"] = 0 })
                },
                new(null, null, null),
                null),
            _ => new(HttpStatusCode.NotFound, null, new(null, null, null), null)
        };

        var result = await _service.ResultsAsync(
            Request(new JsonObject { ["kind"] = "mdbListPublicList", ["listId"] = "42" }, "mixed"),
            TestContext.Current.CancellationToken);

        Assert.Equal(["movie", "series"], result.Items.Select(item => item.MediaType));
        Assert.Equal([603L, 1396L], result.Items.Select(item => item.TmdbId));
    }

    [Fact]
    public async Task MdbListFilteringUsesTheProviderQueryAndRawPageCountForPagination()
    {
        ConfigureMdbList();
        var resources = new List<string>();
        _mdbList.Handler = resource =>
        {
            resources.Add(resource);
            return resource switch
            {
                "user" => new(HttpStatusCode.OK, new JsonObject(), new(null, null, null), null),
                "lists/42" => new(
                    HttpStatusCode.OK,
                    new JsonObject { ["id"] = 42, ["name"] = "Movies" },
                    new(null, null, null),
                    null),
                "lists/42/items?unified=true&mediatype=movie&limit=1000&offset=0" => new(
                    HttpStatusCode.OK,
                    new JsonArray(
                        new JsonObject { ["id"] = 1, ["title"] = "Movie", ["mediatype"] = "movie" },
                        new JsonObject { ["id"] = 2, ["title"] = "Filtered show", ["mediatype"] = "show" },
                        new JsonObject { ["title"] = "Missing id", ["mediatype"] = "movie" }),
                    new(null, null, null),
                    null,
                    true),
                "lists/42/items?unified=true&mediatype=movie&limit=1000&offset=3" => new(
                    HttpStatusCode.OK,
                    new JsonArray(new JsonObject
                    {
                        ["id"] = 3,
                        ["title"] = "Second page",
                        ["mediatype"] = "movie"
                    }),
                    new(null, null, null),
                    null),
                _ => new(HttpStatusCode.NotFound, null, new(null, null, null), null)
            };
        };

        var result = await _service.ResultsAsync(
            Request(new JsonObject { ["kind"] = "mdbListPublicList", ["listId"] = "42" }, "movie"),
            TestContext.Current.CancellationToken);

        Assert.Equal([1L, 3L], result.Items.Select(item => item.TmdbId));
        Assert.Contains(
            "lists/42/items?unified=true&mediatype=movie&limit=1000&offset=3",
            resources);
    }

    [Theory]
    [InlineData(HttpStatusCode.Forbidden)]
    [InlineData(HttpStatusCode.Unauthorized)]
    [InlineData(HttpStatusCode.NotFound)]
    public async Task PrivateForbiddenAndMissingListsHaveOneSafeError(HttpStatusCode status)
    {
        ConfigureMdbList();
        _mdbList.Handler = resource => resource == "user"
            ? new(HttpStatusCode.OK, new JsonObject(), new(null, null, null), null)
            : new(status, null, new(null, null, null), null);

        var error = await Assert.ThrowsAsync<GatewayException>(() => _service.ValidatePublicListAsync(
            new PublicListSelectorRequest("42"),
            TestContext.Current.CancellationToken));

        Assert.Equal(StatusCodes.Status404NotFound, error.StatusCode);
        Assert.Equal("List not available", error.Message);
    }

    [Fact]
    public async Task ImdbAndTvdbMappingsReturnTypedTmdbIdentities()
    {
        ConfigureTmdb();
        _tmdb.Handler = (path, query) => path switch
        {
            "3/find/tt0133093" => Ok(new JsonObject
            {
                ["movie_results"] = new JsonArray(new JsonObject { ["id"] = 603 })
            }),
            "3/find/81189" => Ok(new JsonObject
            {
                ["tv_results"] = new JsonArray(new JsonObject { ["id"] = 1396 })
            }),
            _ => Ok(new JsonObject())
        };

        var response = await _service.ResolveIdentitiesAsync(
            new IdentityResolveRequest([
                new("movie", "tt0133093", null),
                new("series", null, "81189")
            ]),
            TestContext.Current.CancellationToken);

        Assert.Contains(response.Mappings, mapping => mapping is
        { MediaType: "movie", Provider: "imdb", ProviderId: "tt0133093", TmdbId: 603 });
        Assert.Contains(response.Mappings, mapping => mapping is
        { MediaType: "series", Provider: "tvdb", ProviderId: "81189", TmdbId: 1396 });
    }

    [Fact]
    public async Task ExactCollectionKeepsOwnedFutureTitlesButHidesOtherUnreleasedRows()
    {
        ConfigureTmdb();
        _tmdb.Handler = (path, query) => path == "3/collection/10"
            ? Ok(new JsonObject
            {
                ["id"] = 10,
                ["parts"] = new JsonArray(
                    new JsonObject { ["id"] = 1, ["title"] = "Released", ["release_date"] = "2020-01-01" },
                    new JsonObject { ["id"] = 2, ["title"] = "Owned future", ["release_date"] = "2999-01-01" },
                    new JsonObject { ["id"] = 3, ["title"] = "Missing future", ["release_date"] = "2999-01-01" },
                    new JsonObject { ["id"] = 4, ["title"] = "Undated" })
            })
            : Ok(new JsonObject());

        var result = await _service.ResultsAsync(
            new CollectionProviderRequest(
                new JsonObject
                {
                    ["kind"] = "tmdbCollection",
                    ["collectionId"] = 10,
                    ["includeUnreleased"] = false
                },
                "movie",
                new("all", null),
                [2]),
            TestContext.Current.CancellationToken);

        Assert.Equal([1L, 2L], result.Items.Select(item => item.TmdbId));
    }

    [Fact]
    public async Task FranchiseResolutionReusesKnownCollectionsAndReturnsNegativeMemberships()
    {
        ConfigureTmdb();
        var movieCalls = 0;
        _tmdb.Handler = (path, query) =>
        {
            if (path.StartsWith("3/movie/", StringComparison.Ordinal))
            {
                movieCalls += 1;
                return Ok(new JsonObject { ["belongs_to_collection"] = null });
            }
            if (path == "3/collection/10")
            {
                return Ok(new JsonObject
                {
                    ["id"] = 10,
                    ["name"] = "Known collection",
                    ["parts"] = new JsonArray()
                });
            }
            return Ok(new JsonObject());
        };

        var result = await _service.FranchisesAsync(
            new FranchiseResolveRequest([101, 102], [10]),
            TestContext.Current.CancellationToken);

        Assert.Equal(2, movieCalls);
        Assert.Equal([101L, 102L], result.Memberships.Select(row => row.TmdbId));
        Assert.All(result.Memberships, row => Assert.Null(row.CollectionId));
        Assert.Equal(10, Assert.Single(result.Franchises).CollectionId);

        _ = await _service.FranchisesAsync(
            new FranchiseResolveRequest([], [10]),
            TestContext.Current.CancellationToken);
        Assert.Equal(2, movieCalls);
    }

    [Fact]
    public void CollectionDataIsUserAuthenticatedWhileCredentialsRemainAdministratorOnly()
    {
        Assert.NotNull(typeof(CollectionExperienceController).GetCustomAttribute<AuthorizeAttribute>());
        Assert.Equal(
            MediaBrowser.Common.Api.Policies.RequiresElevation,
            typeof(RatingsAdminController).GetCustomAttribute<AuthorizeAttribute>()?.Policy);
    }

    [Fact]
    public async Task RemovingCredentialClearsItsNormalizedCollectionCache()
    {
        ConfigureTmdb();
        var discoverCalls = 0;
        _tmdb.Handler = (path, query) =>
        {
            if (path == "3/discover/movie")
            {
                discoverCalls += 1;
                return Ok(new JsonObject
                {
                    ["total_results"] = 1,
                    ["total_pages"] = 1,
                    ["results"] = new JsonArray(new JsonObject
                    {
                        ["id"] = 1,
                        ["title"] = "One"
                    })
                });
            }
            return Ok(new JsonObject());
        };
        var request = Request(new JsonObject
        {
            ["kind"] = "tmdbDiscover",
            ["parameters"] = new JsonObject()
        });
        _ = await _service.ResultsAsync(request, TestContext.Current.CancellationToken);
        _ = await _service.ResultsAsync(request, TestContext.Current.CancellationToken);
        Assert.Equal(1, discoverCalls);

        _secrets.Remove("tmdb");
        _service.ClearProviderCache("tmdb");
        await Assert.ThrowsAsync<GatewayException>(() =>
            _service.ResultsAsync(request, TestContext.Current.CancellationToken));

        ConfigureTmdb();
        _ = await _service.ResultsAsync(request, TestContext.Current.CancellationToken);
        Assert.Equal(2, discoverCalls);
    }

    public void Dispose()
    {
        try
        {
            Directory.Delete(_directory, true);
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }

    private void ConfigureTmdb()
    {
        _secrets.Set("tmdb", "0123456789abcdef0123456789abcdef");
        _health.SetHealth("tmdb", new ProviderHealthState
        {
            Validation = "valid",
            Valid = true,
            LastCheckedAt = DateTimeOffset.UtcNow.ToUnixTimeSeconds()
        });
    }

    private void ConfigureMdbList()
    {
        _secrets.Set("mdblist", "key");
        _health.SetHealth("mdblist", new ProviderHealthState
        {
            Validation = "valid",
            Valid = true,
            LastCheckedAt = DateTimeOffset.UtcNow.ToUnixTimeSeconds()
        });
    }

    private static CollectionProviderRequest Request(
        JsonObject source,
        string mediaType = "movie")
        => new(source, mediaType, new("all", null));

    private static TmdbResponse Ok(JsonNode body)
        => new(HttpStatusCode.OK, body, null);

    private sealed class MemorySecrets : IRatingSecretStore
    {
        private readonly Dictionary<string, string> _values = new(StringComparer.OrdinalIgnoreCase);

        public bool IsConfigured(string provider) => _values.ContainsKey(provider);

        public string? Get(string provider) => _values.GetValueOrDefault(provider);

        public void Set(string provider, string secret) => _values[provider] = secret;

        public void Remove(string provider) => _values.Remove(provider);
    }

    private sealed class FakeTmdb : ITmdbTransport
    {
        public (string Size, string Path)? ArtworkRequest { get; private set; }

        public Func<string, IReadOnlyDictionary<string, string>, TmdbResponse> Handler { get; set; }
            = (_, _) => Ok(new JsonObject());

        public Task<TmdbResponse> GetAsync(
            string credential,
            string path,
            IReadOnlyDictionary<string, string> query,
            CancellationToken cancellationToken)
            => Task.FromResult(Handler(path, query));

        public Task<ArtworkResponse> GetArtworkAsync(
            string size,
            string path,
            CancellationToken cancellationToken)
        {
            ArtworkRequest = (size, path);
            return Task.FromResult(new ArtworkResponse(
                HttpStatusCode.OK,
                [0xFF, 0xD8, 0xFF],
                "image/jpeg"));
        }
    }

    private sealed class FakeMdbList : IMdbListTransport
    {
        public Func<string, MdbListResponse> Handler { get; set; }
            = _ => new(HttpStatusCode.OK, new JsonArray(), new(null, null, null), null);

        public Task<MdbListResponse> ValidateAsync(string apiKey, CancellationToken cancellationToken)
            => Task.FromResult(Handler("user"));

        public Task<MdbListResponse> BatchAsync(
            string apiKey,
            string provider,
            string mediaType,
            IReadOnlyList<string> ids,
            CancellationToken cancellationToken)
            => Task.FromResult(Handler("batch"));

        public Task<MdbListResponse> ListItemsAsync(
            string apiKey,
            string resource,
            CancellationToken cancellationToken)
            => Task.FromResult(Handler(resource));
    }
}
