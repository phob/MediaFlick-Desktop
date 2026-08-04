//! The subset of Seerr's API shapes this app consumes.
//!
//! Everything is `#[serde(default)]` for the same reason the Jellyfin models
//! are: Seerr renamed itself from Jellyseerr and keeps moving, and a field that
//! appears or disappears between releases must degrade to missing metadata
//! rather than fail the whole call.

use serde::{Deserialize, Serialize};

/// `mediaServerType` on `/settings/public`: 1 Plex, 2 Jellyfin, 3 Emby,
/// 4 not configured.
pub const MEDIA_SERVER_JELLYFIN: i64 = 2;

/// `GET /api/v1/settings/public` — the only endpoint that answers before login.
///
/// An uninitialized instance answers with just `initialized` and
/// `plexClientIdentifier`, so every other field has to tolerate being absent.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PublicSettings {
    pub initialized: bool,
    pub application_title: Option<String>,
    pub media_server_type: Option<i64>,
    pub local_login: bool,
    pub media_server_login: bool,
    /// Seerr's "enable new Jellyfin sign-in": when off, a user who has never
    /// been imported gets a 403 from login instead of an account.
    /// The `Plex` in the name is a leftover from Overseerr.
    pub new_plex_login: bool,
    #[serde(rename = "movie4kEnabled")]
    pub movie_4k_enabled: bool,
    #[serde(rename = "series4kEnabled")]
    pub series_4k_enabled: bool,
    pub partial_requests_enabled: bool,
}

/// `GET /api/v1/status`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StatusInfo {
    pub version: String,
    pub commit_tag: String,
}

/// `POST /api/v1/auth/jellyfin/quickconnect/initiate`.
///
/// Only on builds carrying the Quick Connect login routes — they are absent
/// from v3.3.0, the latest stable release — so this is feature-detected and
/// never required. `code` is approved on our own Jellyfin server; `secret` is
/// what Seerr later redeems for a session.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QuickConnectHandshake {
    pub code: String,
    pub secret: String,
}

impl QuickConnectHandshake {
    /// A handshake missing either half cannot be completed, and is treated the
    /// same as a route that is not there at all.
    pub fn is_usable(&self) -> bool {
        !self.code.trim().is_empty() && !self.secret.trim().is_empty()
    }
}

/// `GET /api/v1/auth/me`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeerrUser {
    pub id: i64,
    pub email: Option<String>,
    pub username: Option<String>,
    pub jellyfin_username: Option<String>,
    pub display_name: Option<String>,
    pub avatar: Option<String>,
    pub permissions: u64,
    /// The Jellyfin account this Seerr user is backed by. The link is bound to
    /// it, which is what keeps user A's cookie from serving user B.
    pub jellyfin_user_id: Option<String>,
}

impl SeerrUser {
    /// What to show in the UI, in Seerr's own order of preference.
    pub fn preferred_name(&self) -> &str {
        [
            self.display_name.as_deref(),
            self.username.as_deref(),
            self.jellyfin_username.as_deref(),
            self.email.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("Seerr user")
    }
}

/// `GET /api/v1/user/{id}/quota`. `/auth/me` carries the permission mask but
/// not current usage, so this is a second call.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UserQuota {
    pub movie: QuotaStatus,
    pub tv: QuotaStatus,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QuotaStatus {
    /// Length of the rolling window, in days. Absent means unlimited.
    pub days: Option<i64>,
    pub limit: Option<i64>,
    pub used: i64,
    pub remaining: Option<i64>,
    /// Seerr's own verdict: the quota is currently exhausted.
    pub restricted: bool,
}

/// Seerr's `MediaStatus` (`server/constants/media.ts`), carried by every
/// `mediaInfo` block. Absent means Seerr has never heard of the title, which is
/// the same thing as `UNKNOWN` for our purposes.
pub mod media_status {
    pub const UNKNOWN: i64 = 1;
    pub const PENDING: i64 = 2;
    pub const PROCESSING: i64 = 3;
    pub const PARTIALLY_AVAILABLE: i64 = 4;
    pub const AVAILABLE: i64 = 5;
    pub const BLACKLISTED: i64 = 6;
}

/// The name the UI switches on. Anything unrecognized reads as `unknown`, so a
/// status Seerr adds later degrades to "you may request this" rather than to a
/// blank badge.
pub fn status_name(status: i64) -> &'static str {
    match status {
        media_status::PENDING => "pending",
        media_status::PROCESSING => "processing",
        media_status::PARTIALLY_AVAILABLE => "partial",
        media_status::AVAILABLE => "available",
        media_status::BLACKLISTED => "blacklisted",
        _ => "unknown",
    }
}

