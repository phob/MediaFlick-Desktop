using System.Text.Json;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Models;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.MediaFlick.Services;

internal sealed record CachedRatingEntry(
    string Provider,
    string ProviderId,
    string MediaType,
    JsonArray Ratings,
    string? SourceUpdatedAt,
    long FetchedAt,
    long StaleAt,
    long ExpiresAt);

internal sealed record ProviderHealthState
{
    public string Validation { get; init; } = "unchecked";

    public bool Valid { get; init; }

    public string? Detail { get; init; }

    public long? QuotaLimit { get; init; }

    public long? QuotaRemaining { get; init; }

    public long? QuotaResetAt { get; init; }

    public long? RetryAt { get; init; }

    public int FailureCount { get; init; }

    public long? LastCheckedAt { get; init; }

    public RatingQuotaResponse Quota
        => new(QuotaLimit, QuotaRemaining, QuotaResetAt);
}

/// <summary>
/// The server-wide, rebuildable provider cache: MDBList rating entries plus
/// MDBList and TMDB credential health (validation, quota, retry timing) used
/// by both ratings and collections. It keeps the established
/// <c>ratings-v1-cache.json</c> file and format.
/// Changes are coalesced into one atomic save shortly after they happen
/// instead of rewriting the file per change; <see cref="Flush"/> and
/// <see cref="Dispose"/> write any pending change immediately.
/// </summary>
internal sealed class ProviderCacheStore : IDisposable
{
    /// <summary>Rating entries kept at most; the oldest fetches go first.</summary>
    internal const int MaxEntries = 100_000;
    internal static readonly TimeSpan DefaultSaveDelay = TimeSpan.FromSeconds(2);
    private const int DocumentVersion = 1;
    private readonly object _lock = new();
    private readonly object _writeLock = new();
    private readonly string _path;
    private readonly ILogger<ProviderCacheStore> _logger;
    private readonly TimeProvider _time;
    private readonly TimeSpan _saveDelay;
    private readonly ITimer _saveTimer;
    private CacheDocument _document;
    private bool _dirty;
    private bool _saveScheduled;
    private bool _disposed;
    private bool _persistFailing;

    public ProviderCacheStore(
        string path,
        ILogger<ProviderCacheStore> logger,
        TimeProvider? timeProvider = null,
        TimeSpan? saveDelay = null)
    {
        _path = path;
        _logger = logger;
        _time = timeProvider ?? TimeProvider.System;
        _saveDelay = saveDelay ?? DefaultSaveDelay;
        _saveTimer = _time.CreateTimer(
            static state => ((ProviderCacheStore)state!).SaveScheduled(),
            this,
            Timeout.InfiniteTimeSpan,
            Timeout.InfiniteTimeSpan);
        _document = Load(path, logger);
        if (SanitizeLoadedDocument() | PruneLocked())
        {
            _dirty = true;
            Flush();
        }
    }

    public IReadOnlyDictionary<string, CachedRatingEntry> Get(
        IEnumerable<RatingTargetRequest> targets)
    {
        lock (_lock)
        {
            var result = new Dictionary<string, CachedRatingEntry>(StringComparer.Ordinal);
            foreach (var target in targets)
            {
                if (_document.Entries.TryGetValue(CacheKey(target), out var entry))
                {
                    result[target.ItemId] = Sanitize(entry);
                }
            }

            return result;
        }
    }

    public CachedRatingEntry? GetStable(RatingTargetRequest target)
    {
        lock (_lock)
        {
            return _document.Entries.TryGetValue(CacheKey(target), out var entry)
                ? Sanitize(entry)
                : null;
        }
    }

    public void Upsert(IEnumerable<(RatingTargetRequest Target, CachedRatingEntry Entry)> entries)
    {
        lock (_lock)
        {
            foreach (var (target, entry) in entries)
            {
                _document.Entries[CacheKey(target)] = Sanitize(entry);
            }

            MarkDirtyLocked();
        }
    }

    public ProviderHealthState Health(string provider)
    {
        lock (_lock)
        {
            return _document.Health.GetValueOrDefault(provider) is { } state
                ? Sanitize(state)
                : new ProviderHealthState();
        }
    }

