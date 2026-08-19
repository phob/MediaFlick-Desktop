//! The JSON API and static assets served on `mediaflick-desktop://app/`.
//!
//! Handlers run on a CEF background thread (never the UI or IO thread), so
//! blocking SQLite and HTTP calls are safe here.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde_json::{Value, json};

use crate::app::services::{self, Services};
use crate::app::services::{ShellFilePickerTarget, ShellRequest};
use crate::app::urls::{encode_path_segment, percent_decode, query_param};
use crate::companion::ProviderError;
use crate::integrations::letterboxd;
use crate::jellyfin::api::items;
use crate::jellyfin::api::model::{BaseItemDto, BaseItemPerson, MediaSourceInfo, MediaStream};
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::play::{self, PlayOptions};
use crate::library::ExternalProfile;
use crate::library::model::technical_media_streams_json;
use crate::library::{
    ItemPlaybackPreference, ItemQuery, ItemSort, Library, resolve_playback_preference, sync,
};
use crate::maintenance::player_setup;
use crate::players::mpv::input::MpvInputBindings;
use crate::preferences::{
    AppSettings, AppearanceSettingsPatch, ApplicationSettingsPatch, PlaybackSettingsPatch,
    PlayerSettingsPatch, StreamingQuality,
};
use crate::seerr::api::SeerrError;
use crate::seerr::api::client::{fetch_tmdb_image, tmdb_image_url};
use crate::seerr::{DiscoverKind, DiscoverOptions, RequestProfileSelection};

/// Rows shown on the home screen.
const HOME_ROW_LIMIT: i64 = 24;
/// Films rotating through the top billboard.
const BILLBOARD_LIMIT: i64 = 5;
/// Bound each proxied video request so a trailer is never buffered wholesale.
const TRAILER_CHUNK_SIZE: u64 = 4 * 1024 * 1024;
/// Posters are content-addressed by image tag, so they never go stale.
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";
const NO_STORE: &str = "no-store";
/// A pathological server must fail Discover safely rather than loop forever or
/// return an ownership check it only partially completed.
const MAX_PERSON_QUERY_PAGES: usize = 100;

#[derive(Debug, Clone)]
pub struct ApiRequest {
    pub method: String,
    pub path: String,
    pub query: String,
    pub body: Vec<u8>,
    /// Browser media requests use this to seek without downloading a trailer
    /// wholesale. Other request headers are intentionally not forwarded.
    pub range: Option<String>,
    /// Raised by CEF's `cancel` callback when the browser abandons the request
    /// (an aborted fetch, a closed page). Handlers block synchronously, so
    /// this flag is how a multi-request handler stops issuing further
    /// upstream calls for an answer nobody will read.
    pub cancelled: Arc<AtomicBool>,
}

impl ApiRequest {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|_| json!({}))
    }

    fn param(&self, key: &str) -> Option<String> {
        query_param(&self.query, key).filter(|value| !value.trim().is_empty())
    }

    fn is(&self, method: &str) -> bool {
        self.method.eq_ignore_ascii_case(method)
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
pub struct ApiResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub cache_control: &'static str,
    pub headers: Vec<(String, String)>,
}

impl ApiResponse {
    fn json(status: u16, value: Value) -> Self {
        let body = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
        drop(value);
        Self {
            status,
            content_type: "application/json; charset=utf-8".to_string(),
            body,
            cache_control: NO_STORE,
            headers: Vec::new(),
        }
    }

    fn ok(value: Value) -> Self {
        Self::json(200, value)
    }

    fn error(status: u16, message: impl Into<String>) -> Self {
        Self::json(status, json!({ "error": message.into() }))
    }

    fn from_api_error(error: &ApiError) -> Self {
        Self::json(
            error.client_status(),
            json!({ "error": error.to_string(), "expired": *error == ApiError::Unauthorized }),
        )
    }

    /// Seerr failures get their own mapping: routed through [`Self::from_api_error`]
    /// a lapsed Seerr session would read to the UI as a lapsed *Jellyfin* one and
    /// send it to the sign-in screen.
    fn from_seerr_error(error: &SeerrError) -> Self {
        Self::json(
            error.client_status(),
            json!({
                "error": error.to_string(),
                "seerrExpired": *error == SeerrError::Unauthorized,
            }),
        )
    }

    fn from_provider_error(error: &ProviderError) -> Self {
        match error {
            ProviderError::Companion(error) => Self::from_api_error(error),
            ProviderError::Direct(error) => Self::from_seerr_error(error),
        }
    }

    fn asset(content_type: &str, body: &'static [u8], cache_control: &'static str) -> Self {
        Self {
            status: 200,
            content_type: content_type.to_string(),
            body: body.to_vec(),
            cache_control,
            headers: Vec::new(),
        }
    }

    fn bytes(content_type: String, body: Vec<u8>, cache_control: &'static str) -> Self {
        Self {
            status: 200,
            content_type,
            body,
            cache_control,
            headers: Vec::new(),
        }
    }

    fn ranged_bytes(
        status: u16,
        content_type: String,
        body: Vec<u8>,
        content_range: Option<String>,
        accept_ranges: Option<String>,
    ) -> Self {
        let mut headers = vec![(
            "Accept-Ranges".to_string(),
            accept_ranges.unwrap_or_else(|| "bytes".to_string()),
        )];
        if let Some(content_range) = content_range {
            headers.push(("Content-Range".to_string(), content_range));
        }
        Self {
            status,
            content_type,
            body,
            cache_control: NO_STORE,
            headers,
        }
    }
}

pub fn handle(request: &ApiRequest) -> ApiResponse {
    if let Some(response) = static_asset(&request.path) {
        return response;
    }
    let Some(api_path) = request.path.strip_prefix("/api/") else {
        // Unknown non-API paths fall back to the shell so client-side routing
        // survives a reload.
        return index_html();
    };

    let Some(services) = services::init() else {
        return ApiResponse::error(
            503,
            services::init_error().unwrap_or("the library database is unavailable"),
        );
    };
    route(&services, api_path, request)
}

fn route(services: &Arc<Services>, path: &str, request: &ApiRequest) -> ApiResponse {
    let segments = path.split('/').collect::<Vec<_>>();
    route_core(services, &segments, request)
        .or_else(|| route_profiles_and_auth(services, &segments, request))
        .or_else(|| route_seerr(services, &segments, request))
        .or_else(|| route_catalog(services, &segments, request))
        .or_else(|| route_playback(services, &segments, request))
        .unwrap_or_else(|| ApiResponse::error(404, format!("unknown endpoint /api/{path}")))
}

fn route_core(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["status"] => status(services),
        ["companion", "info"] if request.is("GET") => companion_info(services, false),
        ["companion", "probe"] if request.is("POST") => companion_info(services, true),
        ["calendar"] if request.is("GET") => calendar(services, request),
        ["settings"] if request.is("GET") => settings_snapshot(services),
        ["settings", "client", "player"] if request.is("PATCH") => {
            patch_player_settings(services, request)
        }
        ["settings", "client", "playback"] if request.is("PATCH") => {
            patch_playback_settings(services, request)
        }
        ["settings", "client", "application"] if request.is("PATCH") => {
            patch_application_settings(services, request)
        }
        ["settings", "appearance"] if request.is("PATCH") => {
            patch_appearance_settings(services, request)
        }
        ["integrations", "ratings"] if request.is("GET") => ratings_status(services),
        ["integrations", "ratings", "credential", provider] if request.is("PUT") => {
            ratings_save_credential(services, &percent_decode(provider), request)
        }
        ["integrations", "ratings", "credential", provider] if request.is("DELETE") => {
            ratings_remove_credential(services, &percent_decode(provider))
        }
        [
            "integrations",
            "ratings",
            "credential",
            provider,
            "validate",
        ] if request.is("POST") => ratings_validate_credential(services, &percent_decode(provider)),
        ["integrations", "ratings", "credential", provider, "reveal"] if request.is("POST") => {
            ratings_reveal_credential(services, &percent_decode(provider))
        }
        ["ratings", "batch"] if request.is("POST") => ratings_batch(services, request),
        ["technical", "batch"] if request.is("POST") => technical_batch(services, request),
        ["shell", "file-picker"] if request.is("POST") => shell_file_picker(services, request),
        ["shell", "mpv", "install"] if request.is("POST") => shell_install_mpv(services, request),
        ["shell", "mpv", "help"] if request.is("POST") => shell_mpv_help(),
        _ => return None,
    };
    Some(response)
}

fn route_profiles_and_auth(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["integrations", "letterboxd"] if request.is("GET") => letterboxd_profiles(services),
        ["integrations", "letterboxd"] if request.is("POST") => {
            letterboxd_add_profile(services, request)
        }
        ["integrations", "letterboxd", id] if request.is("PATCH") => {
            letterboxd_set_enabled(services, &percent_decode(id), request)
        }
        ["integrations", "letterboxd", id] if request.is("DELETE") => {
            letterboxd_remove_profile(services, &percent_decode(id))
        }
        ["integrations", "letterboxd", id, "refresh"] if request.is("POST") => {
            letterboxd_refresh_profile(services, &percent_decode(id))
        }
        ["integrations", "letterboxd", id, "open"] if request.is("POST") => {
            letterboxd_open_profile(services, &percent_decode(id))
        }
        ["letterboxd", "movie", tmdb_id] if request.is("GET") => {
            movie_letterboxd(services, &percent_decode(tmdb_id))
        }
        ["auth", "connect"] if request.is("POST") => auth_connect(services, request),
        ["auth", "login"] if request.is("POST") => auth_login(services, request),
        ["auth", "quickconnect", "start"] if request.is("POST") => {
            quick_connect_start(services, request)
        }
        ["auth", "quickconnect", "poll"] if request.is("POST") => {
            quick_connect_poll(services, request)
        }
        ["auth", "logout"] if request.is("POST") => auth_logout(services, request),
        _ => return None,
    };
    Some(response)
}

fn route_seerr(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["seerr", "status"] if request.is("GET") => match services.requests_provider().status() {
            Ok(value) => ApiResponse::ok(value),
            Err(error) => ApiResponse::from_provider_error(&error),
        },
        ["seerr", "connect"] if request.is("POST") => seerr_connect(services, request),
        ["seerr", "link"] if request.is("POST") => seerr_link(services),
        ["seerr", "link", "poll"] if request.is("POST") => seerr_link_poll(services, request),
        ["seerr", "link", "password"] if request.is("POST") => {
            seerr_link_password(services, request)
        }
        ["seerr", "unlink"] if request.is("POST") => ApiResponse::ok(services.seerr.unlink()),
        ["seerr", "search"] if request.is("GET") => seerr_search(services, request),
        ["seerr", "person", tmdb_id, "credits"] if request.is("GET") => {
            seerr_person_credits(services, &percent_decode(tmdb_id), request)
        }
        ["seerr", "discover", kind] if request.is("GET") => {
            seerr_discover(services, &percent_decode(kind), request)
        }
        ["seerr", "genres", media_type] if request.is("GET") => {
            seerr_genres(services, &percent_decode(media_type))
        }
        ["seerr", "media", media_type, tmdb_id] if request.is("GET") => seerr_media(
            services,
            &percent_decode(media_type),
            &percent_decode(tmdb_id),
        ),
        ["seerr", "request-options", media_type] if request.is("GET") => {
            seerr_request_options(services, &percent_decode(media_type), request)
        }
        ["seerr", "request"] if request.is("POST") => seerr_request(services, request),
        ["seerr", "requests"] if request.is("GET") => seerr_requests(services, request),
        ["seerr", "request", id] if request.is("DELETE") => {
            seerr_cancel_request(services, &percent_decode(id))
        }
        ["seerr", "image", size, file] if request.is("GET") => {
            seerr_image(&percent_decode(size), &percent_decode(file))
        }
        _ => return None,
    };
    Some(response)
}

fn route_catalog(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["home", "resume"] if request.is("GET") => home_resume(services),
        ["home"] if request.is("GET") => home(services),
        ["billboard"] if request.is("GET") => billboard(services),
        ["items"] if request.is("GET") => query_items(services, request),
        ["genres"] if request.is("GET") => match services.library.genres() {
            Ok(genres) => ApiResponse::ok(json!({ "genres": genres })),
            Err(error) => storage_failure(&error),
        },
        ["person", "resolve"] if request.is("GET") => resolve_person(services, request),
        ["item", id] if request.is("GET") => item_detail(services, &percent_decode(id)),
        ["item", id, "synopsis"] if request.is("GET") => {
            item_synopsis(services, &percent_decode(id))
        }
        ["item", id, "about"] if request.is("GET") => item_about(services, &percent_decode(id)),
        ["item", id, "letterboxd"] if request.is("GET") => {
            item_letterboxd(services, &percent_decode(id))
        }
        ["item", id, "children"] if request.is("GET") => children(services, &percent_decode(id)),
        ["item", id, "media"] if request.is("GET") => media_info(services, &percent_decode(id)),
        ["item", id, "playback-preference"] if request.is("PATCH") => {
            set_item_playback_preference(services, &percent_decode(id), request)
        }
        ["item", id, "trailer"] if request.is("GET") => trailer_info(services, &percent_decode(id)),
        ["item", id, "nextup"] if request.is("GET") => next_up(services, &percent_decode(id)),
        ["item", id, "external"] if request.is("POST") => {
            open_external(services, &percent_decode(id), request)
        }
        ["item", id, "played"] if request.is("POST") => {
            set_played(services, &percent_decode(id), request)
        }
        ["item", id, "favorite"] if request.is("POST") => {
            set_favorite(services, &percent_decode(id), request)
        }
        ["image", id, image_type] if request.is("GET") => image(
            services,
            &percent_decode(id),
            &percent_decode(image_type),
            request,
        ),
        ["trailer", id, "stream"] if request.is("GET") => {
            trailer_stream(services, &percent_decode(id), request)
        }
        _ => return None,
    };
    Some(response)
}

