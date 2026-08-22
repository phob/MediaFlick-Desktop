using Jellyfin.Plugin.MediaFlick.Services;
using MediaBrowser.Model.Tasks;

namespace Jellyfin.Plugin.MediaFlick.ScheduledTasks;

public sealed class CollectionsSyncTask : IScheduledTask
{
    private readonly NativeCollectionSync _sync;

    public CollectionsSyncTask(NativeCollectionSync sync)
    {
        _sync = sync;
    }

    public string Name => "Sync MediaFlick native collections";

    public string Description =>
        "Mirrors the library's TMDB collections into Jellyfin BoxSets and keeps their membership in sync.";

    public string Category => "MediaFlick";

    public string Key => "MediaFlickCollectionsSync";

    public IEnumerable<TaskTriggerInfo> GetDefaultTriggers()
    {
        yield return new TaskTriggerInfo
        {
            Type = TaskTriggerInfoType.StartupTrigger,
            MaxRuntimeTicks = TimeSpan.FromMinutes(30).Ticks
        };
        yield return new TaskTriggerInfo
        {
            Type = TaskTriggerInfoType.IntervalTrigger,
            IntervalTicks = TimeSpan.FromHours(1).Ticks
        };
    }

    public Task ExecuteAsync(IProgress<double> progress, CancellationToken cancellationToken)
        => _sync.SyncAsync(progress, cancellationToken);
}
