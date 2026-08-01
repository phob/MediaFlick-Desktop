//! Seerr — the project formerly called Jellyseerr — as a request backend.
//!
//! MediaFlick is a *client* to Seerr, not a second \*arr orchestrator: Seerr owns
//! the quality profiles, root folders, approval rules and quotas. The credential
//! held here is the user's own session cookie, never an instance-wide API key,
//! which by Seerr's own documentation grants administrator access.
//!
//! The session is bound to one Jellyfin account. Every acquisition of a client
//! re-checks that binding, so signing out — or signing in as somebody else —
//! cannot leave user A's Seerr cookie serving user B.

pub mod api;
pub mod headless;

use std::sync::{Arc, RwLock};

use serde_json::{Value, json};

use crate::jellyfin::api::JellyfinClient;
use crate::jellyfin::api::auth as jellyfin_auth;
use crate::library::{Library, SeerrConfig, StoredCredentials};
use crate::preferences::normalize_server_url;

use api::client::{SeerrClient, SessionCookies};
use api::error::SeerrError;
use api::model::{
    self as model, Capabilities, DownloadService, DownloadServiceDetail, MEDIA_SERVER_JELLYFIN,
    MediaDetail, MediaInfo, MediaRequest, PublicSettings, QuickConnectHandshake, RequestPage,
    SearchPage, SearchResult, SeerrUser, StatusInfo, UserQuota,
};

/// Seerr's own login, present in every release.
const LOGIN_PATH: &str = "auth/jellyfin";
/// The Quick Connect login pair, present only on builds newer than v3.3.0.
const QUICK_CONNECT_INITIATE: &str = "auth/jellyfin/quickconnect/initiate";
const QUICK_CONNECT_AUTHENTICATE: &str = "auth/jellyfin/quickconnect/authenticate";

/// An advanced Seerr request pins the title to one linked Radarr/Sonarr
/// destination and one quality profile owned by that destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestProfileSelection {
    pub server_id: i64,
    pub profile_id: i64,
}

/// The discovery rows the UI can ask for. An enum rather than a string so a
/// path segment from the page can never reach the address unchecked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverKind {
    Trending,
    Movies,
    Tv,
    UpcomingMovies,
    UpcomingTv,
}

impl DiscoverKind {
    pub fn from_id(value: &str) -> Option<Self> {
        match value {
            "trending" => Some(Self::Trending),
            "movies" => Some(Self::Movies),
            "tv" => Some(Self::Tv),
            "upcoming-movies" => Some(Self::UpcomingMovies),
            "upcoming-tv" => Some(Self::UpcomingTv),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Trending => "trending",
            Self::Movies => "movies",
            Self::Tv => "tv",
            Self::UpcomingMovies => "upcoming-movies",
            Self::UpcomingTv => "upcoming-tv",
        }
    }

    fn path(self) -> &'static str {
        match self {
            Self::Trending => "discover/trending",
            Self::Movies => "discover/movies",
            Self::Tv => "discover/tv",
            Self::UpcomingMovies => "discover/movies/upcoming",
            Self::UpcomingTv => "discover/tv/upcoming",
        }
    }
}

/// The small, allowlisted set of Seerr discovery controls exposed by the UI.
///
/// Keeping these as application-level names means neither the app scheme nor
/// the Companion plugin becomes a general query-string proxy to Seerr.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoverOptions {
    genre: Option<i64>,
    sort: Option<DiscoverSort>,
    min_rating: Option<u8>,
    release_decade: Option<u16>,
    media_type: Option<TrendingMediaType>,
    time_window: Option<TrendingWindow>,
}

const EARLIEST_RELEASE_DECADE: u16 = 1800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UtcDate {
    year: u16,
    month: u8,
    day: u8,
}

