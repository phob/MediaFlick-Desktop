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
use crate::integrations::letterboxd as letterboxd_integration;
use crate::integrations::letterboxd::ExternalProfile;
use crate::jellyfin::api::items;
use crate::jellyfin::api::model::{BaseItemDto, BaseItemPerson, MediaSourceInfo, MediaStream};
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::play::{self, PlayOptions};
use crate::library::model::technical_media_streams_json;
use crate::library::{
    ItemPlaybackPreference, ItemQuery, ItemSort, Library, resolve_playback_preference, sync,
};
use crate::maintenance::player_setup;
use crate::players::mpv::input::MpvInputBindings;
use crate::preferences::{
    AccountKey, AppSettings, AppearanceSettingsPatch, ApplicationSettingsPatch,
    PlaybackSettingsPatch, PlayerSettingsPatch, StreamingQuality,
};
use crate::seerr::{DiscoverKind, DiscoverOptions, RequestProfileSelection, tmdb_image_path};

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
    if let Some(response) = assets::static_asset(&request.path) {
        return response;
    }
    let Some(api_path) = request.path.strip_prefix("/api/") else {
        // Unknown non-API paths fall back to the shell so client-side routing
        // survives a reload.
        return assets::index_html();
    };

    let Some(services) = services::init() else {
        return ApiResponse::error(
            503,
            services::init_error().unwrap_or("the library database is unavailable"),
        );
    };
    route(&services, api_path, request)
}

mod assets;
mod auth;
mod catalog;
mod collections;
mod images;
mod letterboxd;
mod media;
mod playback;
mod ratings;
mod seerr;
mod settings;
mod shell;

fn route(services: &Arc<Services>, path: &str, request: &ApiRequest) -> ApiResponse {
    let segments = path.split('/').collect::<Vec<_>>();
    route_status(services, &segments, request)
        .or_else(|| settings::route(services, &segments, request))
        .or_else(|| ratings::route(services, &segments, request))
        .or_else(|| collections::route(services, &segments, request))
        .or_else(|| shell::route(services, &segments, request))
        .or_else(|| letterboxd::route(services, &segments, request))
        .or_else(|| auth::route(services, &segments, request))
        .or_else(|| seerr::route(services, &segments, request))
        .or_else(|| catalog::route(services, &segments, request))
        .or_else(|| media::route(services, &segments, request))
        .or_else(|| images::route(services, &segments, request))
        .or_else(|| playback::route(services, &segments, request))
        .unwrap_or_else(|| ApiResponse::error(404, format!("unknown endpoint /api/{path}")))
}

fn route_status(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["status"] => status(services),
        ["companion", "info"] if request.is("GET") => companion_info(services, false),
        ["companion", "probe"] if request.is("POST") => companion_info(services, true),
        _ => return None,
    };
    Some(response)
}

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

fn companion_info(services: &Arc<Services>, force: bool) -> ApiResponse {
    match services.companion.probe(force) {
        Ok(_) => ApiResponse::ok(services.companion.status()),
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn page_param(request: &ApiRequest) -> i64 {
    request
        .param("page")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

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
    use super::*;

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
}
