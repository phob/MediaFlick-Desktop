using MediaBrowser.Model.Plugins;

namespace Jellyfin.Plugin.MediaFlick.Configuration;

public sealed class ServiceConfiguration
{
    public bool Enabled { get; set; }

    public string BaseUrl { get; set; } = string.Empty;

    public string ApiKey { get; set; } = string.Empty;
}

/// <summary>
/// One administrator-defined collection. Inline definitions carry TMDB movie
/// ids; MDBList definitions can carry movies and series. The sync marks the
/// Jellyfin BoxSet with a `MediaFlick` provider id carrying [`Id`], so adoption
/// and rename propagation survive dashboard edits.
/// </summary>
public sealed class CuratedCollectionDefinition
{
    public string Id { get; set; } = string.Empty;

    public string Name { get; set; } = string.Empty;

    /// <summary>Comma-separated TMDB movie ids, definition order preserved.</summary>
    public string TmdbIds { get; set; } = string.Empty;

    /// <summary>
    /// Optional MDBList reference: 'username/listname',
    /// 'user/username/listname', or 'official/slug'. When set, movies and
    /// series resolve through the MDBList API instead of the inline list.
    /// </summary>
    public string MdbListSource { get; set; } = string.Empty;
}

public sealed class PluginConfiguration : BasePluginConfiguration
{
    public ServiceConfiguration Sonarr { get; set; } = new();

    public ServiceConfiguration Radarr { get; set; } = new();

    public ServiceConfiguration Seerr { get; set; } = new();

    public bool AutoImportSeerrUsers { get; set; }

    public List<CuratedCollectionDefinition> CuratedCollections { get; set; } = new();

    /// <summary>
    /// Mirrors the library's TMDB collections into Jellyfin's own BoxSet
    /// feature. Existing BoxSets with the same TMDB provider id are adopted,
    /// never duplicated; sets created here are left behind when this is
    /// turned off.
    /// </summary>
    public bool NativeCollections { get; set; }

    /// <summary>
    /// ASP.NET Data Protection ciphertext. The plaintext MDBList key is never
    /// written to Jellyfin's plugin configuration XML.
    /// </summary>
    public string ProtectedMdbListApiKey { get; set; } = string.Empty;

    /// <summary>
    /// ASP.NET Data Protection ciphertext reserved for future TMDB features.
    /// </summary>
    public string ProtectedTmdbApiKey { get; set; } = string.Empty;
}
