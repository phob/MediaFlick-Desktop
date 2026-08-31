using System.Collections.Concurrent;
using System.Globalization;
using System.Net;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Models;

namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class RatingsService : IDisposable
{
    private const long FreshSeconds = 7 * 24 * 60 * 60;
    private const long NegativeFreshSeconds = 24 * 60 * 60;
    private const long ExpireSeconds = 30 * 24 * 60 * 60;
    private static readonly TimeSpan ForegroundRefreshTimeout = TimeSpan.FromSeconds(25);
    private readonly RatingsCacheStore _cache;
    private readonly IRatingSecretStore _secrets;
    private readonly IMdbListTransport _transport;
    private readonly ITmdbTransport? _tmdbTransport;
    private readonly TimeProvider _timeProvider;
    private readonly SemaphoreSlim _refreshGate = new(1, 1);
    private readonly ConcurrentDictionary<string, byte> _background =
        new(StringComparer.Ordinal);
    private readonly CancellationTokenSource _shutdown = new();

    internal RatingsService(
        RatingsCacheStore cache,
        IRatingSecretStore secrets,
        IMdbListTransport transport,
        TimeProvider? timeProvider = null,
        ITmdbTransport? tmdbTransport = null)
    {
        _cache = cache;
        _secrets = secrets;
        _transport = transport;
        _tmdbTransport = tmdbTransport;
        _timeProvider = timeProvider ?? TimeProvider.System;
    }

    public RatingsCapabilityResponse Capability()
    {
        var mdblist = ProviderStatus(RatingProviders.MdbList);
        var tmdb = ProviderStatus(RatingProviders.Tmdb);
        return new RatingsCapabilityResponse(
            RatingsContract.BoundaryVersion,
            new ContractVersionRange(RatingsContract.BoundaryVersion, RatingsContract.BoundaryVersion),
            mdblist.Configured && mdblist.Valid,
            mdblist.Configured && mdblist.Valid,
            mdblist.Validation,
            RatingsContract.Origin,
            false,
            ["plugin", "none"],
            RatingsContract.SourceCatalog,
            mdblist.Quota,
            mdblist.RetryAt,
            mdblist.LastCheckedAt,
            tmdb);
    }

    public RatingAdminStatusResponse AdminStatus()
        => new(
            ProviderStatus(RatingProviders.MdbList),
            ProviderStatus(RatingProviders.Tmdb));

    public async Task<RatingAdminStatusResponse> SaveCredentialAsync(
        string provider,
        string secret,
        CancellationToken cancellationToken)
    {
        var normalized = RatingProviders.Normalize(provider);
        secret = secret.Trim();
        ValidateSecret(normalized, secret);
        var previousHealth = _cache.Health(normalized);
        try
        {
            if (normalized == RatingProviders.Tmdb)
            {
                await ValidateTmdbAsync(secret, cancellationToken).ConfigureAwait(false);
            }
            else
            {
                await ValidateMdbListAsync(secret, false, cancellationToken).ConfigureAwait(false);
            }
            _secrets.Set(normalized, secret);
        }
        catch
        {
            _cache.SetHealth(normalized, previousHealth);
            throw;
        }

        return AdminStatus();
    }

    public async Task<RatingAdminStatusResponse> ValidateCredentialAsync(
        string provider,
        CancellationToken cancellationToken)
    {
        var normalized = RatingProviders.Normalize(provider);
        var secret = _secrets.Get(normalized)
            ?? throw new RatingRequestException("no credential is saved");
        if (normalized == RatingProviders.Tmdb)
        {
            await ValidateTmdbAsync(secret, cancellationToken).ConfigureAwait(false);
        }
        else
        {
            await ValidateMdbListAsync(secret, true, cancellationToken).ConfigureAwait(false);
        }

        return AdminStatus();
    }

    public RatingAdminStatusResponse RemoveCredential(string provider)
    {
        var normalized = RatingProviders.Normalize(provider);
        _secrets.Remove(normalized);
        _cache.ResetHealth(normalized);
        return AdminStatus();
    }

    public async Task<RatingBatchResponse> BatchAsync(
        RatingBatchRequest? request,
        CancellationToken cancellationToken)
    {
        var targets = RatingsContract.Validate(request);
        var key = ReadMdbListKey();
        var status = _cache.Health(RatingProviders.MdbList);
        if (key is null || !status.Valid)
        {
            throw new RatingsUnavailableException(
                "server MDBList ratings are not configured and valid");
        }

        var now = Now();
        var cached = _cache.Get(targets);
        var stale = targets.Where(target => cached.TryGetValue(target.ItemId, out var entry)
                && entry.StaleAt <= now
                && entry.ExpiresAt > now)
            .ToArray();
        var missing = targets.Where(target => !cached.TryGetValue(target.ItemId, out var entry)
                || entry.ExpiresAt <= now)
            .ToArray();

        if (stale.Length > 0 && !IsBackedOff(status, now))
        {
            QueueStaleRefresh(stale, key);
        }

        string? diagnostic = null;
        if (missing.Length > 0)
        {
            if (IsBackedOff(status, now))
            {
                diagnostic = "MDBList retry timing is being respected; cached ratings remain available.";
            }
            else
            {
                using var refreshTimeout = CancellationTokenSource.CreateLinkedTokenSource(
                    cancellationToken);
                refreshTimeout.CancelAfter(ForegroundRefreshTimeout);
                try
                {
                    diagnostic = await RefreshAsync(
                        missing,
                        key,
                        refreshStale: false,
                        refreshTimeout.Token).ConfigureAwait(false);
                }
                catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
                {
                    diagnostic = "MDBList rating refresh timed out; cached ratings remain available.";
                }
            }
        }

        cached = _cache.Get(targets);
        status = _cache.Health(RatingProviders.MdbList);
        var items = targets
            .Where(target => cached.TryGetValue(target.ItemId, out var entry)
                && entry.ExpiresAt > now)
            .Select(target =>
            {
                var entry = cached[target.ItemId];
                return new RatingBatchItemResponse(
                    target.ItemId,
                    (JsonArray)entry.Ratings.DeepClone(),
                    RatingsContract.Origin,
                    entry.FetchedAt,
                    entry.SourceUpdatedAt,
                    entry.StaleAt <= now);
            })
            .ToArray();
        return new RatingBatchResponse(
            RatingsContract.BoundaryVersion,
            items,
            status.Quota,
            status.RetryAt,
            diagnostic);
    }

    public void Dispose()
    {
        _shutdown.Cancel();
        _shutdown.Dispose();
        _refreshGate.Dispose();
    }

    private RatingProviderStatusResponse ProviderStatus(string provider)
    {
        var configured = false;
        string? readError = null;
        try
        {
            configured = _secrets.IsConfigured(provider);
            if (configured)
            {
                _ = _secrets.Get(provider);
            }
        }
        catch (InvalidOperationException)
        {
            readError = "The saved credential cannot be decrypted; replace or remove it.";
        }

        var state = _cache.Health(provider);
        var validation = configured
            ? readError is null ? RatingsContract.NormalizeValidation(state.Validation) : "unreadable"
            : "absent";
        var valid = configured && readError is null && state.Valid;
        return new RatingProviderStatusResponse(
            configured,
            valid,
            validation,
            readError ?? (configured ? RatingsContract.StatusDetail(validation, provider) : null),
            RatingsContract.NormalizeQuota(state.Quota),
            RatingsContract.NormalizeTimestamp(state.RetryAt),
            RatingsContract.NormalizeTimestamp(state.LastCheckedAt),
            "aspnet_data_protection",
            provider == RatingProviders.MdbList,
            false);
    }

    private async Task ValidateMdbListAsync(
        string key,
        bool preserveValidOnTransientFailure,
        CancellationToken cancellationToken)
    {
        var previous = _cache.Health(RatingProviders.MdbList);
        var now = Now();
        if (IsBackedOff(previous, now))
        {
            return;
        }

        var response = await _transport.ValidateAsync(key, cancellationToken).ConfigureAwait(false);
        _cache.SetHealth(
            RatingProviders.MdbList,
            StateFromResponse(response, previous, now, preserveValidOnTransientFailure));
    }

    private async Task ValidateTmdbAsync(string key, CancellationToken cancellationToken)
    {
        if (_tmdbTransport is null)
        {
            _cache.SetHealth(
                RatingProviders.Tmdb,
                new ProviderHealthState
                {
                    Validation = "unchecked",
                    Valid = false,
                    Detail = "TMDB validation is unavailable.",
                    LastCheckedAt = Now()
                });
            return;
        }
        var response = await _tmdbTransport.GetAsync(
            key,
            "3/configuration",
            new Dictionary<string, string>(),
            cancellationToken).ConfigureAwait(false);
        var valid = response.StatusCode.IsSuccess();
        _cache.SetHealth(
            RatingProviders.Tmdb,
            new ProviderHealthState
            {
                Validation = valid ? "valid" : "invalid",
                Valid = valid,
                Detail = valid ? null : "TMDB rejected the saved credential.",
                RetryAt = response.RetryAt,
                LastCheckedAt = Now()
            });
        if (!valid)
        {
            throw new RatingRequestException("TMDB rejected the supplied credential");
        }
    }

    private void QueueStaleRefresh(IReadOnlyList<RatingTargetRequest> targets, string key)
    {
        var claimed = targets
            .Where(target => _background.TryAdd(RatingsContract.StableKey(target), 0))
            .ToArray();
        if (claimed.Length == 0)
        {
            return;
        }

        _ = Task.Run(async () =>
        {
            try
            {
                await RefreshAsync(claimed, key, true, _shutdown.Token).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (_shutdown.IsCancellationRequested)
            {
            }
            catch (Exception)
            {
                // Background stale refresh is strictly best-effort. The next
                // request can retry after the persisted provider backoff.
            }
            finally
            {
                foreach (var target in claimed)
                {
                    _background.TryRemove(RatingsContract.StableKey(target), out _);
                }
            }
        });
    }

    private async Task<string?> RefreshAsync(
        IReadOnlyList<RatingTargetRequest> requested,
        string key,
        bool refreshStale,
        CancellationToken cancellationToken)
    {
        await _refreshGate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var now = Now();
            var state = _cache.Health(RatingProviders.MdbList);
            if (IsBackedOff(state, now))
            {
                return "MDBList retry timing is being respected; cached ratings remain available.";
            }

            var targets = requested
                .GroupBy(RatingsContract.StableKey, StringComparer.Ordinal)
                .Select(group => group.First())
                .Where(target =>
                {
                    var entry = _cache.GetStable(target);
                    return entry is null || entry.ExpiresAt <= now
                        || (refreshStale && entry.StaleAt <= now);
                })
                .ToArray();
            foreach (var group in targets.GroupBy(
                         target => (target.Provider, target.MediaType)))
            {
                foreach (var chunk in group.Chunk(RatingsContract.MaxUpstreamBatchItems))
                {
                    var response = await _transport.BatchAsync(
                        key,
                        group.Key.Provider,
                        group.Key.MediaType,
                        chunk.Select(target => target.ProviderId).ToArray(),
                        cancellationToken).ConfigureAwait(false);
                    state = StateFromResponse(response, state, Now(), true);
                    _cache.SetHealth(RatingProviders.MdbList, state);
                    if (!response.StatusCode.IsSuccess())
                    {
                        // Never relay upstream response/transport text through
                        // the desktop diagnostic boundary.
                        return RatingsContract.StatusDetail(state.Validation, RatingProviders.MdbList);
                    }

                    CacheResponse(chunk, response.Body, Now());
                    if (state.QuotaRemaining == 0
                        && state.RetryAt is { } retryAt
                        && retryAt > Now())
                    {
                        return "MDBList quota is exhausted; cached ratings remain available.";
                    }
                }
            }

            return null;
        }
        finally
        {
            _refreshGate.Release();
        }
    }

    private void CacheResponse(
        IReadOnlyList<RatingTargetRequest> targets,
        JsonNode? body,
        long now)
    {
        var byIdentity = new Dictionary<string, (JsonArray Ratings, string? Updated)>(
            StringComparer.Ordinal);
        if (body is JsonArray media)
        {
            foreach (var item in media.OfType<JsonObject>())
            {
                var ids = item["ids"] as JsonObject;
                foreach (var provider in new[] { "tmdb", "imdb" })
                {
                    var id = NodeId(ids?[provider]);
                    if (id is not null)
                    {
                        byIdentity[string.Join('|', provider, id)] =
                            (RatingsContract.NormalizeMedia(item),
                                RatingsContract.NormalizeSourceUpdatedAt(item["updated"]));
                    }
                }
            }
        }

        _cache.Upsert(targets.Select(target =>
        {
            var found = byIdentity.GetValueOrDefault(string.Join('|', target.Provider, target.ProviderId));
            var ratings = found.Ratings ?? [];
            var freshSeconds = ratings.Count == 0 ? NegativeFreshSeconds : FreshSeconds;
            return (
                target,
                new CachedRatingEntry(
                    target.Provider,
                    target.ProviderId,
                    target.MediaType,
                    ratings,
                    found.Updated,
                    now,
                    now + freshSeconds,
                    now + ExpireSeconds));
        }));
    }

    private ProviderHealthState StateFromResponse(
        MdbListResponse response,
        ProviderHealthState previous,
        long now,
        bool preserveValidOnTransientFailure)
    {
        var quota = MergeQuota(response.Quota, previous.Quota);
        if (response.StatusCode.IsSuccess())
        {
            quota = MergeBodyQuota(response.Body, quota);
            return new ProviderHealthState
            {
                Validation = "valid",
                Valid = true,
                Detail = "Valid MDBList credential.",
                QuotaLimit = quota.Limit,
                QuotaRemaining = quota.Remaining,
                QuotaResetAt = quota.ResetAt,
                RetryAt = response.RetryAt
                    ?? (quota.Remaining == 0
                        ? quota.ResetAt ?? NextUtcMidnight(now)
                        : null),
                FailureCount = 0,
                LastCheckedAt = now
            };
        }

        if (response.StatusCode is HttpStatusCode.Unauthorized or HttpStatusCode.Forbidden)
        {
            return new ProviderHealthState
            {
                Validation = "invalid",
                Valid = false,
                Detail = "MDBList rejected the saved API key.",
                QuotaLimit = quota.Limit,
                QuotaRemaining = quota.Remaining,
                QuotaResetAt = quota.ResetAt,
                FailureCount = 0,
                LastCheckedAt = now
            };
        }

        if ((int)response.StatusCode == StatusCodes.Status429TooManyRequests)
        {
            return previous with
            {
                Validation = "rate_limited",
                // A quota response is authenticated and therefore validates
                // even a newly saved key; it does not grant extra requests.
                Valid = true,
                Detail = "MDBList quota is exhausted; cached ratings remain available.",
                QuotaLimit = quota.Limit,
                QuotaRemaining = quota.Remaining,
                QuotaResetAt = quota.ResetAt,
                RetryAt = response.RetryAt ?? quota.ResetAt ?? now + 60,
                LastCheckedAt = now
            };
        }

        var failures = Math.Min(previous.FailureCount + 1, 10);
        var delay = Math.Min(30L * (1L << Math.Min(failures, 8)), 6 * 60 * 60);
        return previous with
        {
            Validation = response.StatusCode == HttpStatusCode.GatewayTimeout
                ? "offline"
                : "unavailable",
            Valid = preserveValidOnTransientFailure && previous.Valid,
            Detail = "MDBList is temporarily unavailable; cached ratings remain available.",
            QuotaLimit = quota.Limit,
            QuotaRemaining = quota.Remaining,
            QuotaResetAt = quota.ResetAt,
            RetryAt = response.RetryAt ?? now + delay,
            FailureCount = failures,
            LastCheckedAt = now
        };
    }

    private static RatingQuotaResponse MergeQuota(
        RatingQuotaResponse current,
        RatingQuotaResponse previous)
        => RatingsContract.NormalizeQuota(new(
            current.Limit ?? previous.Limit,
            current.Remaining ?? previous.Remaining,
            current.ResetAt ?? previous.ResetAt));

    private static RatingQuotaResponse MergeBodyQuota(JsonNode? body, RatingQuotaResponse quota)
    {
        if (body is not JsonObject value)
        {
            return quota;
        }

        var limit = NodeLong(value["rate_limit"])
            ?? NodeLong(value["api_requests"])
            ?? quota.Limit;
        var remaining = NodeLong(value["rate_limit_remaining"])
            ?? NodeLong(value["api_requests_remaining"])
            ?? quota.Remaining;
        if (remaining is null
            && limit is { } maximum
            && NodeLong(value["api_requests_count"]) is { } used)
        {
            remaining = Math.Max(0, maximum - used);
        }

        return RatingsContract.NormalizeQuota(new(
            limit,
            remaining,
            NodeLong(value["rate_limit_reset"]) ?? quota.ResetAt));
    }

    private static void ValidateSecret(string provider, string secret)
    {
        if (secret.Length is 0 or > 2048 || secret.Any(char.IsControl))
        {
            throw new RatingRequestException("enter a valid API key");
        }

        if (provider == RatingProviders.Tmdb && !RatingsContract.ValidTmdbKeyShape(secret))
        {
            throw new RatingRequestException(
                "TMDB keys are normally a 32-character v3 key or a v4 JWT token");
        }
    }

    private string? ReadMdbListKey()
    {
        try
        {
            return _secrets.Get(RatingProviders.MdbList);
        }
        catch (InvalidOperationException)
        {
            return null;
        }
    }

    private long Now() => _timeProvider.GetUtcNow().ToUnixTimeSeconds();

    private static bool IsBackedOff(ProviderHealthState status, long now)
        => status.RetryAt is { } retryAt && retryAt > now;

    private static long NextUtcMidnight(long now)
    {
        var next = DateTimeOffset.FromUnixTimeSeconds(now).UtcDateTime.Date.AddDays(1);
        return new DateTimeOffset(next, TimeSpan.Zero).ToUnixTimeSeconds();
    }

    private static string? NodeId(JsonNode? node)
        => NodeString(node) ?? NodeLong(node)?.ToString(CultureInfo.InvariantCulture);

    private static string? NodeString(JsonNode? node)
    {
        try
        {
            return node?.GetValue<string>();
        }
        catch (InvalidOperationException)
        {
            return null;
        }
    }

    private static long? NodeLong(JsonNode? node)
    {
        try
        {
            if (node is JsonValue value && value.TryGetValue<long>(out var number))
            {
                return number;
            }

            return long.TryParse(
                node?.GetValue<string>(),
                NumberStyles.Integer,
                CultureInfo.InvariantCulture,
                out number)
                    ? number
                    : null;
        }
        catch (InvalidOperationException)
        {
            return null;
        }
    }
}

internal sealed class RatingsUnavailableException(string message) : Exception(message);

internal static class HttpStatusCodeExtensions
{
    public static bool IsSuccess(this HttpStatusCode statusCode)
        => (int)statusCode is >= 200 and <= 299;
}
