using Jellyfin.Plugin.MediaFlick.Api;
using Jellyfin.Plugin.MediaFlick.Configuration;
using Xunit;

namespace Jellyfin.Plugin.MediaFlick.Tests;

public sealed class CompanionControllerTests
{
    [Fact]
    public void CuratedContractDoesNotDependOnSavedDefinitions()
    {
        var capabilities = InfoController.Capabilities(new PluginConfiguration(), false);

        Assert.Contains("collections-curated-v1", capabilities);
    }

    [Fact]
    public void CollectionsControllerCanBeActivatedByMvc()
    {
        var constructor = Assert.Single(typeof(CollectionsController).GetConstructors());

        Assert.True(constructor.IsPublic);
    }
}
