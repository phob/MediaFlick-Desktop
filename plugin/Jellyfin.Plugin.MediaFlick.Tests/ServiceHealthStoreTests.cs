using Jellyfin.Plugin.MediaFlick.Services;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class ServiceHealthStoreTests
{
    [Fact]
    public void HealthFollowsTheLatestObservedOutcome()
    {
        var health = new ServiceHealthStore();

        Assert.False(health.IsHealthy("sonarr"));
        Assert.Equal(
            ServiceHealthStore.ServiceHealthState.Unknown,
            health.Get("sonarr").State);

        health.Success("sonarr");

        var success = health.Get("SONARR");
        Assert.True(health.IsHealthy("sonarr"));
        Assert.Equal(ServiceHealthStore.ServiceHealthState.Healthy, success.State);
        Assert.NotNull(success.LastSuccess);
        Assert.Null(success.LastFailure);
        Assert.Null(success.Error);

        health.Failure("sonarr", "request timed out");

        var failure = health.Get("sonarr");
        Assert.False(health.IsHealthy("sonarr"));
        Assert.Equal(ServiceHealthStore.ServiceHealthState.Unhealthy, failure.State);
        Assert.Equal(success.LastSuccess, failure.LastSuccess);
        Assert.NotNull(failure.LastFailure);
        Assert.Equal("request timed out", failure.Error);

        health.Success("sonarr");

        var recovery = health.Get("sonarr");
        Assert.True(health.IsHealthy("sonarr"));
        Assert.Equal(ServiceHealthStore.ServiceHealthState.Healthy, recovery.State);
        Assert.Equal(failure.LastFailure, recovery.LastFailure);
        Assert.Null(recovery.Error);
    }
}
