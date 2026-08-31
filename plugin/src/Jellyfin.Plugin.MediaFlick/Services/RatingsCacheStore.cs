using System.Text.Json;
using System.Text.Json.Nodes;
using Jellyfin.Plugin.MediaFlick.Models;

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

internal sealed class RatingsCacheStore
{
    private const int DocumentVersion = 1;
    private readonly object _lock = new();
    private readonly string _path;
    private CacheDocument _document;

    public RatingsCacheStore(string path)
    {
        _path = path;
        _document = Load(path);
        if (SanitizeLoadedDocument())
        {
            PersistLocked();
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

            PersistLocked();
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
            PersistLocked();
        }
    }

    public void ResetHealth(string provider)
    {
        lock (_lock)
        {
            _document.Health.Remove(provider);
            PersistLocked();
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
            FailureCount = Math.Clamp(state.FailureCount, 0, 10),
            LastCheckedAt = RatingsContract.NormalizeTimestamp(state.LastCheckedAt)
        };
    }

    private static CacheDocument Load(string path)
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
            return parsed is { Version: DocumentVersion }
                ? parsed
                : new CacheDocument();
        }
        catch (IOException)
        {
            return new CacheDocument();
        }
        catch (UnauthorizedAccessException)
        {
            return new CacheDocument();
        }
        catch (JsonException)
        {
            return new CacheDocument();
        }
    }

    private void PersistLocked()
    {
        try
        {
            var directory = Path.GetDirectoryName(_path);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory);
            }

            var temporary = _path + ".tmp";
            File.WriteAllText(
                temporary,
                JsonSerializer.Serialize(_document, CompanionJson.CamelCase));
            File.Move(temporary, _path, true);
        }
        catch (IOException)
        {
            // The in-memory cache remains usable. Rating persistence failure
            // must not make the Jellyfin catalogue or this optional overlay fail.
        }
        catch (UnauthorizedAccessException)
        {
            // Same graceful degradation for a read-only plugin data volume.
        }
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
