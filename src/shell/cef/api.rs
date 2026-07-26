//! The JSON API and static assets served on `mediaflick-desktop://app/`.
//!
//! Handlers run on a CEF background thread (never the UI or IO thread), so
//! blocking SQLite and HTTP calls are safe here.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

use crate::app::services::{self, Services};
use crate::app::urls::{percent_decode, query_param};
use crate::jellyfin::api::items;
use crate::jellyfin::api::model::BaseItemDto;
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::play::{self, PlayOptions};
use crate::library::{ItemQuery, ItemSort};
use crate::preferences::{AppSettings, StreamingQuality};

/// Rows shown on the home screen.
const HOME_ROW_LIMIT: i64 = 24;
/// Posters are content-addressed by image tag, so they never go stale.
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";
const NO_STORE: &str = "no-store";

#[derive(Debug, Clone)]
pub struct ApiRequest {
    pub method: String,
    pub path: String,
    pub query: String,
    pub body: Vec<u8>,
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
}

#[derive(Debug, Clone)]
pub struct ApiResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub cache_control: &'static str,
}

impl ApiResponse {
    fn json(status: u16, value: Value) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8".to_string(),
            body: serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec()),
            cache_control: NO_STORE,
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
        }
    }

    fn bytes(content_type: String, body: Vec<u8>, cache_control: &'static str) -> Self {
        Self {
            status: 200,
            content_type,
            body,
            cache_control,
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
    match segments.as_slice() {
        ["status"] => status(services),
        ["settings"] if request.is("GET") => client_settings(),
        ["auth", "connect"] if request.is("POST") => auth_connect(services, request),
        ["auth", "login"] if request.is("POST") => auth_login(services, request),
        ["auth", "quickconnect", "start"] if request.is("POST") => {
            quick_connect_start(services, request)
        }
        ["auth", "quickconnect", "poll"] if request.is("POST") => {
            quick_connect_poll(services, request)
        }
        ["auth", "logout"] if request.is("POST") => auth_logout(services, request),
        ["home"] if request.is("GET") => home(services),
        ["items"] if request.is("GET") => query_items(services, request),
        ["genres"] if request.is("GET") => match services.library.genres() {
            Ok(genres) => ApiResponse::ok(json!({ "genres": genres })),
            Err(error) => storage_failure(&error),
        },
        ["item", id] if request.is("GET") => item_detail(services, &percent_decode(id)),
        ["item", id, "children"] if request.is("GET") => children(services, &percent_decode(id)),
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
        ["play"] if request.is("POST") => play_item(services, request),
        ["play", "next"] if request.is("POST") => play_next(services, request),
        ["player", "state"] if request.is("GET") => player_state(services),
        ["player", "command"] if request.is("POST") => player_command(services, request),
        ["sync"] if request.is("POST") => {
            services.sync.request();
            ApiResponse::ok(json!({ "requested": true }))
        }
        _ => ApiResponse::error(404, format!("unknown endpoint /api/{path}")),
    }
}

// ------------------------------------------------------------------- session

fn status(services: &Arc<Services>) -> ApiResponse {
    let mut status = services.session.status();
    let stats = services.library.stats();
    if let Some(object) = status.as_object_mut() {
        object.insert("library".to_string(), json!(stats));
        object.insert("syncing".to_string(), json!(services.sync.is_running()));
        object.insert(
            "lastSync".to_string(),
            json!(services.library.meta("sync.completed_at")),
        );
        object.insert(
            "bootstrapped".to_string(),
            json!(services.library.meta("sync.bootstrap_done").as_deref() == Some("1")),
        );
    }
    ApiResponse::ok(status)
}

fn client_settings() -> ApiResponse {
    let settings = AppSettings::load();
    ApiResponse::ok(json!({
        "streamingQuality": settings.streaming_quality.as_str(),
        "playerBackend": settings.effective_backend().as_str(),
        "playerConfigured": settings.player_path().is_some(),
        "serverUrl": settings.jellyfin_url,
    }))
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
    status(services)
}

// ------------------------------------------------------------------ browsing

fn home(services: &Arc<Services>) -> ApiResponse {
    let library = &services.library;
    let resume = library
        .continue_watching(HOME_ROW_LIMIT)
        .unwrap_or_default();
    let recent = library.recently_added(HOME_ROW_LIMIT).unwrap_or_default();
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
        "rows": [
            { "id": "resume", "title": "Continue Watching", "items": resume },
            { "id": "nextUp", "title": "Next Up", "items": next_up },
            { "id": "recent", "title": "Recently Added", "items": recent },
        ],
    }))
}

