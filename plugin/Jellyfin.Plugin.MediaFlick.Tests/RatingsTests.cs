using System.Net;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Api;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Jellyfin.Plugin.MediaFlick.Models;
using Jellyfin.Plugin.MediaFlick.Services;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.DataProtection;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class RatingsTests
{
    [Fact]
    public void AdministratorSecretsAreProtectedReplaceableAndRemovable()
    {
        using var directory = new TemporaryDirectory();
        var definition = new CuratedCollectionDefinition
        {
            Id = "top-250",
            Name = "Top 250",
            MdbListSource = "snoak/imdb-top-250-movies"
        };
        var configuration = new PluginConfiguration
        {
            CuratedCollections = [definition],
            NativeCollections = true
        };
        var protection = DataProtectionProvider.Create(new DirectoryInfo(directory.Path));
        var store = new DataProtectedRatingSecretStore(
            protection,
            directory.Path,
            () => configuration,
            (provider, protectedValue) => configuration =
                DataProtectedRatingSecretStore.CopyWithSecret(
                    configuration,
                    provider,
                    protectedValue));

        store.Set("mdblist", "mdb-super-secret");
        Assert.True(store.IsConfigured("mdblist"));
        Assert.Equal("mdb-super-secret", store.Get("mdblist"));
        Assert.DoesNotContain("mdb-super-secret", configuration.ProtectedMdbListApiKey);

        store.Set("mdblist", "replacement-secret");
        Assert.Equal("replacement-secret", store.Get("mdblist"));
        Assert.DoesNotContain("mdb-super-secret", configuration.ProtectedMdbListApiKey);
        Assert.DoesNotContain("replacement-secret", configuration.ProtectedMdbListApiKey);

        store.Remove("mdblist");
        Assert.False(store.IsConfigured("mdblist"));
        Assert.Null(store.Get("mdblist"));
        Assert.True(configuration.NativeCollections);
        Assert.Same(definition, Assert.Single(configuration.CuratedCollections));
    }

    [Fact]
    public void CuratedListRequestsKeepTheFullRankedItemPayload()
    {
        var path = MdbListHttpTransport.BuildListItemsPath(
            "key with +",
            "lists/snoak/imdb-top-250-movies/items");

        Assert.Equal(
            "lists/snoak/imdb-top-250-movies/items?limit=500&apikey=key%20with%20%2B",
            path);
        Assert.DoesNotContain("extended=ids_only", path, StringComparison.Ordinal);
    }

    [Fact]
    public async Task CuratedResolverRejectsNonemptyListsWithoutUsableTmdbIds()
    {
        var transport = new FakeTransport
        {
            ListItemsResponse = new MdbListResponse(
                HttpStatusCode.OK,
                JsonNode.Parse(
                    """
                    {
                      "movies": [{"tmdb": 238}],
                      "shows": [],
                      "pagination": {"total": 1}
                    }
                    """),
                new RatingQuotaResponse(null, null, null),
                null)
        };
        var secrets = new MemorySecretStore();
        secrets.Set("mdblist", "secret");
        var resolver = new CuratedCollectionResolver(transport, secrets);

        var exception = await Assert.ThrowsAsync<GatewayException>(() => resolver.ResolveAsync(
            string.Empty,
            "snoak/imdb-top-250-movies",
            CancellationToken.None));

        Assert.Equal(StatusCodes.Status502BadGateway, exception.StatusCode);
        Assert.Contains("without usable TMDB identities", exception.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void CapabilityAndAdminStatusAreAuthenticatedAndRedactSecrets()
    {
        AssertAuthorizeAttribute(typeof(RatingsController), null);
        AssertAuthorizeAttribute(typeof(InfoController), null);
        AssertAuthorizeAttribute(
            typeof(RatingsAdminController),
            MediaBrowser.Common.Api.Policies.RequiresElevation);

        using var fixture = new RatingsFixture();
        fixture.Secrets.Set("mdblist", "never-serialize-this-key");
        fixture.Cache.SetHealth("mdblist", ValidState());
        fixture.Secrets.Set("tmdb", "0123456789abcdef0123456789abcdef");
        fixture.Cache.SetHealth("tmdb", new ProviderHealthState
        {
            Validation = "saved",
            Valid = true,
            LastCheckedAt = 100
        });

        var capability = fixture.Service.Capability();
        Assert.True(capability.Available);
        Assert.True(capability.FallbackOnly);
        Assert.Equal(["local", "plugin", "none"], capability.CredentialPrecedence);
        Assert.Equal(1, capability.BoundaryVersion);
        var infoResult = new InfoController(new ServiceHealthStore(), fixture.Service).GetInfo();
        var infoJson = Assert.IsType<JsonResult>(infoResult.Result);
        var info = Assert.IsType<PluginInfoResponse>(infoJson.Value);
        Assert.Contains("ratings-v1", info.Capabilities);
        Assert.Same(capability.Sources, info.Ratings?.Sources);
        var json = JsonSerializer.Serialize(
            new
            {
                capability,
                admin = fixture.Service.AdminStatus()
            },
            CompanionJson.CamelCase);
        Assert.DoesNotContain("never-serialize-this-key", json);
        Assert.DoesNotContain("0123456789abcdef", json);
        Assert.DoesNotContain("apiKey", json, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("aspnet_data_protection", json);
        Assert.Contains("\"fallbackOnly\":true", json);
    }

    [Fact]
    public async Task SavingCredentialsReturnsValidationAndNonSecretQuotaFeedback()
    {
        using var fixture = new RatingsFixture();
        fixture.Transport.ValidateResponse = new MdbListResponse(
            HttpStatusCode.OK,
            JsonNode.Parse(
                """{"api_requests":1000,"api_requests_count":22,"rate_limit_reset":2000}"""),
            new RatingQuotaResponse(1000, 978, 2000),
            null);

        var status = await fixture.Service.SaveCredentialAsync(
            "mdblist",
            "administrator-key",
            CancellationToken.None);
        Assert.True(status.Mdblist.Configured);
        Assert.True(status.Mdblist.Valid);
        Assert.Equal("valid", status.Mdblist.Validation);
        Assert.Equal(978, status.Mdblist.Quota.Remaining);
        Assert.Equal(1, fixture.Transport.ValidateCalls);

        status = await fixture.Service.SaveCredentialAsync(
            "tmdb",
            "0123456789abcdef0123456789abcdef",
            CancellationToken.None);
        Assert.True(status.Tmdb.Valid);
        Assert.True(status.Tmdb.PreparationOnly);
        Assert.False(status.Tmdb.UsedForRatings);
        Assert.Equal(1, fixture.Transport.ValidateCalls);

        var error = await Assert.ThrowsAsync<RatingRequestException>(() =>
            fixture.Service.SaveCredentialAsync(
                "tmdb",
                "not-a-key",
                CancellationToken.None));
        Assert.Contains("32-character", error.Message);
    }

    [Fact]
    public void BatchValidationPinsVersionBoundsAndStableIdentifiers()
    {
        var valid = Target("item", "tmdb", "603");
        Assert.Single(RatingsContract.Validate(new RatingBatchRequest(1, [valid])));
        Assert.Throws<RatingContractVersionException>(() =>
            RatingsContract.Validate(new RatingBatchRequest(2, [valid])));
        Assert.Throws<RatingRequestException>(() => RatingsContract.Validate(
            new RatingBatchRequest(1, Enumerable.Repeat(valid, 501).ToArray())));
        Assert.Throws<RatingRequestException>(() => RatingsContract.Validate(
            new RatingBatchRequest(1, [valid with { ProviderId = "http://internal/" }])));
        Assert.Throws<RatingRequestException>(() => RatingsContract.Validate(
            new RatingBatchRequest(1, [valid with { Provider = "custom" }])));
        Assert.Throws<RatingRequestException>(() => RatingsContract.Validate(
            new RatingBatchRequest(1, [valid, valid])));
    }

    [Fact]
    public void EveryCurrentSourceIsNormalizedFromTheFixedPublicCatalog()
    {
        var normalized = RatingsContract.NormalizeMedia(JsonNode.Parse(
            """
            {
              "score":84,"score_average":81,
              "ratings":[
                {"source":"imdb","value":8.1,"score":81,"votes":10},
                {"source":"tmdb","value":76,"score":76},
                {"source":"letterboxd","value":8,"score":80},
                {"source":"tomatoes","value":97,"score":97},
                {"source":"audience","value":91,"score":91},
                {"source":"metacritic","value":87},
                {"source":"metacriticuser","value":8.4},
                {"source":"rogerebert","value":4},
                {"source":"mal","value":8.7},
                {"source":"trakt","value":78},
                {"source":"future-meter!","value":7.25,"score":73},
                {"source":"missing","value":null,"score":null}
              ]
            }
            """)!);
        var bySource = normalized
            .OfType<JsonObject>()
            .ToDictionary(value => value["sourceId"]!.GetValue<string>());
        Assert.Equal(4, bySource["letterboxd"]["value"]!.GetValue<double>());
        Assert.Equal(7.6, bySource["tmdb"]["value"]!.GetValue<double>(), 3);
        Assert.Equal(97, bySource["tomatoes"]["value"]!.GetValue<double>());
        Assert.Equal(91, bySource["popcorn"]["value"]!.GetValue<double>());
        Assert.DoesNotContain("future_meter", bySource.Keys);
        Assert.DoesNotContain("missing", bySource.Keys);
        Assert.All(bySource.Values, value =>
        {
            Assert.Equal(value["sourceId"]!.GetValue<string>(), value["source"]!.GetValue<string>());
            Assert.Equal(value["sourceId"]!.GetValue<string>(), value["rawSource"]!.GetValue<string>());
        });
        Assert.Contains("mdblist_score", bySource.Keys);
        Assert.Contains("mdblist_score_average", bySource.Keys);

        var catalog = RatingsContract.SourceCatalog.Select(source => source.Id).ToHashSet();
        Assert.Contains("letterboxd", catalog);
        Assert.Contains("tomatoes", catalog);
        Assert.Contains("popcorn", catalog);
    }

    [Fact]
    public async Task UpstreamAndCachePayloadsCannotReflectServerSecrets()
    {
        const string mdbListKey = "mdblist-server-key-never-leaves-the-plugin";
        const string tmdbKey = "0123456789abcdef0123456789abcdef";
        using var fixture = new RatingsFixture();
        fixture.Secrets.Set("mdblist", mdbListKey);
        fixture.Secrets.Set("tmdb", tmdbKey);
        fixture.Cache.SetHealth("mdblist", ValidState() with { Detail = mdbListKey });
        fixture.Cache.SetHealth("tmdb", ValidState() with { Detail = tmdbKey });

        var cachedTarget = Target("cached-card", "tmdb", "604");
        var now = DateTimeOffset.UtcNow.ToUnixTimeSeconds();
        fixture.Cache.Upsert([(cachedTarget, new CachedRatingEntry(
            "tmdb",
            "604",
            "movie",
            new JsonArray(new JsonObject
            {
                ["sourceId"] = mdbListKey,
                ["rawSource"] = tmdbKey,
                ["value"] = 99,
                ["diagnostic"] = mdbListKey
            }),
            tmdbKey,
            now,
            now + 60,
            now + 600))]);
        fixture.Transport.BatchResponse = new MdbListResponse(
            HttpStatusCode.OK,
            JsonNode.Parse(
                $$"""
                [{
                  "ids":{"tmdb":603},
                  "updated":"{{mdbListKey}}",
                  "error":"{{tmdbKey}}",
                  "ratings":[
                    {"source":"{{mdbListKey}}","value":99,"trace":"{{tmdbKey}}"},
                    {"source":"imdb","value":8.7,"score":87,"votes":100,"rawSource":"{{mdbListKey}}"}
                  ]
                }]
                """),
            new RatingQuotaResponse(1000, 999, null),
            null);

        var response = await fixture.Service.BatchAsync(
            new RatingBatchRequest(1, [Target("upstream-card", "tmdb", "603"), cachedTarget]),
            CancellationToken.None);
        Assert.Equal(mdbListKey, fixture.Transport.LastBatchApiKey);
        fixture.Transport.BatchResponse = new MdbListResponse(
            HttpStatusCode.BadGateway,
            JsonNode.Parse($$"""{"error":"{{mdbListKey}}","trace":"{{tmdbKey}}"}"""),
            new RatingQuotaResponse(null, null, null),
            null);
        var errorResponse = await fixture.Service.BatchAsync(
            new RatingBatchRequest(1, [Target("error-card", "tmdb", "605")]),
            CancellationToken.None);
        var desktopVisible = JsonSerializer.Serialize(
            new
            {
                capability = fixture.Service.Capability(),
                admin = fixture.Service.AdminStatus(),
                batch = new[] { response, errorResponse },
                diagnostic = errorResponse.Diagnostic
            },
            CompanionJson.CamelCase);
        var persistedCache = File.ReadAllText(fixture.CachePath);

        Assert.DoesNotContain(mdbListKey, desktopVisible);
        Assert.DoesNotContain(tmdbKey, desktopVisible);
        Assert.DoesNotContain(mdbListKey, persistedCache);
        Assert.DoesNotContain(tmdbKey, persistedCache);
        var upstream = response.Items.Single(item => item.ItemId == "upstream-card");
        var rating = Assert.Single(upstream.Ratings.OfType<JsonObject>());
        Assert.Equal("imdb", rating["sourceId"]!.GetValue<string>());
        Assert.Equal("imdb", rating["rawSource"]!.GetValue<string>());
        var cached = response.Items.Single(item => item.ItemId == "cached-card");
        Assert.Empty(cached.Ratings);
    }

    [Fact]
    public async Task LargeRequestsUseBoundedUpstreamBatchesRatherThanPerCardCalls()
    {
        using var fixture = new RatingsFixture();
        fixture.ConfigureValidKey();
        fixture.Transport.BatchResponse = new MdbListResponse(
            HttpStatusCode.OK,
            new JsonArray(),
            new RatingQuotaResponse(1000, 999, null),
            null);
        var targets = Enumerable.Range(1, 205)
            .Select(id => Target("card-" + id, "tmdb", id.ToString()))
            .ToArray();

        var response = await fixture.Service.BatchAsync(
            new RatingBatchRequest(1, targets),
            CancellationToken.None);
        Assert.Equal(3, fixture.Transport.BatchCalls);
        Assert.Equal(100, fixture.Transport.MaxBatchSizeObserved);
        Assert.Equal(205, response.Items.Count);
        Assert.All(response.Items, item => Assert.Empty(item.Ratings));
    }

    [Fact]
    public async Task ConcurrentUsersShareOneStableIdentityFetchAndDurableCache()
    {
        using var fixture = new RatingsFixture();
        fixture.ConfigureValidKey();
        fixture.Transport.BatchDelay = TimeSpan.FromMilliseconds(75);
        fixture.Transport.BatchResponse = MediaResponse(603);
        var first = fixture.Service.BatchAsync(
            new RatingBatchRequest(1, [Target("card-a", "tmdb", "603")]),
            CancellationToken.None);
        var second = fixture.Service.BatchAsync(
            new RatingBatchRequest(1, [Target("card-b", "tmdb", "603")]),
            CancellationToken.None);

        var responses = await Task.WhenAll(first, second);
        Assert.Equal(1, fixture.Transport.BatchCalls);
        Assert.All(responses, response => Assert.Single(response.Items));
        Assert.All(responses, response => Assert.Equal(
            "server_mdblist",
            response.Items[0].Origin));

        var reloaded = new RatingsCacheStore(fixture.CachePath);
        Assert.Equal(1, reloaded.Count);
        Assert.NotNull(reloaded.GetStable(Target("different-card", "tmdb", "603")));
    }

    [Fact]
    public async Task StaleWhileRevalidateReturnsImmediatelyAndDeduplicatesBackgroundWork()
    {
        using var fixture = new RatingsFixture();
        fixture.ConfigureValidKey();
        var now = DateTimeOffset.UtcNow.ToUnixTimeSeconds();
        var target = Target("stale-card", "tmdb", "603");
        fixture.Cache.Upsert([(target, new CachedRatingEntry(
            "tmdb",
            "603",
            "movie",
            new JsonArray(new JsonObject
            {
                ["sourceId"] = "imdb",
                ["value"] = 8.0
            }),
            "2026-08-01T00:00:00Z",
            now - 100,
            now - 1,
            now + 1000))]);
        fixture.Transport.BatchDelay = TimeSpan.FromMilliseconds(200);
        fixture.Transport.BatchResponse = MediaResponse(603);

        var started = DateTimeOffset.UtcNow;
        var first = await fixture.Service.BatchAsync(
            new RatingBatchRequest(1, [target]),
            CancellationToken.None);
        var second = await fixture.Service.BatchAsync(
            new RatingBatchRequest(1, [target with { ItemId = "second-card" }]),
            CancellationToken.None);
        var elapsed = DateTimeOffset.UtcNow - started;

        Assert.True(elapsed < TimeSpan.FromMilliseconds(150));
        Assert.True(Assert.Single(first.Items).Stale);
        Assert.True(Assert.Single(second.Items).Stale);
        await fixture.Transport.BatchStarted.Task.WaitAsync(
            TimeSpan.FromSeconds(1),
            TestContext.Current.CancellationToken);
        await Task.Delay(300, TestContext.Current.CancellationToken);
        Assert.Equal(1, fixture.Transport.BatchCalls);
    }

    [Fact]
    public async Task QuotaBackoffIsHonoredAcrossRequestsAndRestarts()
    {
        using var fixture = new RatingsFixture();
        fixture.ConfigureValidKey();
        var retryAt = DateTimeOffset.UtcNow.AddHours(1).ToUnixTimeSeconds();
        fixture.Transport.BatchResponse = new MdbListResponse(
            HttpStatusCode.TooManyRequests,
            null,
            new RatingQuotaResponse(1000, 0, retryAt),
            retryAt);

        var request = new RatingBatchRequest(1, [Target("card", "tmdb", "603")]);
        var first = await fixture.Service.BatchAsync(request, CancellationToken.None);
        var second = await fixture.Service.BatchAsync(request, CancellationToken.None);
        Assert.Empty(first.Items);
        Assert.Empty(second.Items);
        Assert.Equal(1, fixture.Transport.BatchCalls);
        Assert.Equal(retryAt, second.RetryAt);
        Assert.Equal(0, second.Quota.Remaining);

        var reloadedCache = new RatingsCacheStore(fixture.CachePath);
        var replacementTransport = new FakeTransport { BatchResponse = MediaResponse(603) };
        using var restarted = new RatingsService(
            reloadedCache,
            fixture.Secrets,
            replacementTransport);
        var afterRestart = await restarted.BatchAsync(request, CancellationToken.None);
        Assert.Empty(afterRestart.Items);
        Assert.Equal(0, replacementTransport.BatchCalls);
        Assert.Equal(retryAt, afterRestart.RetryAt);
    }

    [Fact]
    public async Task UpstreamErrorsDegradeToOptionalEmptyRatingData()
    {
        using var fixture = new RatingsFixture();
        fixture.ConfigureValidKey();
        fixture.Transport.BatchResponse = new MdbListResponse(
            HttpStatusCode.BadGateway,
            null,
            new RatingQuotaResponse(null, null, null),
            null);

        var response = await fixture.Service.BatchAsync(
            new RatingBatchRequest(1, [Target("card", "tmdb", "603")]),
            CancellationToken.None);
        Assert.Empty(response.Items);
        Assert.NotNull(response.Diagnostic);
        Assert.Contains("temporarily unavailable", response.Diagnostic);
        Assert.True(fixture.Service.Capability().Valid);
    }

    private static void AssertAuthorizeAttribute(Type type, string? expectedPolicy)
    {
        var attribute = type.GetCustomAttribute<AuthorizeAttribute>();
        Assert.NotNull(attribute);
        Assert.Equal(expectedPolicy, attribute.Policy);
    }

    private static RatingTargetRequest Target(string itemId, string provider, string providerId)
        => new(itemId, "Movie", "movie", provider, providerId);

    private static ProviderHealthState ValidState()
        => new()
        {
            Validation = "valid",
            Valid = true,
            Detail = "Valid MDBList credential.",
            QuotaLimit = 1000,
            QuotaRemaining = 900,
            QuotaResetAt = DateTimeOffset.UtcNow.AddDays(1).ToUnixTimeSeconds(),
            LastCheckedAt = DateTimeOffset.UtcNow.ToUnixTimeSeconds()
        };

    private static MdbListResponse MediaResponse(long tmdbId)
        => new(
            HttpStatusCode.OK,
            JsonNode.Parse(
                $$"""
                [{
                  "ids":{"tmdb":{{tmdbId}}},
                  "updated":"2026-08-04T20:00:00Z",
                  "score":84,
                  "ratings":[
                    {"source":"imdb","value":8.7,"score":87,"votes":100},
                    {"source":"letterboxd","value":8,"score":80},
                    {"source":"tomatoes","value":97,"score":97},
                    {"source":"audience","value":91,"score":91}
                  ]
                }]
                """),
            new RatingQuotaResponse(1000, 899, DateTimeOffset.UtcNow.AddDays(1).ToUnixTimeSeconds()),
            null);

    private sealed class RatingsFixture : IDisposable
    {
        private readonly TemporaryDirectory _directory = new();

        public RatingsFixture()
        {
            CachePath = System.IO.Path.Combine(_directory.Path, "ratings.json");
            Cache = new RatingsCacheStore(CachePath);
            Service = new RatingsService(Cache, Secrets, Transport);
        }

        public string CachePath { get; }

        public RatingsCacheStore Cache { get; }

        public MemorySecretStore Secrets { get; } = new();

        public FakeTransport Transport { get; } = new();

        public RatingsService Service { get; }

        public void ConfigureValidKey()
        {
            Secrets.Set("mdblist", "test-key");
            Cache.SetHealth("mdblist", ValidState());
        }

        public void Dispose()
        {
            Service.Dispose();
            _directory.Dispose();
        }
    }

    private sealed class MemorySecretStore : IRatingSecretStore
    {
        private readonly Dictionary<string, string> _values =
            new(StringComparer.OrdinalIgnoreCase);

        public bool IsConfigured(string provider) => _values.ContainsKey(provider);

        public string? Get(string provider) => _values.GetValueOrDefault(provider);

        public void Set(string provider, string secret) => _values[provider] = secret;

        public void Remove(string provider) => _values.Remove(provider);
    }

    private sealed class FakeTransport : IMdbListTransport
    {
        private int _batchCalls;
        private int _validateCalls;
        private int _maxBatchSizeObserved;

        public int BatchCalls => _batchCalls;

        public int ValidateCalls => _validateCalls;

        public int MaxBatchSizeObserved => _maxBatchSizeObserved;

        public string? LastBatchApiKey { get; private set; }

        public TimeSpan BatchDelay { get; set; }

        public TaskCompletionSource BatchStarted { get; } =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public MdbListResponse ValidateResponse { get; set; } = new(
            HttpStatusCode.OK,
            JsonNode.Parse("""{"api_requests":1000,"api_requests_count":1}"""),
            new RatingQuotaResponse(1000, 999, null),
            null);

        public MdbListResponse BatchResponse { get; set; } = MediaResponse(603);

        public Task<MdbListResponse> ValidateAsync(
            string apiKey,
            CancellationToken cancellationToken)
        {
            Interlocked.Increment(ref _validateCalls);
            return Task.FromResult(ValidateResponse);
        }

        public async Task<MdbListResponse> BatchAsync(
            string apiKey,
            string provider,
            string mediaType,
            IReadOnlyList<string> ids,
            CancellationToken cancellationToken)
        {
            LastBatchApiKey = apiKey;
            Interlocked.Increment(ref _batchCalls);
            InterlockedExtensions.Max(ref _maxBatchSizeObserved, ids.Count);
            BatchStarted.TrySetResult();
            if (BatchDelay > TimeSpan.Zero)
            {
                await Task.Delay(BatchDelay, cancellationToken);
            }

            return BatchResponse;
        }

        public Task<MdbListResponse> ListItemsAsync(
            string apiKey,
            string resource,
            CancellationToken cancellationToken)
        {
            LastListResource = resource;
            return Task.FromResult(ListItemsResponse ?? BatchResponse);
        }

        public string? LastListResource { get; private set; }

        public MdbListResponse? ListItemsResponse { get; set; }
    }

    private static class InterlockedExtensions
    {
        public static void Max(ref int location, int value)
        {
            var current = Volatile.Read(ref location);
            while (current < value)
            {
                var observed = Interlocked.CompareExchange(ref location, value, current);
                if (observed == current)
                {
                    return;
                }

                current = observed;
            }
        }
    }

    private sealed class TemporaryDirectory : IDisposable
    {
        public TemporaryDirectory()
        {
            Path = System.IO.Path.Combine(
                System.IO.Path.GetTempPath(),
                "mediaflick-ratings-tests-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(Path);
        }

        public string Path { get; }

        public void Dispose()
        {
            try
            {
                Directory.Delete(Path, true);
            }
            catch (IOException)
            {
            }
            catch (UnauthorizedAccessException)
            {
            }
        }
    }
}
