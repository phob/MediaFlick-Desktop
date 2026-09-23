using System.Net;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Jellyfin.Plugin.MediaFlick.Services;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

/// <summary>
/// Upstream text never reaches Desktop or health state, and malformed input
/// fails as a request error or a safely ignored value instead of a 500.
/// </summary>
public sealed class UpstreamInputTests
{
    private const string ServiceHost = "sonarr.internal.example";
    private const string ApiKey = "sonarr-upstream-secret-19ad7c";

    public static TheoryData<HttpStatusCode, bool, int> UpstreamFailures => new()
    {
        { HttpStatusCode.Unauthorized, false, StatusCodes.Status401Unauthorized },
        { HttpStatusCode.Forbidden, true, StatusCodes.Status403Forbidden },
        { HttpStatusCode.NotFound, true, StatusCodes.Status404NotFound },
        { HttpStatusCode.Conflict, true, StatusCodes.Status409Conflict },
        { HttpStatusCode.BadRequest, true, StatusCodes.Status400BadRequest },
        { HttpStatusCode.InternalServerError, false, StatusCodes.Status500InternalServerError }
    };

    [Theory]
    [MemberData(nameof(UpstreamFailures))]
    public async Task UpstreamErrorBodiesNeverReachTheGatewayMessage(
        HttpStatusCode status,
        bool mappedUser,
        int expectedStatus)
    {
        var handler = new StubHandler
        {
            Respond = _ => StubHandler.Json(
                status,
                $$"""{"message":"http://{{ServiceHost}}:8989 rejected {{ApiKey}}","error":"{{ApiKey}}"}""")
        };
        var health = new ServiceHealthStore();
        var client = new CompanionHttpClient(
            new StubFactory(handler),
            health,
            NullLogger<CompanionHttpClient>.Instance);

        var error = await Assert.ThrowsAsync<GatewayException>(() => client.SendAsync(
            "seerr",
            Service(),
            HttpMethod.Get,
            "api/v1/request",
            null,
            mappedUser ? 7 : null,
            TestContext.Current.CancellationToken));

        Assert.Equal(expectedStatus, error.StatusCode);
        Assert.StartsWith("Seerr ", error.Message, StringComparison.Ordinal);
        Assert.DoesNotContain(ServiceHost, error.Message, StringComparison.Ordinal);
        Assert.DoesNotContain(ApiKey, error.Message, StringComparison.Ordinal);
        Assert.NotNull(error.Failure);
        Assert.Equal(error.Failure, health.Get("seerr").Failure);
    }

    [Fact]
    public async Task TransportExceptionTextNeverEntersHealthState()
    {
        var handler = new StubHandler
        {
            Respond = _ => throw new HttpRequestException(
                HttpRequestError.NameResolutionError,
                $"No such host is known ({ServiceHost}:8989)")
        };
        var health = new ServiceHealthStore();
        var client = new CompanionHttpClient(
            new StubFactory(handler),
            health,
            NullLogger<CompanionHttpClient>.Instance);

        var error = await Assert.ThrowsAsync<GatewayException>(() => client.SendAsync(
            "sonarr",
            Service(),
            HttpMethod.Get,
            "api/v3/calendar",
            null,
            null,
            TestContext.Current.CancellationToken));

        Assert.Equal(StatusCodes.Status502BadGateway, error.StatusCode);
        Assert.Equal("could not reach Sonarr", error.Message);
        Assert.Equal(ServiceFailure.Unreachable, error.Failure);
        var record = health.Get("sonarr");
        Assert.Equal(ServiceHealthStore.ServiceHealthState.Unhealthy, record.State);
        Assert.Equal(ServiceFailure.Unreachable, record.Failure);
        Assert.DoesNotContain(ServiceHost, record.ToString(), StringComparison.Ordinal);
    }

    [Fact]
    public async Task ConnectionTestsIgnoreAWronglyTypedVersion()
    {
        var handler = new StubHandler
        {
            Respond = _ => StubHandler.Json(HttpStatusCode.OK, """{"version":{"major":4}}""")
        };
        var client = new CompanionHttpClient(
            new StubFactory(handler),
            new ServiceHealthStore(),
            NullLogger<CompanionHttpClient>.Instance);

        Assert.Null(await client.TestAsync("sonarr", Service(), TestContext.Current.CancellationToken));
    }