fn route_playback(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["play"] if request.is("POST") => play_item(services, request),
        ["play", "next"] if request.is("POST") => play_next(services, request),
        ["player", "state"] if request.is("GET") => player_state(services),
        ["player", "command"] if request.is("POST") => player_command(services, request),
        ["sync"] if request.is("POST") => {
            services.sync.request();
            ApiResponse::ok(json!({ "requested": true }))
        }
        _ => return None,
    };
    Some(response)
}

// ------------------------------------------------------------------- session

fn status(services: &Arc<Services>) -> ApiResponse {
    let mut status = services.session.status();
    let stats = services.library.stats();
    let bootstrap = sync::bootstrap_progress(&services.library);
    if let Some(object) = status.as_object_mut() {
        object.insert("library".to_string(), json!(stats));
        object.insert("syncing".to_string(), json!(services.sync.is_running()));
        object.insert(
            "lastSync".to_string(),
            json!(services.library.meta("sync.completed_at")),
        );
        object.insert("bootstrapped".to_string(), json!(bootstrap.complete));
        object.insert("libraryReady".to_string(), json!(bootstrap.ready));
        object.insert("bootstrap".to_string(), json!(bootstrap));
        object.insert(
            "syncProgress".to_string(),
            json!(services.sync.progress(&services.library)),
        );
        object.insert("companion".to_string(), services.companion.status());
    }
    ApiResponse::ok(status)
}

fn settings_snapshot(services: &Arc<Services>) -> ApiResponse {
    settings_response(&services.preferences.snapshot())
}

fn settings_response(settings: &AppSettings) -> ApiResponse {
    let bindings = MpvInputBindings::load();
    ApiResponse::ok(json!({
        "client": {
            "player": {
                "playerBackend": settings.effective_backend().as_str(),
                "mpvPath": settings.mpv_path,
                "mpchcPath": settings.mpchc_path,
                "defaultFullscreen": settings.default_fullscreen.as_str(),
                "markWatchedNext": bindings.mark_watched_next,
                "playerConfigured": settings.player_path().is_some(),
            },
            "playback": {
                "streamingQuality": settings.streaming_quality.as_str(),
                "skipIntro": settings.skip_intro.as_str(),
                "skipCredits": settings.skip_credits.as_str(),
                "skipRecap": settings.skip_recap.as_str(),
                "skipCommercial": settings.skip_commercial.as_str(),
            },
            "application": {
                "closeBehavior": settings.close_behavior.as_str(),
                "showScrollbars": settings.show_scrollbars,
                "logLevel": settings.log_level,
            },
        },
        "appearance": {
            "theme": settings.appearance.theme.as_str(),
            "accent": settings.appearance.accent.as_str(),
            "density": settings.appearance.density.as_str(),
            "artworkIntensity": settings.appearance.artwork_intensity,
            "backdropIntensity": settings.appearance.backdrop_intensity,
            "reducedMotion": settings.appearance.reduced_motion,
            "cardPreviews": settings.appearance.card_previews,
            "showMediaInfo": settings.appearance.show_media_info,
            "ratingSources": settings.appearance.rating_sources,
        },
        "capabilities": {
            "platform": player_setup::platform_id(),
            "mpchc": cfg!(target_os = "windows"),
            "mpvInstaller": player_setup::supported(),
        },
        // Retained for small existing consumers while they move to the
        // sectioned shape above.
        "streamingQuality": settings.streaming_quality.as_str(),
        "playerBackend": settings.effective_backend().as_str(),
        "playerConfigured": settings.player_path().is_some(),
        "serverUrl": settings.jellyfin_url,
    }))
}

