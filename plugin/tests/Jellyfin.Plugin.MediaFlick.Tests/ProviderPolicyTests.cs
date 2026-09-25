using System.Net;
using Jellyfin.Plugin.MediaFlick.Models;
using Jellyfin.Plugin.MediaFlick.Services;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class ProviderPolicyTests
{
    private const long Now = 1_800_000_000;

    [Theory]
    [InlineData(HttpStatusCode.InternalServerError)]
    [InlineData(HttpStatusCode.BadGateway)]
    [InlineData(HttpStatusCode.ServiceUnavailable)]
    [InlineData(HttpStatusCode.GatewayTimeout)]
    [InlineData(HttpStatusCode.NotFound)]
    public void OutagesKeepAnEstablishedKeyValidAndBackOff(HttpStatusCode status)
    {
        var state = ProviderHealthPolicy.AfterValidation(
            Valid(),
            Outcome(status),
            Now,
            preserveValidOnTransientFailure: true,
            rateLimitProvesCredential: false);

        Assert.True(state.Valid);
        Assert.True(state.RetryAt > Now);
    }

    [Fact]
    public void AnUnverifiedNewKeyIsNotValidAfterAnOutage()
    {
        var state = ProviderHealthPolicy.AfterValidation(
            new ProviderHealthState(),
            Outcome(HttpStatusCode.GatewayTimeout),
            Now,
            preserveValidOnTransientFailure: false,
            rateLimitProvesCredential: false);

        Assert.False(state.Valid);
        Assert.Equal("offline", state.Validation);
    }

    [Theory]
    [InlineData(true, true)]
    [InlineData(false, false)]
    public void RateLimitsValidateOnlyWhereTheProviderAuthenticatesFirst(bool proves, bool valid)
    {
        var state = ProviderHealthPolicy.AfterValidation(
            new ProviderHealthState(),
            Outcome(HttpStatusCode.TooManyRequests, retryAt: Now + 90),
            Now,
            preserveValidOnTransientFailure: false,
            rateLimitProvesCredential: proves);

        Assert.Equal("rate_limited", state.Validation);
        Assert.Equal(valid, state.Valid);
        Assert.Equal(Now + 90, state.RetryAt);
    }

    [Fact]
    public void AnExhaustedQuotaBacksOffUntilItsReset()
    {
        var state = ProviderHealthPolicy.AfterValidation(
            new ProviderHealthState(),
            new ProviderOutcome(HttpStatusCode.OK, null, new RatingQuotaResponse(1000, 0, Now + 500)),
            Now,
            preserveValidOnTransientFailure: false,
            rateLimitProvesCredential: true);

        Assert.True(state.Valid);
        Assert.Equal(Now + 500, state.RetryAt);
    }

    [Fact]
    public void DataRequestsOnlyInvalidateKeysWhereRejectionMeansTheKey()
    {
        Assert.Null(ProviderHealthPolicy.AfterRequest(
            Valid(), HttpStatusCode.Forbidden, null, Now, rejectionInvalidatesKey: false));
        Assert.Null(ProviderHealthPolicy.AfterRequest(
            Valid(), HttpStatusCode.NotFound, null, Now, rejectionInvalidatesKey: true));
        Assert.False(ProviderHealthPolicy.AfterRequest(
            Valid(), HttpStatusCode.Unauthorized, null, Now, rejectionInvalidatesKey: true)!.Valid);

        var outage = ProviderHealthPolicy.AfterRequest(
            Valid(), HttpStatusCode.BadGateway, null, Now, rejectionInvalidatesKey: true)!;
        Assert.True(outage.Valid);
        Assert.True(outage.RetryAt > Now);

        var recovered = ProviderHealthPolicy.AfterRequest(
            outage, HttpStatusCode.OK, null, Now + 100, rejectionInvalidatesKey: true)!;
        Assert.Null(recovered.RetryAt);
    }

    [Theory]
    [InlineData("tt0133093", "tt0133093")]
    [InlineData(" TT0133093 ", "tt0133093")]
    [InlineData("tt12345", "tt12345")]
    [InlineData("tt1234", null)]
    [InlineData("tt1234567890123", null)]
    [InlineData("nm0000206", null)]
    [InlineData("tt01330x3", null)]
    [InlineData("", null)]
    [InlineData(null, null)]
    public void OneImdbRuleNormalizesEveryCaller(string? value, string? expected)
    {
        Assert.Equal(expected, ImdbIds.Normalize(value));
    }

    private static ProviderHealthState Valid()
        => new() { Validation = "valid", Valid = true, LastCheckedAt = Now - 60 };

    private static ProviderOutcome Outcome(HttpStatusCode status, long? retryAt = null)
        => new(status, retryAt, ProviderOutcome.NoQuota);
}
