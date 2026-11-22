using System.Net;
using Jellyfin.Plugin.MediaFlick.Models;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal sealed record ProviderOutcome(
    HttpStatusCode StatusCode,
    long? RetryAt,
    RatingQuotaResponse Quota)
{
    public static RatingQuotaResponse NoQuota { get; } = new(null, null, null);

    public static ProviderOutcome Of(TmdbResponse response)
        => new(response.StatusCode, response.RetryAt, NoQuota);

    public static ProviderOutcome Of(MdbListResponse response)
        => new(response.StatusCode, response.RetryAt, response.Quota);
}

internal static class ProviderHealthPolicy
{
    public const int MaxFailureCount = 10;
    private const long BaseBackoffSeconds = 30;
    private const long MaxBackoffSeconds = 6 * 60 * 60;
    private const long DefaultRateLimitSeconds = 60;

    public static bool IsRejection(HttpStatusCode status)
        => status is HttpStatusCode.Unauthorized or HttpStatusCode.Forbidden;

    public static bool IsRateLimited(HttpStatusCode status)
        => (int)status == StatusCodes.Status429TooManyRequests;

    public static bool IsTransient(HttpStatusCode status)
        => IsRateLimited(status)
            || (int)status is StatusCodes.Status408RequestTimeout or >= 500;

    public static long BackoffSeconds(int failureCount)
        => Math.Min(
            BaseBackoffSeconds << Math.Clamp(failureCount, 0, 8),
            MaxBackoffSeconds);

    public static bool IsBackedOff(ProviderHealthState state, long now)
        => state.RetryAt is { } retryAt && retryAt > now;

    public static ProviderHealthState AfterValidation(
        ProviderHealthState previous,
        ProviderOutcome outcome,
        long now,
        bool preserveValidOnTransientFailure,
        bool rateLimitProvesCredential)
    {
        var quota = MergeQuota(outcome.Quota, previous.Quota);
        if (outcome.StatusCode.IsSuccess())
        {
            return new ProviderHealthState
            {
                Validation = "valid",
                Valid = true,
                QuotaLimit = quota.Limit,
                QuotaRemaining = quota.Remaining,
                QuotaResetAt = quota.ResetAt,
                RetryAt = outcome.RetryAt
                    ?? (quota.Remaining == 0 ? quota.ResetAt ?? NextUtcMidnight(now) : null),
                FailureCount = 0,
                LastCheckedAt = now
            };
        }

        if (IsRejection(outcome.StatusCode))
        {
            return new ProviderHealthState
            {
                Validation = "invalid",
                Valid = false,
                QuotaLimit = quota.Limit,
                QuotaRemaining = quota.Remaining,
                QuotaResetAt = quota.ResetAt,
                RetryAt = outcome.RetryAt,
                FailureCount = 0,
                LastCheckedAt = now
            };
        }

        if (IsRateLimited(outcome.StatusCode))
        {
            return previous with
            {
                Validation = "rate_limited",
                Valid = rateLimitProvesCredential
                    || (preserveValidOnTransientFailure && previous.Valid),
                QuotaLimit = quota.Limit,
                QuotaRemaining = quota.Remaining,
                QuotaResetAt = quota.ResetAt,
                RetryAt = outcome.RetryAt ?? quota.ResetAt ?? now + DefaultRateLimitSeconds,
                LastCheckedAt = now
            };
        }

        var failures = Math.Min(previous.FailureCount + 1, MaxFailureCount);
        return previous with
        {
            Validation = outcome.StatusCode == HttpStatusCode.GatewayTimeout ? "offline" : "unavailable",
            Valid = preserveValidOnTransientFailure && previous.Valid,
            QuotaLimit = quota.Limit,
            QuotaRemaining = quota.Remaining,
            QuotaResetAt = quota.ResetAt,
            RetryAt = outcome.RetryAt ?? now + BackoffSeconds(failures),
            FailureCount = failures,
            LastCheckedAt = now
        };
    }

    public static ProviderHealthState? AfterRequest(
        ProviderHealthState previous,
        HttpStatusCode status,
        long? upstreamRetryAt,
        long now,
        bool rejectionInvalidatesKey)
    {
        if (status.IsSuccess())
        {
            return previous.RetryAt is not null || previous.FailureCount > 0
                ? previous with { RetryAt = null, FailureCount = 0, LastCheckedAt = now }
                : null;
        }

        if (IsRejection(status))
        {
            return rejectionInvalidatesKey
                ? previous with
                {
                    Validation = "invalid",
                    Valid = false,
                    RetryAt = null,
                    FailureCount = 0,
                    LastCheckedAt = now
                }
                : null;
        }

        if (!IsTransient(status))
        {
            return null;
        }

        var failures = Math.Min(previous.FailureCount + 1, MaxFailureCount);
        return previous with
        {
            RetryAt = upstreamRetryAt ?? now + BackoffSeconds(failures),
            FailureCount = failures,
            LastCheckedAt = now
        };
    }

    public static RatingQuotaResponse MergeQuota(
        RatingQuotaResponse current,
        RatingQuotaResponse previous)
        => RatingsContract.NormalizeQuota(new(
            current.Limit ?? previous.Limit,
            current.Remaining ?? previous.Remaining,
            current.ResetAt ?? previous.ResetAt));

    private static long NextUtcMidnight(long now)
    {
        var next = DateTimeOffset.FromUnixTimeSeconds(now).UtcDateTime.Date.AddDays(1);
        return new DateTimeOffset(next, TimeSpan.Zero).ToUnixTimeSeconds();
    }
}