fn patch_player_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match serde_json::from_value::<PlayerSettingsPatch>(request.json()) {
        Ok(patch) => patch,
        Err(error) => return ApiResponse::error(400, format!("invalid player settings: {error}")),
    };
    match services.preferences.patch_player(patch) {
        Ok(change) => settings_response(&change.settings),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn patch_playback_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match serde_json::from_value::<PlaybackSettingsPatch>(request.json()) {
        Ok(patch) => patch,
        Err(error) => {
            return ApiResponse::error(400, format!("invalid playback settings: {error}"));
        }
    };
    match services.preferences.patch_playback(patch) {
        Ok(change) => settings_response(&change.settings),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn patch_application_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match serde_json::from_value::<ApplicationSettingsPatch>(request.json()) {
        Ok(patch) => patch,
        Err(error) => {
            return ApiResponse::error(400, format!("invalid application settings: {error}"));
        }
    };
    match services.preferences.patch_application(patch) {
        Ok(change) => settings_response(&change.settings),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn patch_appearance_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match serde_json::from_value::<AppearanceSettingsPatch>(request.json()) {
        Ok(patch) => patch,
        Err(error) => {
            return ApiResponse::error(400, format!("invalid appearance settings: {error}"));
        }
    };
    match services.preferences.patch_appearance(patch) {
        Ok(change) => settings_response(&change.settings),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn ratings_sources(services: &Arc<Services>) -> Vec<String> {
    services.preferences.snapshot().appearance.rating_sources
}

fn ratings_status(services: &Arc<Services>) -> ApiResponse {
    ApiResponse::ok(services.ratings.status(&ratings_sources(services)))
}

fn ratings_save_credential(
    services: &Arc<Services>,
    provider: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let Some(key) = request
        .json()
        .get("key")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return ApiResponse::error(400, "an API key is required");
    };
    match services
        .ratings
        .save_credential(provider, &key, &ratings_sources(services))
    {
        Ok(status) => ApiResponse::ok(status),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn ratings_remove_credential(services: &Arc<Services>, provider: &str) -> ApiResponse {
    match services
        .ratings
        .remove_credential(provider, &ratings_sources(services))
    {
        Ok(status) => ApiResponse::ok(status),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn ratings_validate_credential(services: &Arc<Services>, provider: &str) -> ApiResponse {
    match services
        .ratings
        .validate_credential(provider, &ratings_sources(services))
    {
        Ok(status) => ApiResponse::ok(status),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn ratings_reveal_credential(services: &Arc<Services>, provider: &str) -> ApiResponse {
    match services.ratings.reveal_credential(provider) {
        Ok(key) => ApiResponse::ok(json!({ "key": key })),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn ratings_batch(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let ids = request
        .json()
        .get("ids")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match services.ratings.batch(&ids) {
        Ok(ratings) => ApiResponse::ok(ratings),
        Err(error) => ApiResponse::error(500, error.to_string()),
    }
}

/// Jellyfin answers `/Items?ids=` comfortably at this width; it is the same
/// shape the old background enrichment fetcher used.
const TECHNICAL_BATCH_SIZE: usize = 40;

/// Live technical stream descriptors for visible cards, batched by the UI's
/// badge scheduler. Container ids (Series, Season) answer with the streams of
/// a representative episode. Nothing is persisted; a failure is silent on
/// cards.
fn technical_batch(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let mut seen = HashSet::new();
    let ids = request
        .json()
        .get("ids")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .filter(|id| seen.insert(id.to_string()))
                .take(100)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return ApiResponse::ok(json!({ "items": [] }));
    }
    // Series and Season cards advertise the same badges as movies, but their
    // rows are containers Jellyfin reports without streams. Each is answered
    // by a representative cached episode; a container with no cached episode
    // is dropped rather than queried uselessly.
    let sources = match services.library.technical_stream_sources(&ids) {
        Ok(sources) => sources,
        Err(error) => return storage_failure(&error),
    };
    let mut cards_by_source: HashMap<String, Vec<String>> = HashMap::new();
    let mut source_ids = Vec::new();
    for (card_id, source_id) in sources {
        let cards = cards_by_source.entry(source_id.clone()).or_default();
        if cards.is_empty() {
            source_ids.push(source_id);
        }
        cards.push(card_id);
    }
    if source_ids.is_empty() {
        return ApiResponse::ok(json!({ "items": [] }));
    }
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let mut results = Vec::new();
    for chunk in source_ids.chunks(TECHNICAL_BATCH_SIZE) {
        // The UI aborts a batch once none of its cards remains mounted; the
        // fetch dies in the browser, but this handler is already blocked in
        // upstream calls. Checking between chunks keeps rapid scrolling from
        // running every remaining request for an answer nobody will read.
        if request.is_cancelled() {
            return ApiResponse::error(499, "the browser abandoned the request");
        }
        match items::fetch_media_stream_batch(&client, &user_id, chunk) {
            Ok(response) => {
                for dto in &response.items {
                    let streams = technical_media_streams_json(&dto.media_streams);
                    for card_id in cards_by_source.get(&dto.id).into_iter().flatten() {
                        results.push(json!({
                            "id": card_id,
                            "mediaStreams": streams.clone(),
                        }));
                    }
                }
            }
            Err(error) => {
                services.session.note_error(&error);
                return ApiResponse::from_api_error(&error);
            }
        }
    }
    ApiResponse::ok(json!({ "items": results }))
}

fn shell_file_picker(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let request_id = match shell_request_id(body.get("requestId").and_then(Value::as_str)) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let target = match body.get("target").and_then(Value::as_str) {
        Some("mpv") => ShellFilePickerTarget::Mpv,
        Some("mpchc") => ShellFilePickerTarget::Mpchc,
        _ => return ApiResponse::error(400, "target must be mpv or mpchc"),
    };
    match services.shell.request(ShellRequest::FilePicker {
        request_id: request_id.clone(),
        target,
    }) {
        Ok(()) => ApiResponse::ok(json!({ "requestId": request_id, "queued": true })),
        Err(error) => ApiResponse::error(503, error),
    }
}

fn shell_install_mpv(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    if !player_setup::supported() {
        return ApiResponse::error(
            409,
            "automatic mpv installation is not available on this platform",
        );
    }
    let body = request.json();
    let request_id = match shell_request_id(body.get("requestId").and_then(Value::as_str)) {
        Ok(id) => id,
        Err(response) => return response,
    };
    match services.shell.request(ShellRequest::InstallMpv {
        request_id: request_id.clone(),
    }) {
        Ok(()) => ApiResponse::ok(json!({ "requestId": request_id, "queued": true })),
        Err(error) => ApiResponse::error(503, error),
    }
}

fn shell_mpv_help() -> ApiResponse {
    super::open_external_link(player_setup::MPV_HELP_URL);
    ApiResponse::ok(json!({ "opened": true }))
}

fn shell_request_id(value: Option<&str>) -> Result<String, ApiResponse> {
    let value = value.unwrap_or_default().trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ApiResponse::error(
            400,
            "requestId must be a short URL-safe identifier",
        ));
    }
    Ok(value.to_string())
}

fn letterboxd_scope(services: &Arc<Services>) -> Result<(String, String), ApiResponse> {
    if !services.session.is_authenticated() {
        return Err(ApiResponse::error(
            401,
            "sign in to manage connected profiles",
        ));
    }
    let credentials = services.library.credentials();
    match (credentials.server_id, credentials.user_id) {
        (Some(server_id), Some(user_id)) => Ok((server_id, user_id)),
        _ => Err(ApiResponse::error(
            401,
            "sign in to manage connected profiles",
        )),
    }
}

fn valid_profile_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn letterboxd_profiles(services: &Arc<Services>) -> ApiResponse {
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    match services
        .library
        .external_profiles("letterboxd", &server_id, &user_id)
    {
        Ok(profiles) => ApiResponse::ok(json!({ "profiles": profiles })),
        Err(error) => storage_failure(&error),
    }
}

fn letterboxd_add_profile(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let profile =
        match letterboxd::normalize_profile(request.json()["profile"].as_str().unwrap_or_default())
        {
            Ok(profile) => profile,
            Err(error) => return ApiResponse::error(400, error),
        };
    let existing = match services
        .library
        .external_profiles("letterboxd", &server_id, &user_id)
    {
        Ok(existing) => existing,
        Err(error) => return storage_failure(&error),
    };
    if existing.len() >= letterboxd::MAX_CONNECTED_PROFILES
        && !existing
            .iter()
            .any(|saved| saved.profile_key == profile.username)
    {
        return ApiResponse::error(
            409,
            format!(
                "up to {} Letterboxd profiles can be connected",
                letterboxd::MAX_CONNECTED_PROFILES
            ),
        );
    }
    let verification = letterboxd::verify(&profile);
    let display_name = verification
        .display_name()
        .unwrap_or(&profile.username)
        .to_string();
    let now = unix_now();
    let record = ExternalProfile {
        id: crate::app::ids::random_hex(16),
        provider: "letterboxd".to_string(),
        profile_key: profile.username.clone(),
        display_name,
        canonical_url: profile.canonical_url,
        enabled: true,
        verification_status: verification.as_str().to_string(),
        created_at: now,
        last_checked_at: Some(now),
        jellyfin_server_id: server_id,
        jellyfin_user_id: user_id,
    };
    match services.library.save_external_profile(&record) {
        Ok(profile) => ApiResponse::ok(json!({ "profile": profile })),
        Err(error) => storage_failure(&error),
    }
}

fn letterboxd_set_enabled(services: &Arc<Services>, id: &str, request: &ApiRequest) -> ApiResponse {
    if !valid_profile_id(id) {
        return ApiResponse::error(404, "profile not found");
    }
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let Some(enabled) = request.json()["enabled"].as_bool() else {
        return ApiResponse::error(400, "enabled must be true or false");
    };
    match services
        .library
        .set_external_profile_enabled(id, &server_id, &user_id, enabled)
    {
        Ok(Some(profile)) => ApiResponse::ok(json!({ "profile": profile })),
        Ok(None) => ApiResponse::error(404, "profile not found"),
        Err(error) => storage_failure(&error),
    }
}

fn letterboxd_remove_profile(services: &Arc<Services>, id: &str) -> ApiResponse {
    if !valid_profile_id(id) {
        return ApiResponse::error(404, "profile not found");
    }
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    match services
        .library
        .remove_external_profile(id, &server_id, &user_id)
    {
        Ok(true) => ApiResponse::ok(json!({ "removed": true })),
        Ok(false) => ApiResponse::error(404, "profile not found"),
        Err(error) => storage_failure(&error),
    }
}

fn letterboxd_refresh_profile(services: &Arc<Services>, id: &str) -> ApiResponse {
    if !valid_profile_id(id) {
        return ApiResponse::error(404, "profile not found");
    }
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let existing = match services.library.external_profile(id, &server_id, &user_id) {
        Ok(Some(profile)) if profile.provider == "letterboxd" => profile,
        Ok(_) => return ApiResponse::error(404, "profile not found"),
        Err(error) => return storage_failure(&error),
    };
    let source = match letterboxd::normalize_profile(&existing.profile_key) {
        Ok(profile) => profile,
        Err(_) => return ApiResponse::error(409, "stored Letterboxd profile is invalid"),
    };
    let verification = letterboxd::verify(&source);
    let display_name = verification
        .display_name()
        .map(str::to_string)
        .unwrap_or_else(|| existing.display_name.clone());
    let record = ExternalProfile {
        canonical_url: source.canonical_url,
        display_name,
        verification_status: verification.as_str().to_string(),
        last_checked_at: Some(unix_now()),
        ..existing
    };
    match services.library.save_external_profile(&record) {
        Ok(profile) => ApiResponse::ok(json!({ "profile": profile })),
        Err(error) => storage_failure(&error),
    }
}

fn letterboxd_open_profile(services: &Arc<Services>, id: &str) -> ApiResponse {
    if !valid_profile_id(id) {
        return ApiResponse::error(404, "profile not found");
    }
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    match services.library.external_profile(id, &server_id, &user_id) {
        Ok(Some(profile)) if profile.provider == "letterboxd" => {
            super::open_external_link(&profile.canonical_url);
            ApiResponse::ok(json!({ "opened": true, "url": profile.canonical_url }))
        }
        Ok(_) => ApiResponse::error(404, "profile not found"),
        Err(error) => storage_failure(&error),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn auth_connect(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let server = body["server"].as_str().unwrap_or_default();
    match services.session.connect(server) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn auth_login(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let result = services.session.login(
        body["server"].as_str().unwrap_or_default(),
        body["username"].as_str().unwrap_or_default(),
        body["password"].as_str().unwrap_or_default(),
    );
    match result {
        Ok(_) => {
            // Signing in as somebody else must not inherit their Seerr link.
            services.seerr.revalidate();
            services.companion.clear();
            if let Err(error) = services.companion.probe(true) {
                services.session.note_error(&error);
                tracing::debug!(target: "companion", "post-login probe failed: {error}");
            }
            services.sync.request();
            status(services)
        }
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn quick_connect_start(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    match services
        .session
        .quick_connect_start(body["server"].as_str().unwrap_or_default())
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn quick_connect_poll(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let result = services.session.quick_connect_poll(
        body["server"].as_str().unwrap_or_default(),
        body["secret"].as_str().unwrap_or_default(),
    );
    match result {
        Ok(value) => {
            if value["authenticated"] == json!(true) {
                services.seerr.revalidate();
                services.companion.clear();
                if let Err(error) = services.companion.probe(true) {
                    services.session.note_error(&error);
                    tracing::debug!(target: "companion", "post-login probe failed: {error}");
                }
                services.sync.request();
            }
            ApiResponse::ok(value)
        }
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn auth_logout(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let forget = request.json()["forgetLibrary"].as_bool().unwrap_or(false);
    services.session.logout(forget);
    services.companion.clear();
    // The Seerr link belongs to the account that just went away. Every read
    // path re-checks that anyway, but doing it here means a signed-out machine
    // keeps no Seerr cookie on disk.
    services.seerr.revalidate();
    status(services)
}

fn companion_info(services: &Arc<Services>, force: bool) -> ApiResponse {
    match services.companion.probe(force) {
        Ok(_) => ApiResponse::ok(services.companion.status()),
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn calendar(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let Some(start) = request.param("start") else {
        return ApiResponse::error(400, "calendar start is required");
    };
    let Some(end) = request.param("end") else {
        return ApiResponse::error(400, "calendar end is required");
    };
    if !is_iso_date(&start) || !is_iso_date(&end) || end < start {
        return ApiResponse::error(
            400,
            "calendar dates must be YYYY-MM-DD with end after start",
        );
    }
    match services.companion.calendar(&start, &end) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn is_iso_date(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

// ---------------------------------------------------------------------- seerr

fn seerr_connect(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    match services
        .seerr
        .connect(body["server"].as_str().unwrap_or_default())
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_seerr_error(&error),
    }
}

/// Starts the password-less link. Answers `{"method":"password"}` — not an
/// error — whenever Quick Connect is unavailable on either side, since every
/// Seerr release supports the password path this then falls back to.
fn seerr_link(services: &Arc<Services>) -> ApiResponse {
    match services.seerr.link_start() {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_seerr_error(&error),
    }
}

fn seerr_link_poll(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    match services
        .seerr
        .link_poll(body["secret"].as_str().unwrap_or_default())
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_seerr_error(&error),
    }
}

fn seerr_link_password(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let result = services.seerr.link_with_password(
        body["username"].as_str().unwrap_or_default(),
        body["password"].as_str().unwrap_or_default(),
    );
    match result {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_seerr_error(&error),
    }
}

fn seerr_search(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let query = request.param("q").unwrap_or_default();
    match services
        .requests_provider()
        .search(&query, page_param(request))
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_person_credits(
    services: &Arc<Services>,
    tmdb_id: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let Ok(tmdb_id) = tmdb_id.parse::<i64>() else {
        return ApiResponse::error(400, "that is not a TMDB person id");
    };
    if tmdb_id <= 0 {
        return ApiResponse::error(400, "that is not a TMDB person id");
    }

    let mut value = match services.requests_provider().person_credits(tmdb_id) {
        Ok(value) => value,
        Err(error) => return ApiResponse::from_provider_error(&error),
    };
    // During progressive catalog fill, SQLite cannot yet prove that every
    // Seerr credit is non-local. An exact Jellyfin identity lets this secondary
    // section verify ownership against the live complete person relation. A
    // failure hides Discover only; the independently loaded server grid stays.
    if let Some(person_id) = request.param("personId")
        && let Err(error) = join_server_person_availability(services, &person_id, &mut value)
    {
        services.session.note_error(&error);
        return ApiResponse::from_api_error(&error);
    }
    ApiResponse::ok(value)
}

fn join_server_person_availability(
    services: &Arc<Services>,
    person_id: &str,
    value: &mut Value,
) -> Result<(), ApiError> {
    let (client, user_id) = services.session.client_and_user()?;
    // The provider first joined against SQLite, which can contain both unseen
    // progressive rows and stale rows awaiting deletion reconciliation. The
    // live exact-person pass is authoritative, so rebuild availability rather
    // than only adding to that provisional answer.
    clear_person_availability(value);
    let mut offset = 0;
    for _ in 0..MAX_PERSON_QUERY_PAGES {
        let page = items::fetch_person_items(
            &client,
            &user_id,
            person_id,
            offset,
            items::PERSON_PAGE_SIZE,
        )?;
        let received = i64::try_from(page.items.len()).unwrap_or(i64::MAX);
        if received == 0 {
            if page.total_record_count > offset {
                return Err(ApiError::Decode(
                    "the server omitted part of an exact person filmography".to_string(),
                ));
            }
            return Ok(());
        }
        join_person_items(value, &page.items);
        offset = offset.saturating_add(received);
        if page.total_record_count > 0 && offset >= page.total_record_count {
            return Ok(());
        }
        if page.total_record_count <= 0 && received < items::PERSON_PAGE_SIZE {
            return Ok(());
        }
    }
    Err(ApiError::Decode(
        "the exact person filmography exceeded the safe paging limit".to_string(),
    ))
}

fn clear_person_availability(value: &mut Value) {
    if let Some(results) = value["results"].as_array_mut() {
        for result in results {
            result["libraryItemId"] = Value::Null;
        }
    }
}

fn join_person_items(value: &mut Value, server_items: &[BaseItemDto]) {
    let local = server_items
        .iter()
        .filter_map(|item| {
            let media_type = match item.item_type.as_deref() {
                Some("Movie") => "movie",
                Some("Series") => "tv",
                _ => return None,
            };
            let tmdb_id = item.provider_id("Tmdb")?.parse::<i64>().ok()?;
            (tmdb_id > 0).then(|| ((media_type.to_string(), tmdb_id), item.id.clone()))
        })
        .collect::<HashMap<_, _>>();
    let Some(results) = value["results"].as_array_mut() else {
        return;
    };
    for result in results {
        let Some(media_type) = result["mediaType"].as_str() else {
            continue;
        };
        let Some(tmdb_id) = result["tmdbId"].as_i64() else {
            continue;
        };
        if let Some(item_id) = local.get(&(media_type.to_string(), tmdb_id)) {
            result["libraryItemId"] = Value::String(item_id.clone());
        }
    }
}

fn seerr_discover(services: &Arc<Services>, kind: &str, request: &ApiRequest) -> ApiResponse {
    let Some(kind) = DiscoverKind::from_id(kind) else {
        return ApiResponse::error(404, "unknown discover row");
    };
    let options = match DiscoverOptions::from_values(
        request.param("genre").as_deref(),
        request.param("sort").as_deref(),
        request.param("minRating").as_deref(),
        request.param("decade").as_deref(),
        request.param("mediaType").as_deref(),
        request.param("timeWindow").as_deref(),
    ) {
        Ok(options) => options,
        Err(error) => return ApiResponse::error(400, &error),
    };
    match services
        .requests_provider()
        .discover(kind, page_param(request), &options)
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_genres(services: &Arc<Services>, media_type: &str) -> ApiResponse {
    if !matches!(media_type, "movie" | "tv") {
        return ApiResponse::error(404, "unknown genre kind");
    }
    match services.requests_provider().genres(media_type) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_media(services: &Arc<Services>, media_type: &str, tmdb_id: &str) -> ApiResponse {
    let Ok(tmdb_id) = tmdb_id.parse::<i64>() else {
        return ApiResponse::error(400, "that is not a TMDB id");
    };
    match services.requests_provider().media(media_type, tmdb_id) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_request_options(
    services: &Arc<Services>,
    media_type: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let is_4k = request
        .param("is4k")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
    match services
        .requests_provider()
        .request_options(media_type, is_4k)
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_request(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    // An explicit list means "these seasons"; its absence means "the whole
    // show", which Seerr expands to whatever it does not already have.
    let seasons = body["seasons"].as_array().map(|seasons| {
        seasons
            .iter()
            .filter_map(serde_json::Value::as_i64)
            .collect::<Vec<_>>()
    });
    let server_id = body["serverId"].as_i64();
    let profile_id = body["profileId"].as_i64();
    let profile = match (server_id, profile_id) {
        (None, None) => None,
        (Some(server_id), Some(profile_id)) if server_id >= 0 && profile_id > 0 => {
            Some(RequestProfileSelection {
                server_id,
                profile_id,
            })
        }
        _ => {
            return ApiResponse::error(
                400,
                "the download destination and quality profile must be selected together",
            );
        }
    };
    let result = services.requests_provider().create(
        body["mediaType"].as_str().unwrap_or_default(),
        body["tmdbId"].as_i64().unwrap_or_default(),
        seasons,
        body["is4k"].as_bool().unwrap_or(false),
        profile,
    );
    match result {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_requests(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let number = |key: &str, fallback: i64| {
        request
            .param(key)
            .and_then(|value| value.parse().ok())
            .unwrap_or(fallback)
    };
    let result = services.requests_provider().requests(
        number("take", 20),
        number("skip", 0),
        &request.param("filter").unwrap_or_else(|| "all".to_string()),
    );
    match result {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_cancel_request(services: &Arc<Services>, request_id: &str) -> ApiResponse {
    let Ok(request_id) = request_id.parse::<i64>() else {
        return ApiResponse::error(400, "that is not a request id");
    };
    match services.requests_provider().cancel(request_id) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

/// The TMDB poster proxy, in the same posture as [`open_external`]: the UI
/// names a rendition size and an image file, never an address, and
/// [`tmdb_image_url`] is what decides whether those two compose into a request
/// at all.
///
/// Art for titles the library does not have is exactly the browsing that pulls
/// the most bytes, so it shares the pruned on-disk cache the Jellyfin image
/// proxy already has, and is served as immutable — a TMDB file name addresses
/// one unchanging image.
fn seerr_image(size: &str, file: &str) -> ApiResponse {
    let Some(url) = tmdb_image_url(size, file) else {
        return ApiResponse::error(404, "no such poster");
    };
    let key = cache_key("tmdb", size, file, 0);
    let cache_path = crate::app::paths::image_cache_dir().join(&key);
    if let Ok(bytes) = std::fs::read(&cache_path)
        && !bytes.is_empty()
    {
        return ApiResponse::bytes(mime_for_image(&bytes), bytes, IMMUTABLE_CACHE);
    }
    match fetch_tmdb_image(&url) {
        Ok((bytes, content_type)) => {
            store_image(&cache_path, &bytes);
            ApiResponse::bytes(content_type, bytes, IMMUTABLE_CACHE)
        }
        Err(error) => {
            tracing::debug!(target: "app.api", "could not fetch poster art: {error}");
            ApiResponse::from_seerr_error(&error)
        }
    }
}

fn page_param(request: &ApiRequest) -> i64 {
    request
        .param("page")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

// ------------------------------------------------------------------ browsing

fn home(services: &Arc<Services>) -> ApiResponse {
    let library = &services.library;
    let resume = library
        .continue_watching(HOME_ROW_LIMIT)
        .unwrap_or_default();
    let recent = library.recently_added(HOME_ROW_LIMIT).unwrap_or_default();
    let latest_movies = latest_home_items(library, "Movie");
    let latest_shows = latest_home_items(library, "Series");

    // This response is the startup snapshot. Keep it entirely SQLite-backed so
    // a healthy durable cache can paint while the loading screen is still up;
    // live Next Up enrichment arrives independently from `home_resume`.
    ApiResponse::ok(json!({
        "rows": [
            { "id": "resume", "title": "Continue Watching", "items": resume },
            { "id": "recent", "title": "Recently Added", "items": recent },
            { "id": "latest-movies", "title": "Latest Movies", "items": latest_movies },
            { "id": "latest-shows", "title": "Latest Series", "items": latest_shows },
        ],
    }))
}

/// Enriches the cached Continue Watching shelf with Jellyfin's server-owned
/// Next Up decisions without holding the rest of the home page behind a
/// network request.
fn home_resume(services: &Arc<Services>) -> ApiResponse {
    let resume = services
        .library
        .continue_watching(HOME_ROW_LIMIT)
        .unwrap_or_default();
    // Next Up is server-side logic; replicating it locally would drift.
    let next_up = services
        .session
        .client_and_user()
        .and_then(|(client, user_id)| items::fetch_next_up(&client, &user_id, None, HOME_ROW_LIMIT))
        .map(|response| {
            response
                .items
                .iter()
                .map(summary_from_dto)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|error| {
            services.session.note_error(&error);
            tracing::debug!(target: "app.api", "Next Up unavailable: {error}");
            Vec::new()
        });

    ApiResponse::ok(json!({
        "items": merge_next_up(resume, next_up),
    }))
}

/// New releases are separate from Recently Added: importing an older title
/// moves it to the front of the latter, but not to the front of these shelves.
fn latest_home_items(library: &Library, kind: &str) -> Vec<Value> {
    library
        .query(&ItemQuery {
            kinds: vec![kind.to_string()],
            sort: ItemSort::Year,
            limit: HOME_ROW_LIMIT,
            ..Default::default()
        })
        .map(|page| page.items)
        .unwrap_or_default()
}

fn billboard(services: &Arc<Services>) -> ApiResponse {
    match services.library.random_billboard_titles(BILLBOARD_LIMIT) {
        Ok(items) => ApiResponse::ok(json!({ "items": items })),
        Err(error) => storage_failure(&error),
    }
}

/// Continue Watching and Next Up share one row: half-watched items first, in the
/// order the cache returned them, then the next unwatched episode of everything
/// else.
///
/// A show can legitimately show up in both lists — Jellyfin counts an
/// in-progress episode as that series' Next Up — so entries are keyed by series
/// as well as by id. Without the series key a partly watched episode and its own
/// Next Up successor would sit next to each other as two cards for one show.
///
/// Both sources are already capped at `HOME_ROW_LIMIT`, and the merged row keeps
/// all of what survives deduplication rather than re-applying that cap to the
/// total: trimming it back to one row's worth would put a library with a handful
/// of half-watched items and many unwatched series out of reach entirely.
fn merge_next_up(resume: Vec<Value>, next_up: Vec<Value>) -> Vec<Value> {
    fn key(item: &Value, field: &str) -> Option<String> {
        item[field].as_str().map(str::to_string)
    }

    let mut seen: HashSet<String> = resume
        .iter()
        .flat_map(|item| [key(item, "id"), key(item, "seriesId")])
        .flatten()
        .collect();

    let mut merged = resume;
    for item in next_up {
        let keys: Vec<String> = [key(&item, "id"), key(&item, "seriesId")]
            .into_iter()
            .flatten()
            .collect();
        if keys.iter().any(|key| seen.contains(key)) {
            continue;
        }
        seen.extend(keys);
        merged.push(item);
    }
    merged
}

fn query_items(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    if let Some(person_id) = request.param("personId") {
        return query_person_items(services, &person_id, request);
    }

    let query = ItemQuery {
        search: request.param("search"),
        kinds: request
            .param("kind")
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|kind| !kind.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        genre: request.param("genre"),
        release_decade: request
            .param("decade")
            .as_deref()
            .and_then(crate::library::release_decade_from_id),
        parent_id: request.param("parentId"),
        series_id: request.param("seriesId"),
        watched: request
            .param("watched")
            .and_then(|value| match value.as_str() {
                "true" | "1" => Some(true),
                "false" | "0" => Some(false),
                _ => None,
            }),
        favorite: request
            .param("favorite")
            .map(|value| value == "true" || value == "1"),
        sort: request
            .param("sort")
            .as_deref()
            .and_then(ItemSort::from_id)
            .unwrap_or_default(),
        offset: request
            .param("offset")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        limit: request
            .param("limit")
            .and_then(|value| value.parse().ok())
            .unwrap_or(60),
    };
    match services.library.query(&query) {
        Ok(page) => ApiResponse::ok(json!({ "items": page.items, "total": page.total })),
        Err(error) => storage_failure(&error),
    }
}

fn query_person_items(
    services: &Arc<Services>,
    person_id: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let offset = request
        .param("offset")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let limit = request
        .param("limit")
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_person_items(&client, &user_id, person_id, offset, limit) {
        Ok(response) => ApiResponse::ok(json!({
            "items": response.items.iter().map(summary_from_dto).collect::<Vec<_>>(),
            "total": response.total_record_count,
        })),
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn person_identity(dto: &BaseItemDto, fallback_tmdb_id: Option<i64>) -> Value {
    json!({
        "jellyfinId": dto.id,
        "tmdbId": dto
            .provider_id("Tmdb")
            .and_then(|id| id.parse::<i64>().ok())
            .filter(|id| *id > 0)
            .or(fallback_tmdb_id),
        "name": dto.display_name(),
        "imageTag": dto.primary_image_tag(),
    })
}

/// Bridges Jellyfin and TMDB person namespaces without ever treating a fuzzy
/// name match as identity. A missing provider id may use one unambiguous exact
/// name; a known conflicting id is always excluded.
fn resolve_person(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let jellyfin_id = request.param("jellyfinId");
    let tmdb_id = request
        .param("tmdbId")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0);
    let name = request.param("name").unwrap_or_default();
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };

    if let Some(jellyfin_id) = jellyfin_id {
        return match items::fetch_item(&client, &user_id, &jellyfin_id) {
            Ok(Some(person)) if person.item_type.as_deref() == Some("Person") => {
                let provider_id = person
                    .provider_id("Tmdb")
                    .and_then(|id| id.parse::<i64>().ok())
                    .filter(|id| *id > 0);
                if tmdb_id.is_some() && provider_id.is_some() && tmdb_id != provider_id {
                    return ApiResponse::error(
                        409,
                        "the Jellyfin and TMDB person ids do not match",
                    );
                }
                ApiResponse::ok(json!({
                    "person": person_identity(&person, tmdb_id),
                    "candidates": [],
                    "ambiguous": false,
                }))
            }
            Ok(Some(_)) => ApiResponse::error(409, "that Jellyfin id is not a person"),
            Ok(None) => ApiResponse::error(404, "the server has no person with that id"),
            Err(error) => {
                services.session.note_error(&error);
                ApiResponse::from_api_error(&error)
            }
        };
    }

    if name.trim().is_empty() {
        return ApiResponse::error(400, "a person name is required to resolve that deep link");
    }
    match items::fetch_people(&client, &user_id, &name) {
        Ok(response) => {
            let mut seen = HashSet::new();
            let exact = response
                .items
                .into_iter()
                // `/Persons` is already type-scoped. Some older servers omit
                // `Type` in this lightweight response, so require its stable id
                // and exact name rather than rejecting a valid candidate.
                .filter(|person| !person.id.trim().is_empty())
                .filter(|person| person.display_name().eq_ignore_ascii_case(name.trim()))
                .filter(|person| seen.insert(person.id.clone()))
                .filter(|person| {
                    tmdb_id.is_none_or(|id| {
                        person
                            .provider_id("Tmdb")
                            .and_then(|value| value.parse::<i64>().ok())
                            .filter(|value| *value > 0)
                            .is_none_or(|provider_id| provider_id == id)
                    })
                })
                .collect::<Vec<_>>();
            if exact.len() == 1 {
                return ApiResponse::ok(json!({
                    "person": person_identity(&exact[0], tmdb_id),
                    "candidates": [],
                    "ambiguous": false,
                }));
            }
            let candidates = exact
                .iter()
                .map(|person| person_identity(person, tmdb_id))
                .collect::<Vec<_>>();
            ApiResponse::ok(json!({
                "person": Value::Null,
                "ambiguous": candidates.len() > 1,
                "candidates": candidates,
            }))
        }
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn item_detail(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    match services.library.item(item_id) {
        // The thin catalog row answers instantly; prose, cast, and critic
        // scores arrive separately through the live `about` endpoint.
        Ok(Some(cached)) => ApiResponse::ok(cached),
        // A deep link can outrun the catalog; fetch that one item and cache it.
        Ok(None) => fetch_and_cache_item(services, item_id),
        Err(error) => storage_failure(&error),
    }
}

fn item_synopsis(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_item_synopsis(&client, &user_id, item_id) {
        Ok(Some(dto)) => ApiResponse::ok(json!({ "overview": dto.overview })),
        Ok(None) => {
            forget_item(services, item_id);
            ApiResponse::error(404, "the server has no item with that id")
        }
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

const ITEM_ABOUT_CAST_LIMIT: usize = 24;
const ITEM_ABOUT_CREW_LIMIT: usize = 24;
const ITEM_ABOUT_CREW_PER_JOB_LIMIT: usize = 6;

fn is_cast_credit(person: &BaseItemPerson) -> bool {
    person.person_type.as_deref() == Some("Actor")
        || (person.person_type.is_none()
            && person
                .role
                .as_deref()
                .is_some_and(|role| !role.trim().is_empty()))
}

/// Keeps the live about payload and its headshot fan-out bounded while
/// preserving Jellyfin's credit order and a useful spread of crew jobs.
fn bounded_about_people(people: &[BaseItemPerson]) -> Vec<Value> {
    let mut selected = Vec::with_capacity(ITEM_ABOUT_CAST_LIMIT + ITEM_ABOUT_CREW_LIMIT);
    let mut cast = 0;
    let mut crew = 0;
    let mut crew_by_job = HashMap::<String, usize>::new();

    for person in people {
        if person
            .name
            .as_deref()
            .is_none_or(|name| name.trim().is_empty())
        {
            continue;
        }
        if is_cast_credit(person) {
            if cast >= ITEM_ABOUT_CAST_LIMIT {
                continue;
            }
            cast += 1;
        } else {
            let Some(job) = person
                .person_type
                .as_deref()
                .filter(|job| !job.trim().is_empty())
            else {
                continue;
            };
            let job_count = crew_by_job.entry(job.to_string()).or_default();
            if crew >= ITEM_ABOUT_CREW_LIMIT || *job_count >= ITEM_ABOUT_CREW_PER_JOB_LIMIT {
                continue;
            }
            crew += 1;
            *job_count += 1;
        }
        selected.push(json!({
            "id": person.id,
            "name": person.name,
            "role": person.role,
            "type": person.person_type,
            "imageTag": person.primary_image_tag,
        }));
    }
    selected
}

/// Rich metadata for one item, fetched live from Jellyfin and never persisted.
/// The detail page draws the cached thin row first and fills this in when it
/// lands; when the server is unreachable the UI keeps its plain error state.
fn item_about(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_item_about(&client, &user_id, item_id) {
        Ok(Some(dto)) => {
            let people = bounded_about_people(&dto.people);
            let studios = dto
                .studios
                .iter()
                .filter_map(|studio| studio.name.clone())
                .collect::<Vec<_>>();
            ApiResponse::ok(json!({
                "overview": dto.overview,
                "criticRating": dto.critic_rating,
                "people": people,
                "tags": dto.tags,
                "studios": studios,
            }))
        }
        Ok(None) => {
            forget_item(services, item_id);
            ApiResponse::error(404, "the server has no item with that id")
        }
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn item_letterboxd(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let item = match services.library.item(item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return ApiResponse::error(404, "item not found"),
        Err(error) => return storage_failure(&error),
    };
    // Letterboxd's RSS movieId namespace is TMDB's movie namespace. Refuse a
    // series or episode even if it happens to carry the same numeric provider
    // id, or a TV record could inherit an unrelated film review.
    if item["kind"].as_str() != Some("Movie") {
        return ApiResponse::ok(json!({
            "reviews": [],
            "configuredProfiles": 0,
            "unavailableProfiles": 0,
        }));
    }
    let Some(tmdb_id) = item["providerIds"]["tmdb"]
        .as_str()
        .filter(|value| !value.is_empty())
    else {
        return ApiResponse::ok(json!({
            "reviews": [],
            "configuredProfiles": 0,
            "unavailableProfiles": 0,
        }));
    };
    letterboxd_reviews_for_movie(services, &server_id, &user_id, tmdb_id)
}

fn movie_letterboxd(services: &Arc<Services>, tmdb_id: &str) -> ApiResponse {
    let Some(tmdb_id) = canonical_tmdb_movie_id(tmdb_id) else {
        return ApiResponse::error(400, "that is not a TMDB movie id");
    };
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    letterboxd_reviews_for_movie(services, &server_id, &user_id, &tmdb_id)
}

fn canonical_tmdb_movie_id(value: &str) -> Option<String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let id = value.parse::<i64>().ok()?;
    (id > 0).then(|| id.to_string())
}

fn letterboxd_reviews_for_movie(
    services: &Arc<Services>,
    server_id: &str,
    user_id: &str,
    tmdb_id: &str,
) -> ApiResponse {
    let profiles = match services
        .library
        .external_profiles("letterboxd", server_id, user_id)
    {
        Ok(profiles) => profiles,
        Err(error) => return storage_failure(&error),
    };
    ApiResponse::ok(json!(
        services.letterboxd.reviews_for_item(&profiles, tmdb_id)
    ))
}

fn fetch_and_cache_item(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_item(&client, &user_id, item_id) {
        Ok(Some(dto)) => {
            let _ = services.library.ingest_page(std::slice::from_ref(&dto));
            match services.library.item(item_id) {
                Ok(Some(item)) => ApiResponse::ok(item),
                Ok(None) => ApiResponse::ok(summary_from_dto(&dto)),
                Err(error) => storage_failure(&error),
            }
        }
        Ok(None) => {
            forget_item(services, item_id);
            ApiResponse::error(404, "the server has no item with that id")
        }
        Err(error) => {
            if matches!(error, ApiError::Status { status: 404 }) {
                forget_item(services, item_id);
            }
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn children(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    // Only containers have a child list worth asking the server about; a movie
    // detail page asks for children too and must not pay for a round trip.
    let overviews = if matches!(
        services.library.kind(item_id).as_deref(),
        Some("Series" | "Season")
    ) {
        reconcile_children(services, item_id)
    } else {
        None
    };
    match services.library.children(item_id) {
        Ok(mut children) => {
            // Episode synopses are not cached; they ride along from the live
            // reconcile that just answered. Offline, rows simply have none.
            if let Some(overviews) = &overviews {
                for child in &mut children {
                    let Some(id) = child["id"].as_str().map(str::to_string) else {
                        continue;
                    };
                    if let (Some(overview), Some(object)) =
                        (overviews.get(&id), child.as_object_mut())
                    {
                        object.insert("overview".to_string(), overview.clone());
                    }
                }
            }
            ApiResponse::ok(json!({ "items": children }))
        }
        Err(error) => storage_failure(&error),
    }
}

/// Re-reads one parent's child list from the server before answering.
///
/// The cache alone cannot be trusted on a detail page: deleting episodes in
/// Jellyfin leaves their rows behind until the next identity sweep, and the
/// season view is exactly where those ghosts surface — a wall of art-less cards
/// with a dead Play button. The image-404 eviction only cleans up rows whose
/// poster happens to be requested, so it misses lazily-loaded cards below the
/// fold and episodes that never had artwork.
///
/// One small non-recursive request per navigation buys a correct list, and it
/// also makes newly added episodes appear without waiting for a sweep.
///
/// Returns each live child's synopsis so the response can carry it without the
/// cache ever storing prose; `None` means the server could not be asked.
fn reconcile_children(services: &Arc<Services>, parent_id: &str) -> Option<HashMap<String, Value>> {
    let (client, user_id) = services.session.client_and_user().ok()?;

    let mut live_items = Vec::new();
    let mut overviews = HashMap::new();
    let mut offset = 0;
    loop {
        let page = match items::fetch_children(&client, &user_id, parent_id, offset) {
            Ok(page) => page,
            Err(error) => {
                // Offline, or the server is unwell. The cached list is still the
                // best answer available, so leave it exactly as it is.
                tracing::debug!(
                    target: "app.api",
                    "could not reconcile the children of {parent_id}: {error}"
                );
                services.session.note_error(&error);
                return None;
            }
        };
        let received = page.items.len() as i64;
        if page.items.is_empty() {
            break;
        }
        for item in &page.items {
            overviews.insert(item.id.clone(), json!(item.overview));
        }
        live_items.extend(page.items);
        offset += received;
        if received < items::CHILDREN_PAGE_SIZE {
            break;
        }
    }

    // An empty `live_items` here came from a successful request, so it is the server
    // saying this parent has no children left — unlike the library-wide sweep,
    // where the blast radius makes that answer too dangerous to trust.
    match services.library.reconcile_children(parent_id, &live_items) {
        Ok(changes) => {
            if !changes.is_empty() {
                tracing::info!(
                    target: "app.api",
                    changed = changes.item_ids.len(),
                    parent_id,
                    "reconciled changed child rows"
                );
                crate::app::services::notify_library_changed(changes);
            }
        }
        Err(error) => {
            tracing::warn!(target: "app.api", "could not commit reconciled children: {error}");
            return None;
        }
    }
    Some(overviews)
}

/// Container, codec, and track detail for the detail page.
///
/// Folders have no streams of their own, so they are answered from here without
/// a round trip rather than letting the server return an empty source list.
fn media_info(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    if matches!(
        services.library.kind(item_id).as_deref(),
        Some("Series" | "Season")
    ) {
        return ApiResponse::ok(json!({
            "sources": [],
            "playbackPreference": Value::Null,
        }));
    }
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_media_sources(&client, &user_id, item_id) {
        Ok(sources) => {
            let preference = match services.library.playback_preference(item_id) {
                Ok(preference) => preference,
                Err(error) => return storage_failure(&error),
            };
            ApiResponse::ok(json!({
                "sources": sources.iter().map(media_source_json).collect::<Vec<_>>(),
                "playbackPreference": resolve_playback_preference(preference.as_ref(), &sources),
            }))
        }
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

/// Saves one complete source/audio/subtitle selection for an exact item.
///
/// The browser only submits indices. Metadata snapshots are rebuilt from the
/// current Jellyfin response here, so persisted language/accessibility intent
/// cannot drift from the track the user actually selected.
fn set_item_playback_preference(
    services: &Arc<Services>,
    item_id: &str,
    request: &ApiRequest,
) -> ApiResponse {
    if matches!(
        services.library.kind(item_id).as_deref(),
        None | Some("Series" | "Season")
    ) {
        return ApiResponse::error(404, "this item has no selectable media tracks");
    }
    let body = request.json();
    let Some(source_index) = body["mediaSourceIndex"]
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
    else {
        return ApiResponse::error(400, "mediaSourceIndex is required");
    };
    let requested_source_id = body["mediaSourceId"]
        .as_str()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let sources = match items::fetch_media_sources(&client, &user_id, item_id) {
        Ok(sources) => sources,
        Err(error) => {
            services.session.note_error(&error);
            return ApiResponse::from_api_error(&error);
        }
    };
    let Some(source) = sources
        .get(source_index)
        .filter(|source| match requested_source_id {
            Some(id) => source.id.as_deref() == Some(id),
            None => source.id.as_deref().is_none_or(str::is_empty),
        })
    else {
        return ApiResponse::error(409, "the available media sources changed; try again");
    };

    let requested_audio_index = body["audioStreamIndex"].as_i64();
    let audio = match requested_audio_index {
        Some(index) if index >= 0 => source
            .streams_of_type("Audio")
            .find(|stream| stream.index == index),
        Some(_) => return ApiResponse::error(400, "audioStreamIndex must not be negative"),
        None if source.streams_of_type("Audio").next().is_none() => None,
        None => return ApiResponse::error(400, "audioStreamIndex is required for this source"),
    };
    if requested_audio_index.is_some() && audio.is_none() {
        return ApiResponse::error(409, "the selected audio track is no longer available");
    }

    let requested_subtitle_index = body["subtitleStreamIndex"].as_i64();
    let subtitle = match requested_subtitle_index {
        Some(index) if index >= 0 => source
            .streams_of_type("Subtitle")
            .find(|stream| stream.index == index),
        Some(_) => return ApiResponse::error(400, "subtitleStreamIndex must not be negative"),
        None => None,
    };
    if requested_subtitle_index.is_some() && subtitle.is_none() {
        return ApiResponse::error(409, "the selected subtitle track is no longer available");
    }

    let preference = ItemPlaybackPreference::capture(source, source_index, audio, subtitle);
    if let Err(error) = services
        .library
        .save_playback_preference(item_id, &preference)
    {
        return storage_failure(&error);
    }
    ApiResponse::ok(json!({
        "playbackPreference": resolve_playback_preference(Some(&preference), &sources),
    }))
}

/// The first local trailer attached to an item, if the server has one.
fn trailer_info(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_local_trailers(&client, &user_id, item_id) {
        Ok(trailers) => match trailers
            .into_iter()
            .find(|trailer| !trailer.id.trim().is_empty())
        {
            Some(trailer) => ApiResponse::ok(json!({
                "trailer": {
                    "id": trailer.id,
                    "name": trailer.display_name(),
                    "embedUrl": Value::Null,
                }
            })),
            None => remote_trailer_info(services, &client, &user_id, item_id),
        },
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn remote_trailer_info(
    services: &Arc<Services>,
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) -> ApiResponse {
    match items::fetch_remote_trailers(client, user_id, item_id) {
        Ok(trailers) => {
            let trailer = trailers.into_iter().find_map(|trailer| {
                youtube_embed_url(&trailer.url).map(|embed_url| {
                    json!({
                        "id": Value::Null,
                        "name": trailer.name.unwrap_or_else(|| "Trailer".to_string()),
                        "embedUrl": embed_url,
                    })
                })
            });
            ApiResponse::ok(json!({ "trailer": trailer }))
        }
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

/// Converts only a plain YouTube watch/share/embed URL into the privacy-enhanced
/// player. Jellyfin metadata is external input, so it never becomes an iframe
/// address verbatim.
fn youtube_embed_url(value: &str) -> Option<String> {
    let value = value.trim();
    let prefixes = [
        "https://www.youtube.com/watch?v=",
        "https://youtube.com/watch?v=",
        "https://m.youtube.com/watch?v=",
        "https://youtu.be/",
        "https://www.youtube.com/embed/",
        "https://www.youtube-nocookie.com/embed/",
    ];
    let remainder = prefixes
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))?;
    let id = remainder.split(['&', '?', '#', '/']).next()?;
    if id.len() != 11
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    Some(format!(
        "https://www.youtube-nocookie.com/embed/{id}?autoplay=1&mute=1&controls=0&disablekb=1&enablejsapi=1&fs=0&iv_load_policy=3&modestbranding=1&playsinline=1&rel=0&showinfo=0&start=5"
    ))
}

/// Authenticated, byte-range-aware access to one local trailer.
///
/// The UI receives only an opaque item id. The Jellyfin token stays in the
/// native client, just as it does for artwork and full playback.
fn trailer_stream(services: &Arc<Services>, trailer_id: &str, request: &ApiRequest) -> ApiResponse {
    let client = match services.session.client() {
        Ok(client) => client,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let path = format!("/Videos/{}/stream", encode_path_segment(trailer_id));
    let range = bounded_byte_range(request.range.as_deref());
    match client.get_bytes_range(&path, &[("static", "true".to_string())], Some(&range)) {
        Ok(response) => ApiResponse::ranged_bytes(
            response.status,
            response.content_type,
            response.body,
            response.content_range,
            response.accept_ranges,
        ),
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

/// Accept one simple `bytes=` range and cap it to a small proxy chunk.
fn bounded_byte_range(value: Option<&str>) -> String {
    let fallback = format!("bytes=0-{}", TRAILER_CHUNK_SIZE - 1);
    let Some(spec) = value
        .and_then(|value| value.trim().strip_prefix("bytes="))
        .filter(|value| !value.contains(','))
    else {
        return fallback;
    };
    let Some((start, end)) = spec.split_once('-') else {
        return fallback;
    };
    if let Ok(start) = start.parse::<u64>() {
        let max_end = start.saturating_add(TRAILER_CHUNK_SIZE - 1);
        let end = if end.is_empty() {
            max_end
        } else {
            match end.parse::<u64>() {
                Ok(end) if end >= start => end.min(max_end),
                _ => return fallback,
            }
        };
        return format!("bytes={start}-{end}");
    }
    if start.is_empty()
        && let Ok(suffix) = end.parse::<u64>()
        && suffix > 0
    {
        return format!("bytes=-{}", suffix.min(TRAILER_CHUNK_SIZE));
    }
    fallback
}

fn media_source_json(source: &MediaSourceInfo) -> Value {
    let streams = |kind: &str| {
        source
            .streams_of_type(kind)
            .map(media_stream_json)
            .collect::<Vec<_>>()
    };
    json!({
        "id": source.id,
        "name": source.display_name(),
        "container": source.container,
        // Only the file name: the server's directory layout is not the UI's
        // business, but "which file is this" is exactly what this page is for.
        // Jellyfin answers non-admin users with an opaque id in `Path`, which
        // `file_name_of` filters out — the source name carries the release
        // there, and the UI falls back to it.
        "fileName": source.path.as_deref().and_then(file_name_of),
        "size": source.size,
        "bitrate": source.bitrate,
        "defaultAudioStreamIndex": source.default_audio_stream_index,
        "defaultSubtitleStreamIndex": source.default_subtitle_stream_index,
        "video": streams("video"),
        "audio": streams("audio"),
        "subtitles": streams("subtitle"),
    })
}

fn media_stream_json(stream: &MediaStream) -> Value {
    json!({
        "index": stream.index,
        "type": stream.stream_type,
        "codec": stream.codec,
        "profile": stream.profile,
        "language": stream.language,
        "title": stream.title,
        "displayTitle": stream.display_title,
        "width": stream.width,
        "height": stream.height,
        "channels": stream.channels,
        "audioSpatialFormat": stream.audio_spatial_format,
        "videoRange": stream.video_range,
        "videoRangeType": stream.video_range_type,
        "bitDepth": stream.bit_depth,
        "isDefault": stream.is_default,
        "isForced": stream.is_forced,
        "isHearingImpaired": stream.is_hearing_impaired,
        "isExternal": stream.is_external,
    })
}

/// The basename of a real file path.
///
/// `Path` is admin-only in Jellyfin; ordinary users get an opaque id there
/// instead, so a name with no extension is not a file name and is dropped
/// rather than printed as if it were one.
fn file_name_of(path: &str) -> Option<&str> {
    let name = path.rsplit(['/', '\\']).next()?;
    let extension = name.rsplit_once('.')?.1;
    (!extension.is_empty() && extension.len() <= 5 && extension.chars().all(char::is_alphanumeric))
        .then_some(name)
}

/// The episode a series' Play button should start.
///
/// Next Up is server-side logic, exactly as on the home screen, so a series
/// page agrees with the Next Up row. It runs out on a fully watched show and is
/// unavailable offline, and in both cases the first episode is a better answer
/// than a page with nothing to play.
fn next_up(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    if services.library.kind(item_id).as_deref() != Some("Series") {
        return ApiResponse::ok(json!({ "item": Value::Null }));
    }
    let from_server = match services.session.client_and_user() {
        Ok((client, user_id)) => items::fetch_next_up(&client, &user_id, Some(item_id), 1)
            .map(|response| response.items.first().map(summary_from_dto))
            .unwrap_or_else(|error| {
                services.session.note_error(&error);
                tracing::debug!(target: "app.api", "Next Up unavailable for {item_id}: {error}");
                None
            }),
        Err(_) => None,
    };
    let item = match from_server {
        Some(item) => Some(item),
        None => services.library.first_episode(item_id).unwrap_or_default(),
    };
    ApiResponse::ok(json!({ "item": item }))
}

#[derive(Clone, Copy)]
enum ExternalProvider {
    Imdb,
    Tmdb,
    Tvdb,
    Letterboxd,
    Trakt,
}

impl ExternalProvider {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "imdb" => Self::Imdb,
            "tmdb" => Self::Tmdb,
            "tvdb" => Self::Tvdb,
            "letterboxd" => Self::Letterboxd,
            "trakt" => Self::Trakt,
            _ => return None,
        })
    }

    const fn id_field(self) -> &'static str {
        match self {
            Self::Imdb | Self::Trakt => "imdb",
            Self::Tmdb | Self::Letterboxd => "tmdb",
            Self::Tvdb => "tvdb",
        }
    }

    fn url(self, id: &str, kind: &str) -> Option<String> {
        if !valid_external_id(self.id_field(), id) {
            return None;
        }
        Some(match (self, kind) {
            // IMDb keeps movies, series, and episodes under one `/title/` route.
            (Self::Imdb, "Movie" | "Series" | "Episode") => {
                format!("https://www.imdb.com/title/{id}/")
            }
            (Self::Tmdb, "Movie") => format!("https://www.themoviedb.org/movie/{id}"),
            (Self::Tmdb, "Series") => format!("https://www.themoviedb.org/tv/{id}"),
            (Self::Tvdb, "Movie") => format!("https://thetvdb.com/dereferrer/movie/{id}"),
            (Self::Tvdb, "Series") => format!("https://thetvdb.com/dereferrer/series/{id}"),
            (Self::Tvdb, "Episode") => {
                format!("https://thetvdb.com/dereferrer/episode/{id}")
            }
            (Self::Letterboxd, "Movie") => format!("https://letterboxd.com/tmdb/{id}"),
            (Self::Trakt, "Movie") => format!("https://trakt.tv/movies/{id}"),
            (Self::Trakt, "Series") => format!("https://trakt.tv/shows/{id}"),
            _ => return None,
        })
    }
}

/// Opens an item's exact provider page in the default browser.
///
/// The UI names a provider, never a URL: the id and the item kind both come
/// from the cached row here, so nothing the page can say turns into a launched
/// address.
fn open_external(services: &Arc<Services>, item_id: &str, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let Some(provider) = body["provider"].as_str().and_then(ExternalProvider::parse) else {
        return ApiResponse::error(404, "unknown external information provider");
    };
    let item = match services.library.item(item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return ApiResponse::error(404, "no cached item with that id"),
        Err(error) => return storage_failure(&error),
    };
    let kind = item["kind"].as_str().unwrap_or_default();
    let id = item["providerIds"][provider.id_field()]
        .as_str()
        .unwrap_or("");
    let Some(url) = provider.url(id, kind) else {
        return ApiResponse::error(404, "this item has no id for that database");
    };
    super::open_external_link(&url);
    ApiResponse::ok(json!({ "opened": true, "url": url }))
}

fn valid_external_id(source: &str, id: &str) -> bool {
    if id.is_empty() || id.len() > 32 {
        return false;
    }
    match source {
        "imdb" => id.strip_prefix("tt").is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        }),
        "tmdb" | "tvdb" => {
            id.bytes().all(|byte| byte.is_ascii_digit()) && id.bytes().any(|byte| byte != b'0')
        }
        _ => false,
    }
}

fn set_played(services: &Arc<Services>, item_id: &str, request: &ApiRequest) -> ApiResponse {
    let played = request.json()["played"].as_bool().unwrap_or(true);
    if let Err(response) = user_data_write(services, item_id, |client, user_id| {
        items::set_played(client, user_id, item_id, played)
    }) {
        return response;
    }
    let _ = services.library.set_local_played(item_id, played);
    ApiResponse::ok(json!({ "played": played }))
}

fn set_favorite(services: &Arc<Services>, item_id: &str, request: &ApiRequest) -> ApiResponse {
    let favorite = request.json()["favorite"].as_bool().unwrap_or(true);
    if let Err(response) = user_data_write(services, item_id, |client, user_id| {
        items::set_favorite(client, user_id, item_id, favorite)
    }) {
        return response;
    }
    let _ = services.library.set_local_favorite(item_id, favorite);
    ApiResponse::ok(json!({ "favorite": favorite }))
}

/// The server is the source of truth for watch state, so it is written first
/// and the local mirror only follows a success.
fn user_data_write(
    services: &Arc<Services>,
    item_id: &str,
    write: impl FnOnce(&JellyfinClient, &str) -> Result<(), ApiError>,
) -> Result<(), ApiResponse> {
    let (client, user_id) = services
        .session
        .client_and_user()
        .map_err(|error| ApiResponse::from_api_error(&error))?;
    write(&client, &user_id).map_err(|error| {
        services.session.note_error(&error);
        tracing::warn!(target: "app.api", item_id, "user-data write failed: {error}");
        ApiResponse::from_api_error(&error)
    })
}

// -------------------------------------------------------------------- images

static IMAGE_WRITES: AtomicUsize = AtomicUsize::new(0);
/// Prune once the cache is clearly larger than a big library's poster set.
const IMAGE_CACHE_MAX_FILES: usize = 4_000;
const IMAGE_CACHE_PRUNE_EVERY: usize = 200;

fn image(
    services: &Arc<Services>,
    item_id: &str,
    image_type: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let tag = request.param("tag").unwrap_or_default();
    let max_width = request
        .param("maxWidth")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
        .min(4_000);
    let key = cache_key(item_id, image_type, &tag, max_width);
    let cache_path = crate::app::paths::image_cache_dir().join(&key);

    if let Ok(bytes) = std::fs::read(&cache_path)
        && !bytes.is_empty()
    {
        return ApiResponse::bytes(mime_for_image(&bytes), bytes, IMMUTABLE_CACHE);
    }

    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let mut query = Vec::new();
    if !tag.is_empty() {
        query.push(("tag", tag));
    }
    if max_width > 0 {
        query.push(("maxWidth", max_width.to_string()));
    }
    query.push(("quality", "90".to_string()));

    match client.get_bytes(&items::image_path(item_id, image_type), &query) {
        Ok((bytes, content_type)) => {
            store_image(&cache_path, &bytes);
            ApiResponse::bytes(content_type, bytes, IMMUTABLE_CACHE)
        }
        Err(error) => {
            // A missing image is the first sign of a replaced file, because the
            // grid renders posters long before anything tries to play them.
            if matches!(error, ApiError::Status { status: 404 }) {
                forget_if_server_disowns(services, &client, &user_id, item_id);
            }
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

/// Evicts a cached item, but only once the server confirms it is really gone.
///
/// An image 404 alone is not proof: an item can exist with no artwork under
/// that tag. `fetch_item` queries `/Items?ids=`, so a missing item comes back as
/// an empty result rather than an error, which cleanly separates "deleted" from
/// "the server is unwell" — the latter lands in `Err` and is left alone.
fn forget_if_server_disowns(
    services: &Arc<Services>,
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) {
    match items::fetch_item(client, user_id, item_id) {
        Ok(None) => forget_item(services, item_id),
        Ok(Some(_)) => {}
        Err(error) => {
            tracing::debug!(
                target: "app.api",
                "could not confirm whether {item_id} still exists: {error}"
            );
        }
    }
}

/// Only characters that are safe in a file name survive, so a hostile item id
/// cannot escape the cache directory.
fn cache_key(item_id: &str, image_type: &str, tag: &str, max_width: u32) -> String {
    let sanitize = |value: &str| -> String {
        value
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .take(64)
            .collect()
    };
    format!(
        "{}-{}-{}-{max_width}.img",
        sanitize(item_id),
        sanitize(image_type),
        sanitize(tag)
    )
}

fn store_image(path: &std::path::Path, bytes: &[u8]) {
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() || std::fs::write(path, bytes).is_err() {
        return;
    }
    if IMAGE_WRITES
        .fetch_add(1, Ordering::Relaxed)
        .is_multiple_of(IMAGE_CACHE_PRUNE_EVERY)
    {
        prune_image_cache(parent);
    }
}

/// Drops the oldest quarter of the cache once it grows past the cap.
fn prune_image_cache(directory: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files = entries
        .flatten()
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect::<Vec<_>>();
    if files.len() <= IMAGE_CACHE_MAX_FILES {
        return;
    }
    files.sort_by_key(|(modified, _)| *modified);
    let remove = files.len() - IMAGE_CACHE_MAX_FILES * 3 / 4;
    for (_, path) in files.into_iter().take(remove) {
        let _ = std::fs::remove_file(path);
    }
    tracing::debug!(target: "app.api", removed = remove, "pruned the poster cache");
}

fn mime_for_image(bytes: &[u8]) -> String {
    let kind = match bytes {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
        [b'G', b'I', b'F', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', ..] => "image/webp",
        _ => "application/octet-stream",
    };
    kind.to_string()
}

// ------------------------------------------------------------------ playback

fn play_item(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let Some(item_id) = body["itemId"].as_str().filter(|id| !id.is_empty()) else {
        return ApiResponse::error(400, "itemId is required");
    };
    let options = PlayOptions {
        item_id: item_id.to_string(),
        resume: body["resume"].as_bool().unwrap_or(false),
        start_ticks: body["startTicks"].as_i64(),
        media_source_id: body["mediaSourceId"].as_str().map(str::to_string),
        media_source_index: body["mediaSourceIndex"]
            .as_u64()
            .and_then(|index| usize::try_from(index).ok()),
        audio_stream_index: body["audioStreamIndex"].as_i64(),
        subtitle_stream_index: body["subtitleStreamIndex"].as_i64(),
        quality: body["quality"].as_str().and_then(StreamingQuality::from_id),
    };
    start_playback(services, &options)
}

/// Used by the UI when mpv reports end-of-file or a mark-watched-and-next.
fn play_next(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let Some(item_id) = body["itemId"].as_str().filter(|id| !id.is_empty()) else {
        return ApiResponse::error(400, "itemId is required");
    };
    let next = match services.library.next_episode(item_id) {
        Ok(Some(next)) => next,
        Ok(None) => return ApiResponse::ok(json!({ "started": false })),
        Err(error) => return storage_failure(&error),
    };
    let Some(next_id) = next["id"].as_str() else {
        return ApiResponse::ok(json!({ "started": false }));
    };
    start_playback(
        services,
        &PlayOptions {
            item_id: next_id.to_string(),
            resume: true,
            ..Default::default()
        },
    )
}

fn start_playback(services: &Arc<Services>, options: &PlayOptions) -> ApiResponse {
    match play::start(services, options, "own UI") {
        Ok(prepared) => ApiResponse::ok(json!({
            "started": true,
            "itemId": options.item_id,
            "playMethod": prepared.play_method,
            "mediaSource": prepared.media_source_name,
            "startTicks": prepared.request.start_time_ticks.unwrap_or(0),
        })),
        Err(play::StartError::NoPlayer) => ApiResponse::error(
            409,
            "No media player is configured. Open Settings to set up mpv or MPC-HC.",
        ),
        Err(play::StartError::NotReady) => {
            ApiResponse::error(503, "the playback coordinator is not ready yet")
        }
        Err(play::StartError::Api(error)) => {
            // A 404 from `PlaybackInfo` means the item no longer exists on the
            // server, so the cached row is a phantom: drop it now rather than
            // offering a Play button that can never work.
            if matches!(error, ApiError::Status { status: 404 }) {
                forget_item(services, &options.item_id);
            }
            ApiResponse::from_api_error(&error)
        }
    }
}

fn player_state(services: &Arc<Services>) -> ApiResponse {
    let Some(playback) = services.playback() else {
        return ApiResponse::ok(json!({ "active": false }));
    };
    let snapshot = playback.snapshot();
    let capabilities = playback.capabilities();
    ApiResponse::ok(json!({
        "active": snapshot.active,
        "playbackId": snapshot.playback_id,
        "itemId": snapshot.item_id,
        "mediaSourceId": snapshot.media_source_id,
        "playSessionId": snapshot.play_session_id,
        "positionMs": snapshot.position_ms,
        "durationMs": snapshot.duration_ms,
        "paused": snapshot.paused,
        "volume": snapshot.volume,
        "mute": snapshot.mute,
        "stopReason": snapshot.stop_reason,
        "capabilities": {
            "chapterMarkers": capabilities.chapter_markers,
            "externalSubtitles": capabilities.external_subtitles,
            "injectedHotkeys": capabilities.injected_hotkeys,
            "absoluteVolume": capabilities.absolute_volume,
            "pushesPosition": capabilities.pushes_position,
        },
    }))
}

fn player_command(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    use crate::playback::PlayerCommand;

    let body = request.json();
    let command = match body["command"].as_str().unwrap_or_default() {
        "pause" => Some(PlayerCommand::SetPause(true)),
        "resume" => Some(PlayerCommand::SetPause(false)),
        "toggle-pause" => Some(PlayerCommand::SetPause(
            !body["paused"].as_bool().unwrap_or(false),
        )),
        "seek" => body["positionMs"]
            .as_f64()
            .filter(|value| value.is_finite())
            .map(PlayerCommand::SeekMilliseconds),
        "set-volume" => body["volume"]
            .as_f64()
            .filter(|value| value.is_finite())
            .map(PlayerCommand::SetVolume),
        "set-mute" => body["mute"].as_bool().map(PlayerCommand::SetMute),
        "set-playback-rate" => body["rate"]
            .as_f64()
            .filter(|rate| rate.is_finite() && *rate > 0.0)
            .map(PlayerCommand::SetPlaybackRate),
        "set-audio-track" => body["audioTrack"]
            .as_i64()
            .filter(|track| *track > 0)
            .map(PlayerCommand::SetAudioTrack),
        "set-subtitle-track" => match body["subtitleUrl"].as_str().map(str::trim) {
            Some(url) if !url.is_empty() => Some(PlayerCommand::AddSubtitle(url.to_string())),
            // A null track turns subtitles off.
            _ => Some(PlayerCommand::SetSubtitleTrack(
                body["subtitleTrack"].as_i64().filter(|track| *track > 0),
            )),
        },
        "stop" => Some(PlayerCommand::Stop),
        _ => None,
    };
    let Some(command) = command else {
        return ApiResponse::error(400, "unsupported player command");
    };
    let Some(playback) = services.playback() else {
        return ApiResponse::error(503, "the playback coordinator is not ready yet");
    };
    playback.control(command);
    ApiResponse::ok(json!({ "accepted": true }))
}

// -------------------------------------------------------------------- assets

// The bundle is built by `build.rs` (Vite, in `ui/`) and staged into `OUT_DIR`.
// It is emitted with fixed names — no content hashing and no code splitting,
// because the assets never cross a network: they are embedded here and served
// from memory.
fn static_asset(path: &str) -> Option<ApiResponse> {
    match path {
        "" | "/" | "/index.html" => Some(index_html()),
        "/app.js" => Some(ApiResponse::asset(
            "text/javascript; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/app.js")),
            NO_STORE,
        )),
        "/app.css" => Some(ApiResponse::asset(
            "text/css; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/app.css")),
            NO_STORE,
        )),
        _ => None,
    }
}

fn index_html() -> ApiResponse {
    ApiResponse::asset(
        "text/html; charset=utf-8",
        include_bytes!(concat!(env!("OUT_DIR"), "/index.html")),
        NO_STORE,
    )
}

// --------------------------------------------------------------------- misc

fn summary_from_dto(dto: &BaseItemDto) -> Value {
    json!({
        "id": dto.id,
        "kind": dto.item_type,
        "name": dto.display_name(),
        "year": dto.production_year,
        "runtimeTicks": dto.run_time_ticks,
        "communityRating": dto.community_rating,
        "officialRating": dto.official_rating,
        "seriesId": dto.series_id,
        "seriesName": dto.series_name,
        "indexNumber": dto.index_number,
        "parentIndexNumber": dto.parent_index_number,
        "primaryImageTag": dto.primary_image_tag(),
        "thumbImageTag": dto.image_tag("Thumb"),
        "logoImageTag": dto.image_tag("Logo"),
        "backdropImageTag": dto.backdrop_image_tags.first(),
        "childCount": dto.child_count,
        "premiereDate": dto.premiere_date,
        "seasonId": dto.season_id,
        "played": dto.user_data.as_ref().is_some_and(|data| data.played),
        "playCount": dto.user_data.as_ref().map(|data| data.play_count).unwrap_or(0),
        "positionTicks": dto
            .user_data
            .as_ref()
            .map(|data| data.playback_position_ticks)
            .unwrap_or(0),
        "favorite": dto.user_data.as_ref().is_some_and(|data| data.is_favorite),
    })
}

/// Evicts a cached item the server has disowned, and asks for a sync so the
/// replacement (Jellyfin re-creates the item with a new id) is picked up.
fn forget_item(services: &Arc<Services>, item_id: &str) {
    match services.library.forget(item_id) {
        Ok(changes) if !changes.is_empty() => {
            tracing::info!(
                target: "app.api",
                item_id,
                "dropped a cached item the server no longer has"
            );
            crate::app::services::notify_library_changed(changes);
            services.sync.request();
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(target: "app.api", "failed to drop stale item {item_id}: {error}");
        }
    }
}

fn storage_failure(error: &rusqlite::Error) -> ApiResponse {
    tracing::warn!(target: "app.api", "library query failed: {error}");
    ApiResponse::error(500, format!("library query failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        ApiRequest, ApiResponse, ExternalProvider, HOME_ROW_LIMIT, ITEM_ABOUT_CAST_LIMIT,
        ITEM_ABOUT_CREW_LIMIT, bounded_about_people, bounded_byte_range, cache_key,
        canonical_tmdb_movie_id, clear_person_availability, file_name_of, handle, is_iso_date,
        join_person_items, latest_home_items, media_source_json, merge_next_up, mime_for_image,
        summary_from_dto, youtube_embed_url,
    };
    use crate::jellyfin::api::model::{BaseItemDto, MediaSourceInfo};
    use crate::library::Library;
    use serde_json::json;

    fn get(path: &str) -> ApiResponse {
        handle(&ApiRequest {
            method: "GET".to_string(),
            path: path.to_string(),
            query: String::new(),
            body: Vec::new(),
            range: None,
            cancelled: Default::default(),
        })
    }

    fn external_url(provider: &str, id: &str, kind: &str) -> Option<String> {
        ExternalProvider::parse(provider)?.url(id, kind)
    }

    #[test]
    fn the_shell_is_served_at_the_root_and_for_unknown_routes() {
        for path in ["/", "", "/index.html", "/library", "/item/abc"] {
            let response = get(path);
            assert_eq!(response.status, 200, "path {path}");
            assert!(response.content_type.starts_with("text/html"));
            assert!(String::from_utf8_lossy(&response.body).contains("<!doctype html>"));
        }
    }

    #[test]
    fn ui_assets_are_served_with_their_own_content_types() {
        assert!(get("/app.js").content_type.starts_with("text/javascript"));
        assert!(get("/app.css").content_type.starts_with("text/css"));
    }

    #[test]
    fn request_bodies_that_are_not_json_degrade_to_an_empty_object() {
        let request = ApiRequest {
            method: "POST".to_string(),
            path: "/api/auth/login".to_string(),
            query: String::new(),
            body: b"not json".to_vec(),
            range: None,
            cancelled: Default::default(),
        };
        assert_eq!(request.json(), json!({}));
    }

    #[test]
    fn query_parameters_are_decoded_and_blank_values_ignored() {
        let request = ApiRequest {
            method: "GET".to_string(),
            path: "/api/items".to_string(),
            query: "search=the%20matrix&genre=&limit=20".to_string(),
            body: Vec::new(),
            range: None,
            cancelled: Default::default(),
        };
        assert_eq!(request.param("search").as_deref(), Some("the matrix"));
        assert_eq!(request.param("genre"), None);
        assert_eq!(request.param("limit").as_deref(), Some("20"));
    }

    #[test]
    fn calendar_dates_are_strict_iso_days() {
        assert!(is_iso_date("2026-08-02"));
        assert!(!is_iso_date("2026-8-2"));
        assert!(!is_iso_date("2026/08/02"));
        assert!(!is_iso_date("../../etc"));
    }

    #[test]
    fn discovered_letterboxd_lookups_require_a_positive_tmdb_movie_id() {
        assert_eq!(canonical_tmdb_movie_id("603").as_deref(), Some("603"));
        assert_eq!(canonical_tmdb_movie_id("000603").as_deref(), Some("603"));
        for invalid in ["", "0", "+603", "-1", "603.0", "movie-603"] {
            assert_eq!(canonical_tmdb_movie_id(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn cache_keys_cannot_escape_the_cache_directory() {
        let key = cache_key("../../etc/passwd", "Primary", "tag", 400);
        assert!(!key.contains('/'));
        assert!(!key.contains('.') || key.ends_with(".img"));
        assert_eq!(key, "etcpasswd-Primary-tag-400.img");
    }

    #[test]
    fn image_mime_types_are_sniffed_from_the_payload() {
        assert_eq!(mime_for_image(&[0x89, b'P', b'N', b'G', 0]), "image/png");
        assert_eq!(mime_for_image(&[0xFF, 0xD8, 0xFF, 0]), "image/jpeg");
        assert_eq!(mime_for_image(b"RIFF...."), "image/webp");
        assert_eq!(mime_for_image(b"nonsense"), "application/octet-stream");
    }

    #[test]
    fn trailer_ranges_are_bounded_and_malformed_ranges_fall_back() {
        assert_eq!(bounded_byte_range(None), "bytes=0-4194303");
        assert_eq!(bounded_byte_range(Some("bytes=100-")), "bytes=100-4194403");
        assert_eq!(
            bounded_byte_range(Some("bytes=100-9999999")),
            "bytes=100-4194403"
        );
        assert_eq!(bounded_byte_range(Some("bytes=100-200")), "bytes=100-200");
        assert_eq!(bounded_byte_range(Some("bytes=-9999999")), "bytes=-4194304");
        assert_eq!(bounded_byte_range(Some("not-a-range")), "bytes=0-4194303");
    }

    #[test]
    fn only_plain_youtube_trailers_become_privacy_enhanced_embeds() {
        assert_eq!(
            youtube_embed_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ&feature=share"),
            Some(
                "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?autoplay=1&mute=1&controls=0&disablekb=1&enablejsapi=1&fs=0&iv_load_policy=3&modestbranding=1&playsinline=1&rel=0&showinfo=0&start=5"
                    .to_string()
            )
        );
        assert!(youtube_embed_url("https://youtu.be/dQw4w9WgXcQ").is_some());
        assert!(youtube_embed_url("http://www.youtube.com/watch?v=dQw4w9WgXcQ").is_none());
        assert!(youtube_embed_url("https://example.com/embed/dQw4w9WgXcQ").is_none());
        assert!(youtube_embed_url("https://youtu.be/not-valid").is_none());
    }

    #[test]
    fn next_up_entries_are_shaped_like_cached_rows() {
        let dto: BaseItemDto = serde_json::from_str(
            r#"{"Id":"e1","Name":"Half Loop","Type":"Episode","SeriesName":"Severance",
                "IndexNumber":2,"ParentIndexNumber":1,
                "ImageTags":{"Primary":"still-tag","Thumb":"thumb-tag","Logo":"logo-tag"},
                "BackdropImageTags":["backdrop-tag"],
                "UserData":{"Played":false,"PlaybackPositionTicks":42}}"#,
        )
        .expect("dto");
        let summary = summary_from_dto(&dto);
        assert_eq!(summary["id"], "e1");
        assert_eq!(summary["kind"], "Episode");
        assert_eq!(summary["seriesName"], "Severance");
        assert_eq!(summary["positionTicks"], 42);
        assert_eq!(summary["played"], false);
        assert_eq!(summary["favorite"], false);
        assert_eq!(summary["primaryImageTag"], "still-tag");
        assert_eq!(summary["thumbImageTag"], "thumb-tag");
        assert_eq!(summary["logoImageTag"], "logo-tag");
        assert_eq!(summary["backdropImageTag"], "backdrop-tag");
    }

    #[test]
    fn live_about_credits_are_bounded_before_serialization() {
        let mut people = vec![json!({
            "Name": "Unclassified performer",
            "Role": "Self",
        })];
        people.extend((0..30).map(|index| {
            json!({ "Id": format!("actor-{index}"), "Name": format!("Actor {index}"), "Type": "Actor" })
        }));
        for job in ["Director", "Writer", "Producer", "Composer", "Editor"] {
            people.extend(
                (0..10).map(|index| json!({ "Name": format!("{job} {index}"), "Type": job })),
            );
        }
        for kind in ["Movie", "Series"] {
            let dto: BaseItemDto = serde_json::from_value(json!({
                "Type": kind,
                "People": people.clone(),
            }))
            .expect("dto");

            let selected = bounded_about_people(&dto.people);
            assert_eq!(
                selected.len(),
                ITEM_ABOUT_CAST_LIMIT + ITEM_ABOUT_CREW_LIMIT,
                "{kind}"
            );
            assert_eq!(selected[0]["name"], "Unclassified performer");
            assert_eq!(
                selected
                    .iter()
                    .filter(|person| person["type"] == "Actor" || person["type"].is_null())
                    .count(),
                ITEM_ABOUT_CAST_LIMIT,
                "{kind}"
            );
            assert_eq!(
                selected
                    .iter()
                    .filter(|person| person["type"] != "Actor" && !person["type"].is_null())
                    .count(),
                ITEM_ABOUT_CREW_LIMIT,
                "{kind}"
            );
            for job in ["Director", "Writer", "Producer", "Composer", "Editor"] {
                assert!(
                    selected
                        .iter()
                        .filter(|person| person["type"] == job)
                        .count()
                        <= super::ITEM_ABOUT_CREW_PER_JOB_LIMIT,
                    "{kind} {job}"
                );
            }
        }
    }

    #[test]
    fn latest_home_rows_are_kind_scoped_and_ordered_by_release_year() {
        let library = Library::open_in_memory().expect("library");
        let items: Vec<BaseItemDto> = [
            r#"{"Id":"old-movie","Name":"Old Film","Type":"Movie","ProductionYear":1999}"#,
            r#"{"Id":"new-movie","Name":"New Film","Type":"Movie","ProductionYear":2026}"#,
            r#"{"Id":"old-show","Name":"Old Show","Type":"Series","ProductionYear":2010}"#,
            r#"{"Id":"new-show","Name":"New Show","Type":"Series","ProductionYear":2025}"#,
        ]
        .into_iter()
        .map(|value| serde_json::from_str(value).expect("dto"))
        .collect();
        library.upsert_page(&items).expect("seed");

        let movies = latest_home_items(&library, "Movie");
        let shows = latest_home_items(&library, "Series");

        assert_eq!(movies[0]["id"], "new-movie");
        assert_eq!(movies[1]["id"], "old-movie");
        assert!(movies.iter().all(|item| item["kind"] == "Movie"));
        assert_eq!(shows[0]["id"], "new-show");
        assert_eq!(shows[1]["id"], "old-show");
        assert!(shows.iter().all(|item| item["kind"] == "Series"));
    }

    #[test]
    fn live_person_items_override_progressively_incomplete_local_availability() {
        let dtos: Vec<BaseItemDto> = [
            r#"{"Id":"m1","Name":"Movie","Type":"Movie","ProviderIds":{"Tmdb":"603"}}"#,
            r#"{"Id":"s1","Name":"Series","Type":"Series","ProviderIds":{"tmdb":"603"}}"#,
            r#"{"Id":"e1","Name":"Episode","Type":"Episode","ProviderIds":{"Tmdb":"603"}}"#,
        ]
        .into_iter()
        .map(|value| serde_json::from_str(value).expect("dto"))
        .collect();
        let mut credits = json!({ "results": [
            { "mediaType": "movie", "tmdbId": 603, "libraryItemId": null },
            { "mediaType": "tv", "tmdbId": 603, "libraryItemId": null },
            { "mediaType": "movie", "tmdbId": 404, "libraryItemId": "stale" }
        ] });

        clear_person_availability(&mut credits);
        join_person_items(&mut credits, &dtos);

        assert_eq!(credits["results"][0]["libraryItemId"], "m1");
        assert_eq!(credits["results"][1]["libraryItemId"], "s1");
        assert_eq!(
            credits["results"][2]["libraryItemId"],
            serde_json::Value::Null
        );
    }

    fn episode(id: &str, series_id: &str) -> serde_json::Value {
        json!({ "id": id, "kind": "Episode", "seriesId": series_id })
    }

    #[test]
    fn continue_watching_comes_before_next_up_in_the_merged_row() {
        let merged = merge_next_up(
            vec![episode("e1", "sev"), json!({ "id": "m1", "kind": "Movie" })],
            vec![episode("e9", "silo"), episode("e8", "andor")],
        );
        let ids: Vec<&str> = merged
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["e1", "m1", "e9", "e8"]);
    }

    /// The in-progress episode is also its series' Next Up, so the naive
    /// concatenation would show the same card twice.
    #[test]
    fn a_show_already_being_watched_is_not_repeated_by_next_up() {
        let merged = merge_next_up(
            vec![episode("e1", "sev")],
            // Same episode by id, then the successor within the same series.
            vec![
                episode("e1", "sev"),
                episode("e2", "sev"),
                episode("e9", "silo"),
            ],
        );
        let ids: Vec<&str> = merged
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["e1", "e9"]);
    }

    /// A full Continue Watching list must not squeeze Next Up off the row.
    #[test]
    fn next_up_survives_a_continue_watching_list_that_fills_a_row_on_its_own() {
        let limit = HOME_ROW_LIMIT as usize;
        let resume: Vec<_> = (0..limit)
            .map(|index| json!({ "id": format!("r{index}"), "kind": "Movie" }))
            .collect();
        let merged = merge_next_up(resume, vec![episode("e9", "silo")]);
        assert_eq!(merged.len(), limit + 1);
        assert_eq!(merged.last().expect("last")["id"], "e9");
    }

    #[test]
    fn api_errors_carry_the_session_expiry_flag() {
        use crate::jellyfin::api::ApiError;
        let response = ApiResponse::from_api_error(&ApiError::Unauthorized);
        assert_eq!(response.status, 401);
        let body: serde_json::Value = serde_json::from_slice(&response.body).expect("json");
        assert_eq!(body["expired"], true);

        let response = ApiResponse::from_api_error(&ApiError::Status { status: 404 });
        assert_eq!(response.status, 404);
        let body: serde_json::Value = serde_json::from_slice(&response.body).expect("json");
        assert_eq!(body["expired"], false);
    }

    #[test]
    fn seerr_errors_do_not_carry_the_jellyfin_expiry_flag() {
        use crate::seerr::api::SeerrError;

        let response = ApiResponse::from_seerr_error(&SeerrError::Unauthorized);
        assert_eq!(response.status, 401);
        let body: serde_json::Value = serde_json::from_slice(&response.body).expect("json");
        assert!(body.get("expired").is_none());
        assert_eq!(body["seerrExpired"], true);
    }

    #[test]
    fn external_links_are_built_per_provider_and_kind() {
        assert_eq!(
            ExternalProvider::parse("letterboxd").map(ExternalProvider::id_field),
            Some("tmdb")
        );
        assert_eq!(
            ExternalProvider::parse("trakt").map(ExternalProvider::id_field),
            Some("imdb")
        );
        assert_eq!(
            external_url("imdb", "tt0133093", "Movie").as_deref(),
            Some("https://www.imdb.com/title/tt0133093/")
        );
        assert_eq!(
            external_url("tmdb", "603", "Movie").as_deref(),
            Some("https://www.themoviedb.org/movie/603")
        );
        assert_eq!(
            external_url("tmdb", "95396", "Series").as_deref(),
            Some("https://www.themoviedb.org/tv/95396")
        );
        assert_eq!(
            external_url("tvdb", "371980", "Episode").as_deref(),
            Some("https://thetvdb.com/dereferrer/episode/371980")
        );
        assert_eq!(
            external_url("letterboxd", "603", "Movie").as_deref(),
            Some("https://letterboxd.com/tmdb/603")
        );
        assert_eq!(
            external_url("trakt", "tt0133093", "Movie").as_deref(),
            Some("https://trakt.tv/movies/tt0133093")
        );
        assert_eq!(
            external_url("trakt", "tt11280740", "Series").as_deref(),
            Some("https://trakt.tv/shows/tt11280740")
        );
    }

    /// An episode's TMDb id is an episode id, so there is no `/tv/` page for it
    /// and offering one would link an unrelated show.
    #[test]
    fn external_links_are_absent_where_the_id_does_not_address_a_page() {
        assert_eq!(external_url("tmdb", "12345", "Episode"), None);
        assert_eq!(external_url("tmdb", "12345", "Season"), None);
        assert_eq!(external_url("tvdb", "12345", "Season"), None);
        assert_eq!(external_url("letterboxd", "95396", "Series"), None);
        assert_eq!(external_url("trakt", "tt0133093", "Episode"), None);
        assert_eq!(external_url("imdb", "", "Movie"), None);
        assert_eq!(external_url("trakt", "12345", "Movie"), None);
    }

    /// Ids reach `open_external_link` as a path segment, so anything that is
    /// not a plain token must not produce a URL at all.
    #[test]
    fn external_links_reject_ids_that_are_not_plain_tokens() {
        let too_long = "a".repeat(33);
        for id in ["tt1/../evil", "603?x=1", "603#f", "603 604", &too_long] {
            assert_eq!(external_url("imdb", id, "Movie"), None, "id {id}");
        }
        assert_eq!(external_url("imdb", "603", "Movie"), None);
        assert_eq!(external_url("tmdb", "tt0133093", "Movie"), None);
        assert_eq!(external_url("tmdb", "000", "Movie"), None);
    }

    #[test]
    fn media_source_paths_are_reduced_to_a_file_name() {
        assert_eq!(
            file_name_of("/mnt/media/Movies/The Matrix (1999)/matrix.mkv"),
            Some("matrix.mkv")
        );
        assert_eq!(
            file_name_of(r"D:\Media\Movies\matrix.mkv"),
            Some("matrix.mkv")
        );
    }

    /// Non-admin users get an opaque id in `Path`, which must not be printed as
    /// if it were the name of a file.
    #[test]
    fn media_source_paths_that_are_not_file_names_are_dropped() {
        assert_eq!(file_name_of("6967c5ef-2daf-4951-9d03-38db9a7f5351"), None);
        assert_eq!(file_name_of("/mnt/media/Movies/The Matrix (1999)"), None);
        assert_eq!(file_name_of(""), None);
    }

    #[test]
    fn media_sources_are_flattened_into_video_audio_and_subtitle_lists() {
        let source: MediaSourceInfo = serde_json::from_str(
            r#"{"Id":"src","Name":"matrix","Container":"mkv","Size":123,"Bitrate":456,
                "Path":"/mnt/matrix.mkv",
                "MediaStreams":[
                    {"Index":0,"Type":"Video","Codec":"hevc","Width":3840,"Height":2160},
                    {"Index":1,"Type":"Audio","Codec":"dts","Channels":6,"IsDefault":true},
                    {"Index":2,"Type":"Subtitle","Codec":"subrip","Language":"eng",
                     "IsHearingImpaired":true,"IsExternal":true}]}"#,
        )
        .expect("source");
        let value = media_source_json(&source);
        assert_eq!(value["container"], "mkv");
        assert_eq!(value["fileName"], "matrix.mkv");
        assert_eq!(value["video"][0]["height"], 2160);
        assert_eq!(value["audio"][0]["channels"], 6);
        assert_eq!(value["subtitles"][0]["isExternal"], true);
        assert_eq!(value["subtitles"][0]["isHearingImpaired"], true);
        assert_eq!(value["video"].as_array().map(Vec::len), Some(1));
    }
}
