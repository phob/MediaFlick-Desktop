using Jellyfin.Plugin.MediaFlick.Services;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class ServiceHealthStoreTests
{
    [Fact]
    public void HealthFollowsTheLatestObservedOutcomeWithoutExpiring()
    {
        var time = new ManualTime(new DateTimeOffset(2026, 9, 23, 12, 0, 0, TimeSpan.Zero));
        var health = new ServiceHealthStore(time);

        Assert.False(health.IsHealthy("sonarr"));

        health.Success("sonarr");
        time.Advance(TimeSpan.FromHours(1));
        Assert.True(health.IsHealthy("sonarr"));

        health.Failure("sonarr", ServiceFailure.Timeout);
        Assert.False(health.IsHealthy("sonarr"));

        health.Success("sonarr");
        Assert.True(health.IsHealthy("sonarr"));
    }
}