impl UtcDate {
    fn today() -> Self {
        let days = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() / 86_400)
            .unwrap_or_default();
        Self::from_unix_days(i64::try_from(days).unwrap_or_default())
    }

    // Gregorian civil date conversion by Howard Hinnant. Keeping this tiny
    // avoids adding a date-time dependency solely to cap one query parameter.
    fn from_unix_days(days: i64) -> Self {
        let days = days + 719_468;
        let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
        let day_of_era = days - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let mut year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_prime = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
        let month = month_prime + if month_prime < 10 { 3 } else { -9 };
        year += i64::from(month <= 2);
        Self {
            year: u16::try_from(year).unwrap_or_default(),
            month: u8::try_from(month).unwrap_or_default(),
            day: u8::try_from(day).unwrap_or_default(),
        }
    }

    fn decade(self) -> u16 {
        self.year / 10 * 10
    }

    fn iso8601(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscoverSort {
    Popular,
    Rating,
    Newest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrendingMediaType {
    All,
    Movie,
    Tv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrendingWindow {
    Day,
    Week,
}

impl DiscoverOptions {
    pub fn from_values(
        genre: Option<&str>,
        sort: Option<&str>,
        min_rating: Option<&str>,
        release_decade: Option<&str>,
        media_type: Option<&str>,
        time_window: Option<&str>,
    ) -> Result<Self, String> {
        let genre = genre
            .map(|value| {
                value
                    .parse::<i64>()
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| "genre must be a positive number".to_string())
            })
            .transpose()?;
        let sort = sort
            .map(|value| match value {
                "popular" => Ok(DiscoverSort::Popular),
                "rating" => Ok(DiscoverSort::Rating),
                "newest" => Ok(DiscoverSort::Newest),
                _ => Err("unknown discovery sort".to_string()),
            })
            .transpose()?;
        let min_rating = min_rating
            .map(|value| {
                value
                    .parse::<u8>()
                    .ok()
                    .filter(|value| *value <= 10)
                    .ok_or_else(|| "minimum rating must be between 0 and 10".to_string())
            })
            .transpose()?;
        // Keep the public contract narrower than arbitrary upstream dates.
        // The UI starts film at 1800 and television at 1900, while this shared
        // boundary safely accepts both media types and rejects future decades.
        let current_decade = UtcDate::today().decade();
        let release_decade = release_decade
            .map(|value| {
                value
                    .parse::<u16>()
                    .ok()
                    .filter(|value| {
                        value.is_multiple_of(10)
                            && (EARLIEST_RELEASE_DECADE..=current_decade).contains(value)
                    })
                    .ok_or_else(|| {
                        format!(
                            "release decade must be a ten-year start from {EARLIEST_RELEASE_DECADE} through {current_decade}"
                        )
                    })
            })
            .transpose()?;
        let media_type = media_type
            .map(|value| match value {
                "all" => Ok(TrendingMediaType::All),
                "movie" => Ok(TrendingMediaType::Movie),
                "tv" => Ok(TrendingMediaType::Tv),
                _ => Err("unknown trending media type".to_string()),
            })
            .transpose()?;
        let time_window = time_window
            .map(|value| match value {
                "day" => Ok(TrendingWindow::Day),
                "week" => Ok(TrendingWindow::Week),
                _ => Err("unknown trending time window".to_string()),
            })
            .transpose()?;

        Ok(Self {
            genre,
            sort,
            min_rating,
            release_decade,
            media_type,
            time_window,
        })
    }

    /// Query pairs accepted by Seerr's documented discovery routes.
    pub fn query_pairs(&self, kind: DiscoverKind, page: i64) -> Vec<(&'static str, String)> {
        self.query_pairs_for(kind, page, false, UtcDate::today())
    }

    /// The Companion API keeps the application-level decade allowlist rather
    /// than exposing arbitrary upstream date strings. A current plugin expands
    /// this value to the same Seerr date pair as a direct session.
    pub fn companion_query_pairs(
        &self,
        kind: DiscoverKind,
        page: i64,
    ) -> Vec<(&'static str, String)> {
        self.query_pairs_for(kind, page, true, UtcDate::today())
    }

    fn query_pairs_for(
        &self,
        kind: DiscoverKind,
        page: i64,
        companion: bool,
        today: UtcDate,
    ) -> Vec<(&'static str, String)> {
        let mut query = vec![("page", page.clamp(1, 1_000).to_string())];

        match kind {
            DiscoverKind::Trending => {
                if let Some(media_type) = self.media_type {
                    query.push((
                        "mediaType",
                        match media_type {
                            TrendingMediaType::All => "all",
                            TrendingMediaType::Movie => "movie",
                            TrendingMediaType::Tv => "tv",
                        }
                        .to_string(),
                    ));
                }
                if let Some(time_window) = self.time_window {
                    query.push((
                        "timeWindow",
                        match time_window {
                            TrendingWindow::Day => "day",
                            TrendingWindow::Week => "week",
                        }
                        .to_string(),
                    ));
                }
            }
            DiscoverKind::Movies | DiscoverKind::Tv => {
                if let Some(genre) = self.genre {
                    query.push(("genre", genre.to_string()));
                }
                if let Some(decade) = self.release_decade {
                    if companion {
                        query.push(("releaseDecade", decade.to_string()));
                    } else {
                        let upper_bound = if decade == today.decade() {
                            today.iso8601()
                        } else {
                            format!("{}-12-31", decade + 9)
                        };
                        let (gte, lte) = match kind {
                            DiscoverKind::Movies => {
                                ("primaryReleaseDateGte", "primaryReleaseDateLte")
                            }
                            DiscoverKind::Tv => ("firstAirDateGte", "firstAirDateLte"),
                            _ => unreachable!(),
                        };
                        query.push((gte, format!("{decade:04}-01-01")));
                        query.push((lte, upper_bound));
                    }
                }
                if let Some(sort) = self.sort {
                    let value = match (sort, kind) {
                        (DiscoverSort::Popular, _) => "popularity.desc",
                        (DiscoverSort::Rating, _) => "vote_average.desc",
                        (DiscoverSort::Newest, DiscoverKind::Movies) => "primary_release_date.desc",
                        (DiscoverSort::Newest, DiscoverKind::Tv) => "first_air_date.desc",
                        (DiscoverSort::Newest, _) => unreachable!(),
                    };
                    query.push(("sortBy", value.to_string()));
                    // TMDB's vote-average sort otherwise promotes titles with
                    // a single perfect vote above established favourites.
                    if sort == DiscoverSort::Rating {
                        query.push(("voteCountGte", "50".to_string()));
                    }
                }
                if let Some(min_rating) = self.min_rating {
                    query.push(("voteAverageGte", min_rating.to_string()));
                }
            }
            DiscoverKind::UpcomingMovies | DiscoverKind::UpcomingTv => {}
        }

        query
    }
}

#[derive(Debug, Clone, Default)]
struct SeerrState {
    /// Opaque `seerr_config.updated_at` revision used for optimistic writes.
    revision: i64,
    base_url: Option<String>,
    cookies: SessionCookies,
    user_id: Option<i64>,
    user_name: Option<String>,
    jellyfin_server_id: Option<String>,
    jellyfin_user_id: Option<String>,
    movie_4k_enabled: bool,
    series_4k_enabled: bool,
    partial_requests_enabled: bool,
    /// Set once `/auth/me` itself has answered 401; the UI must re-link.
    expired: bool,
}

impl SeerrState {
    fn from_config(config: SeerrConfig, revision: i64) -> Self {
        Self {
            revision,
            base_url: config.base_url,
            cookies: config
                .cookies
                .as_deref()
                .map(SessionCookies::from_json)
                .unwrap_or_default(),
            user_id: config.user_id,
            user_name: config.user_name,
            jellyfin_server_id: config.jellyfin_server_id,
            jellyfin_user_id: config.jellyfin_user_id,
            movie_4k_enabled: config.movie_4k_enabled,
            series_4k_enabled: config.series_4k_enabled,
            partial_requests_enabled: config.partial_requests_enabled,
            expired: false,
        }
    }

    fn to_config(&self) -> SeerrConfig {
        SeerrConfig {
            base_url: self.base_url.clone(),
            cookies: (!self.cookies.is_empty()).then(|| self.cookies.to_json()),
            user_id: self.user_id,
            user_name: self.user_name.clone(),
            jellyfin_server_id: self.jellyfin_server_id.clone(),
            jellyfin_user_id: self.jellyfin_user_id.clone(),
            movie_4k_enabled: self.movie_4k_enabled,
            series_4k_enabled: self.series_4k_enabled,
            partial_requests_enabled: self.partial_requests_enabled,
        }
    }

    fn is_linked(&self) -> bool {
        !self.expired && self.cookies.has_session() && self.base_url.is_some()
    }
}

pub struct SeerrSession {
    library: Arc<Library>,
    state: RwLock<SeerrState>,
}

impl SeerrSession {
    pub fn restore(library: Arc<Library>) -> Self {
        let (config, revision) = library.seerr_config_snapshot();
        let state = SeerrState::from_config(config, revision);
        Self {
            library,
            state: RwLock::new(state),
        }
    }

    fn read(&self) -> SeerrState {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// A client carrying the stored session, or the reason one cannot be built.
    pub fn client(&self) -> Result<SeerrClient, SeerrError> {
        self.revalidate();
        let state = self.read();
        if state.expired {
            return Err(SeerrError::Unauthorized);
        }
        let base_url = state.base_url.ok_or(SeerrError::NotConfigured)?;
        Ok(SeerrClient::new(&base_url, state.cookies))
    }

    /// Probes an instance and remembers it as the link target.
    ///
    /// `/settings/public` rather than `/status`, because an *uninitialized*
    /// Seerr treats `POST /auth/jellyfin` as its setup wizard — it would make
    /// whoever logs in the instance owner. Requiring `initialized: true` here
    /// is what keeps a client from silently becoming that wizard. The probe is
    /// also where a CSRF-protected instance hands out the cookie pair the
    /// first POST will need.
    pub fn connect(&self, server_url: &str) -> Result<Value, SeerrError> {
        let normalized = normalize_server_url(server_url).ok_or(SeerrError::NotConfigured)?;
        let client = SeerrClient::new(&normalized, SessionCookies::default());
        let settings = probe_public(&client)?;
        // The version is informational and feature-detects the Quick Connect
        // login routes later, so a failure here must not fail the connect.
        let version = client
            .get_json::<StatusInfo>("status", &[])
            .map(|status| status.version)
            .unwrap_or_default();

        // The probes above can take minutes. Re-read the stored link and check
        // its Jellyfin binding only after they finish, before preserving any
        // part of the previous session.
        self.revalidate();
        let previous = self.read();
        let same_instance = previous.base_url.as_deref() == Some(normalized.as_str());
        if !same_instance {
            self.discard_remote_session(&previous);
        }

        let mut next = if same_instance {
            // Re-probing the instance we are already linked to: keep the
            // session and only take the (possibly rotated) CSRF pair.
            let mut next = previous;
            next.cookies.merge(client.cookies());
            next
        } else {
            SeerrState {
                revision: previous.revision,
                cookies: client.cookies(),
                ..SeerrState::default()
            }
        };
        next.base_url = Some(normalized.clone());
        next.movie_4k_enabled = settings.movie_4k_enabled;
        next.series_4k_enabled = settings.series_4k_enabled;
        next.partial_requests_enabled = settings.partial_requests_enabled;
        let linked = next.is_linked();
        let linked = if self.commit(next) {
            linked
        } else {
            tracing::debug!(
                target: "seerr.session",
                "discarded a stale Seerr connect result because the stored link changed"
            );
            false
        };

        tracing::info!(target: "seerr.session", url = %normalized, "connected to Seerr");
        Ok(json!({
            "serverUrl": normalized,
            "version": version,
            "applicationTitle": settings.application_title,
            "localLogin": settings.local_login,
            "mediaServerLogin": settings.media_server_login,
            "newSignInAllowed": settings.new_plex_login,
            "movie4kEnabled": settings.movie_4k_enabled,
            "series4kEnabled": settings.series_4k_enabled,
            "partialRequestsEnabled": settings.partial_requests_enabled,
            "linked": linked,
        }))
    }

    // -------------------------------------------------------------------- link

    /// Links with the user's media-server password — the path every released
    /// Seerr has, and therefore the one the UI must always be able to fall back
    /// to. The password is used once and never stored; what is kept is the
    /// session cookie Seerr answers with.
    pub fn link_with_password(&self, username: &str, password: &str) -> Result<Value, SeerrError> {
        let username = username.trim();
        if username.is_empty() || password.is_empty() {
            return Err(SeerrError::Unusable(
                "enter the username and password of your media-server account".to_string(),
            ));
        }
        let client = self.link_client()?;
        // One GET before the write: it re-checks the instance is still usable
        // and refreshes a CSRF pair that may have rotated since the last run.
        let settings = probe_public(&client)?;
        let _: Value = client
            .post_json(
                LOGIN_PATH,
                &json!({ "username": username, "password": password }),
            )
            .map_err(map_login_error)?;
        let status = self.finish_link(&client, Some(&settings))?;
        Ok(json!({ "method": "password", "linked": true, "status": status }))
    }

    /// Links without a password, when both halves of the flow support it.
    ///
    /// Seerr starts a Quick Connect handshake against the same Jellyfin server,
    /// we approve its code as an already-authenticated client of that server,
    /// and Seerr redeems it for a session. This needs a Seerr new enough to
    /// carry the routes — they are absent from v3.3.0, the latest stable
    /// release — *and* Quick Connect enabled on the Jellyfin server, which is
    /// off by default. Every step therefore probes and falls back to the
    /// password path instead of surfacing an error the user cannot act on.
    pub fn link_start(&self) -> Result<Value, SeerrError> {
        let client = self.link_client()?;
        let settings = probe_public(&client)?;
        let Some(jellyfin) = self.jellyfin_client() else {
            return Ok(password_required(
                "there is no Jellyfin session to approve with",
            ));
        };
        // Asked before Seerr is involved, so a server with Quick Connect off
        // never leaves an orphaned handshake behind.
        if !jellyfin_auth::quick_connect_enabled(&jellyfin) {
            return Ok(password_required(
                "Quick Connect is turned off on the Jellyfin server",
            ));
        }
        let handshake: QuickConnectHandshake =
            match client.post_json(QUICK_CONNECT_INITIATE, &json!({})) {
                Ok(handshake) => handshake,
                // An unreachable instance is not a missing feature: the password
                // path would fail the same way, so it is reported rather than hidden.
                Err(error @ SeerrError::Transport(_)) => return Err(error),
                Err(error) => {
                    return Ok(password_required(&format!(
                        "this Seerr has no Quick Connect login ({error})"
                    )));
                }
            };
        if !handshake.is_usable() {
            return Ok(password_required(
                "Seerr answered without a usable handshake",
            ));
        }
        // The one call that makes this password-less, and the one that proves
        // the handshake belongs to *our* server: a code minted elsewhere is not
        // one this server will approve.
        match jellyfin_auth::quick_connect_authorize(&jellyfin, &handshake.code) {
            Ok(true) => {}
            Ok(false) => {
                return Ok(password_required(
                    "the Jellyfin server did not recognize the Seerr handshake",
                ));
            }
            Err(error) => {
                return Ok(password_required(&format!(
                    "the Jellyfin server would not approve the handshake ({error})"
                )));
            }
        }
        // The initiate may have rotated the CSRF pair, and the poll that can
        // follow is a separate request built from what is stored.
        self.persist_cookies(&client);
        self.complete_quick_connect(&client, &handshake.secret, Some(&settings))
    }

    /// Retries the redemption half of [`Self::link_start`]. The code is already
    /// approved by the time that call returns, so this exists for a Seerr that
    /// has not caught up yet rather than for a user who has still to act.
    pub fn link_poll(&self, secret: &str) -> Result<Value, SeerrError> {
        let secret = secret.trim();
        if secret.is_empty() {
            return Err(SeerrError::Unusable(
                "there is no Quick Connect attempt to finish".to_string(),
            ));
        }
        let client = self.link_client()?;
        let settings = probe_public(&client)?;
        self.complete_quick_connect(&client, secret, Some(&settings))
    }

    fn complete_quick_connect(
        &self,
        client: &SeerrClient,
        secret: &str,
        settings: Option<&PublicSettings>,
    ) -> Result<Value, SeerrError> {
        match client.post_json::<_, Value>(QUICK_CONNECT_AUTHENTICATE, &json!({ "secret": secret }))
        {
            Ok(_) => {}
            // Seerr has not observed the approval yet. The caller polls; it is
            // not told to type a password, because none is needed.
            Err(SeerrError::Status { status }) if is_pending(status) => {
                return Ok(json!({
                    "method": "quickconnect",
                    "linked": false,
                    "secret": secret,
                }));
            }
            Err(error) => return Err(map_login_error(error)),
        }
        let status = self.finish_link(client, settings)?;
        Ok(json!({ "method": "quickconnect", "linked": true, "status": status }))
    }

    /// Ends the session at the instance, then forgets it locally. The address
    /// stays, so re-linking does not mean retyping it.
    pub fn unlink(&self) -> Value {
        self.revalidate();
        self.discard_remote_session(&self.read());
        self.unlink_locally();
        tracing::info!(target: "seerr.session", "unlinked from Seerr");
        self.status()
    }

    /// Turns a session Seerr has just handed us into a stored link, or refuses
    /// it outright.
    ///
    /// The guard is the point of this function. `/api/v1/status` exposes no
    /// media-server identity and every `/settings` route is admin-only, so an
    /// unauthenticated probe cannot tell which Jellyfin server an instance is
    /// wired to. What *can* be checked is the account behind the session that
    /// was just established — and if it is not the one this app is signed in
    /// as, nothing about it is worth keeping.
    fn finish_link(
        &self,
        client: &SeerrClient,
        settings: Option<&PublicSettings>,
    ) -> Result<Value, SeerrError> {
        let user: SeerrUser = client.get_json("auth/me", &[]).map_err(map_login_error)?;
        let credentials = self.library.credentials();
        let Some(signed_in_as) = credentials.user_id.clone() else {
            return Err(self.refuse(client, "this app is not signed in to Jellyfin"));
        };
        let Some(linked_to) = user.jellyfin_user_id.clone() else {
            return Err(self.refuse(
                client,
                "Seerr did not say which media-server account that login belongs to",
            ));
        };
        if !same_media_server_user(&linked_to, &signed_in_as) {
            return Err(self.refuse(
                client,
                "that Seerr account belongs to a different media-server user \
                 than the one signed in here",
            ));
        }

        let name = user.preferred_name().to_string();
        if !self.store_link(client, &user, &credentials, settings) {
            return Err(self.refuse(
                client,
                "the stored Seerr link changed while signing in; try again",
            ));
        }
        tracing::info!(target: "seerr.session", user = %name, "linked to Seerr");
        Ok(self.status())
    }

    /// Fails closed: a session that will not be kept is ended at the instance
    /// too, and nothing about it reaches the database.
    fn refuse(&self, client: &SeerrClient, message: &str) -> SeerrError {
        if let Err(error) = client.post_empty("auth/logout") {
            tracing::debug!(
                target: "seerr.session",
                "could not discard the refused Seerr session: {error}"
            );
        }
        tracing::warn!(target: "seerr.session", "refused a Seerr link: {message}");
        SeerrError::Unusable(message.to_string())
    }

    /// Writes the new session against the link as it now stands, retrying a
    /// revision that moved underneath — but only while it is still the same
    /// instance, since a link that moved elsewhere must not be overwritten.
    fn store_link(
        &self,
        client: &SeerrClient,
        user: &SeerrUser,
        credentials: &StoredCredentials,
        settings: Option<&PublicSettings>,
    ) -> bool {
        for _ in 0..3 {
            let mut next = self.read();
            if next.base_url.as_deref() != Some(client.base_url()) {
                return false;
            }
            next.cookies = client.cookies();
            next.user_id = Some(user.id);
            next.user_name = Some(user.preferred_name().to_string());
            next.jellyfin_server_id = credentials.server_id.clone();
            next.jellyfin_user_id = credentials.user_id.clone();
            if let Some(settings) = settings {
                next.movie_4k_enabled = settings.movie_4k_enabled;
                next.series_4k_enabled = settings.series_4k_enabled;
                next.partial_requests_enabled = settings.partial_requests_enabled;
            }
            next.expired = false;
            if self.commit(next) {
                return true;
            }
        }
        false
    }

    /// A client for the configured instance whether or not a session is held —
    /// unlike [`Self::client`], which refuses once one has lapsed. Linking is
    /// exactly what a lapsed session needs.
    fn link_client(&self) -> Result<SeerrClient, SeerrError> {
        self.revalidate();
        let state = self.read();
        let base_url = state.base_url.ok_or(SeerrError::NotConfigured)?;
        Ok(SeerrClient::new(&base_url, state.cookies))
    }

    /// The Jellyfin client used to approve a Quick Connect code. Built from the
    /// stored credentials rather than from [`Session`](crate::jellyfin::session::Session),
    /// which keeps this module free of a dependency on it — the same row
    /// [`Self::revalidate`] already reads.
    fn jellyfin_client(&self) -> Option<JellyfinClient> {
        let credentials = self.library.credentials();
        let (Some(server_url), Some(token)) = (credentials.server_url, credentials.token) else {
            return None;
        };
        Some(JellyfinClient::new(
            &server_url,
            &credentials.device_id,
            Some(&token),
        ))
    }

    /// Keeps a rotated CSRF pair across a call boundary, so a request that
    /// arrives later — and rebuilds its client from storage — still has it.
    fn persist_cookies(&self, client: &SeerrClient) {
        let revision = self.read().revision;
        self.absorb_probe(client, revision);
    }

    // ------------------------------------------------------------------ reads

    /// Titles matching `query`, joined against the local cache.
    ///
    /// `person` results are dropped: they carry no TMDB media id, have no
    /// Jellyfin counterpart, and nothing about them can be requested.
    pub fn search(&self, query: &str, page: i64) -> Result<Value, SeerrError> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(empty_page());
        }
        let page = page.clamp(1, 1_000);
        let results: SearchPage = self.call(|client| {
            client.get_json(
                "search",
                &[
                    ("query", query.to_string()),
                    ("page", page.to_string()),
                    ("language", "en".to_string()),
                ],
            )
        })?;
        Ok(self.joined_page(results))
    }

    /// One of Seerr's discovery rows. The kind is validated by the caller, so a
    /// path segment from the UI never reaches the address unchecked.
    pub fn discover(
        &self,
        kind: DiscoverKind,
        page: i64,
        options: &DiscoverOptions,
    ) -> Result<Value, SeerrError> {
        let query = options.query_pairs(kind, page);
        let results: SearchPage =
            self.call(|client| client.get_json(kind.path(), query.as_slice()))?;
        Ok(self.joined_page(results))
    }

    /// The genre cards Seerr builds from TMDB, including their backdrop art.
    pub fn genres(&self, media_type: &str) -> Result<Value, SeerrError> {
        let path = match media_type {
            model::MOVIE => "discover/genreslider/movie",
            model::TV => "discover/genreslider/tv",
            _ => {
                return Err(SeerrError::Unusable(
                    "genres are only available for movies and series".to_string(),
                ));
            }
        };
        self.call(|client| client.get_json(path, &[]))
    }

    /// One title in full, with the local item it corresponds to if the library
    /// already has it.
    pub fn media_detail(&self, media_type: &str, tmdb_id: i64) -> Result<Value, SeerrError> {
        let Some(kind) = model::library_kind(media_type) else {
            return Err(SeerrError::Unusable(format!(
                "{media_type} is not a kind of title that can be requested"
            )));
        };
        if tmdb_id <= 0 {
            return Err(SeerrError::Unusable("that is not a TMDB id".to_string()));
        }
        let path = format!("{media_type}/{tmdb_id}");
        let detail: MediaDetail = self.call(|client| client.get_json(&path, &[]))?;

        let library_item_id = self
            .library
            .ids_by_tmdb(kind, &[tmdb_id.to_string()])
            .unwrap_or_default()
            .remove(&tmdb_id.to_string());
        let info = detail.media_info.clone().unwrap_or_default();
        let title = detail.display_title().to_string();
        let year = detail.year();
        let runtime = detail.runtime_minutes();
        let original_title = detail
            .original_title
            .clone()
            .or_else(|| detail.original_name.clone());
        let release_date = detail
            .release_date
            .clone()
            .or_else(|| detail.first_air_date.clone());
        let studios = names_of(&detail.production_companies);
        let networks = names_of(&detail.networks);
        let creators = unique_names(
            detail
                .created_by
                .iter()
                .map(|person| person.name.as_str())
                .chain(crew_names(&detail, |credit| {
                    credit.job.as_deref() == Some("Creator")
                })),
        );
        let directors = unique_names(crew_names(&detail, |credit| {
            credit.job.as_deref() == Some("Director")
        }));
        let writers = unique_names(crew_names(&detail, |credit| {
            credit.department.as_deref() == Some("Writing")
                || matches!(
                    credit.job.as_deref(),
                    Some("Writer" | "Screenplay" | "Story" | "Teleplay")
                )
        }));
        let production_countries = detail
            .production_countries
            .iter()
            .map(|country| {
                json!({
                    "code": country.code,
                    "name": country.name,
                })
            })
            .collect::<Vec<_>>();
        let spoken_languages = detail
            .spoken_languages
            .iter()
            .map(|language| {
                json!({
                    "code": language.code,
                    "name": language.english_name.as_deref().unwrap_or(&language.name),
                })
            })
            .collect::<Vec<_>>();
        let cast = detail
            .credits
            .cast
            .iter()
            .take(20)
            .map(|person| {
                json!({
                    "id": person.id,
                    "name": person.name,
                    "character": person.character,
                    "profilePath": person.profile_path,
                })
            })
            .collect::<Vec<_>>();
        let genres = names_of(&detail.genres);
        let seasons = season_list(&detail, &info);
        let trailer = trailer_of(&detail);
        let release_dates = release_dates_of(&detail);
        let content_ratings = content_ratings_of(&detail);
        let next_episode = detail.next_episode_to_air.as_ref().map(|episode| {
            json!({
                "name": episode.name,
                "airDate": episode.air_date,
                "seasonNumber": episode.season_number,
                "episodeNumber": episode.episode_number,
            })
        });
        Ok(json!({
            "mediaType": media_type,
            "tmdbId": detail.id,
            "title": title,
            "year": year,
            "overview": detail.overview,
            "tagline": detail.tagline,
            "originalTitle": original_title,
            "posterPath": detail.poster_path,
            "backdropPath": detail.backdrop_path,
            "voteAverage": detail.vote_average,
            "voteCount": detail.vote_count,
            "runtimeMinutes": runtime,
            "genres": genres,
            "status": model::status_name(info.status),
            "status4k": model::status_name(info.status_4k),
            "libraryItemId": library_item_id,
            "seasons": seasons,
            "releaseDate": release_date,
            "firstAirDate": detail.first_air_date,
            "lastAirDate": detail.last_air_date,
            "productionStatus": detail.status,
            "inProduction": detail.in_production,
            "seriesType": detail.series_type,
            "numberOfSeasons": detail.number_of_seasons,
            "numberOfEpisodes": detail.number_of_episodes,
            "originalLanguage": detail.original_language,
            "homepage": detail.homepage,
            "budget": positive(detail.budget),
            "revenue": positive(detail.revenue),
            "studios": studios,
            "networks": networks,
            "creators": creators,
            "directors": directors,
            "writers": writers,
            "productionCountries": production_countries,
            "spokenLanguages": spoken_languages,
            "cast": cast,
            "trailer": trailer,
            "releaseDates": release_dates,
            "contentRatings": content_ratings,
            "nextEpisode": next_episode,
        }))
    }

    /// The quality profiles on the linked Radarr or Sonarr destinations that
    /// can receive this request. Seerr exposes these only as an advanced
    /// request choice, so the user's permission is checked before returning
    /// any destination metadata.
    pub fn request_options(&self, media_type: &str, is_4k: bool) -> Result<Value, SeerrError> {
        let service = request_service(media_type)?;
        let user: SeerrUser = self.call(|client| client.get_json("auth/me", &[]))?;
        let capabilities = Capabilities::derive(user.permissions, true, true);
        if !capabilities.advanced_request {
            return Err(SeerrError::PermissionDenied);
        }

        let path = format!("service/{service}");
        let mut servers: Vec<DownloadService> = self.call(|client| client.get_json(&path, &[]))?;
        servers.retain(|server| server.id >= 0 && server.is_4k == is_4k);
        servers.sort_by_key(|server| (!server.is_default, server.name.to_lowercase()));

        let mut destinations = Vec::with_capacity(servers.len());
        for server in servers {
            let path = format!("service/{service}/{}", server.id);
            let mut detail: DownloadServiceDetail =
                self.call(|client| client.get_json(&path, &[]))?;
            detail
                .profiles
                .retain(|profile| profile.id > 0 && !profile.name.trim().is_empty());
            detail
                .profiles
                .sort_by_key(|profile| profile.name.to_lowercase());
            destinations.push(json!({
                "id": server.id,
                "name": server.name,
                "isDefault": server.is_default,
                "profiles": detail.profiles.iter().map(|profile| json!({
                    "id": profile.id,
                    "name": profile.name,
                    "isDefault": profile.id == server.active_profile_id,
                })).collect::<Vec<_>>(),
            }));
        }

        Ok(json!({ "destinations": destinations }))
    }

    // ----------------------------------------------------------------- writes

    /// Asks Seerr for a title. Never retried — see [`SeerrClient`] — because an
    /// uncertain outcome must not become a second request.
    ///
    /// `seasons` is what makes a partial series requestable one season at a
    /// time; `None` on a series means "everything Seerr does not already have",
    /// which is Seerr's own `all`.
    pub fn create_request(
        &self,
        media_type: &str,
        tmdb_id: i64,
        seasons: Option<Vec<i64>>,
        is_4k: bool,
        profile: Option<RequestProfileSelection>,
    ) -> Result<Value, SeerrError> {
        if model::library_kind(media_type).is_none() {
            return Err(SeerrError::Unusable(format!(
                "{media_type} is not a kind of title that can be requested"
            )));
        }
        if tmdb_id <= 0 {
            return Err(SeerrError::Unusable("that is not a TMDB id".to_string()));
        }
        let mut body = json!({
            "mediaType": media_type,
            "mediaId": tmdb_id,
            "is4k": is_4k,
        });
        if let Some(profile) = profile {
            if profile.server_id < 0 || profile.profile_id <= 0 {
                return Err(SeerrError::Unusable(
                    "the download destination must be non-negative and the quality profile must be positive"
                        .to_string(),
                ));
            }
            let user: SeerrUser = self.call(|client| client.get_json("auth/me", &[]))?;
            if !Capabilities::derive(user.permissions, true, true).advanced_request {
                return Err(SeerrError::PermissionDenied);
            }
            body["serverId"] = json!(profile.server_id);
            body["profileId"] = json!(profile.profile_id);
        }
        if media_type == model::TV {
            body["seasons"] = match seasons {
                Some(seasons) if !seasons.is_empty() => json!(seasons),
                // Seerr expands this to whatever it does not already have,
                // which is what an unqualified "request this show" means.
                _ => json!("all"),
            };
        }
        let created: MediaRequest = self.call(|client| client.post_json("request", &body))?;
        tracing::info!(
            target: "seerr.session",
            media_type,
            tmdb_id,
            status = model::request_status_name(created.status),
            "requested a title through Seerr"
        );
        Ok(self.request_json(&created))
    }

    /// The user's own requests. Deliberately never the household's: Seerr shows
    /// everyone's to an administrator, and this is a personal view.
    pub fn requests(&self, take: i64, skip: i64, filter: &str) -> Result<Value, SeerrError> {
        self.revalidate();
        let state = self.read();
        if state.base_url.is_none() {
            return Err(SeerrError::NotConfigured);
        }
        // Without an account there is nobody to scope the list to, and an
        // unscoped one would be the household's.
        let Some(user_id) = state.user_id else {
            return Err(SeerrError::Unauthorized);
        };
        let take = take.clamp(1, 100);
        let skip = skip.max(0);
        let filter = match filter {
            "all" | "pending" | "approved" | "processing" | "available" | "failed" => filter,
            _ => "all",
        }
        .to_string();
        let page: RequestPage = self.call(|client| {
            client.get_json(
                "request",
                &[
                    ("take", take.to_string()),
                    ("skip", skip.to_string()),
                    ("filter", filter),
                    ("sort", "added".to_string()),
                    ("requestedBy", user_id.to_string()),
                ],
            )
        })?;

        let results = page
            .results
            .iter()
            .map(|request| self.request_json(request))
            .collect::<Vec<_>>();
        Ok(json!({
            "page": page.page_info.page,
            "totalPages": page.page_info.pages,
            "totalResults": page.page_info.results,
            "results": results,
        }))
    }

    /// Cancels one of the user's own pending requests.
    ///
    /// A refusal here is the case the 401 disambiguation exists for: Seerr
    /// answers 401 — not 403 — when the session is valid but the request is not
    /// the user's to cancel, and that must not read as a lapsed session.
    pub fn cancel_request(&self, request_id: i64) -> Result<Value, SeerrError> {
        if request_id <= 0 {
            return Err(SeerrError::Unusable("that is not a request id".to_string()));
        }
        let path = format!("request/{request_id}");
        self.call(|client| client.delete(&path))?;
        tracing::info!(target: "seerr.session", request_id, "cancelled a Seerr request");
        Ok(json!({ "cancelled": true, "id": request_id }))
    }

    /// One request in the shape the UI renders. The title is deliberately
    /// absent: Seerr's request rows reference a TMDB id and nothing else, and
    /// resolving each one here would be a blocking fan-out on this thread.
    fn request_json(&self, request: &MediaRequest) -> Value {
        let media = request.media.clone().unwrap_or_default();
        let media_type = if request.media_type.is_empty() {
            media.media_type.clone().unwrap_or_default()
        } else {
            request.media_type.clone()
        };
        let tmdb_id = media.tmdb_id;
        let library_item_id = tmdb_id.and_then(|tmdb_id| {
            self.local_ids(&media_type, &[tmdb_id.to_string()])
                .remove(&tmdb_id.to_string())
        });
        json!({
            "id": request.id,
            "status": model::request_status_name(request.status),
            "mediaType": media_type,
            "tmdbId": tmdb_id,
            "is4k": request.is4k,
            "createdAt": request.created_at,
            "updatedAt": request.updated_at,
            "mediaStatus": model::status_name(if request.is4k {
                media.status_4k
            } else {
                media.status
            }),
            "seasons": request
                .seasons
                .iter()
                .map(|season| season.season_number)
                .collect::<Vec<_>>(),
            "libraryItemId": library_item_id,
        })
    }

    /// Turns one page of Seerr results into the shape the UI renders, with the
    /// local cache joined in.
    ///
    /// The join is one query per media type rather than one per result: a page
    /// of twenty titles costs two lookups, not twenty.
    fn joined_page(&self, page: SearchPage) -> Value {
        let media = page
            .results
            .into_iter()
            .filter(SearchResult::is_media)
            .collect::<Vec<_>>();
        let ids_of = |media_type: &str| {
            media
                .iter()
                .filter(|result| result.media_type == media_type)
                .map(|result| result.id.to_string())
                .collect::<Vec<_>>()
        };
        let movies = self.local_ids(model::MOVIE, &ids_of(model::MOVIE));
        let series = self.local_ids(model::TV, &ids_of(model::TV));

        let results = media
            .iter()
            .map(|result| {
                let owned = if result.media_type == model::MOVIE {
                    &movies
                } else {
                    &series
                };
                let info = result.media_info.clone().unwrap_or_default();
                json!({
                    "mediaType": result.media_type,
                    "tmdbId": result.id,
                    "title": result.display_title(),
                    "year": result.year(),
                    "overview": result.overview,
                    "posterPath": result.poster_path,
                    "backdropPath": result.backdrop_path,
                    "voteAverage": result.vote_average,
                    "status": model::status_name(info.status),
                    "status4k": model::status_name(info.status_4k),
                    // The killer join: every result resolves to "play it" or
                    // "request it", never to a dead end.
                    "libraryItemId": owned.get(&result.id.to_string()),
                })
            })
            .collect::<Vec<_>>();

        json!({
            "page": page.page,
            "totalPages": page.total_pages,
            "totalResults": page.total_results,
            "results": results,
        })
    }

    /// TMDB id → Jellyfin id for one media type. A storage failure degrades to
    /// "the library does not have it", which offers a request rather than
    /// failing a whole page of results.
    fn local_ids(
        &self,
        media_type: &str,
        tmdb_ids: &[String],
    ) -> std::collections::HashMap<String, String> {
        let Some(kind) = model::library_kind(media_type) else {
            return Default::default();
        };
        self.library
            .ids_by_tmdb(kind, tmdb_ids)
            .unwrap_or_else(|error| {
                tracing::warn!(
                    target: "seerr.session",
                    "could not join Seerr results against the library: {error}"
                );
                Default::default()
            })
    }

    /// Runs one call against the linked instance, keeping the stored cookies
    /// fresh and disambiguating the 401 Seerr overloads.
    ///
    /// Seerr answers 401 both to a lapsed session *and* to a valid session that
    /// merely lacks a permission — `DELETE /request/{id}` on somebody else's
    /// request is the case that proved it. Taking the first at face value would
    /// log the user out for pressing a button they were never allowed to press,
    /// so a 401 is confirmed against `/auth/me` before it counts as an expiry.
    fn call<T>(
        &self,
        call: impl FnOnce(&SeerrClient) -> Result<T, SeerrError>,
    ) -> Result<T, SeerrError> {
        let client = self.client()?;
        let revision = self.read().revision;
        let result = call(&client);
        if !matches!(result, Err(SeerrError::Unauthorized)) {
            self.absorb_probe(&client, revision);
            return result;
        }
        match client.get_json::<SeerrUser>("auth/me", &[]) {
            Err(SeerrError::Unauthorized) => {
                if self.probe_is_current(revision) {
                    self.mark_expired();
                }
                Err(SeerrError::Unauthorized)
            }
            // The session answered for itself, so the refusal was about the
            // action, not the cookie.
            _ => {
                self.absorb_probe(&client, revision);
                Err(SeerrError::PermissionDenied)
            }
        }
    }

    /// What the UI needs to decide whether to show the Seerr views at all.
    ///
    /// A linked session is confirmed against the instance rather than assumed:
    /// Express sessions lapse without warning, and the answer carries the
    /// permission mask and quota, neither of which is worth caching.
    pub fn status(&self) -> Value {
        self.revalidate();
        let state = self.read();
        let mut status = Self::status_from_state(&state);
        if !state.is_linked() {
            return status;
        }

        let client = match self.client() {
            Ok(client) => client,
            Err(_) => return Self::status_from_state(&self.read()),
        };
        let user: SeerrUser = match client.get_json("auth/me", &[]) {
            Ok(user) => user,
            Err(SeerrError::Unauthorized) => {
                // This *is* `/auth/me`, so a 401 here is unambiguous, provided
                // the link probed is still the one stored now.
                if !self.probe_is_current(state.revision) {
                    return Self::status_from_state(&self.read());
                }
                self.mark_expired();
                status["expired"] = json!(true);
                return status;
            }
            Err(error) => {
                tracing::debug!(target: "seerr.session", "could not read the Seerr user: {error}");
                if !self.probe_is_current(state.revision) {
                    return Self::status_from_state(&self.read());
                }
                return status;
            }
        };

        let capabilities = Capabilities::derive(
            user.permissions,
            state.movie_4k_enabled,
            state.series_4k_enabled,
        );
        // Quota needs its own call — `/auth/me` carries the mask but not usage
        // — and is a nicety, so a failure leaves it null rather than failing.
        let quota = client
            .get_json::<UserQuota>(&format!("user/{}/quota", user.id), &[])
            .ok();

        if !self.absorb_probe(&client, state.revision) {
            return Self::status_from_state(&self.read());
        }

        status["linked"] = json!(true);
        status["user"] = json!({
            "id": user.id,
            "name": user.preferred_name(),
            "avatar": user.avatar,
            "jellyfinUserId": user.jellyfin_user_id,
        });
        status["capabilities"] = serde_json::to_value(capabilities).unwrap_or(Value::Null);
        status["quota"] = quota
            .and_then(|quota| serde_json::to_value(quota).ok())
            .unwrap_or(Value::Null);
        status
    }

    fn status_from_state(state: &SeerrState) -> Value {
        json!({
            "configured": state.base_url.is_some(),
            "linked": false,
            "expired": state.expired,
            "serverUrl": state.base_url.clone(),
            "instance": {
                "movie4kEnabled": state.movie_4k_enabled,
                "series4kEnabled": state.series_4k_enabled,
                "partialRequestsEnabled": state.partial_requests_enabled,
            },
            "user": Value::Null,
            "capabilities": Value::Null,
            "quota": Value::Null,
        })
    }

    /// Drops the link when the Jellyfin account it was made under is no longer
    /// the one signed in. Cheap enough to run on every acquisition, and run
    /// eagerly by the sign-in and sign-out paths so a signed-out machine keeps
    /// no Seerr cookie on disk.
    pub fn revalidate(&self) {
        self.refresh_from_storage();
        let state = self.read();
        let Some(bound_user) = state.jellyfin_user_id.clone() else {
            // Nothing linked yet: only an instance address, which is not secret.
            return;
        };
        let credentials = self.library.credentials();
        let same_user = credentials.user_id.as_deref() == Some(bound_user.as_str());
        // Older rows may predate the server id; only a genuine mismatch counts.
        let same_server = match (&credentials.server_id, &state.jellyfin_server_id) {
            (Some(current), Some(bound)) => current == bound,
            _ => true,
        };
        if same_user && same_server {
            return;
        }
        tracing::info!(
            target: "seerr.session",
            "dropping the Seerr link: the Jellyfin account it was made under is no longer signed in"
        );
        self.unlink_locally();
    }

    /// Forgets the session without touching the instance address, so re-linking
    /// does not mean retyping it.
    fn unlink_locally(&self) {
        // A lost revision race must not leave the previous account's cookie
        // behind, so the clear is retried against the refreshed revision.
        for _ in 0..3 {
            let mut next = self.read();
            if next.cookies.is_empty() && next.jellyfin_user_id.is_none() {
                return;
            }
            next.cookies = SessionCookies::default();
            next.user_id = None;
            next.user_name = None;
            next.jellyfin_server_id = None;
            next.jellyfin_user_id = None;
            next.expired = false;
            if self.commit(next) {
                return;
            }
        }
        tracing::warn!(
            target: "seerr.session",
            "could not drop the Seerr link: the stored link kept changing"
        );
    }

    /// Best-effort server-side teardown before a stored session is abandoned.
    fn discard_remote_session(&self, state: &SeerrState) {
        let (Some(base_url), false) = (&state.base_url, state.cookies.is_empty()) else {
            return;
        };
        let client = SeerrClient::new(base_url, state.cookies.clone());
        if let Err(error) = client.post_empty("auth/logout") {
            tracing::debug!(target: "seerr.session", "server-side Seerr logout failed: {error}");
        }
    }

    fn mark_expired(&self) {
        if self.read().expired {
            return;
        }
        self.state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .expired = true;
        tracing::warn!(
            target: "seerr.session",
            "the Seerr session has lapsed; re-linking is required"
        );
    }

    /// Takes back whatever cookies a call rotated, so a refreshed CSRF pair
    /// survives the restart. The probe result is discarded if another process
    /// changed the stored link while the request was in flight.
    fn absorb_probe(&self, client: &SeerrClient, expected_revision: i64) -> bool {
        if !self.probe_is_current(expected_revision) {
            return false;
        }
        let cookies = client.cookies();
        let mut next = self.read();
        if next.revision != expected_revision {
            return false;
        }
        if next.cookies == cookies {
            return true;
        }
        next.cookies = cookies;
        self.commit(next)
    }

    fn probe_is_current(&self, expected_revision: i64) -> bool {
        let (config, revision) = match self.library.try_seerr_config_snapshot() {
            Ok(snapshot) => snapshot.unwrap_or_default(),
            Err(error) => {
                tracing::warn!(
                    target: "seerr.session",
                    "failed to verify the stored Seerr link revision: {error}"
                );
                return false;
            }
        };
        if revision == expected_revision {
            return true;
        }
        self.replace_state(SeerrState::from_config(config, revision));
        tracing::debug!(
            target: "seerr.session",
            "discarded a stale Seerr probe because the stored link changed"
        );
        false
    }

    fn refresh_from_storage(&self) {
        let revision = self.read().revision;
        let (config, stored_revision) = match self.library.try_seerr_config_snapshot() {
            Ok(snapshot) => snapshot.unwrap_or_default(),
            Err(error) => {
                tracing::warn!(
                    target: "seerr.session",
                    "failed to refresh the stored Seerr link: {error}"
                );
                return;
            }
        };
        if stored_revision != revision {
            self.replace_state(SeerrState::from_config(config, stored_revision));
        }
    }

    fn replace_state(&self, next: SeerrState) {
        *self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
    }

    fn commit(&self, mut next: SeerrState) -> bool {
        match self
            .library
            .save_seerr_config_if_revision(&next.to_config(), next.revision)
        {
            Ok(Some(revision)) => {
                next.revision = revision;
                self.replace_state(next);
                true
            }
            Ok(None) => {
                self.refresh_from_storage();
                false
            }
            Err(error) => {
                tracing::warn!(target: "seerr.session", "failed to persist the Seerr link: {error}");
                false
            }
        }
    }
}

/// Reads `/settings/public` and refuses an instance that must not be logged
/// into.
///
/// `/settings/public` rather than `/status`, because an *uninitialized* Seerr
/// treats `POST /auth/jellyfin` as its setup wizard — it would make whoever
/// logs in the instance owner, with full administrator permissions. This gate
/// is what keeps a client from silently becoming that wizard.
///
/// It is also the GET that every write is preceded by: a CSRF-protected
/// instance hands out its `_csrf` / `XSRF-TOKEN` pair here, and the very next
/// POST already needs it.
fn probe_public(client: &SeerrClient) -> Result<PublicSettings, SeerrError> {
    let settings: PublicSettings = client.get_json("settings/public", &[])?;
    if !settings.initialized {
        return Err(SeerrError::Unusable(
            "this Seerr instance has not finished its setup wizard yet".to_string(),
        ));
    }
    // Only present on an initialized instance, so it is checked, not required.
    if settings
        .media_server_type
        .is_some_and(|kind| kind != MEDIA_SERVER_JELLYFIN)
    {
        return Err(SeerrError::Unusable(
            "this Seerr instance is not connected to a Jellyfin server".to_string(),
        ));
    }
    Ok(settings)
}

/// The answer for a search with nothing to search for, so an empty field costs
/// no round trip.
fn empty_page() -> Value {
    json!({ "page": 1, "totalPages": 0, "totalResults": 0, "results": [] })
}

fn request_service(media_type: &str) -> Result<&'static str, SeerrError> {
    match media_type {
        model::MOVIE => Ok("radarr"),
        model::TV => Ok("sonarr"),
        _ => Err(SeerrError::Unusable(format!(
            "{media_type} is not a kind of title that can be requested"
        ))),
    }
}

