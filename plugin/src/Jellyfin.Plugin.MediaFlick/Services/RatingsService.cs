using System.Collections.Concurrent;
using System.Globalization;
using System.Net;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Models;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class RatingsService : IDisposable
{
    private const long FreshSeconds = 7 * 24 * 60 * 60;
    private const long NegativeFreshSeconds = 24 * 60 * 60;
    private const long ExpireSeconds = 30 * 24 * 60 * 60;
    private const string BackgroundRefreshSubject = "background-refresh";
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
    private readonly ILogger<RatingsService> _logger;
    private readonly FailureLogGate _backgroundFailures = new();

    internal RatingsService(
        RatingsCacheStore cache,
        IRatingSecretStore secrets,
        IMdbListTransport transport,
        ILogger<RatingsService> logger,
        TimeProvider? timeProvider = null,
        ITmdbTransport? tmdbTransport = null)
    {
        _cache = cache;
        _secrets = secrets;
        _transport = transport;
        _logger = logger;
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
                await ValidateTmdbAsync(secret, false, cancellationToken).ConfigureAwait(false);
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
            await ValidateTmdbAsync(secret, true, cancellationToken).ConfigureAwait(false);
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

    /// <summary>
    /// Only a 401/403 rejects the key. Rate limits, timeouts, and outages
    /// leave a saved key's validity unchanged, but a new key that could not
    /// be verified is not saved.
    /// </summary>
    private async Task ValidateTmdbAsync(
        string key,
        bool preserveValidOnTransientFailure,
        CancellationToken cancellationToken)
    {
        if (_tmdbTransport is null)
        {
            _cache.SetHealth(
                RatingProviders.Tmdb,
                new ProviderHealthState
                {
                    Validation = "unchecked",
                    Valid = false,
                    LastCheckedAt = Now()
                });
            return;
        }
        var previous = _cache.Health(RatingProviders.Tmdb);
        var response = await _tmdbTransport.GetAsync(
            key,
            "3/configuration",
            new Dictionary<string, string>(),
            cancellationToken).ConfigureAwait(false);
        _cache.SetHealth(
            RatingProviders.Tmdb,
            ProviderHealthPolicy.AfterValidation(
                previous,
                ProviderOutcome.Of(response),
                Now(),
                preserveValidOnTransientFailure,
                rateLimitProvesCredential: false));
        if (ProviderHealthPolicy.IsRejection(response.StatusCode))
        {
            throw new RatingRequestException("TMDB rejected the supplied credential");
        }
        if (!response.StatusCode.IsSuccess() && !preserveValidOnTransientFailure)
        {
            throw new RatingRequestException(
                "TMDB could not be reached to verify the credential; try again later");
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
                if (_backgroundFailures.Recovered(BackgroundRefreshSubject))
                {
                    _logger.LogInformation("Background MDBList ratings refresh is working again");
                }
            }
            catch (OperationCanceledException) when (_shutdown.IsCancellationRequested)
            {
            }
            catch (Exception exception)
            {
                // Background stale refresh is strictly best-effort. The next
                // request can retry after the persisted provider backoff.
                // Upstream HTTP outcomes are logged by the transport; this is
                // an unexpected failure, reported once per exception type and
                // never with text that could carry the API key.
                _logger.Log(
                    _backgroundFailures.Failure(
                        BackgroundRefreshSubject,
                        exception.GetType().FullName ?? exception.GetType().Name),
                    CompanionLogging.WithoutSecret(exception, key),
                    "Background MDBList ratings refresh failed for {TitleCount} titles ({ExceptionType}); stale ratings remain available",
                    claimed.Length,
                    exception.GetType().Name);
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

    private static ProviderHealthState StateFromResponse(
        MdbListResponse response,
        ProviderHealthState previous,
        long now,
        bool preserveValidOnTransientFailure)
    {
        var outcome = ProviderOutcome.Of(response);
        if (response.StatusCode.IsSuccess())
        {
            outcome = outcome with
            {
                Quota = MergeBodyQuota(
                    response.Body,
                    ProviderHealthPolicy.MergeQuota(response.Quota, previous.Quota))
            };
        }

        // MDBList authenticates before rate limiting, so a quota answer still
        // validates even a newly saved key; it does not grant extra requests.
        return ProviderHealthPolicy.AfterValidation(
            previous,
            outcome,
            now,
            preserveValidOnTransientFailure,
            rateLimitProvesCredential: true);
    }

    private static RatingQuotaResponse MergeBodyQuota(JsonNode? body, RatingQuotaResponse quota)
    {
        if (body is not JsonObject value)
        {
            return quota;
        }

        var limit = JsonRead.Integer(value["rate_limit"])
            ?? JsonRead.Integer(value["api_requests"])
            ?? quota.Limit;
        var remaining = JsonRead.Integer(value["rate_limit_remaining"])
            ?? JsonRead.Integer(value["api_requests_remaining"])
            ?? quota.Remaining;
        if (remaining is null
            && limit is { } maximum
            && JsonRead.Integer(value["api_requests_count"]) is { } used)
        {
            remaining = Math.Max(0, maximum - used);
        }

        return RatingsContract.NormalizeQuota(new(
            limit,
            remaining,
            JsonRead.Integer(value["rate_limit_reset"]) ?? quota.ResetAt));
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
        => ProviderHealthPolicy.IsBackedOff(status, now);

    private static string? NodeId(JsonNode? node)
        => JsonRead.String(node) ?? JsonRead.Integer(node)?.ToString(CultureInfo.InvariantCulture);
}

internal sealed class RatingsUnavailableException(string message) : Exception(message);

internal static class HttpStatusCodeExtensions
{
    public static bool IsSuccess(this HttpStatusCode statusCode)
        => (int)statusCode is >= 200 and <= 299;
}