fn query_items(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
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

fn item_detail(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    match services.library.item(item_id) {
        Ok(Some(item)) => ApiResponse::ok(item),
        // A deep link can outrun the sync; fetch that one item and cache it.
        Ok(None) => fetch_and_cache_item(services, item_id),
        Err(error) => storage_failure(&error),
    }
}

fn fetch_and_cache_item(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_item(&client, &user_id, item_id) {
        Ok(Some(dto)) => {
            let _ = services.library.upsert_page(std::slice::from_ref(&dto));
            match services.library.item(item_id) {
                Ok(Some(item)) => ApiResponse::ok(item),
                Ok(None) => ApiResponse::ok(summary_from_dto(&dto)),
                Err(error) => storage_failure(&error),
            }
        }
        Ok(None) => ApiResponse::error(404, "the server has no item with that id"),
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn children(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    match services.library.children(item_id) {
        Ok(children) => ApiResponse::ok(json!({ "items": children })),
        Err(error) => storage_failure(&error),
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

    let (client, _) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let mut query = Vec::new();
    if !tag.is_empty() {
        query.push(("tag", tag.clone()));
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
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
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
    let settings = AppSettings::load();
    let Some(player_path) = settings.player_path().map(str::to_string) else {
        return ApiResponse::error(
            409,
            "No media player is configured. Open Client Settings to set up mpv or MPC-HC.",
        );
    };
    let Some(playback) = services.playback() else {
        return ApiResponse::error(503, "the playback coordinator is not ready yet");
    };

    let prepared = match play::prepare(
        &services.session,
        &services.library,
        settings.streaming_quality,
        options,
    ) {
        Ok(prepared) => prepared,
        Err(error) => return ApiResponse::from_api_error(&error),
    };

    tracing::info!(
        target: "app.api",
        item_id = %options.item_id,
        play_method = %prepared.play_method,
        media_source = %prepared.media_source_name,
        headers = %crate::app::logger::redacted_header_summary(&prepared.request.headers),
        launch = %crate::app::logger::launch_summary(&prepared.request),
        "starting playback from the own UI"
    );
    playback.open(
        player_path,
        settings.default_fullscreen,
        prepared.request.clone(),
    );
    playback.update_context(playback_context(&prepared.request));

    ApiResponse::ok(json!({
        "started": true,
        "itemId": options.item_id,
        "playMethod": prepared.play_method,
        "mediaSource": prepared.media_source_name,
        "startTicks": prepared.request.start_time_ticks.unwrap_or(0),
    }))
}

/// Feeds the player adapters the same identity the launch carries so their
/// pending playback is complete before the file loads.
fn playback_context(
    request: &crate::playback::PlaybackRequest,
) -> crate::playback::PlaybackContext {
    crate::playback::PlaybackContext {
        media_url: Some(request.media_url.clone()),
        item_id: request.item_id.clone(),
        media_source_id: request.media_source_id.clone(),
        play_session_id: request.play_session_id.clone(),
        device_id: request.device_id.clone(),
        start_time_ticks: request.start_time_ticks,
        runtime_ticks: request.runtime_ticks,
        title: request.title.clone(),
        audio_stream_index: request.audio_stream_index,
        subtitle_stream_index: request.subtitle_stream_index,
        play_method: request.play_method.clone(),
        ..Default::default()
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

fn static_asset(path: &str) -> Option<ApiResponse> {
    match path {
        "" | "/" | "/index.html" => Some(index_html()),
        "/app.js" => Some(ApiResponse::asset(
            "text/javascript; charset=utf-8",
            include_bytes!("../ui/app/app.js"),
            NO_STORE,
        )),
        "/app.css" => Some(ApiResponse::asset(
            "text/css; charset=utf-8",
            include_bytes!("../ui/app/app.css"),
            NO_STORE,
        )),
        _ => None,
    }
}

fn index_html() -> ApiResponse {
    ApiResponse::asset(
        "text/html; charset=utf-8",
        include_bytes!("../ui/app/index.html"),
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

fn storage_failure(error: &rusqlite::Error) -> ApiResponse {
    tracing::warn!(target: "app.api", "library query failed: {error}");
    ApiResponse::error(500, format!("library query failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{ApiRequest, ApiResponse, cache_key, handle, mime_for_image, summary_from_dto};
    use crate::jellyfin::api::model::BaseItemDto;
    use serde_json::json;

    fn get(path: &str) -> ApiResponse {
        handle(&ApiRequest {
            method: "GET".to_string(),
            path: path.to_string(),
            query: String::new(),
            body: Vec::new(),
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
        };
        assert_eq!(request.param("search").as_deref(), Some("the matrix"));
        assert_eq!(request.param("genre"), None);
        assert_eq!(request.param("limit").as_deref(), Some("20"));
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
    fn next_up_entries_are_shaped_like_cached_rows() {
        let dto: BaseItemDto = serde_json::from_str(
            r#"{"Id":"e1","Name":"Half Loop","Type":"Episode","SeriesName":"Severance",
                "IndexNumber":2,"ParentIndexNumber":1,
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
