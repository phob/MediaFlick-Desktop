using Jellyfin.Plugin.MediaFlick.Models;

namespace Jellyfin.Plugin.MediaFlick.Services;

public sealed class CalendarCache
{
    private readonly object _gate = new();
    private CalendarState _state = CalendarState.Empty;

    public CalendarState Snapshot()
    {
        lock (_gate)
        {
            return _state;
        }
    }

    public void ReplaceSource(
        string source,
        IReadOnlyList<CalendarEntry> entries,
        DateOnly windowStart,
        DateOnly windowEnd,
        DateTimeOffset refreshedAt)
    {
        lock (_gate)
        {
            var bySource = new Dictionary<string, IReadOnlyList<CalendarEntry>>(
                _state.BySource,
                StringComparer.OrdinalIgnoreCase)
            {
                [source] = entries
            };
            var sources = new Dictionary<string, SourceStatus>(
                _state.Sources,
                StringComparer.OrdinalIgnoreCase)
            {
                [source] = new SourceStatus(true, true, false, refreshedAt, null)
            };
            _state = new CalendarState(bySource, sources, windowStart, windowEnd, refreshedAt);
        }
    }

    public void MarkFailed(string source, bool enabled, string error)
    {
        lock (_gate)
        {
            var sources = new Dictionary<string, SourceStatus>(
                _state.Sources,
                StringComparer.OrdinalIgnoreCase);
            var previous = sources.GetValueOrDefault(source);
            sources[source] = new SourceStatus(
                enabled,
                false,
                previous?.RefreshedAt is not null,
                previous?.RefreshedAt,
                error);
            _state = _state with { Sources = sources };
        }
    }

    public sealed record CalendarState(
        IReadOnlyDictionary<string, IReadOnlyList<CalendarEntry>> BySource,
        IReadOnlyDictionary<string, SourceStatus> Sources,
        DateOnly WindowStart,
        DateOnly WindowEnd,
        DateTimeOffset? RefreshedAt)
    {
        public static CalendarState Empty { get; } = new(
            new Dictionary<string, IReadOnlyList<CalendarEntry>>(StringComparer.OrdinalIgnoreCase),
            new Dictionary<string, SourceStatus>(StringComparer.OrdinalIgnoreCase),
            DateOnly.FromDateTime(DateTime.UtcNow).AddDays(-7),
            DateOnly.FromDateTime(DateTime.UtcNow).AddDays(60),
            null);
    }
}
