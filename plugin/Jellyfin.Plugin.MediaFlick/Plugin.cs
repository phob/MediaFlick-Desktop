using System.Globalization;
using Jellyfin.Plugin.MediaFlick.Configuration;
using MediaBrowser.Common.Configuration;
using MediaBrowser.Common.Plugins;
using MediaBrowser.Model.Plugins;
using MediaBrowser.Model.Serialization;

namespace Jellyfin.Plugin.MediaFlick;

public sealed class Plugin : BasePlugin<PluginConfiguration>, IHasWebPages
{
    private readonly object _configurationLock = new();

    public Plugin(IApplicationPaths applicationPaths, IXmlSerializer xmlSerializer)
        : base(applicationPaths, xmlSerializer)
    {
        Instance = this;
    }

    public override string Name => "MediaFlick Companion";

    public override Guid Id => Guid.Parse("11d8f2bb-2b9d-4ce1-8c33-5a0f809dfd2f");

    public static Plugin? Instance { get; private set; }

    /// <summary>
    /// Applies a whole-configuration mutation under one lock. Secret writes and
    /// ordinary dashboard saves cannot accidentally replace one another.
    /// </summary>
    public void MutateConfiguration(Func<PluginConfiguration, PluginConfiguration> mutation)
    {
        ArgumentNullException.ThrowIfNull(mutation);
        lock (_configurationLock)
        {
            UpdateConfiguration(mutation(Configuration));
        }
    }

    public IEnumerable<PluginPageInfo> GetPages()
    {
        yield return new PluginPageInfo
        {
            Name = "MediaFlick Companion",
            EmbeddedResourcePath = string.Format(
                CultureInfo.InvariantCulture,
                "{0}.Configuration.configPage.html",
                GetType().Namespace)
        };
    }
}