fn positive(value: Option<i64>) -> Option<i64> {
    value.filter(|value| *value > 0)
}

fn names_of(values: &[model::NamedId]) -> Vec<String> {
    unique_names(values.iter().map(|value| value.name.as_str()))
}

fn crew_names<'a>(
    detail: &'a MediaDetail,
    predicate: impl Fn(&model::Credit) -> bool + 'a,
) -> impl Iterator<Item = &'a str> + 'a {
    detail
        .credits
        .crew
        .iter()
        .filter(move |credit| predicate(credit))
        .map(|credit| credit.name.as_str())
}

fn unique_names<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut names = Vec::new();
    for value in values.map(str::trim).filter(|value| !value.is_empty()) {
        if !names
            .iter()
            .any(|known: &String| known.eq_ignore_ascii_case(value))
        {
            names.push(value.to_string());
        }
    }
    names
}

fn trailer_of(detail: &MediaDetail) -> Option<Value> {
    detail
        .related_videos
        .iter()
        .filter(|video| {
            video.site == "YouTube"
                && video.video_type == "Trailer"
                && video.key.len() == 11
                && video
                    .key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .max_by_key(|video| video.size)
        .map(|video| {
            json!({
                "name": if video.name.trim().is_empty() {
                    "Trailer"
                } else {
                    video.name.as_str()
                },
                "key": video.key,
            })
        })
}

fn release_dates_of(detail: &MediaDetail) -> Vec<Value> {
    detail
        .releases
        .results
        .iter()
        .flat_map(|country| {
            country.release_dates.iter().filter_map(|release| {
                let kind = match release.release_type {
                    1 => "premiere",
                    2 => "limited-cinema",
                    3 => "cinema",
                    4 => "digital",
                    5 => "physical",
                    6 => "tv",
                    _ => return None,
                };
                (!country.region.trim().is_empty() && !release.release_date.trim().is_empty()).then(
                    || {
                        json!({
                            "region": country.region,
                            "type": kind,
                            "date": release.release_date,
                            "certification": release.certification,
                        })
                    },
                )
            })
        })
        .collect()
}

fn content_ratings_of(detail: &MediaDetail) -> Vec<Value> {
    detail
        .content_ratings
        .results
        .iter()
        .filter(|rating| !rating.region.trim().is_empty() && !rating.rating.trim().is_empty())
        .map(|rating| {
            json!({
                "region": rating.region,
                "rating": rating.rating,
            })
        })
        .collect()
}

/// The seasons a series can be requested by, each carrying what Seerr already
/// has of it.
///
/// Season 0 is left out, as Seerr's own request modal leaves it out: specials
/// are not a season the *arr side tracks as requestable.
fn season_list(detail: &MediaDetail, info: &MediaInfo) -> Vec<Value> {
    detail
        .seasons
        .iter()
        .filter(|season| season.season_number >= 1)
        .map(|season| {
            let known = info
                .seasons
                .iter()
                .find(|entry| entry.season_number == season.season_number);
            let status = known
                .map(|entry| entry.status)
                .unwrap_or(model::media_status::UNKNOWN);
            let status_4k = known
                .map(|entry| entry.status_4k)
                .unwrap_or(model::media_status::UNKNOWN);
            json!({
                "seasonNumber": season.season_number,
                "name": season.name,
                "episodeCount": season.episode_count,
                "airDate": season.air_date,
                "status": model::status_name(status),
                "status4k": model::status_name(status_4k),
            })
        })
        .collect()
}

/// The answer that sends the UI to the password form. The reason is logged
/// rather than shown: a Quick Connect that is merely unavailable is not a
/// failure the user can act on, and every release supports the password path.
fn password_required(reason: &str) -> Value {
    tracing::debug!(target: "seerr.session", "falling back to Seerr password login: {reason}");
    json!({ "method": "password", "linked": false })
}

/// Failures of a *login*, which are not failures of a session.
///
/// A 401 here is a rejected credential, not a lapsed cookie, and must not put
/// the UI into the re-link prompt it is already in. A 403 is Seerr's answer for
/// a user it has never imported while "enable new sign-ins" is off — something
/// only an administrator can fix, so it says so.
fn map_login_error(error: SeerrError) -> SeerrError {
    match error {
        SeerrError::Unauthorized => SeerrError::LoginRejected,
        SeerrError::Status { status: 403 } => SeerrError::UnknownUser,
        other => other,
    }
}

/// Whether a refused Quick Connect redemption is worth polling again. Seerr
/// answers 404/405 when the route is absent and 401/403 when the login itself
/// was refused; those are answers, not delays.
fn is_pending(status: u16) -> bool {
    matches!(status, 400 | 409 | 425)
}

/// Compares two media-server user ids. Jellyfin hands its GUIDs out both with
/// and without dashes depending on the endpoint and version, so a plain string
/// comparison would reject a match — and, worse, look like the account switch
/// this guard exists to catch.
fn same_media_server_user(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|character| *character != '-')
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let (left, right) = (normalize(left), normalize(right));
    !left.is_empty() && left == right
}

#[cfg(test)]
mod tests {
    use super::{
        DiscoverKind, DiscoverOptions, RequestProfileSelection, SeerrClient, SeerrError,
        SeerrSession, SeerrState, SessionCookies, UtcDate, Value, json, same_media_server_user,
    };
    use crate::library::{Library, SeerrConfig};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    fn library() -> Arc<Library> {
        Arc::new(Library::open_in_memory().expect("library"))
    }

    /// A throwaway HTTP server answering one canned response per request and
    /// recording the request heads it saw. The cookie plumbing is the part of
    /// this milestone that only a real socket can prove.
    fn fake_server(responses: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base_url = format!("http://{}", listener.local_addr().expect("address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for (stream, response) in listener.incoming().zip(responses) {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) if line == "\r\n" => break,
                        Ok(_) => head.push_str(&line),
                    }
                }
                // Drain the request body before answering: closing a socket
                // that still holds unread data resets the connection, and the
                // client sees that as a transport failure rather than the
                // response it was just sent. It is kept on the end of the head
                // so a test can assert on what was actually sent.
                let length = head
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if length > 0 {
                    let mut body = vec![0u8; length];
                    if reader.read_exact(&mut body).is_ok() {
                        head.push_str(&String::from_utf8_lossy(&body));
                    }
                }
                seen.lock().expect("lock").push(head);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (base_url, requests)
    }

    /// `Connection: close` keeps one request to one connection, so the canned
    /// responses line up with the requests one for one.
    fn response(status: &str, body: &str, cookies: &[&str]) -> String {
        let mut head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for cookie in cookies {
            head.push_str(&format!("Set-Cookie: {cookie}\r\n"));
        }
        format!("{head}\r\n{body}")
    }

    /// What a forward-auth proxy answers with: a 302 towards its own sign-on
    /// flow, with an HTML body nothing should be reading.
    fn redirect_response(location: &str) -> String {
        let body = "<html><body>Found.</body></html>";
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\n\
             Content-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// What a proxy, a sign-on page, or Seerr's own front end answers with.
    fn html_response(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    const INITIALIZED: &str = r#"{"initialized":true,"applicationTitle":"Seerr","mediaServerType":2,
        "localLogin":true,"mediaServerLogin":true,"newPlexLogin":true,"movie4kEnabled":false,
        "series4kEnabled":false,"partialRequestsEnabled":true}"#;
    const VERSION: &str = r#"{"version":"3.3.0","commitTag":"local"}"#;
    /// `/auth/me` for a Seerr account backed by the Jellyfin user `uid`.
    const ME: &str = r#"{"id":7,"displayName":"pho","jellyfinUserId":"uid","permissions":32}"#;
    const QUOTA: &str = r#"{"movie":{"used":0},"tv":{"used":0}}"#;
    /// The `Set-Cookie` an established Seerr session arrives on.
    const SESSION: &str = "connect.sid=s%3Aabc.def; Path=/; HttpOnly";

    fn signed_in(library: &Library, user_id: &str) {
        signed_in_to(library, "http://server:8096", user_id);
    }

    fn signed_in_to(library: &Library, server_url: &str, user_id: &str) {
        let mut credentials = library.credentials();
        credentials.server_url = Some(server_url.to_string());
        credentials.user_id = Some(user_id.to_string());
        credentials.server_id = Some("srv".to_string());
        credentials.token = Some("tok".to_string());
        library.save_credentials(&credentials).expect("credentials");
    }

    /// An instance that has been connected to but not linked — the state
    /// `POST /api/seerr/connect` leaves behind.
    fn configured(library: &Library, base_url: &str) {
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some(base_url.to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");
    }

    /// The link as the session itself sees it, once the account guard has run.
    /// Deliberately not `status()`: that confirms a live link against the
    /// instance, which these tests have no server for.
    fn is_linked(session: &SeerrSession) -> bool {
        session.revalidate();
        session.read().is_linked()
    }

    fn linked(library: &Library, jellyfin_user_id: &str) {
        linked_to(library, "https://seerr.test", jellyfin_user_id);
    }

    /// An established link, as the link flow leaves it: a session cookie, the
    /// Seerr account, and the Jellyfin account it is bound to.
    fn linked_to(library: &Library, base_url: &str, jellyfin_user_id: &str) {
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some(base_url.to_string()),
                cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                user_name: Some("pho".to_string()),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some(jellyfin_user_id.to_string()),
                partial_requests_enabled: true,
                ..SeerrConfig::default()
            })
            .expect("seerr config");
    }

    /// Request bodies are pretty-printed on the wire, so what they contain is
    /// asserted against a whitespace-free form of the whole recorded request.
    fn compact(request: &str) -> String {
        request.chars().filter(|c| !c.is_whitespace()).collect()
    }

    /// A signed-in machine with a live Seerr link against `base_url`.
    fn session_linked_to(base_url: &str) -> (Arc<Library>, SeerrSession) {
        let library = library();
        signed_in(&library, "uid");
        linked_to(&library, base_url, "uid");
        let session = SeerrSession::restore(library.clone());
        (library, session)
    }

    #[test]
    fn a_fresh_session_is_neither_configured_nor_linked() {
        let session = SeerrSession::restore(library());
        let status = session.status();
        assert_eq!(status["configured"], false);
        assert_eq!(status["linked"], false);
        assert_eq!(status["user"], serde_json::Value::Null);
        assert!(matches!(
            session.client(),
            Err(super::SeerrError::NotConfigured)
        ));
    }

    #[test]
    fn restoring_reads_the_persisted_link() {
        let library = library();
        signed_in(&library, "uid");
        linked(&library, "uid");

        let session = SeerrSession::restore(library);
        let client = session.client().expect("client");
        assert_eq!(
            client.url("auth/me", &[]),
            "https://seerr.test/api/v1/auth/me"
        );
        assert!(client.cookies().has_session());
    }

    #[test]
    fn an_unusable_address_is_rejected_before_any_request() {
        let session = SeerrSession::restore(library());
        assert!(matches!(
            session.connect("file:///etc/passwd"),
            Err(super::SeerrError::NotConfigured)
        ));
        assert!(matches!(
            session.connect("javascript:alert(1)"),
            Err(super::SeerrError::NotConfigured)
        ));
        assert!(matches!(
            session.connect("  "),
            Err(super::SeerrError::NotConfigured)
        ));
    }

    #[test]
    fn switching_jellyfin_user_drops_the_link_from_memory_and_disk() {
        let library = library();
        signed_in(&library, "uid");
        linked(&library, "uid");
        let session = SeerrSession::restore(library.clone());
        assert!(is_linked(&session));

        // Somebody else signs in on the same machine.
        signed_in(&library, "other-uid");

        assert!(!is_linked(&session));
        assert_eq!(session.status()["linked"], false);
        let stored = library.seerr_config();
        assert_eq!(stored.cookies, None);
        assert_eq!(stored.user_id, None);
        // The instance address survives, so re-linking needs no retyping.
        assert_eq!(stored.base_url.as_deref(), Some("https://seerr.test"));
    }

    #[test]
    fn signing_out_of_jellyfin_drops_the_link() {
        let library = library();
        signed_in(&library, "uid");
        linked(&library, "uid");
        let session = SeerrSession::restore(library.clone());

        library.clear_session(false).expect("sign out");

        assert!(!is_linked(&session));
        assert_eq!(library.seerr_config().cookies, None);
    }

    #[test]
    fn a_link_made_against_another_jellyfin_server_is_dropped() {
        let library = library();
        signed_in(&library, "uid");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some("https://seerr.test".to_string()),
                cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
                jellyfin_server_id: Some("a-different-server".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");

        let session = SeerrSession::restore(library.clone());
        assert!(!is_linked(&session));
        assert_eq!(library.seerr_config().cookies, None);
    }

    #[test]
    fn connecting_captures_the_csrf_pair_from_the_very_first_probe() {
        let (base_url, requests) = fake_server(vec![
            response(
                "200 OK",
                INITIALIZED,
                &[
                    "_csrf=secret; Path=/; HttpOnly; SameSite=Strict",
                    "XSRF-TOKEN=token123; Path=/",
                ],
            ),
            response("200 OK", VERSION, &[]),
        ]);
        let library = library();
        let session = SeerrSession::restore(library.clone());

        let result = session.connect(&base_url).expect("connect");
        assert_eq!(result["serverUrl"], base_url);
        assert_eq!(result["version"], "3.3.0");
        assert_eq!(result["partialRequestsEnabled"], true);
        assert_eq!(result["linked"], false);

        let requests = requests.lock().expect("lock");
        assert!(requests[0].starts_with("GET /api/v1/settings/public HTTP/1.1"));
        assert!(requests[1].starts_with("GET /api/v1/status HTTP/1.1"));
        // The pair the probe handed out is already on the second request, and
        // is persisted so the first write after a restart still has it.
        assert!(requests[1].contains("cookie: XSRF-TOKEN=token123; _csrf=secret"));
        let stored = library.seerr_config();
        assert_eq!(stored.base_url.as_deref(), Some(base_url.as_str()));
        assert!(
            stored
                .cookies
                .as_deref()
                .is_some_and(|cookies| cookies.contains("XSRF-TOKEN"))
        );
        assert!(stored.partial_requests_enabled);
    }

    #[test]
    fn an_uninitialized_instance_is_refused_before_it_can_be_set_up_by_accident() {
        let (base_url, _) = fake_server(vec![response(
            "200 OK",
            r#"{"initialized":false,"plexClientIdentifier":"abc"}"#,
            &[],
        )]);
        let library = library();
        let session = SeerrSession::restore(library.clone());

        let error = session.connect(&base_url).expect_err("refused");
        assert!(matches!(error, super::SeerrError::Unusable(_)));
        assert!(error.to_string().contains("setup wizard"));
        assert_eq!(library.seerr_config(), SeerrConfig::default());
    }

    /// The commonest way a Seerr address goes wrong: it reaches a proxy, a
    /// sign-on page, or Seerr's own web front end, all of which answer 200 with
    /// HTML. A JSON parser position for that would send the user hunting for a
    /// fault in Seerr rather than in what they typed.
    #[test]
    fn an_address_that_answers_with_a_web_page_says_so() {
        let (base_url, _) = fake_server(vec![html_response(
            "200 OK",
            "<!DOCTYPE html>\n<html>\n<head>\n<title>Sign in</title>\n</head>\n<body>x</body>\n</html>",
        )]);
        let session = SeerrSession::restore(library());

        let error = session.connect(&base_url).expect_err("refused");
        assert!(matches!(error, SeerrError::Unusable(_)), "{error:?}");
        let message = error.to_string();
        assert!(message.contains("a web page"), "{message}");
        // Never a parser position: that is a fact about our decoder, not about
        // the address the user has to fix.
        assert!(!message.contains("line"), "{message}");
    }

    /// An address behind a sign-on proxy — authentik, Authelia, oauth2-proxy,
    /// Cloudflare Access — never reaches Seerr at all. Following the redirect
    /// would land on a login page and fail somewhere deep in it, reporting a
    /// fault that says nothing about the real problem.
    #[test]
    fn an_address_behind_a_sign_on_proxy_is_named_rather_than_chased() {
        let (base_url, requests) = fake_server(vec![
            redirect_response("https://auth.example.de/application/o/authorize/?state=jwt"),
            // Never reached: chasing this is exactly what must not happen.
            response("200 OK", INITIALIZED, &[]),
        ]);
        let session = SeerrSession::restore(library());

        let error = session.connect(&base_url).expect_err("refused");
        assert!(matches!(error, SeerrError::Unusable(_)), "{error:?}");
        let message = error.to_string();
        assert!(message.contains("auth.example.de"), "{message}");
        assert!(message.contains("sign-on proxy"), "{message}");

        assert_eq!(
            requests.lock().expect("lock").len(),
            1,
            "the redirect was followed"
        );
    }

    /// A body that really is JSON but the wrong shape is a different fault, and
    /// must not be reported as a wrong address.
    #[test]
    fn a_json_body_of_the_wrong_shape_stays_a_decode_failure() {
        let (base_url, _) = fake_server(vec![response("200 OK", "[1, 2, 3]", &[])]);
        let session = SeerrSession::restore(library());
        assert!(matches!(
            session.connect(&base_url),
            Err(SeerrError::Decode(_))
        ));
    }

    #[test]
    fn an_instance_wired_to_another_media_server_is_refused() {
        let (base_url, _) = fake_server(vec![response(
            "200 OK",
            r#"{"initialized":true,"mediaServerType":1}"#,
            &[],
        )]);
        let session = SeerrSession::restore(library());
        let error = session.connect(&base_url).expect_err("refused");
        assert!(error.to_string().contains("Jellyfin"));
    }

    #[test]
    fn a_failing_version_probe_does_not_fail_the_connect() {
        let (base_url, _) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("404 Not Found", "{}", &[]),
        ]);
        let session = SeerrSession::restore(library());
        let result = session.connect(&base_url).expect("connect");
        assert_eq!(result["version"], "");
    }

    #[test]
    fn moving_to_another_instance_logs_the_old_session_out_with_its_csrf_header() {
        let (old_url, old_requests) = fake_server(vec![response("204 No Content", "", &[])]);
        let (new_url, _) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("200 OK", VERSION, &[]),
        ]);
        let library = library();
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some(old_url.clone()),
                cookies: Some(r#"{"XSRF-TOKEN":"token123","connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                user_name: Some("pho".to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");
        let session = SeerrSession::restore(library.clone());

        session.connect(&new_url).expect("connect");

        let logout = old_requests.lock().expect("lock");
        assert_eq!(logout.len(), 1, "the old instance was not logged out");
        assert!(logout[0].starts_with("POST /api/v1/auth/logout HTTP/1.1"));
        assert!(logout[0].contains("cookie: XSRF-TOKEN=token123; connect.sid=abc"));
        // csurf accepts the token echoed as a header; without it a
        // CSRF-protected instance rejects every write.
        assert!(logout[0].contains("x-xsrf-token: token123"));

        // Nothing of the old link survives the move.
        let stored = library.seerr_config();
        assert_eq!(stored.base_url.as_deref(), Some(new_url.as_str()));
        assert_eq!(stored.user_id, None);
        assert!(
            stored
                .cookies
                .as_deref()
                .is_none_or(|cookies| !cookies.contains("connect.sid"))
        );
    }

    #[test]
    fn re_probing_the_same_instance_keeps_the_session_and_refreshes_the_csrf_pair() {
        let (base_url, _) = fake_server(vec![
            response("200 OK", INITIALIZED, &["XSRF-TOKEN=rotated; Path=/"]),
            response("200 OK", VERSION, &[]),
        ]);
        let library = library();
        signed_in(&library, "uid");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some(base_url.clone()),
                cookies: Some(r#"{"XSRF-TOKEN":"stale","connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                user_name: Some("pho".to_string()),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");
        let session = SeerrSession::restore(library.clone());

        let result = session.connect(&base_url).expect("connect");
        assert_eq!(result["linked"], true);

        let stored = library.seerr_config();
        assert_eq!(stored.user_id, Some(7));
        let cookies = stored.cookies.expect("cookies");
        assert!(cookies.contains(r#""connect.sid":"abc""#));
        assert!(cookies.contains(r#""XSRF-TOKEN":"rotated""#));
        assert!(is_linked(&session));
    }

    #[test]
    fn re_probing_after_an_account_switch_does_not_preserve_the_previous_link() {
        let (base_url, _) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("200 OK", VERSION, &[]),
        ]);
        let library = library();
        signed_in(&library, "uid");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some(base_url.clone()),
                cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                user_name: Some("pho".to_string()),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");
        let session = SeerrSession::restore(library.clone());

        signed_in(&library, "other-uid");
        let result = session.connect(&base_url).expect("connect");

        assert_eq!(result["linked"], false);
        let stored = library.seerr_config();
        assert_eq!(stored.base_url.as_deref(), Some(base_url.as_str()));
        assert_eq!(stored.cookies, None);
        assert_eq!(stored.user_id, None);
        assert_eq!(stored.jellyfin_user_id, None);
    }

    #[test]
    fn a_stale_probe_cannot_overwrite_a_newer_stored_link() {
        let library = library();
        signed_in(&library, "uid");
        linked(&library, "uid");
        let session = SeerrSession::restore(library.clone());
        let stale_revision = session.read().revision;
        let stale_client = SeerrClient::new(
            "https://seerr.test",
            SessionCookies::from_json(r#"{"connect.sid":"stale"}"#),
        );

        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some("https://new.test".to_string()),
                cookies: Some(r#"{"connect.sid":"new"}"#.to_string()),
                user_id: Some(99),
                user_name: Some("new user".to_string()),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                ..SeerrConfig::default()
            })
            .expect("new link");

        assert!(!session.absorb_probe(&stale_client, stale_revision));
        let stored = library.seerr_config();
        assert_eq!(stored.base_url.as_deref(), Some("https://new.test"));
        assert_eq!(stored.user_id, Some(99));
        assert!(
            stored
                .cookies
                .as_deref()
                .is_some_and(|cookies| cookies.contains(r#""connect.sid":"new""#))
        );
        assert_eq!(session.read().base_url.as_deref(), Some("https://new.test"));
    }

    #[test]
    fn a_configured_but_unlinked_instance_reports_its_switches() {
        let library = library();
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some("https://seerr.test".to_string()),
                movie_4k_enabled: true,
                partial_requests_enabled: true,
                ..SeerrConfig::default()
            })
            .expect("seerr config");

        let status = SeerrSession::restore(library).status();
        assert_eq!(status["configured"], true);
        assert_eq!(status["linked"], false);
        assert_eq!(status["serverUrl"], "https://seerr.test");
        assert_eq!(status["instance"]["movie4kEnabled"], true);
        assert_eq!(status["instance"]["series4kEnabled"], false);
        assert_eq!(status["instance"]["partialRequestsEnabled"], true);
    }

    #[test]
    fn the_state_round_trips_through_its_stored_form() {
        let config = SeerrConfig {
            base_url: Some("https://seerr.test".to_string()),
            cookies: Some(r#"{"XSRF-TOKEN":"t","connect.sid":"abc"}"#.to_string()),
            user_id: Some(7),
            user_name: Some("pho".to_string()),
            jellyfin_server_id: Some("srv".to_string()),
            jellyfin_user_id: Some("uid".to_string()),
            movie_4k_enabled: true,
            series_4k_enabled: true,
            partial_requests_enabled: true,
        };
        assert_eq!(
            SeerrState::from_config(config.clone(), 42).to_config(),
            config
        );
    }

    #[test]
    fn an_empty_cookie_jar_is_stored_as_no_cookies_at_all() {
        let state = SeerrState::from_config(
            SeerrConfig {
                base_url: Some("https://seerr.test".to_string()),
                ..SeerrConfig::default()
            },
            0,
        );
        assert_eq!(state.to_config().cookies, None);
    }

    #[test]
    fn a_poisoned_state_lock_keeps_the_existing_state() {
        let session = SeerrSession::restore(library());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut state = session.state.write().expect("state");
            state.base_url = Some("https://seerr.test".to_string());
            panic!("poison the state lock");
        }));

        assert_eq!(
            session.read().base_url.as_deref(),
            Some("https://seerr.test")
        );
    }

    #[test]
    fn linking_with_a_password_stores_the_session_and_the_account_it_belongs_to() {
        let (base_url, requests) = fake_server(vec![
            response("200 OK", INITIALIZED, &["XSRF-TOKEN=token123; Path=/"]),
            response("200 OK", ME, &[SESSION]),
            response("200 OK", ME, &[]),
            response("200 OK", ME, &[]),
            response("200 OK", QUOTA, &[]),
        ]);
        let library = library();
        signed_in(&library, "uid");
        configured(&library, &base_url);
        let session = SeerrSession::restore(library.clone());

        let result = session.link_with_password("pho", "hunter2").expect("link");
        assert_eq!(result["method"], "password");
        assert_eq!(result["linked"], true);
        assert_eq!(result["status"]["linked"], true);
        assert_eq!(result["status"]["user"]["name"], "pho");
        assert_eq!(result["status"]["capabilities"]["movie"]["request"], true);

        let requests = requests.lock().expect("lock");
        // A GET precedes the write, so a rotated CSRF pair is in hand before
        // the login needs it — and is echoed as the header csurf looks for.
        assert!(requests[0].starts_with("GET /api/v1/settings/public HTTP/1.1"));
        assert!(requests[1].starts_with("POST /api/v1/auth/jellyfin HTTP/1.1"));
        assert!(requests[1].contains("x-xsrf-token: token123"));
        assert!(requests[2].starts_with("GET /api/v1/auth/me HTTP/1.1"));
        assert!(requests[2].contains("cookie: XSRF-TOKEN=token123; connect.sid=s%3Aabc.def"));

        let stored = library.seerr_config();
        assert_eq!(stored.user_id, Some(7));
        assert_eq!(stored.user_name.as_deref(), Some("pho"));
        assert_eq!(stored.jellyfin_user_id.as_deref(), Some("uid"));
        assert_eq!(stored.jellyfin_server_id.as_deref(), Some("srv"));
        assert!(stored.partial_requests_enabled);
        assert!(
            stored
                .cookies
                .as_deref()
                .is_some_and(|cookies| cookies.contains("connect.sid"))
        );
        assert!(is_linked(&session));
    }

    /// The guard that password login rests on: Seerr cannot be asked which
    /// Jellyfin server it is wired to before logging in, so the account behind
    /// the session it hands back is what gets checked.
    #[test]
    fn a_login_as_a_different_media_server_user_is_refused_and_logged_out() {
        let (base_url, requests) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("200 OK", ME, &[SESSION]),
            response(
                "200 OK",
                r#"{"id":9,"displayName":"someone","jellyfinUserId":"another-uid"}"#,
                &[],
            ),
            response("204 No Content", "", &[]),
        ]);
        let library = library();
        signed_in(&library, "uid");
        configured(&library, &base_url);
        let session = SeerrSession::restore(library.clone());

        let error = session
            .link_with_password("someone", "hunter2")
            .expect_err("refused");
        assert!(matches!(error, SeerrError::Unusable(_)));
        assert!(error.to_string().contains("different media-server user"));

        let requests = requests.lock().expect("lock");
        assert_eq!(requests.len(), 4, "the refused session was not logged out");
        assert!(requests[3].starts_with("POST /api/v1/auth/logout HTTP/1.1"));

        // Fail closed: nothing about the refused session is on disk.
        let stored = library.seerr_config();
        assert_eq!(stored.cookies, None);
        assert_eq!(stored.user_id, None);
        assert_eq!(stored.jellyfin_user_id, None);
        assert!(!is_linked(&session));
    }

    #[test]
    fn a_login_seerr_cannot_attribute_to_an_account_is_refused() {
        let (base_url, _) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("200 OK", "{}", &[SESSION]),
            response("200 OK", r#"{"id":9,"displayName":"someone"}"#, &[]),
            response("204 No Content", "", &[]),
        ]);
        let library = library();
        signed_in(&library, "uid");
        configured(&library, &base_url);

        let error = SeerrSession::restore(library.clone())
            .link_with_password("someone", "hunter2")
            .expect_err("refused");
        assert!(error.to_string().contains("did not say which"));
        assert_eq!(library.seerr_config().cookies, None);
    }

    /// Jellyfin hands its GUIDs out both with and without dashes; a plain
    /// comparison would read a match as the account switch this guard exists
    /// to catch.
    #[test]
    fn the_account_guard_ignores_how_the_id_is_punctuated() {
        assert!(same_media_server_user(
            "8AB2E0F0-3B5C-4D3E-9F00-000000000001",
            "8ab2e0f03b5c4d3e9f00000000000001"
        ));
        assert!(!same_media_server_user("uid", "other-uid"));
        assert!(!same_media_server_user("", ""));
    }

    #[test]
    fn a_user_seerr_has_never_imported_gets_its_own_message() {
        let (base_url, _) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("403 Forbidden", r#"{"message":"Access denied"}"#, &[]),
        ]);
        let library = library();
        signed_in(&library, "uid");
        configured(&library, &base_url);

        let error = SeerrSession::restore(library)
            .link_with_password("pho", "hunter2")
            .expect_err("refused");
        assert_eq!(error, SeerrError::UnknownUser);
        assert!(error.to_string().contains("administrator"));
    }

    /// A mistyped password must not read as a lapsed session: the user is
    /// establishing one, and `Unauthorized` is what puts the UI into the
    /// re-link prompt they are already in.
    #[test]
    fn a_rejected_password_is_not_reported_as_a_lapsed_session() {
        let (base_url, _) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("401 Unauthorized", r#"{"message":"Unauthorized"}"#, &[]),
        ]);
        let library = library();
        signed_in(&library, "uid");
        configured(&library, &base_url);

        let error = SeerrSession::restore(library)
            .link_with_password("pho", "wrong")
            .expect_err("rejected");
        assert_eq!(error, SeerrError::LoginRejected);
    }

    #[test]
    fn linking_needs_an_instance_and_credentials_before_anything_is_sent() {
        let library = library();
        signed_in(&library, "uid");
        let session = SeerrSession::restore(library.clone());
        assert!(matches!(
            session.link_with_password("pho", "hunter2"),
            Err(SeerrError::NotConfigured)
        ));
        assert!(matches!(
            session.link_start(),
            Err(SeerrError::NotConfigured)
        ));

        configured(&library, "https://seerr.test");
        let session = SeerrSession::restore(library);
        assert!(matches!(
            session.link_with_password("  ", "hunter2"),
            Err(SeerrError::Unusable(_))
        ));
        assert!(matches!(
            session.link_poll("  "),
            Err(SeerrError::Unusable(_))
        ));
    }

    #[test]
    fn unlinking_ends_the_session_at_the_instance_and_keeps_only_the_address() {
        let (base_url, requests) = fake_server(vec![response("204 No Content", "", &[])]);
        let library = library();
        signed_in(&library, "uid");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some(base_url.clone()),
                cookies: Some(r#"{"XSRF-TOKEN":"token123","connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                user_name: Some("pho".to_string()),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                partial_requests_enabled: true,
                ..SeerrConfig::default()
            })
            .expect("seerr config");
        let session = SeerrSession::restore(library.clone());

        let status = session.unlink();
        assert_eq!(status["linked"], false);
        assert_eq!(status["configured"], true);

        let requests = requests.lock().expect("lock");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("POST /api/v1/auth/logout HTTP/1.1"));
        assert!(requests[0].contains("x-xsrf-token: token123"));

        let stored = library.seerr_config();
        assert_eq!(stored.base_url.as_deref(), Some(base_url.as_str()));
        assert_eq!(stored.cookies, None);
        assert_eq!(stored.user_id, None);
        assert_eq!(stored.jellyfin_user_id, None);
    }

    #[test]
    fn quick_connect_links_without_a_password_when_both_halves_support_it() {
        let (seerr_url, seerr_requests) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("200 OK", r#"{"code":"AB12CD","secret":"s3cret"}"#, &[]),
            response("200 OK", ME, &[SESSION]),
            response("200 OK", ME, &[]),
            response("200 OK", ME, &[]),
            response("200 OK", QUOTA, &[]),
        ]);
        let (jellyfin_url, jellyfin_requests) = fake_server(vec![
            response("200 OK", "true", &[]),
            response("200 OK", "true", &[]),
        ]);
        let library = library();
        signed_in_to(&library, &jellyfin_url, "uid");
        configured(&library, &seerr_url);
        let session = SeerrSession::restore(library.clone());

        let result = session.link_start().expect("link");
        assert_eq!(result["method"], "quickconnect");
        assert_eq!(result["linked"], true);
        assert_eq!(result["status"]["user"]["name"], "pho");

        let seerr_requests = seerr_requests.lock().expect("lock");
        assert!(
            seerr_requests[1]
                .starts_with("POST /api/v1/auth/jellyfin/quickconnect/initiate HTTP/1.1")
        );
        assert!(
            seerr_requests[2]
                .starts_with("POST /api/v1/auth/jellyfin/quickconnect/authenticate HTTP/1.1")
        );
        // The code Seerr minted is approved on our own server, by us — this is
        // the step that makes the flow password-less, and the one that proves
        // the handshake belongs to the server we are signed in to.
        let jellyfin_requests = jellyfin_requests.lock().expect("lock");
        assert!(jellyfin_requests[0].starts_with("GET /QuickConnect/Enabled HTTP/1.1"));
        assert!(
            jellyfin_requests[1].starts_with("POST /QuickConnect/Authorize?code=AB12CD HTTP/1.1")
        );

        assert_eq!(library.seerr_config().user_id, Some(7));
        assert!(is_linked(&session));
    }

    /// Quick Connect is off by default on Jellyfin, so this is the common case
    /// — and it must not look like a failure.
    #[test]
    fn quick_connect_defers_to_the_password_path_when_the_server_has_it_off() {
        let (seerr_url, seerr_requests) = fake_server(vec![response("200 OK", INITIALIZED, &[])]);
        let (jellyfin_url, _) = fake_server(vec![response("200 OK", "false", &[])]);
        let library = library();
        signed_in_to(&library, &jellyfin_url, "uid");
        configured(&library, &seerr_url);

        let result = SeerrSession::restore(library)
            .link_start()
            .expect("no error surfaces");
        assert_eq!(result["method"], "password");
        assert_eq!(result["linked"], false);
        // No handshake was started, so none is left dangling on the instance.
        assert_eq!(seerr_requests.lock().expect("lock").len(), 1);
    }

    /// The Quick Connect login routes are absent from every stable release up
    /// to and including v3.3.0.
    #[test]
    fn quick_connect_defers_to_the_password_path_when_seerr_has_no_such_route() {
        let (seerr_url, _) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("404 Not Found", r#"{"message":"Not Found"}"#, &[]),
        ]);
        let (jellyfin_url, _) = fake_server(vec![response("200 OK", "true", &[])]);
        let library = library();
        signed_in_to(&library, &jellyfin_url, "uid");
        configured(&library, &seerr_url);

        let result = SeerrSession::restore(library)
            .link_start()
            .expect("no error surfaces");
        assert_eq!(result["method"], "password");
    }

    /// A server that refuses to approve the code — because the handshake is not
    /// its own — is the same fallback, not an error.
    #[test]
    fn a_handshake_our_server_will_not_approve_falls_back_to_the_password_path() {
        let (seerr_url, _) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("200 OK", r#"{"code":"AB12CD","secret":"s3cret"}"#, &[]),
        ]);
        let (jellyfin_url, _) = fake_server(vec![
            response("200 OK", "true", &[]),
            response("403 Forbidden", "{}", &[]),
        ]);
        let library = library();
        signed_in_to(&library, &jellyfin_url, "uid");
        configured(&library, &seerr_url);

        let result = SeerrSession::restore(library)
            .link_start()
            .expect("no error surfaces");
        assert_eq!(result["method"], "password");
    }

    #[test]
    fn a_seerr_that_has_not_caught_up_yet_is_polled_rather_than_failed() {
        let (seerr_url, _) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("200 OK", r#"{"code":"AB12CD","secret":"s3cret"}"#, &[]),
            response("400 Bad Request", r#"{"message":"not ready"}"#, &[]),
        ]);
        let (jellyfin_url, _) = fake_server(vec![
            response("200 OK", "true", &[]),
            response("200 OK", "true", &[]),
        ]);
        let library = library();
        signed_in_to(&library, &jellyfin_url, "uid");
        configured(&library, &seerr_url);

        let result = SeerrSession::restore(library)
            .link_start()
            .expect("pending, not failed");
        assert_eq!(result["method"], "quickconnect");
        assert_eq!(result["linked"], false);
        assert_eq!(result["secret"], "s3cret");
    }

    #[test]
    fn polling_finishes_the_link_the_start_call_left_open() {
        let (seerr_url, requests) = fake_server(vec![
            response("200 OK", INITIALIZED, &[]),
            response("200 OK", ME, &[SESSION]),
            response("200 OK", ME, &[]),
            response("200 OK", ME, &[]),
            response("200 OK", QUOTA, &[]),
        ]);
        let library = library();
        signed_in(&library, "uid");
        configured(&library, &seerr_url);
        let session = SeerrSession::restore(library.clone());

        let result = session.link_poll("s3cret").expect("link");
        assert_eq!(result["linked"], true);
        assert_eq!(result["status"]["linked"], true);

        let requests = requests.lock().expect("lock");
        assert!(requests[1].starts_with("POST /api/v1/auth/jellyfin/quickconnect/authenticate"));
        assert_eq!(
            library.seerr_config().jellyfin_user_id.as_deref(),
            Some("uid")
        );
    }

    // ------------------------------------------------------------------ reads

    const SEARCH: &str = r#"{"page":1,"totalPages":1,"totalResults":3,"results":[
        {"id":603,"mediaType":"movie","title":"The Matrix","releaseDate":"1999-03-30",
         "posterPath":"/matrix.jpg","mediaInfo":{"tmdbId":603,"status":5,"status4k":1}},
        {"id":603,"mediaType":"tv","name":"Not The Matrix","firstAirDate":"2010-01-01"},
        {"id":6384,"mediaType":"person","name":"Keanu Reeves"}]}"#;

    fn seed_library(library: &Library) {
        let dto = |json: &str| serde_json::from_str(json).expect("dto");
        library
            .upsert_page(&[
                dto(r#"{"Id":"m1","Name":"The Matrix","Type":"Movie",
                        "ProviderIds":{"Tmdb":"603"}}"#),
                dto(r#"{"Id":"s1","Name":"Severance","Type":"Series",
                        "ProviderIds":{"Tmdb":"95396"}}"#),
            ])
            .expect("seed");
    }

    /// The join this whole chunk turns on: a result the library already has
    /// resolves to that item, and one it does not resolves to a request.
    #[test]
    fn search_results_are_joined_to_the_library_by_kind_and_tmdb_id() {
        let (base_url, requests) = fake_server(vec![response("200 OK", SEARCH, &[])]);
        let (library, session) = session_linked_to(&base_url);
        seed_library(&library);

        let page = session.search("matrix", 1).expect("search");
        let results = page["results"].as_array().expect("results");
        // The person is gone: nothing to join, nothing to request.
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["mediaType"], "movie");
        assert_eq!(results[0]["title"], "The Matrix");
        assert_eq!(results[0]["year"], 1999);
        assert_eq!(results[0]["status"], "available");
        assert_eq!(results[0]["libraryItemId"], "m1");
        // Same TMDB id, other namespace: it must not inherit the movie's item.
        assert_eq!(results[1]["mediaType"], "tv");
        assert_eq!(results[1]["libraryItemId"], Value::Null);
        assert_eq!(results[1]["status"], "unknown");
        assert_eq!(page["totalResults"], 3);

        let requests = requests.lock().expect("lock");
        assert!(requests[0].starts_with("GET /api/v1/search?query=matrix&page=1"));
        assert!(requests[0].contains("cookie: connect.sid=abc"));
    }

    #[test]
    fn an_empty_search_term_costs_no_round_trip() {
        let (base_url, requests) = fake_server(Vec::new());
        let (_library, session) = session_linked_to(&base_url);

        let page = session.search("   ", 1).expect("search");
        assert_eq!(page["results"].as_array().map(Vec::len), Some(0));
        assert!(requests.lock().expect("lock").is_empty());
    }

    #[test]
    fn discover_rows_are_named_rather_than_addressed() {
        let (base_url, requests) = fake_server(vec![response("200 OK", SEARCH, &[])]);
        let (_library, session) = session_linked_to(&base_url);
        let options =
            DiscoverOptions::from_values(None, None, None, None, Some("movie"), Some("week"))
                .expect("options");

        session
            .discover(super::DiscoverKind::Trending, 2, &options)
            .expect("discover");
        assert!(
            requests.lock().expect("lock")[0].starts_with(
                "GET /api/v1/discover/trending?page=2&mediaType=movie&timeWindow=week"
            )
        );
        assert_eq!(
            super::DiscoverKind::from_id("movies"),
            Some(DiscoverKind::Movies)
        );
        assert_eq!(
            super::DiscoverKind::from_id("upcoming-tv").map(DiscoverKind::path),
            Some("discover/tv/upcoming")
        );
        assert_eq!(super::DiscoverKind::from_id("../settings"), None);
    }

    #[test]
    fn discover_filters_are_allowlisted_and_shaped_for_each_media_kind() {
        let options = DiscoverOptions::from_values(
            Some("18"),
            Some("rating"),
            Some("7"),
            Some("1990"),
            None,
            None,
        )
        .expect("movie options");
        let today = UtcDate {
            year: 2026,
            month: 8,
            day: 1,
        };
        let movie = options.query_pairs_for(DiscoverKind::Movies, 4, false, today);
        assert_eq!(
            movie,
            vec![
                ("page", "4".to_string()),
                ("genre", "18".to_string()),
                ("primaryReleaseDateGte", "1990-01-01".to_string()),
                ("primaryReleaseDateLte", "1999-12-31".to_string()),
                ("sortBy", "vote_average.desc".to_string()),
                ("voteCountGte", "50".to_string()),
                ("voteAverageGte", "7".to_string()),
            ]
        );
        assert!(
            options
                .companion_query_pairs(DiscoverKind::Movies, 4)
                .contains(&("releaseDecade", "1990".to_string()))
        );

        let tv_options =
            DiscoverOptions::from_values(None, Some("newest"), None, Some("2020"), None, None)
                .expect("tv options");
        let tv = tv_options.query_pairs_for(DiscoverKind::Tv, 1, false, today);
        assert!(tv.contains(&("sortBy", "first_air_date.desc".to_string())));
        assert!(tv.contains(&("firstAirDateGte", "2020-01-01".to_string())));
        assert!(tv.contains(&("firstAirDateLte", "2026-08-01".to_string())));
        assert!(
            DiscoverOptions::from_values(Some("../settings"), None, None, None, None, None)
                .is_err()
        );
        assert!(
            DiscoverOptions::from_values(None, Some("random"), None, None, None, None).is_err()
        );
        assert!(DiscoverOptions::from_values(None, None, None, Some("1995"), None, None).is_err());
        assert!(DiscoverOptions::from_values(None, None, None, Some("9990"), None, None).is_err());
        assert_eq!(
            UtcDate::from_unix_days(0),
            UtcDate {
                year: 1970,
                month: 1,
                day: 1,
            }
        );
    }

    #[test]
    fn a_series_detail_offers_the_seasons_that_can_be_requested() {
        let (base_url, requests) = fake_server(vec![response(
            "200 OK",
            r#"{"id":95396,"name":"Severance","firstAirDate":"2022-02-18",
                "episodeRunTime":[45],"genres":[{"id":18,"name":"Drama"}],
                "seasons":[{"id":1,"seasonNumber":0,"name":"Specials","episodeCount":2},
                           {"id":2,"seasonNumber":1,"name":"Season 1","episodeCount":9},
                           {"id":3,"seasonNumber":2,"name":"Season 2","episodeCount":10}],
                "mediaInfo":{"tmdbId":95396,"status":4,"status4k":1,
                             "seasons":[{"seasonNumber":1,"status":5,"status4k":1}]}}"#,
            &[],
        )]);
        let (library, session) = session_linked_to(&base_url);
        seed_library(&library);

        let detail = session.media_detail("tv", 95396).expect("detail");
        assert_eq!(detail["title"], "Severance");
        assert_eq!(detail["status"], "partial");
        assert_eq!(detail["runtimeMinutes"], 45);
        assert_eq!(detail["genres"][0], "Drama");
        assert_eq!(detail["libraryItemId"], "s1");

        let seasons = detail["seasons"].as_array().expect("seasons");
        // Specials are left out, exactly as Seerr's own request modal does.
        assert_eq!(seasons.len(), 2);
        assert_eq!(seasons[0]["seasonNumber"], 1);
        assert_eq!(seasons[0]["status"], "available");
        assert_eq!(seasons[1]["seasonNumber"], 2);
        assert_eq!(seasons[1]["status"], "unknown");

        assert!(requests.lock().expect("lock")[0].starts_with("GET /api/v1/tv/95396 HTTP/1.1"));
    }

    #[test]
    fn a_movie_detail_keeps_rich_metadata_and_safe_trailer_release_data() {
        let (base_url, _) = fake_server(vec![response(
            "200 OK",
            r#"{"id":603,"title":"The Matrix","originalTitle":"The Matrix",
                "releaseDate":"1999-03-30","status":"Released","runtime":136,
                "overview":"A hacker discovers the truth.","tagline":"Welcome to the Real World.",
                "voteAverage":8.2,"voteCount":26000,"originalLanguage":"en",
                "budget":63000000,"revenue":467200000,
                "genres":[{"id":28,"name":"Action"}],
                "productionCompanies":[{"id":79,"name":"Village Roadshow Pictures"}],
                "productionCountries":[{"iso_3166_1":"US","name":"United States"}],
                "spokenLanguages":[{"iso_639_1":"en","name":"English","englishName":"English"}],
                "credits":{"cast":[{"id":6384,"name":"Keanu Reeves","character":"Neo",
                                    "profilePath":"/keanu.jpg"}],
                           "crew":[{"id":1,"name":"Lana Wachowski","job":"Director",
                                    "department":"Directing"},
                                   {"id":2,"name":"Lilly Wachowski","job":"Screenplay",
                                    "department":"Writing"}]},
                "relatedVideos":[{"site":"YouTube","type":"Trailer","key":"abcdefghijk",
                                  "name":"Official Trailer","size":1080},
                                 {"site":"YouTube","type":"Trailer","key":"not/a/key",
                                  "name":"Unsafe","size":2160}],
                "releases":{"results":[{"iso_3166_1":"US","release_dates":[
                    {"type":3,"release_date":"1999-03-31T00:00:00.000Z","certification":"R"}]}]},
                "mediaInfo":{"tmdbId":603,"status":5,"status4k":1}}"#,
            &[],
        )]);
        let (library, session) = session_linked_to(&base_url);
        seed_library(&library);

        let detail = session.media_detail("movie", 603).expect("detail");
        assert_eq!(detail["overview"], "A hacker discovers the truth.");
        assert_eq!(detail["productionStatus"], "Released");
        assert_eq!(detail["voteAverage"], 8.2);
        assert_eq!(detail["voteCount"], 26_000);
        assert_eq!(detail["directors"], json!(["Lana Wachowski"]));
        assert_eq!(detail["writers"], json!(["Lilly Wachowski"]));
        assert_eq!(detail["cast"][0]["character"], "Neo");
        assert_eq!(detail["trailer"]["key"], "abcdefghijk");
        assert_eq!(detail["releaseDates"][0]["type"], "cinema");
        assert_eq!(detail["releaseDates"][0]["certification"], "R");
    }

    /// A `person` id in the media route would address an unrelated title, so it
    /// is refused before a request is sent.
    #[test]
    fn a_detail_for_a_kind_that_is_not_requestable_is_refused_locally() {
        let (base_url, requests) = fake_server(Vec::new());
        let (_library, session) = session_linked_to(&base_url);

        assert!(matches!(
            session.media_detail("person", 6384),
            Err(SeerrError::Unusable(_))
        ));
        assert!(matches!(
            session.media_detail("movie", 0),
            Err(SeerrError::Unusable(_))
        ));
        assert!(requests.lock().expect("lock").is_empty());
    }

    // ----------------------------------------------------------------- writes

    #[test]
    fn requesting_a_movie_sends_one_unretried_write() {
        let (base_url, requests) = fake_server(vec![response(
            "201 Created",
            r#"{"id":12,"status":1,"type":"movie","is4k":false,"createdAt":"2026-07-27T10:00:00Z",
                "media":{"tmdbId":603,"mediaType":"movie","status":2,"status4k":1}}"#,
            &[],
        )]);
        let (library, session) = session_linked_to(&base_url);
        seed_library(&library);

        let created = session
            .create_request("movie", 603, None, false, None)
            .expect("request");
        assert_eq!(created["id"], 12);
        assert_eq!(created["status"], "pending");
        assert_eq!(created["mediaStatus"], "pending");
        assert_eq!(created["tmdbId"], 603);
        assert_eq!(created["libraryItemId"], "m1");

        let requests = requests.lock().expect("lock");
        assert_eq!(requests.len(), 1, "a write must never be retried");
        assert!(requests[0].starts_with("POST /api/v1/request HTTP/1.1"));
        let body = compact(&requests[0]);
        assert!(body.contains(r#""mediaType":"movie""#));
        assert!(body.contains(r#""mediaId":603"#));
        // Movies have no seasons; sending an empty list would be rejected.
        assert!(!body.contains("seasons"));
    }

    #[test]
    fn requesting_named_seasons_asks_for_exactly_those() {
        let (base_url, requests) = fake_server(vec![response(
            "201 Created",
            r#"{"id":13,"status":1,"type":"tv","media":{"tmdbId":95396,"mediaType":"tv","status":2},
                "seasons":[{"seasonNumber":2,"status":1}]}"#,
            &[],
        )]);
        let (_library, session) = session_linked_to(&base_url);

        let created = session
            .create_request("tv", 95396, Some(vec![2]), false, None)
            .expect("request");
        assert_eq!(created["seasons"], json!([2]));
        assert!(compact(&requests.lock().expect("lock")[0]).contains(r#""seasons":[2]"#));
    }

    #[test]
    fn advanced_request_options_are_scoped_to_the_matching_download_service() {
        let (base_url, requests) = fake_server(vec![
            response(
                "200 OK",
                r#"{"id":7,"displayName":"pho","jellyfinUserId":"uid","permissions":8224}"#,
                &[],
            ),
            response(
                "200 OK",
                r#"[{"id":0,"name":"Movies","is4k":false,"isDefault":true,"activeProfileId":2},
                    {"id":1,"name":"Movies 4K","is4k":true,"isDefault":false,"activeProfileId":3}]"#,
                &[],
            ),
            response(
                "200 OK",
                r#"{"profiles":[{"id":2,"name":"HD-1080p"},{"id":1,"name":"Any"}]}"#,
                &[],
            ),
        ]);
        let (_library, session) = session_linked_to(&base_url);

        let options = session.request_options("movie", false).expect("options");
        let destination = &options["destinations"][0];
        assert_eq!(destination["id"], 0);
        assert_eq!(destination["name"], "Movies");
        assert_eq!(destination["profiles"][0]["name"], "Any");
        assert_eq!(destination["profiles"][1]["isDefault"], true);

        let requests = requests.lock().expect("lock");
        assert_eq!(requests.len(), 3);
        assert!(requests[0].starts_with("GET /api/v1/auth/me HTTP/1.1"));
        assert!(requests[1].starts_with("GET /api/v1/service/radarr HTTP/1.1"));
        assert!(requests[2].starts_with("GET /api/v1/service/radarr/0 HTTP/1.1"));
    }

    #[test]
    fn a_selected_profile_is_permission_checked_and_sent_with_one_write() {
        let (base_url, requests) = fake_server(vec![
            response(
                "200 OK",
                r#"{"id":7,"displayName":"pho","jellyfinUserId":"uid","permissions":8224}"#,
                &[],
            ),
            response(
                "201 Created",
                r#"{"id":15,"status":1,"type":"movie",
                    "media":{"tmdbId":603,"mediaType":"movie","status":2}}"#,
                &[],
            ),
        ]);
        let (_library, session) = session_linked_to(&base_url);

        session
            .create_request(
                "movie",
                603,
                None,
                false,
                Some(RequestProfileSelection {
                    server_id: 0,
                    profile_id: 2,
                }),
            )
            .expect("request");

        let requests = requests.lock().expect("lock");
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.starts_with("POST "))
                .count(),
            1,
            "a write must never be retried"
        );
        let body = compact(&requests[1]);
        assert!(body.contains(r#""serverId":0"#));
        assert!(body.contains(r#""profileId":2"#));
    }

    /// "Request this show" with no season named is Seerr's own `all`, which it
    /// expands to whatever it does not already have.
    #[test]
    fn requesting_a_series_without_naming_seasons_asks_for_all_of_them() {
        let (base_url, requests) = fake_server(vec![response(
            "201 Created",
            r#"{"id":14,"status":1,"type":"tv","media":{"tmdbId":95396,"mediaType":"tv"}}"#,
            &[],
        )]);
        let (_library, session) = session_linked_to(&base_url);

        session
            .create_request("tv", 95396, None, false, None)
            .expect("request");
        assert!(compact(&requests.lock().expect("lock")[0]).contains(r#""seasons":"all""#));
    }

    #[test]
    fn requests_are_scoped_to_the_signed_in_seerr_user() {
        let (base_url, requests) = fake_server(vec![response(
            "200 OK",
            r#"{"pageInfo":{"pages":1,"pageSize":20,"results":1,"page":1},
                "results":[{"id":12,"status":2,"type":"movie","is4k":false,
                            "media":{"tmdbId":603,"mediaType":"movie","status":5,"status4k":1}}]}"#,
            &[],
        )]);
        let (library, session) = session_linked_to(&base_url);
        seed_library(&library);

        let page = session.requests(20, 0, "all").expect("requests");
        assert_eq!(page["totalResults"], 1);
        let first = &page["results"][0];
        assert_eq!(first["status"], "approved");
        assert_eq!(first["mediaStatus"], "available");
        // Available and already in the library: the card links to the item.
        assert_eq!(first["libraryItemId"], "m1");

        let requests = requests.lock().expect("lock");
        assert!(requests[0].starts_with("GET /api/v1/request?"));
        assert!(requests[0].contains("requestedBy=7"), "{}", requests[0]);
        assert!(requests[0].contains("take=20"));
    }

    /// A 4K request reports the 4K availability, not the ordinary one.
    #[test]
    fn a_four_k_request_reports_the_four_k_status() {
        let (base_url, _) = fake_server(vec![response(
            "200 OK",
            r#"{"pageInfo":{"pages":1,"page":1,"results":1},
                "results":[{"id":15,"status":1,"type":"movie","is4k":true,
                            "media":{"tmdbId":603,"mediaType":"movie","status":5,"status4k":2}}]}"#,
            &[],
        )]);
        let (_library, session) = session_linked_to(&base_url);

        let page = session
            .requests(20, 0, "nonsense filter")
            .expect("requests");
        assert_eq!(page["results"][0]["mediaStatus"], "pending");
    }

    #[test]
    fn cancelling_a_request_sends_a_delete_with_the_csrf_header() {
        let (base_url, requests) = fake_server(vec![response("204 No Content", "", &[])]);
        let library = library();
        signed_in(&library, "uid");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some(base_url.clone()),
                cookies: Some(r#"{"XSRF-TOKEN":"token123","connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");
        let session = SeerrSession::restore(library);

        assert_eq!(
            session.cancel_request(12).expect("cancel")["cancelled"],
            true
        );
        let requests = requests.lock().expect("lock");
        assert_eq!(requests.len(), 1, "a cancellation must never be retried");
        assert!(requests[0].starts_with("DELETE /api/v1/request/12 HTTP/1.1"));
        assert!(requests[0].contains("x-xsrf-token: token123"));
    }

    /// Seerr answers 401 — not 403 — to a valid session that may not cancel
    /// somebody else's request. Taking that at face value would sign the user
    /// out for pressing a button they were never allowed to press.
    #[test]
    fn a_refused_write_is_a_permission_error_not_a_lapsed_session() {
        let (base_url, requests) = fake_server(vec![
            response("401 Unauthorized", r#"{"message":"Unauthorized"}"#, &[]),
            // `/auth/me` answers, so the session itself is fine.
            response("200 OK", ME, &[]),
        ]);
        let (library, session) = session_linked_to(&base_url);

        let error = session.cancel_request(12).expect_err("refused");
        assert_eq!(error, SeerrError::PermissionDenied);

        let requests = requests.lock().expect("lock");
        assert!(requests[1].starts_with("GET /api/v1/auth/me HTTP/1.1"));
        // The link survives: nothing about this was a session expiry.
        assert!(!session.read().expired);
        assert!(library.seerr_config().cookies.is_some());
        assert!(is_linked(&session));
    }

    #[test]
    fn a_lapsed_session_is_confirmed_before_the_re_link_prompt_appears() {
        let (base_url, _) = fake_server(vec![
            response("401 Unauthorized", r#"{"message":"Unauthorized"}"#, &[]),
            response("401 Unauthorized", r#"{"message":"Unauthorized"}"#, &[]),
        ]);
        let (_library, session) = session_linked_to(&base_url);

        let error = session.cancel_request(12).expect_err("expired");
        assert_eq!(error, SeerrError::Unauthorized);
        assert!(session.read().expired);
        // Every later acquisition refuses until the user re-links.
        assert!(matches!(session.client(), Err(SeerrError::Unauthorized)));
    }

    #[test]
    fn reads_need_a_linked_instance() {
        let session = SeerrSession::restore(library());
        assert!(matches!(
            session.search("matrix", 1),
            Err(SeerrError::NotConfigured)
        ));
        assert!(matches!(
            session.requests(20, 0, "all"),
            Err(SeerrError::NotConfigured)
        ));
        assert!(matches!(
            session.cancel_request(0),
            Err(SeerrError::Unusable(_))
        ));
    }

    /// Guards against the credentials row disappearing under a live session.
    #[test]
    fn a_link_with_no_jellyfin_binding_is_left_alone() {
        let library = library();
        signed_in(&library, "uid");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some("https://seerr.test".to_string()),
                cookies: Some(r#"{"XSRF-TOKEN":"t"}"#.to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");

        let session = SeerrSession::restore(library.clone());
        session.revalidate();
        assert!(library.seerr_config().cookies.is_some());
    }
}