    public void SetHealth(string provider, ProviderHealthState state)
    {
        lock (_lock)
        {
            _document.Health[provider] = Sanitize(state);
            MarkDirtyLocked();
        }
    }

    public void ResetHealth(string provider)
    {
        lock (_lock)
        {
            if (_document.Health.Remove(provider))
            {
                MarkDirtyLocked();
            }
        }
    }

    public int Count
    {
        get
        {
            lock (_lock)
            {
                return _document.Entries.Count;
            }
        }
    }

    /// <summary>
    /// Writes pending changes now. A failed write keeps them pending, so the
    /// next change or flush retries without losing the in-memory state.
    /// </summary>
    public void Flush()
    {
        // Writers queue on _writeLock so an older snapshot can never replace
        // a newer file; readers only wait for the short snapshot below.
        lock (_writeLock)
        {
            string json;
            lock (_lock)
            {
                if (!_dirty)
                {
                    return;
                }

                PruneLocked();
                json = JsonSerializer.Serialize(_document, CompanionJson.CamelCase);
                _dirty = false;
            }

            if (!TryWrite(json))
            {
                lock (_lock)
                {
                    _dirty = true;
                }
            }
        }
    }

    public void Dispose()
    {
        lock (_lock)
        {
            if (_disposed)
            {
                return;
            }

            _disposed = true;
        }

        _saveTimer.Dispose();
        Flush();
    }

    private void MarkDirtyLocked()
    {
        _dirty = true;
        if (_saveScheduled || _disposed)
        {
            return;
        }

        _saveScheduled = true;
        _saveTimer.Change(_saveDelay, Timeout.InfiniteTimeSpan);
    }

    private void SaveScheduled()
    {
        lock (_lock)
        {
            _saveScheduled = false;
        }

        try
        {
            Flush();
        }
        catch (Exception exception) when (exception is JsonException or NotSupportedException)
        {
            // A timer callback must not throw. The document holds only
            // sanitized primitives, so this is not expected in practice.
            _logger.LogWarning(exception, "Could not serialize the MediaFlick provider cache");
        }
    }

    /// <summary>
    /// Drops expired rating entries and, past <see cref="MaxEntries"/>, the
    /// oldest fetches. Health state is a handful of providers and is kept.
    /// </summary>
    private bool PruneLocked()
    {
        var now = _time.GetUtcNow().ToUnixTimeSeconds();
        var changed = false;
        foreach (var (key, entry) in _document.Entries.ToArray())
        {
            if (entry.ExpiresAt <= now)
            {
                _document.Entries.Remove(key);
                changed = true;
            }
        }

        var excess = _document.Entries.Count - MaxEntries;
        if (excess > 0)
        {
            foreach (var key in _document.Entries
                .OrderBy(static pair => pair.Value.FetchedAt)
                .Take(excess)
                .Select(static pair => pair.Key)
                .ToArray())
            {
                _document.Entries.Remove(key);
            }

            changed = true;
        }

        return changed;
    }

    private static string CacheKey(RatingTargetRequest target)
        => string.Join('|', target.Provider, target.MediaType, target.ProviderId);

    private bool SanitizeLoadedDocument()
    {
        var changed = false;
        foreach (var (key, entry) in _document.Entries.ToArray())
        {
            var sanitized = Sanitize(entry);
            if (!JsonNode.DeepEquals(entry.Ratings, sanitized.Ratings)
                || entry.SourceUpdatedAt != sanitized.SourceUpdatedAt)
            {
                _document.Entries[key] = sanitized;
                changed = true;
            }
        }

        foreach (var (provider, state) in _document.Health.ToArray())
        {
            var sanitized = Sanitize(state);
            if (state != sanitized)
            {
                _document.Health[provider] = sanitized;
                changed = true;
            }
        }

        return changed;
    }

    private static CachedRatingEntry Sanitize(CachedRatingEntry entry)
        => entry with
        {
            Ratings = RatingsContract.NormalizeCachedRatings(entry.Ratings),
            SourceUpdatedAt = RatingsContract.NormalizeSourceUpdatedAt(
                entry.SourceUpdatedAt is null ? null : JsonValue.Create(entry.SourceUpdatedAt))
        };