/// Seerr's `MediaRequestStatus`.
pub fn request_status_name(status: i64) -> &'static str {
    match status {
        1 => "pending",
        2 => "approved",
        3 => "declined",
        4 => "failed",
        _ => "unknown",
    }
}

/// The two TMDB namespaces this app joins against the local cache. `person`
/// results exist too and are dropped before the join: they have no Jellyfin
/// counterpart and nothing to request.
pub const MOVIE: &str = "movie";
pub const TV: &str = "tv";

/// The local `items.kind` a Seerr media type joins to. Movie and TV ids live in
/// separate TMDB namespaces, so the join is keyed on the pair, never the id.
pub fn library_kind(media_type: &str) -> Option<&'static str> {
    match media_type {
        MOVIE => Some("Movie"),
        TV => Some("Series"),
        _ => None,
    }
}

/// One page of `/api/v1/search` or `/api/v1/discover/*`; both answer the same
/// envelope.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SearchPage {
    pub page: i64,
    pub total_pages: i64,
    pub total_results: i64,
    pub results: Vec<SearchResult>,
}

/// A TMDB title as Seerr returns it, with whatever Seerr itself knows about its
/// availability folded in under `mediaInfo`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SearchResult {
    /// The TMDB id, which is only unique *within* `media_type`.
    pub id: i64,
    pub media_type: String,
    /// Combined person credits repeat titles for separate characters. These
    /// fields let the exact-person caller reject non-cast/adult rows before
    /// deduplicating by the stable `(media_type, id)` identity.
    pub adult: bool,
    pub character: Option<String>,
    /// Movies carry `title`, series carry `name`; people carry `name` too,
    /// which is one reason they are dropped rather than rendered.
    pub title: Option<String>,
    pub name: Option<String>,
    pub release_date: Option<String>,
    pub first_air_date: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
    pub vote_average: Option<f64>,
    pub media_info: Option<MediaInfo>,
}

/// Seerr's exact TMDB-person endpoint. Crew is intentionally not represented:
/// an actor search consumes only `cast` relationships.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PersonCombinedCredits {
    pub id: i64,
    pub cast: Vec<SearchResult>,
}

impl SearchResult {
    pub fn display_title(&self) -> &str {
        [self.title.as_deref(), self.name.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|value| !value.is_empty())
            .unwrap_or("Untitled")
    }

    pub fn year(&self) -> Option<i64> {
        year_of(
            self.release_date
                .as_deref()
                .or(self.first_air_date.as_deref()),
        )
    }

    /// Whether this result addresses something requestable. `person` results
    /// share the envelope and must not reach the library join.
    pub fn is_media(&self) -> bool {
        library_kind(&self.media_type).is_some()
    }
}

/// `mediaInfo`: what Seerr knows about a title it has been asked for before.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaInfo {
    /// Seerr's own media row id, not the TMDB one.
    pub id: Option<i64>,
    pub tmdb_id: Option<i64>,
    pub media_type: Option<String>,
    pub status: i64,
    #[serde(rename = "status4k")]
    pub status_4k: i64,
    /// Per-season availability, which is what makes a partial series
    /// requestable one season at a time.
    pub seasons: Vec<SeasonStatus>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeasonStatus {
    pub season_number: i64,
    pub status: i64,
    #[serde(rename = "status4k")]
    pub status_4k: i64,
}

