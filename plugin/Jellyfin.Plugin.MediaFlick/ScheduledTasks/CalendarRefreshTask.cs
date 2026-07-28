using Jellyfin.Plugin.MediaFlick.Services;
using MediaBrowser.Model.Tasks;

namespace Jellyfin.Plugin.MediaFlick.ScheduledTasks;

public sealed class CalendarRefreshTask : IScheduledTask
{
    private readonly CalendarService _calendar;

    public CalendarRefreshTask(CalendarService calendar)
    {
        _calendar = calendar;
    }

    public string Name => "Refresh MediaFlick release calendar";

    public string Description =>
        "Refreshes the cached Sonarr and Radarr calendar used by MediaFlick clients.";

    public string Category => "MediaFlick";

    public string Key => "MediaFlickCalendarRefresh";

    public IEnumerable<TaskTriggerInfo> GetDefaultTriggers()
    {
        yield return new TaskTriggerInfo
        {
            Type = TaskTriggerInfoType.StartupTrigger,
            MaxRuntimeTicks = TimeSpan.FromMinutes(2).Ticks
        };
        yield return new TaskTriggerInfo
        {
            Type = TaskTriggerInfoType.IntervalTrigger,
            IntervalTicks = TimeSpan.FromMinutes(15).Ticks
        };
    }

    public Task ExecuteAsync(IProgress<double> progress, CancellationToken cancellationToken)
        => _calendar.RefreshAsync(progress, cancellationToken);
}
