using System.Net;
using System.Text;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Jellyfin.Plugin.MediaFlick.Models;
using Jellyfin.Plugin.MediaFlick.Services;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class CacheBoundsTests : IDisposable
{
    private static readonly DateTimeOffset Start = new(2026, 9, 23, 12, 0, 0, TimeSpan.Zero);
    private readonly string _directory = Path.Combine(
        Path.GetTempPath(),
        "mediaflick-cache-bounds-" + Guid.NewGuid().ToString("N"));

    public CacheBoundsTests()
    {
        Directory.CreateDirectory(_directory);
    }

    [Fact]
    public void BoundedCachesExpireEntriesAndEvictTheOldestPastTheCap()
    {
        var time = new ManualTime(Start);
        var cache = new BoundedCache<string, int>(10, TimeSpan.FromMinutes(5), time);

        cache.Set("expiring", 0);
        time.Advance(TimeSpan.FromMinutes(5));
        Assert.False(cache.TryGet("expiring", out _));

        for (var index = 0; index < 11; index++)
        {
            cache.Set("key-" + index, index);
            time.Advance(TimeSpan.FromSeconds(1));
        }

        Assert.True(cache.Count <= 10);
        Assert.False(cache.TryGet("key-0", out _));
        Assert.True(cache.TryGet("key-10", out var newest));
        Assert.Equal(10, newest);
    }

    [Fact]
    public async Task CollectionResultsAreRefetchedOnceTheirCacheLifetimeEnds()
    {
        var time = new ManualTime(Start);
        var tmdb = new CountingTmdb();
        var secrets = new Secrets();
        secrets.Set("tmdb", "0123456789abcdef0123456789abcdef");
        using var state = Store(time);
        state.SetHealth("tmdb", new ProviderHealthState { Validation = "valid", Valid = true });
        var service = new CollectionProviderService(
            tmdb,
            new NoMdbList(),
            secrets,
            state,
            NullLogger<CollectionProviderService>.Instance,
            timeProvider: time);
        var request = new CollectionProviderRequest(
            new JsonObject { ["kind"] = "tmdbDiscover", ["parameters"] = new JsonObject() },
            "movie",
            new CollectionResultLimit("all", null));

        await service.ResultsAsync(request, TestContext.Current.CancellationToken);
        time.Advance(CollectionProviderService.CacheLifetime - TimeSpan.FromMinutes(1));
        await service.ResultsAsync(request, TestContext.Current.CancellationToken);
        Assert.Equal(1, tmdb.DiscoverCalls);

        time.Advance(TimeSpan.FromMinutes(1));
        await service.ResultsAsync(request, TestContext.Current.CancellationToken);
        Assert.Equal(2, tmdb.DiscoverCalls);
    }

    [Fact]
    public async Task CacheChangesAreBatchedIntoOneLaterSave()
    {
        var path = Path.Combine(_directory, "batched.json");
        using var store = new ProviderCacheStore(
            path,
            NullLogger<ProviderCacheStore>.Instance,
            saveDelay: TimeSpan.FromMilliseconds(100));

        store.Upsert([Entry("603")]);
        store.Upsert([Entry("604")]);
        store.SetHealth("mdblist", new ProviderHealthState { Validation = "valid", Valid = true });
        Assert.False(File.Exists(path));

        for (var attempt = 0; attempt < 100 && !File.Exists(path); attempt++)
        {
            await Task.Delay(20, TestContext.Current.CancellationToken);
        }

        using var reloaded = new ProviderCacheStore(path, NullLogger<ProviderCacheStore>.Instance);
        Assert.Equal(2, reloaded.Count);
        Assert.True(reloaded.Health("mdblist").Valid);
    }

    [Fact]
    public void ShutdownWritesPendingChangesAndDropsExpiredEntries()
    {
        var path = Path.Combine(_directory, "shutdown.json");
        var time = new ManualTime(Start);
        var store = new ProviderCacheStore(
            path,
            NullLogger<ProviderCacheStore>.Instance,
            time,
            TimeSpan.FromHours(1));
        var now = Start.ToUnixTimeSeconds();
        store.Upsert([Entry("603", now + 60), Entry("604", now + 3600)]);
        Assert.False(File.Exists(path));

        time.Advance(TimeSpan.FromMinutes(2));
        store.Dispose();

        using var reloaded = new ProviderCacheStore(path, NullLogger<ProviderCacheStore>.Instance, time);
        Assert.Equal(1, reloaded.Count);
        Assert.Null(reloaded.GetStable(Target("603")));
        Assert.NotNull(reloaded.GetStable(Target("604")));
    }

    [Fact]
    public async Task OversizedServiceResponsesFailSafelyWithoutBufferingThem()
    {
        var health = new ServiceHealthStore();
        var handler = new StubHandler();
        var client = new CompanionHttpClient(
            new StubFactory(handler),
            health,
            NullLogger<CompanionHttpClient>.Instance,
            64);

        handler.Respond = _ => StubHandler.Json(HttpStatusCode.OK, $$"""{"padding":"{{new string('x', 100)}}"}""");
        var declared = await Assert.ThrowsAsync<GatewayException>(() => Send(client));
        handler.Respond = _ => new HttpResponseMessage(HttpStatusCode.OK)
        {
            Content = new StreamContent(new UnknownLengthStream(new byte[200]))
        };
        var streamed = await Assert.ThrowsAsync<GatewayException>(() => Send(client));

        Assert.All([declared, streamed], error =>
            Assert.Equal(StatusCodes.Status502BadGateway, error.StatusCode));
        Assert.False(health.IsHealthy("sonarr"));

        handler.Respond = _ => new HttpResponseMessage(HttpStatusCode.OK)
        {
            Content = new ByteArrayContent([0xEF, 0xBB, 0xBF, .. Encoding.UTF8.GetBytes("""{"ok":true}""")])
        };
        var parsed = await Send(client);
        Assert.True(parsed?["ok"]?.GetValue<bool>());
        Assert.True(health.IsHealthy("sonarr"));
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

    private static Task<JsonNode?> Send(CompanionHttpClient client)
        => client.SendAsync(
            "sonarr",
            new ServiceConfiguration
            {
                Enabled = true,
                BaseUrl = "http://sonarr.internal.example:8989",
                ApiKey = "key"
            },
            HttpMethod.Get,
            "api/v3/calendar",
            null,
            null,
            TestContext.Current.CancellationToken);

    private ProviderCacheStore Store(TimeProvider time)
        => new(
            Path.Combine(_directory, Guid.NewGuid().ToString("N") + ".json"),
            NullLogger<ProviderCacheStore>.Instance,
            time);

    private static RatingTargetRequest Target(string tmdbId)
        => new("card-" + tmdbId, "Movie", "movie", "tmdb", tmdbId);

    private static (RatingTargetRequest, CachedRatingEntry) Entry(string tmdbId, long? expiresAt = null)
    {
        var now = DateTimeOffset.UtcNow.ToUnixTimeSeconds();
        return (Target(tmdbId), new CachedRatingEntry(
            "tmdb",
            tmdbId,
            "movie",
            [],
            null,
            now,
            now + 60,
            expiresAt ?? now + 600));
    }

    /// <summary>A body whose length is only known by reading it.</summary>
    private sealed class UnknownLengthStream(byte[] content) : MemoryStream(content)
    {
        public override bool CanSeek => false;
    }

    private sealed class Secrets : IProviderSecretStore
    {
        private readonly Dictionary<string, string> _values = new(StringComparer.OrdinalIgnoreCase);

        public bool IsConfigured(string provider) => _values.ContainsKey(provider);

        public string? Get(string provider) => _values.GetValueOrDefault(provider);

        public void Set(string provider, string secret) => _values[provider] = secret;

        public void Remove(string provider) => _values.Remove(provider);
    }

    private sealed class CountingTmdb : ITmdbTransport
    {
        public int DiscoverCalls { get; private set; }

        public Task<TmdbResponse> GetAsync(
            string credential,
            string path,
            IReadOnlyDictionary<string, string> query,
            CancellationToken cancellationToken)
        {
            if (path == "3/discover/movie")
            {
                DiscoverCalls += 1;
                return Task.FromResult(new TmdbResponse(
                    HttpStatusCode.OK,
                    new JsonObject
                    {
                        ["total_results"] = 1,
                        ["total_pages"] = 1,
                        ["results"] = new JsonArray(new JsonObject { ["id"] = 1, ["title"] = "One" })
                    },
                    null));
            }

            return Task.FromResult(new TmdbResponse(HttpStatusCode.OK, new JsonObject(), null));
        }

        public Task<ArtworkResponse> GetArtworkAsync(
            string size,
            string path,
            CancellationToken cancellationToken)
            => Task.FromResult(new ArtworkResponse(HttpStatusCode.NotFound, [], "application/octet-stream"));
    }

    private sealed class NoMdbList : IMdbListTransport
    {
        private static readonly MdbListResponse NotFound =
            new(HttpStatusCode.NotFound, null, new RatingQuotaResponse(null, null, null), null);

        public Task<MdbListResponse> ValidateAsync(string apiKey, CancellationToken cancellationToken)
            => Task.FromResult(NotFound);

        public Task<MdbListResponse> BatchAsync(
            string apiKey,
            string provider,
            string mediaType,
            IReadOnlyList<string> ids,
            CancellationToken cancellationToken)
            => Task.FromResult(NotFound);

        public Task<MdbListResponse> ListItemsAsync(
            string apiKey,
            string resource,
            CancellationToken cancellationToken)
            => Task.FromResult(NotFound);
    }
}