/// `GET /api/v1/movie/{id}` and `GET /api/v1/tv/{id}`, which differ only in
/// which of the paired fields they populate.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaDetail {
    pub id: i64,
    pub title: Option<String>,
    pub name: Option<String>,
    pub original_title: Option<String>,
    pub original_name: Option<String>,
    pub overview: Option<String>,
    pub tagline: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub release_date: Option<String>,
    pub first_air_date: Option<String>,
    pub last_air_date: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub series_type: Option<String>,
    pub in_production: bool,
    pub runtime: Option<i64>,
    pub episode_run_time: Vec<i64>,
    pub genres: Vec<NamedId>,
    pub vote_average: Option<f64>,
    pub vote_count: Option<i64>,
    pub number_of_seasons: Option<i64>,
    pub number_of_episodes: Option<i64>,
    pub original_language: Option<String>,
    pub homepage: Option<String>,
    pub budget: Option<i64>,
    pub revenue: Option<i64>,
    pub production_companies: Vec<NamedId>,
    pub networks: Vec<NamedId>,
    pub created_by: Vec<NamedId>,
    pub production_countries: Vec<ProductionCountry>,
    pub spoken_languages: Vec<SpokenLanguage>,
    pub related_videos: Vec<RelatedVideo>,
    pub credits: Credits,
    pub releases: MovieReleases,
    pub content_ratings: ContentRatings,
    pub next_episode_to_air: Option<NextEpisode>,
    pub seasons: Vec<SeasonDetail>,
    pub media_info: Option<MediaInfo>,
}

impl MediaDetail {
    pub fn display_title(&self) -> &str {
        [self.title.as_deref(), self.name.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|value| !value.is_empty())
            .unwrap_or("Untitled")
    }

    pub fn year(&self) -> Option<i64> {
        year_of(
            self.release_date
                .as_deref()
                .or(self.first_air_date.as_deref()),
        )
    }

