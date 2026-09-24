using Jellyfin.Plugin.MediaFlick.Api;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class CompanionControllerTests
{
    [Fact]
    public void CollectionExperienceContractDoesNotDependOnProviderReadiness()
    {
        var capabilities = InfoController.Capabilities(new PluginConfiguration(), false);

        Assert.Contains("collection-experience-v1", capabilities);
        Assert.Contains("franchise-memberships-v1", capabilities);
    }
}
