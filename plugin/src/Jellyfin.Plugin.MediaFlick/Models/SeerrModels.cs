namespace Jellyfin.Plugin.MediaFlick.Models;

// The Desktop-facing Seerr contract. Every value is rebuilt from upstream
// data into these fixed shapes; no upstream JSON passes through unchanged.
// Desktop mirrors them in ui/src/lib/api/types.ts (SeerrStatusInfo, SeerrResult,
// SeerrMediaDetail, SeerrPage, SeerrRequest, SeerrRequestOptions).

/// <summary>The signed-in user's Seerr state.</summary>
public sealed record SeerrStatusResponse(
    bool Linked,
    bool Mapped,
    SeerrInstanceResponse Instance,
    SeerrUserResponse User,
    SeerrCapabilitiesResponse Capabilities,
    SeerrQuotaResponse? Quota);

public sealed record SeerrInstanceResponse(
    bool Movie4kEnabled,
    bool Series4kEnabled,
    bool PartialRequestsEnabled);

public sealed record SeerrUserResponse(
    int Id,
    string Name,
    string? Avatar,
    string JellyfinUserId);

public sealed record SeerrCapabilityResponse(bool Request, bool AutoApprove);

public sealed record SeerrCapabilitiesResponse(
    SeerrCapabilityResponse Movie,
    SeerrCapabilityResponse Tv,
    SeerrCapabilityResponse Movie4k,
    SeerrCapabilityResponse Tv4k,
    bool AdvancedRequest);

public sealed record SeerrQuotaResponse(SeerrQuotaLimitResponse Movie, SeerrQuotaLimitResponse Tv);

public sealed record SeerrQuotaLimitResponse(
    int? Days,
    int? Limit,
    int Used,
    int? Remaining,
    bool Restricted);

public sealed record SeerrPageResponse<T>(
    int Page,
    int TotalPages,
    int TotalResults,
    IReadOnlyList<T> Results);

/// <summary>
/// One movie or series. Desktop fills <see cref="LibraryItemId"/> from its
/// local catalog; the Companion always answers null.
/// <see cref="Activity"/> explains a <c>processing</c> status: downloading,
/// searching, in-cinemas, unreleased, or awaiting-episodes. It is null for
/// every other status and whenever Radarr or Sonarr cannot say.
/// </summary>
public sealed record SeerrResultResponse(
    string MediaType,
    int? TmdbId,
    string Title,
    int? Year,
    string? Overview,
    string? PosterPath,
    string? BackdropPath,
    double? VoteAverage,
    string Status,
    string Status4k,
    string? LibraryItemId = null,
    string? Activity = null,
    string? Activity4k = null);

public sealed record SeerrGenreResponse(int Id, string Name, IReadOnlyList<string> Backdrops);

public sealed record SeerrSeasonResponse(
    int SeasonNumber,
    string? Name,
    int EpisodeCount,
    string? AirDate,
    string Status,
    string Status4k,
    string? Activity = null,
    string? Activity4k = null);

public sealed record SeerrExternalIdsResponse(string? Imdb, int? Tvdb);

public sealed record SeerrCodeNameResponse(string? Code, string? Name);

public sealed record SeerrCastMemberResponse(
    int? Id,
    string? Name,
    string? Character,
    string? ProfilePath);

public sealed record SeerrTrailerResponse(string Name, string? Key);

public sealed record SeerrReleaseDateResponse(
    string Region,
    string Type,
    string Date,
    string? Certification);

public sealed record SeerrContentRatingResponse(string? Region, string? Rating);

public sealed record SeerrNextEpisodeResponse(
    string? Name,
    string? AirDate,
    int? SeasonNumber,
    int? EpisodeNumber);

public sealed record SeerrMediaDetailResponse(
    string MediaType,
    int? TmdbId,
    string Title,
    string? OriginalTitle,
    int? Year,
    string? Overview,
    string? Tagline,
    string? PosterPath,
    string? BackdropPath,
    double? VoteAverage,
    long? VoteCount,
    string Status,
    string Status4k,
    string? LibraryItemId,
    int? RuntimeMinutes,
    IReadOnlyList<string> Genres,
    IReadOnlyList<SeerrSeasonResponse> Seasons,
    string? ReleaseDate,
    string? FirstAirDate,
    string? LastAirDate,
    string? ProductionStatus,
    bool? InProduction,
    string? SeriesType,
    int? NumberOfSeasons,
    int? NumberOfEpisodes,
    string? OriginalLanguage,
    string? Homepage,
    SeerrExternalIdsResponse ExternalIds,
    long? Budget,
    long? Revenue,
    IReadOnlyList<string> Studios,
    IReadOnlyList<string> Networks,
    IReadOnlyList<string> Creators,
    IReadOnlyList<string> Directors,
    IReadOnlyList<string> Writers,
    IReadOnlyList<SeerrCodeNameResponse> ProductionCountries,
    IReadOnlyList<SeerrCodeNameResponse> SpokenLanguages,
    IReadOnlyList<SeerrCastMemberResponse> Cast,
    SeerrTrailerResponse? Trailer,
    IReadOnlyList<SeerrReleaseDateResponse> ReleaseDates,
    IReadOnlyList<SeerrContentRatingResponse> ContentRatings,
    SeerrNextEpisodeResponse? NextEpisode,
    string? Activity = null,
    string? Activity4k = null);

public sealed record SeerrQualityProfileResponse(int Id, string Name, bool IsDefault);

public sealed record SeerrRequestDestinationResponse(
    int Id,
    string Name,
    bool IsDefault,
    IReadOnlyList<SeerrQualityProfileResponse> Profiles);

public sealed record SeerrRequestOptionsResponse(
    IReadOnlyList<SeerrRequestDestinationResponse> Destinations);

public sealed record SeerrRequestResponse(
    int? Id,
    string Status,
    string MediaType,
    int? TmdbId,
    bool Is4k,
    string? CreatedAt,
    string? UpdatedAt,
    string MediaStatus,
    IReadOnlyList<int> Seasons,
    string? LibraryItemId = null,
    string? MediaActivity = null);

public sealed record SeerrCancelResponse(bool Cancelled, int Id);
