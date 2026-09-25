using System.Net;
using System.Text;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Jellyfin.Plugin.MediaFlick.Services;
using Microsoft.Extensions.Logging;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class CompanionLoggingTests
{
    private const string ServiceHost = "seerr.internal.example";
    private const string ApiKey = "seerr-log-secret-4d81e2";

    [Fact]
    public async Task UpstreamOutagesWarnOnceAndNeverLogAddressesKeysOrBodies()
    {
        var handler = new StubHandler();
        var logger = new CapturingLogger<CompanionHttpClient>();
        var client = new CompanionHttpClient(
            new StubFactory(handler),
            new ServiceHealthStore(),
            logger);
        var service = new ServiceConfiguration
        {
            Enabled = true,
            BaseUrl = $"http://{ServiceHost}:5055",
            ApiKey = ApiKey
        };

        handler.Respond = request => throw new HttpRequestException(
            HttpRequestError.ConnectionError,
            $"Connection refused ({ServiceHost}:5055) {request.RequestUri}?apikey={ApiKey}");
        await Assert.ThrowsAsync<GatewayException>(() => SendAsync(client, service));
        await Assert.ThrowsAsync<GatewayException>(() => SendAsync(client, service));

        handler.Respond = _ => Json(HttpStatusCode.OK, """{"version":"1"}""");
        await SendAsync(client, service);

        handler.Respond = _ => Json(
            HttpStatusCode.Unauthorized,
            $$"""{"message":"key {{ApiKey}} is not valid"}""");
        await Assert.ThrowsAsync<GatewayException>(() => SendAsync(client, service));

        var entries = logger.Entries;
        Assert.Equal(
            [LogLevel.Warning, LogLevel.Information, LogLevel.Warning],
            entries.Select(entry => entry.Level).Where(level => level >= LogLevel.Information));
        Assert.All(entries, entry =>
        {
            Assert.Null(entry.Exception);
            Assert.DoesNotContain(ServiceHost, entry.Rendered, StringComparison.Ordinal);
            Assert.DoesNotContain(ApiKey, entry.Rendered, StringComparison.Ordinal);
        });
    }

    [Fact]
    public async Task RepeatedNonJsonSuccessResponsesWarnOnceWithoutRecoveryNoise()
    {
        var handler = new StubHandler
        {
            Respond = _ => new HttpResponseMessage(HttpStatusCode.OK)
            {
                Content = new StringContent("<html>proxy login</html>", Encoding.UTF8, "text/html")
            }
        };
        var logger = new CapturingLogger<CompanionHttpClient>();
        var client = new CompanionHttpClient(
            new StubFactory(handler),
            new ServiceHealthStore(),
            logger);
        var service = new ServiceConfiguration
        {
            Enabled = true,
            BaseUrl = $"http://{ServiceHost}:5055",
            ApiKey = ApiKey
        };

        for (var attempt = 0; attempt < 3; attempt++)
        {
            await Assert.ThrowsAsync<GatewayException>(() => SendAsync(client, service));
        }

        handler.Respond = _ => Json(HttpStatusCode.OK, """{"version":"1"}""");
        await SendAsync(client, service);

        Assert.Equal(
            [LogLevel.Warning, LogLevel.Information],
            logger.Entries.Select(entry => entry.Level).Where(level => level >= LogLevel.Information));
    }

    [Fact]
    public void RatingsCachePersistenceFailureWarnsOncePerOutage()
    {
        var directory = Path.Combine(
            Path.GetTempPath(),
            "mediaflick-logging-tests-" + Guid.NewGuid().ToString("N"));
        // A directory at the cache path makes every atomic replace fail.
        var cachePath = Path.Combine(directory, "ratings.json");
        Directory.CreateDirectory(cachePath);
        try
        {
            var logger = new CapturingLogger<ProviderCacheStore>();
            using var store = new ProviderCacheStore(cachePath, logger);

            store.SetHealth("mdblist", new ProviderHealthState { Validation = "valid", Valid = true });
            store.Flush();
            store.SetHealth("tmdb", new ProviderHealthState { Validation = "valid", Valid = true });
            store.Flush();

            Assert.Equal(
                [LogLevel.Warning],
                logger.Entries.Select(entry => entry.Level).Where(level => level >= LogLevel.Information));
            Assert.True(store.Health("mdblist").Valid);
        }
        finally
        {
            Directory.Delete(directory, true);
        }
    }

    private static Task<System.Text.Json.Nodes.JsonNode?> SendAsync(
        CompanionHttpClient client,
        ServiceConfiguration service)
        => client.SendAsync(
            "seerr",
            service,
            HttpMethod.Get,
            "api/v1/status",
            null,
            null,
            TestContext.Current.CancellationToken);

    private static HttpResponseMessage Json(HttpStatusCode status, string body)
        => StubHandler.Json(status, body);
}