    private static ProviderHealthState Sanitize(ProviderHealthState state)
    {
        var validation = RatingsContract.NormalizeValidation(state.Validation);
        var valid = state.Valid && validation is not ("invalid" or "unchecked");
        return new ProviderHealthState
        {
            Validation = validation,
            Valid = valid,
            // Status responses derive fixed wording from validation/provider;
            // never retain a provider or transport string in the cache.
            Detail = null,
            QuotaLimit = RatingsContract.NormalizeQuota(state.Quota).Limit,
            QuotaRemaining = RatingsContract.NormalizeQuota(state.Quota).Remaining,
            QuotaResetAt = RatingsContract.NormalizeQuota(state.Quota).ResetAt,
            RetryAt = RatingsContract.NormalizeTimestamp(state.RetryAt),
            FailureCount = Math.Clamp(state.FailureCount, 0, ProviderHealthPolicy.MaxFailureCount),
            LastCheckedAt = RatingsContract.NormalizeTimestamp(state.LastCheckedAt)
        };
    }

    private static CacheDocument Load(string path, ILogger logger)
    {
        try
        {
            if (!File.Exists(path))
            {
                return new CacheDocument();
            }

            var parsed = JsonSerializer.Deserialize<CacheDocument>(
                File.ReadAllText(path),
                CompanionJson.CamelCase);
            if (parsed is { Version: DocumentVersion })
            {
                return parsed;
            }

            logger.LogInformation(
                "Ignoring the MediaFlick ratings cache with unsupported version {Version}; ratings will be fetched again",
                parsed?.Version);
            return new CacheDocument();
        }
        catch (IOException exception)
        {
            logger.LogWarning(exception, "Could not read the MediaFlick ratings cache; starting with an empty cache");
            return new CacheDocument();
        }
        catch (UnauthorizedAccessException exception)
        {
            logger.LogWarning(exception, "Could not read the MediaFlick ratings cache; starting with an empty cache");
            return new CacheDocument();
        }
        catch (JsonException exception)
        {
            // JsonException text names a position, not cached values.
            logger.LogWarning(exception, "The MediaFlick ratings cache is corrupt; starting with an empty cache");
            return new CacheDocument();
        }
    }

    /// <summary>Atomically replaces the cache file. Called under _writeLock only.</summary>
    private bool TryWrite(string json)
    {
        try
        {
            var directory = Path.GetDirectoryName(_path);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory);
            }

            var temporary = _path + ".tmp";
            File.WriteAllText(temporary, json);
            File.Move(temporary, _path, true);
            if (_persistFailing)
            {
                _persistFailing = false;
                _logger.LogInformation("The MediaFlick ratings cache is being saved again");
            }

            return true;
        }
        catch (IOException exception)
        {
            // The in-memory cache remains usable. Rating persistence failure
            // must not make the Jellyfin catalogue or this optional overlay fail.
            ReportPersistFailure(exception);
            return false;
        }
        catch (UnauthorizedAccessException exception)
        {
            // Same graceful degradation for a read-only plugin data volume.
            ReportPersistFailure(exception);
            return false;
        }
    }

    private void ReportPersistFailure(Exception exception)
    {
        // Every save retries persistence; warn once per outage.
        if (_persistFailing)
        {
            _logger.LogDebug(exception, "The MediaFlick ratings cache still cannot be saved");
            return;
        }

        _persistFailing = true;
        _logger.LogWarning(
            exception,
            "Could not save the MediaFlick ratings cache; ratings stay cached in memory until the plugin data directory is writable");
    }

    private sealed class CacheDocument
    {
        public int Version { get; set; } = DocumentVersion;

        public Dictionary<string, CachedRatingEntry> Entries { get; set; } =
            new(StringComparer.Ordinal);

        public Dictionary<string, ProviderHealthState> Health { get; set; } =
            new(StringComparer.OrdinalIgnoreCase);
    }
}
