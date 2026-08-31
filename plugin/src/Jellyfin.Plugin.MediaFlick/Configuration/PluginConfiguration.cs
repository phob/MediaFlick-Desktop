using MediaBrowser.Model.Plugins;

namespace Jellyfin.Plugin.MediaFlick.Configuration;

public sealed class ServiceConfiguration
{
    public bool Enabled { get; set; }

    public string BaseUrl { get; set; } = string.Empty;

    public string ApiKey { get; set; } = string.Empty;
}

public sealed class PluginConfiguration : BasePluginConfiguration
{
    public ServiceConfiguration Sonarr { get; set; } = new();

    public ServiceConfiguration Radarr { get; set; } = new();

    public ServiceConfiguration Seerr { get; set; } = new();

    public bool AutoImportSeerrUsers { get; set; }

    /// <summary>
    /// ASP.NET Data Protection ciphertext. The plaintext MDBList key is never
    /// written to Jellyfin's plugin configuration XML.
    /// </summary>
    public string ProtectedMdbListApiKey { get; set; } = string.Empty;

    /// <summary>
    /// ASP.NET Data Protection ciphertext for TMDB-backed collection features.
    /// </summary>
    public string ProtectedTmdbApiKey { get; set; } = string.Empty;
}
