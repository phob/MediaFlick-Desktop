using System.Text.Json;

namespace Jellyfin.Plugin.MediaFlick.Models;

/// <summary>
/// The MediaFlick API contract is camelCase. Jellyfin's host-wide MVC options
/// serialize PascalCase, so every typed response must opt in explicitly.
/// </summary>
public static class CompanionJson
{
    public static JsonSerializerOptions CamelCase { get; } = new(JsonSerializerDefaults.Web);
}

public sealed record PluginInfoResponse(
    string PluginVersion,
    int ApiVersion,
    IReadOnlyList<string> Capabilities,
    IReadOnlyDictionary<string, bool> Services,
    RatingsCapabilityResponse? Ratings = null);

public sealed record SourceStatus(
    bool Enabled,
    bool Available,
    bool Stale,
    DateTimeOffset? RefreshedAt,
    string? Error);

public sealed record CalendarEntry(
    string Kind,
    string Date,
    string DateKind,
    string Title,
    string? SeriesTitle,
    int? Season,
    int? Episode,
    int? TmdbId,
    int? TvdbId,
    bool Monitored,
    bool HasFile,
    string? PosterUrl,
    string? LibraryItemId = null,
    int? SeriesTmdbId = null,
    int? SeriesTvdbId = null,
    string? SeriesLibraryItemId = null);

public sealed record CalendarResponse(
    IReadOnlyList<CalendarEntry> Entries,
    DateTimeOffset? RefreshedAt,
    IReadOnlyDictionary<string, SourceStatus> Sources,
    string WindowStart,
    string WindowEnd,
    string Provider);

public sealed record SeerrRequestBody(
    string MediaType,
    int TmdbId,
    IReadOnlyList<int>? Seasons,
    bool Is4k,
    int? ServerId,
    int? ProfileId);

public sealed record ServiceConfigurationUpdate(
    bool Enabled,
    string BaseUrl,
    string? ApiKey);

public sealed record CompanionConfigurationUpdate(
    ServiceConfigurationUpdate Sonarr,
    ServiceConfigurationUpdate Radarr,
    ServiceConfigurationUpdate Seerr,
    bool AutoImportSeerrUsers,
    bool NativeCollections);

public sealed record ConnectionTestResponse(string Service, bool Connected, string? Version);