    /// Movies carry one runtime; series carry a list of typical ones.
    pub fn runtime_minutes(&self) -> Option<i64> {
        self.runtime
            .filter(|minutes| *minutes > 0)
            .or_else(|| self.episode_run_time.first().copied())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NamedId {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProductionCountry {
    #[serde(rename = "iso_3166_1")]
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SpokenLanguage {
    #[serde(rename = "iso_639_1")]
    pub code: String,
    pub name: String,
    pub english_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RelatedVideo {
    pub site: String,
    pub key: String,
    pub name: String,
    pub size: i64,
    #[serde(rename = "type")]
    pub video_type: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Credits {
    pub cast: Vec<Credit>,
    pub crew: Vec<Credit>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Credit {
    pub id: i64,
    pub name: String,
    pub character: Option<String>,
    pub job: Option<String>,
    pub department: Option<String>,
    pub profile_path: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MovieReleases {
    pub results: Vec<ReleaseCountry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ReleaseCountry {
    #[serde(rename = "iso_3166_1")]
    pub region: String,
    pub release_dates: Vec<ReleaseDate>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ReleaseDate {
    pub certification: String,
    pub release_date: String,
    #[serde(rename = "type")]
    pub release_type: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ContentRatings {
    pub results: Vec<ContentRating>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ContentRating {
    #[serde(rename = "iso_3166_1")]
    pub region: String,
    pub rating: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NextEpisode {
    pub name: String,
    pub air_date: Option<String>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,
}

/// One linked Radarr or Sonarr instance from `GET /api/v1/service/{kind}`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DownloadService {
    pub id: i64,
    pub name: String,
    #[serde(rename = "is4k")]
    pub is_4k: bool,
    pub is_default: bool,
    pub active_profile_id: i64,
}

/// The quality profiles Seerr reads from one linked Radarr/Sonarr instance.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DownloadServiceDetail {
    pub server: DownloadService,
    pub profiles: Vec<QualityProfile>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QualityProfile {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeasonDetail {
    pub id: i64,
    pub season_number: i64,
    pub name: Option<String>,
    pub episode_count: i64,
    pub air_date: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
}

/// `GET /api/v1/request` and the row `POST /api/v1/request` answers with.
///
/// Deliberately carries no title: Seerr's request rows reference a media row by
/// TMDB id and nothing more, so the UI resolves titles through the media detail
/// endpoint — one cached query per distinct title rather than a blocking fan-out
/// on this thread.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaRequest {
    pub id: i64,
    pub status: i64,
    #[serde(rename = "type")]
    pub media_type: String,
    pub is4k: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub media: Option<MediaInfo>,
    pub seasons: Vec<RequestedSeason>,
    pub requested_by: Option<SeerrUser>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RequestedSeason {
    pub season_number: i64,
    pub status: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RequestPage {
    pub page_info: PageInfo,
    pub results: Vec<MediaRequest>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PageInfo {
    pub pages: i64,
    pub page_size: i64,
    pub results: i64,
    pub page: i64,
}

/// The leading year of a TMDB date, which is `YYYY-MM-DD` or empty.
fn year_of(date: Option<&str>) -> Option<i64> {
    date?.get(..4)?.parse().ok()
}

/// Seerr's permission mask (`server/lib/permissions.ts`). Only the bits this
/// app acts on are named.
pub mod permission {
    pub const ADMIN: u64 = 2;
    pub const REQUEST: u64 = 32;
    pub const AUTO_APPROVE: u64 = 128;
    pub const AUTO_APPROVE_MOVIE: u64 = 256;
    pub const AUTO_APPROVE_TV: u64 = 512;
    pub const REQUEST_4K: u64 = 1024;
    pub const REQUEST_4K_MOVIE: u64 = 2048;
    pub const REQUEST_4K_TV: u64 = 4096;
    pub const REQUEST_ADVANCED: u64 = 8192;
    pub const AUTO_APPROVE_4K: u64 = 32768;
    pub const AUTO_APPROVE_4K_MOVIE: u64 = 65536;
    pub const AUTO_APPROVE_4K_TV: u64 = 131072;
    pub const REQUEST_MOVIE: u64 = 262144;
    pub const REQUEST_TV: u64 = 524288;
}

/// What one media kind may do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub request: bool,
    pub auto_approve: bool,
}

/// Seerr's permissions are per-type — movie, TV, each with a 4K variant and an
/// auto-approve bit — so this cannot collapse to a `canRequest` boolean.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub movie: Capability,
    pub tv: Capability,
    #[serde(rename = "movie4k")]
    pub movie_4k: Capability,
    #[serde(rename = "tv4k")]
    pub tv_4k: Capability,
    pub advanced_request: bool,
}

impl Capabilities {
    /// Derives what the user may ask for from their permission mask and the
    /// instance's own 4K switches: a 4K permission on an instance with 4K
    /// turned off is not something to offer.
    pub fn derive(permissions: u64, movie_4k_enabled: bool, series_4k_enabled: bool) -> Self {
        let admin = permissions & permission::ADMIN != 0;
        let has = |bits: u64| admin || permissions & bits != 0;
        Self {
            movie: Capability {
                request: has(permission::REQUEST | permission::REQUEST_MOVIE),
                auto_approve: has(permission::AUTO_APPROVE | permission::AUTO_APPROVE_MOVIE),
            },
            tv: Capability {
                request: has(permission::REQUEST | permission::REQUEST_TV),
                auto_approve: has(permission::AUTO_APPROVE | permission::AUTO_APPROVE_TV),
            },
            movie_4k: Capability {
                request: movie_4k_enabled
                    && has(permission::REQUEST_4K | permission::REQUEST_4K_MOVIE),
                auto_approve: movie_4k_enabled
                    && has(permission::AUTO_APPROVE_4K | permission::AUTO_APPROVE_4K_MOVIE),
            },
            tv_4k: Capability {
                request: series_4k_enabled
                    && has(permission::REQUEST_4K | permission::REQUEST_4K_TV),
                auto_approve: series_4k_enabled
                    && has(permission::AUTO_APPROVE_4K | permission::AUTO_APPROVE_4K_TV),
            },
            advanced_request: has(permission::REQUEST_ADVANCED),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Capabilities, MediaDetail, PublicSettings, SearchPage, SeerrUser, library_kind, permission,
        status_name,
    };

    #[test]
    fn an_uninitialized_instance_still_deserializes() {
        let settings: PublicSettings =
            serde_json::from_str(r#"{"initialized":false,"plexClientIdentifier":"abc"}"#)
                .expect("settings");
        assert!(!settings.initialized);
        assert_eq!(settings.media_server_type, None);
        assert!(!settings.partial_requests_enabled);
    }

    #[test]
    fn the_live_public_settings_shape_is_read_whole() {
        let settings: PublicSettings = serde_json::from_str(
            r#"{"initialized":true,"applicationTitle":"Seerr","mediaServerType":2,
                "localLogin":true,"mediaServerLogin":true,"newPlexLogin":true,
                "movie4kEnabled":false,"series4kEnabled":false,"partialRequestsEnabled":true}"#,
        )
        .expect("settings");
        assert!(settings.initialized);
        assert_eq!(settings.media_server_type, Some(2));
        assert_eq!(settings.application_title.as_deref(), Some("Seerr"));
        assert!(settings.new_plex_login);
        assert!(!settings.movie_4k_enabled);
        assert!(settings.partial_requests_enabled);
    }

    #[test]
    fn a_plain_user_may_request_but_not_approve() {
        let capabilities = Capabilities::derive(permission::REQUEST, false, false);
        assert!(capabilities.movie.request);
        assert!(capabilities.tv.request);
        assert!(!capabilities.movie.auto_approve);
        assert!(!capabilities.tv.auto_approve);
    }

    #[test]
    fn per_type_bits_do_not_leak_across_types() {
        let capabilities = Capabilities::derive(permission::REQUEST_MOVIE, false, false);
        assert!(capabilities.movie.request);
        assert!(!capabilities.tv.request);

        let capabilities = Capabilities::derive(permission::AUTO_APPROVE_TV, false, false);
        assert!(capabilities.tv.auto_approve);
        assert!(!capabilities.movie.auto_approve);
    }

    #[test]
    fn admins_may_do_everything_the_instance_allows() {
        let capabilities = Capabilities::derive(permission::ADMIN, true, true);
        assert!(capabilities.movie.request && capabilities.movie.auto_approve);
        assert!(capabilities.tv.request && capabilities.tv.auto_approve);
        assert!(capabilities.movie_4k.request && capabilities.tv_4k.request);
        assert!(capabilities.advanced_request);
    }

    #[test]
    fn advanced_request_permission_is_reported_without_granting_media_permissions() {
        let capabilities = Capabilities::derive(permission::REQUEST_ADVANCED, true, true);
        assert!(capabilities.advanced_request);
        assert!(!capabilities.movie.request);
        assert!(!capabilities.tv.request);
    }

    #[test]
    fn four_k_permissions_stay_hidden_while_the_instance_has_four_k_off() {
        let mask = permission::REQUEST_4K | permission::AUTO_APPROVE_4K;
        let capabilities = Capabilities::derive(mask, false, false);
        assert!(!capabilities.movie_4k.request);
        assert!(!capabilities.tv_4k.request);
        assert!(!capabilities.movie_4k.auto_approve);

        let capabilities = Capabilities::derive(mask, true, false);
        assert!(capabilities.movie_4k.request);
        assert!(!capabilities.tv_4k.request);
    }

    #[test]
    fn a_user_with_no_permissions_may_do_nothing() {
        let capabilities = Capabilities::derive(0, true, true);
        assert_eq!(capabilities, Capabilities::default());
    }

    #[test]
    fn the_display_name_falls_back_through_seerrs_own_order() {
        let user: SeerrUser = serde_json::from_str(
            r#"{"id":3,"email":"a@b.c","jellyfinUsername":"pho","displayName":"  ","permissions":32}"#,
        )
        .expect("user");
        assert_eq!(user.id, 3);
        assert_eq!(user.preferred_name(), "pho");
        assert_eq!(SeerrUser::default().preferred_name(), "Seerr user");
    }

    #[test]
    fn a_search_page_mixes_movies_series_and_people() {
        let page: SearchPage = serde_json::from_str(
            r#"{"page":1,"totalPages":2,"totalResults":3,"results":[
                {"id":603,"mediaType":"movie","title":"The Matrix","releaseDate":"1999-03-30",
                 "posterPath":"/f89U3ADr1oiB1s9GkdPOEpXUk5H.jpg","voteAverage":8.2,
                 "mediaInfo":{"id":4,"tmdbId":603,"status":5,"status4k":1}},
                {"id":1396,"mediaType":"tv","name":"Breaking Bad","firstAirDate":"2008-01-20"},
                {"id":6384,"mediaType":"person","name":"Keanu Reeves"}]}"#,
        )
        .expect("page");
        assert_eq!(page.total_pages, 2);
        assert_eq!(page.results[0].display_title(), "The Matrix");
        assert_eq!(page.results[0].year(), Some(1999));
        assert_eq!(page.results[1].display_title(), "Breaking Bad");
        assert_eq!(page.results[1].year(), Some(2008));
        // The person shares the envelope but has nothing to join or request.
        assert!(page.results[0].is_media() && page.results[1].is_media());
        assert!(!page.results[2].is_media());
    }

    /// Movie and TV ids are separate TMDB namespaces, so the local join is
    /// keyed on the pair — the id alone would collide across them.
    #[test]
    fn media_types_map_to_the_local_kind_they_can_join_against() {
        assert_eq!(library_kind("movie"), Some("Movie"));
        assert_eq!(library_kind("tv"), Some("Series"));
        assert_eq!(library_kind("person"), None);
        assert_eq!(library_kind("collection"), None);
    }

    #[test]
    fn a_result_seerr_has_never_heard_of_reads_as_unknown() {
        let page: SearchPage =
            serde_json::from_str(r#"{"results":[{"id":1,"mediaType":"movie","title":"New"}]}"#)
                .expect("page");
        assert!(page.results[0].media_info.is_none());
        assert_eq!(status_name(0), "unknown");
        assert_eq!(status_name(4), "partial");
        assert_eq!(status_name(5), "available");
        // A status added by a later Seerr degrades to "you may request this".
        assert_eq!(status_name(99), "unknown");
    }

    #[test]
    fn a_series_detail_carries_its_seasons_and_their_availability() {
        let detail: MediaDetail = serde_json::from_str(
            r#"{"id":1396,"name":"Breaking Bad","firstAirDate":"2008-01-20",
                "episodeRunTime":[45,47],"numberOfSeasons":5,
                "genres":[{"id":18,"name":"Drama"}],
                "seasons":[{"id":1,"seasonNumber":0,"name":"Specials","episodeCount":9},
                           {"id":2,"seasonNumber":1,"name":"Season 1","episodeCount":7}],
                "mediaInfo":{"tmdbId":1396,"status":4,"status4k":1,
                             "seasons":[{"seasonNumber":1,"status":5,"status4k":1}]}}"#,
        )
        .expect("detail");
        assert_eq!(detail.display_title(), "Breaking Bad");
        assert_eq!(detail.year(), Some(2008));
        assert_eq!(detail.runtime_minutes(), Some(45));
        assert_eq!(detail.seasons.len(), 2);
        let info = detail.media_info.expect("media info");
        assert_eq!(status_name(info.status), "partial");
        assert_eq!(info.seasons[0].season_number, 1);
        assert_eq!(status_name(info.seasons[0].status), "available");
    }

    #[test]
    fn a_movie_detail_uses_the_other_half_of_every_paired_field() {
        let detail: MediaDetail = serde_json::from_str(
            r#"{"id":603,"title":"The Matrix","releaseDate":"1999-03-30","runtime":136}"#,
        )
        .expect("detail");
        assert_eq!(detail.display_title(), "The Matrix");
        assert_eq!(detail.year(), Some(1999));
        assert_eq!(detail.runtime_minutes(), Some(136));
        assert!(detail.seasons.is_empty());
        assert_eq!(MediaDetail::default().year(), None);
    }
}
