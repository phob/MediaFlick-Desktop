namespace Jellyfin.Plugin.MediaFlick.Tests;

/// <summary>A clock tests move by hand. Timers still use the system clock.</summary>
internal sealed class ManualTime(DateTimeOffset now) : TimeProvider
{
    private DateTimeOffset _now = now;

    public override DateTimeOffset GetUtcNow() => _now;

    public void Advance(TimeSpan by) => _now += by;
}