    [Fact]
    public void CalendarSourceErrorsAreFixedReasons()
    {
        Assert.Equal("unreachable", CalendarService.FailureReason(ServiceFailure.Unreachable));
        Assert.Equal("API key rejected", CalendarService.FailureReason(ServiceFailure.Rejected));
        Assert.Equal("timed out", CalendarService.FailureReason(ServiceFailure.Timeout));
        Assert.Equal("not configured", CalendarService.FailureReason(ServiceFailure.NotConfigured));
        Assert.Equal("unavailable", CalendarService.FailureReason(ServiceFailure.RequestFailed));
    }

    [Theory]
    [InlineData("true", true)]
    [InlineData("false", false)]
    [InlineData("1", true)]
    [InlineData("0", false)]
    [InlineData("\"TRUE\"", true)]
    [InlineData("\"0\"", false)]
    [InlineData("\"yes\"", null)]
    [InlineData("null", null)]
    [InlineData("{}", null)]
    [InlineData("[]", null)]
    public void FlagsAcceptProviderEncodingsAndIgnoreEverythingElse(string json, bool? expected)
    {
        var node = JsonNode.Parse($$"""{"value":{{json}}}""")!["value"];
        Assert.Equal(expected, JsonRead.Flag(node));
    }

    [Theory]
    [InlineData("""{"has_more":"yes","pagination":"page 2"}""", false)]
    [InlineData("""{"has_more":{"value":true},"pagination":[1]}""", false)]
    [InlineData("""{"has_more":1}""", true)]
    [InlineData("""{"pagination":{"has_more":"true"}}""", true)]
    [InlineData("""{"pagination":{"total":"30","offset":0,"limit":10}}""", true)]
    [InlineData("""{"pagination":{"total":10,"offset":0,"limit":10}}""", false)]
    public void MdbListPaginationToleratesWronglyTypedMarkers(string json, bool expected)
    {
        var detail = Assert.IsType<JsonObject>(JsonNode.Parse(json));
        Assert.Equal(expected, MdbListHttpTransport.BodyHasMore(detail));
    }

    [Theory]
    [InlineData("""{"private":false}""", false)]
    [InlineData("""{"private":0}""", false)]
    [InlineData("""{"private":null}""", false)]
    [InlineData("""{}""", false)]
    [InlineData("""{"private":true}""", true)]
    [InlineData("""{"private":"true"}""", true)]
    [InlineData("""{"private":"unknown"}""", true)]
    [InlineData("""{"private":{"state":"hidden"}}""", true)]
    [InlineData("""{"privacy":"Private"}""", true)]
    [InlineData("""{"privacy":7}""", false)]
    public void MalformedPrivacyMarkersFailClosed(string json, bool expected)
    {
        var detail = Assert.IsType<JsonObject>(JsonNode.Parse(json));
        Assert.Equal(expected, CollectionProviderService.IsPrivateList(detail));
    }

    [Theory]
    [InlineData("""{}""", false)]
    [InlineData("""{"includeUnreleased":null}""", false)]
    [InlineData("""{"includeUnreleased":true}""", true)]
    [InlineData("""{"includeUnreleased":false}""", false)]
    public void IncludeUnreleasedAcceptsBooleans(string json, bool expected)
    {
        var source = Assert.IsType<JsonObject>(JsonNode.Parse(json));
        Assert.Equal(expected, CollectionProviderService.IncludeUnreleased(source));
    }

    [Theory]
    [InlineData("""{"includeUnreleased":"yes"}""")]
    [InlineData("""{"includeUnreleased":1}""")]
    [InlineData("""{"includeUnreleased":{}}""")]
    public void AWronglyTypedIncludeUnreleasedIsABadRequest(string json)
    {
        var source = Assert.IsType<JsonObject>(JsonNode.Parse(json));
        var error = Assert.Throws<GatewayException>(() =>
            CollectionProviderService.IncludeUnreleased(source));
        Assert.Equal(StatusCodes.Status400BadRequest, error.StatusCode);
    }

    [Fact]
    public void SeerrMediaIgnoresWronglyTypedEpisodeRuntimes()
    {
        var source = Assert.IsType<JsonObject>(JsonNode.Parse(
            """{"id":1396,"name":"Breaking Bad","episodeRunTime":["long",{"minutes":1},0,47]}"""));

        var detail = Assert.IsType<JsonObject>(SeerrGateway.ShapeMedia(source, "tv"));

        Assert.Equal(47, detail["runtimeMinutes"]?.GetValue<int>());
    }

    private static ServiceConfiguration Service()
        => new()
        {
            Enabled = true,
            BaseUrl = $"http://{ServiceHost}:8989",
            ApiKey = ApiKey
        };
}
