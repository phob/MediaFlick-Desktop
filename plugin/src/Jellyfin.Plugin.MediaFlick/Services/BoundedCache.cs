namespace Jellyfin.Plugin.MediaFlick.Services;

/// <summary>
/// A thread-safe in-memory cache with a fixed lifetime and entry cap. Expired
/// entries are dropped when read or when the cache fills; past the cap the
/// oldest entries are evicted, so memory stays bounded however many distinct
/// keys clients request.
/// </summary>
internal sealed class BoundedCache<TKey, TValue>
    where TKey : notnull
{
    private readonly object _lock = new();
    private readonly Dictionary<TKey, Entry> _entries;
    private readonly TimeProvider _time;
    private readonly TimeSpan _lifetime;
    private readonly int _capacity;

    public BoundedCache(
        int capacity,
        TimeSpan lifetime,
        TimeProvider time,
        IEqualityComparer<TKey>? comparer = null)
    {
        ArgumentOutOfRangeException.ThrowIfLessThan(capacity, 1);
        _capacity = capacity;
        _lifetime = lifetime;
        _time = time;
        _entries = new Dictionary<TKey, Entry>(comparer);
    }

    public int Count
    {
        get
        {
            lock (_lock)
            {
                return _entries.Count;
            }
        }
    }

    public bool TryGet(TKey key, out TValue value)
    {
        var now = _time.GetUtcNow();
        lock (_lock)
        {
            if (_entries.TryGetValue(key, out var entry))
            {
                if (now - entry.StoredAt < _lifetime)
                {
                    value = entry.Value;
                    return true;
                }

                _entries.Remove(key);
            }
        }

        value = default!;
        return false;
    }

    public void Set(TKey key, TValue value)
    {
        var now = _time.GetUtcNow();
        lock (_lock)
        {
            _entries[key] = new Entry(value, now);
            if (_entries.Count <= _capacity)
            {
                return;
            }

            foreach (var (candidate, entry) in _entries.ToArray())
            {
                if (now - entry.StoredAt >= _lifetime)
                {
                    _entries.Remove(candidate);
                }
            }

            // Trim a little below the cap so a full cache does not rescan on
            // every insert.
            var excess = _entries.Count - Math.Max(1, _capacity - (_capacity / 10));
            if (excess <= 0)
            {
                return;
            }

            foreach (var candidate in _entries
                .OrderBy(static pair => pair.Value.StoredAt)
                .Take(excess)
                .Select(static pair => pair.Key)
                .ToArray())
            {
                _entries.Remove(candidate);
            }
        }
    }

    public void RemoveWhere(Func<TValue, bool> predicate)
    {
        lock (_lock)
        {
            foreach (var (key, entry) in _entries.ToArray())
            {
                if (predicate(entry.Value))
                {
                    _entries.Remove(key);
                }
            }
        }
    }

    public void Clear()
    {
        lock (_lock)
        {
            _entries.Clear();
        }
    }

    private readonly record struct Entry(TValue Value, DateTimeOffset StoredAt);
}
