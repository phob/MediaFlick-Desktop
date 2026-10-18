//! The JSON API and static assets served on `mediaflick-desktop://app/`.
//!
//! Handlers run on a CEF background thread (never the UI or IO thread), so
//! blocking SQLite and HTTP calls are safe here.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::app::ids::valid_opaque_id;
use crate::app::services::Services;
use crate::app::services::ShellRequest;
use crate::app::urls::{encode_path_segment, percent_decode, query_param};
use crate::companion::CompanionStatus;
use crate::integrations::letterboxd as letterboxd_integration;
use crate::integrations::letterboxd::ExternalProfile;
use crate::jellyfin::api::items;
use crate::jellyfin::api::model::{BaseItemDto, BaseItemPerson, MediaSourceInfo, MediaStream};
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::play::{self, PlayOptions};
use crate::jellyfin::session::{SessionScope, SessionStatus};
use crate::library::sync::{Bootstrap, SyncProgress};
use crate::library::{
    CatalogKind, ItemPlaybackPreference, ItemQuery, ItemSort, ItemSummary, KindFilter, Library,
    LibraryStats, SeriesStatus, SortDirection, WatchFilter, resolve_playback_preference, sync,
};
use crate::maintenance::player_setup;
use crate::preferences::{
    AccountKey, AppSettings, AppearanceSettingsPatch, ApplicationSettingsPatch, HomeBuiltIn,
    HomeElement, HomeElementId, HomeSettings, PlaybackSettingsPatch, PlayerSettingsPatch,
    StreamingQuality,
};
use crate::seerr::{DiscoverKind, DiscoverOptions, RequestProfileSelection, tmdb_image_path};
use crate::shell::locator;

const HOME_ROW_LIMIT: i64 = 24;
const BILLBOARD_LIMIT: i64 = 5;
const TRAILER_CHUNK_SIZE: u64 = 4 * 1024 * 1024;
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";
const NO_STORE: &str = "no-store";
const MAX_PERSON_QUERY_PAGES: usize = 100;

#[derive(Debug, Clone)]
pub struct ApiRequest {
    pub method: String,
    pub path: String,
    pub query: String,
    pub body: RawBody,
    pub range: Option<String>,
    /// Raised by CEF's `cancel` callback when the browser abandons the request
    /// (an aborted fetch, a closed page). Handlers block synchronously, so
    /// this flag is how a multi-request handler stops issuing further
    /// upstream calls for an answer nobody will read.
    pub cancelled: Arc<AtomicBool>,
}

impl ApiRequest {
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

/// The signed-in account this request runs against. Its failures go through
/// [`scoped_failure`] and its cache writes through `commit_if_current`, so a
/// response that arrives after an account switch cannot expire or rewrite the
/// account that replaced it.
fn session_scope(services: &Services) -> Result<SessionScope, ApiResponse> {
    services
        .session
        .scope()
        .map_err(|error| ApiResponse::from_api_error(&error))
}

fn scoped_failure(services: &Services, scope: &SessionScope, error: &ApiError) -> ApiResponse {
    services.session.note_scoped_error(scope, error);
    ApiResponse::from_api_error(error)
}

fn stale_account_response() -> ApiResponse {
    ApiResponse::error(
        409,
        "the Jellyfin account changed while the request was running",
    )
}

pub fn handle(request: &ApiRequest) -> ApiResponse {
    dispatch(request, locator::init)
}

fn dispatch(request: &ApiRequest, services: impl FnOnce() -> Option<Arc<Services>>) -> ApiResponse {
    if let Some(response) = assets::static_asset(&request.path) {
        return response;
    }
    let Some(api_path) = request.path.strip_prefix("/api/") else {
        return assets::index_html();
    };

    let Some(services) = services() else {
        return ApiResponse::error(
            503,
            locator::init_error().unwrap_or("the library database is unavailable"),
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
mod libraries;
mod media;
mod playback;
mod ratings;
mod routes;
mod seerr;
mod settings;
mod shell;
mod wire;

pub use routes::RawBody;
use routes::{Binary, BinaryAnswer, Body, Bytes, Relay, routes};
pub(super) use wire::Wire;

/// Every module's routes, in the order a request tries them. Each module
/// declares its own with [`routes::routes!`]; their tables also answer which
/// methods a known path accepts, for a 405 and its `Allow` header.
const MODULES: &[&routes::Routes] = &[
    &ROUTES,
    &settings::ROUTES,
    &ratings::ROUTES,
    &collections::ROUTES,
    &shell::ROUTES,
    &letterboxd::ROUTES,
    &auth::ROUTES,
    &seerr::ROUTES,
    &catalog::ROUTES,
    &libraries::ROUTES,
    &media::ROUTES,
    &images::ROUTES,
    &playback::ROUTES,
];

fn route(services: &Arc<Services>, path: &str, request: &ApiRequest) -> ApiResponse {
    let item_id: String;
    let mut segments = path.split('/').collect::<Vec<_>>();
    if let ["item", id, ..] = segments.as_slice() {
        item_id = percent_decode(id);
        if !valid_opaque_id(&item_id) {
            return ApiResponse::error(400, "invalid item id");
        }
        segments[1] = &item_id;
    }
    MODULES
        .iter()
        .find_map(|routes| (routes.serve)(services, &segments, request))
        .unwrap_or_else(|| unrouted(path, &segments))
}

fn unrouted(path: &str, segments: &[&str]) -> ApiResponse {
    let allowed = MODULES
        .iter()
        .flat_map(|routes| routes.table)
        .filter(|route| route.matches(segments))
        .map(|route| route.method)
        .collect::<Vec<_>>();
    if allowed.is_empty() {
        return ApiResponse::error(404, format!("unknown endpoint /api/{path}"));
    }
    let mut response = ApiResponse::error(405, format!("/api/{path} does not accept this method"));
    response
        .headers
        .push(("Allow".to_string(), allowed.join(", ")));
    response
}

routes! {
    |services, request|
    GET ["status"] -> AppStatus = Ok(app_status(services)),
    GET ["startup"] -> StartupResponse = Ok(startup(services, request)),
    GET ["companion", "info"] -> CompanionStatus = companion_info(services, false),
    POST ["companion", "probe"] -> CompanionStatus = companion_info(services, true),
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct AppStatus {
    #[serde(flatten)]
    session: SessionStatus,
    library: LibraryStats,
    syncing: bool,
    last_sync: Option<String>,
    bootstrap: Bootstrap,
    sync_progress: SyncProgress,
    companion: CompanionStatus,
}

fn app_status(services: &Services) -> AppStatus {
    let sync = services.sync.status(
        sync::bootstrap_progress(&services.library),
        services.session.generation(),
    );
    AppStatus {
        session: services.session.status(),
        library: services.library.stats(),
        syncing: services.sync.is_running(),
        last_sync: services.library.meta("sync.completed_at").ok().flatten(),
        bootstrap: sync.bootstrap,
        sync_progress: sync.progress,
        companion: services.companion.status(),
    }
}

/// `/api/startup`: everything the first frame reads, in one request. Without
/// it the UI asks for status, then waits for the answer before it can ask for
/// the account's preferences and Home. Each part is what its own route
/// answers; a part its route would answer with an error is `null`, so the UI
/// requests it separately and surfaces its error there. `home` adds the local
/// Home and billboard once the catalog is ready, for a launch that opens on
/// Home.
#[derive(Serialize, ts_rs::TS)]
struct StartupResponse {
    status: AppStatus,
    settings: settings::SettingsView,
    viewing: Option<settings::ViewingView>,
    browsing: Option<settings::BrowsingRoutes>,
    home: Option<catalog::HomeResponse>,
    billboard: Option<catalog::Billboard>,
}

fn startup(services: &Arc<Services>, request: &ApiRequest) -> StartupResponse {
    let status = app_status(services);
    let authenticated = status.session.authenticated;
    let home = authenticated
        && status.bootstrap == Bootstrap::Ready
        && request.param("home").as_deref() == Some("1");
    StartupResponse {
        settings: settings::settings_snapshot(services),
        viewing: startup_part(authenticated, || settings::viewing(services)),
        browsing: startup_part(authenticated, || settings::browsing(services)),
        home: startup_part(home, || catalog::home_view(services)),
        billboard: startup_part(home, || catalog::billboard_view(services)),
        status,
    }
}

fn startup_part<T>(wanted: bool, answer: impl FnOnce() -> Result<T, ApiResponse>) -> Option<T> {
    if wanted { answer().ok() } else { None }
}

fn companion_info(services: &Arc<Services>, force: bool) -> Result<CompanionStatus, ApiResponse> {
    match services.companion.probe(force) {
        Ok(_) => Ok(services.companion.status()),
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn page_param(request: &ApiRequest) -> i64 {
    request
        .param("page")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

/// Evicts a cached item the server has disowned, and asks for a sync so the
/// replacement (Jellyfin re-creates the item with a new id) is picked up. The
/// 404 is only proof for the account that received it.
fn forget_item(services: &Arc<Services>, scope: &SessionScope, item_id: &str) {
    let forgotten = services.session.commit_if_current(
        scope,
        || (),
        |live| Ok(services.library.forget(live, item_id)),
    );
    let Ok(forgotten) = forgotten else {
        tracing::debug!(
            target: "app.api",
            item_id,
            "kept a cached item: the account changed before the server disowned it"
        );
        return;
    };
    match forgotten {
        Ok(changes) if !changes.is_empty() => {
            tracing::info!(
                target: "app.api",
                item_id,
                "dropped a cached item the server no longer has"
            );
            services.shell.library_changed(changes);
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
    use crate::app::services::test_support::TestServices;

    fn get(path: &str) -> ApiResponse {
        handle(&ApiRequest {
            method: "GET".to_string(),
            path: path.to_string(),
            query: String::new(),
            body: RawBody::new(Vec::new()),
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

    fn post(path: &str, body: &[u8]) -> ApiRequest {
        ApiRequest {
            method: "POST".to_string(),
            path: path.to_string(),
            query: String::new(),
            body: RawBody::new(body.to_vec()),
            range: None,
            cancelled: Default::default(),
        }
    }

    fn send(fixture: &TestServices, request: &ApiRequest) -> ApiResponse {
        dispatch(request, || Some(fixture.services.clone()))
    }

    #[test]
    fn status_sends_every_field_the_ui_type_declares() {
        let fixture = TestServices::signed_out();
        let response = send(
            &fixture,
            &ApiRequest {
                method: "GET".to_string(),
                path: "/api/status".to_string(),
                query: String::new(),
                body: RawBody::new(Vec::new()),
                range: None,
                cancelled: Default::default(),
            },
        );
        let body: Value = serde_json::from_slice(&response.body).expect("status json");
        for key in [
            "authenticated",
            "bootstrap",
            "companion",
            "deviceId",
            "expired",
            "lastSync",
            "library",
            "serverName",
            "serverUrl",
            "syncProgress",
            "syncing",
            "userId",
            "userName",
        ] {
            assert!(body.get(key).is_some(), "{key}");
        }
        assert_eq!(body["authenticated"], false);
    }

    fn error_of(response: &ApiResponse) -> String {
        let body: Value = serde_json::from_slice(&response.body).expect("json error");
        body["error"].as_str().unwrap_or_default().to_string()
    }

    fn silent_server() -> std::net::TcpListener {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        listener.set_nonblocking(true).expect("nonblocking");
        listener
    }

    fn server_url(listener: &std::net::TcpListener) -> String {
        format!("http://{}", listener.local_addr().expect("address"))
    }

    #[test]
    fn malformed_bodies_are_rejected_before_anything_reaches_jellyfin() {
        let server = silent_server();
        let fixture = TestServices::signed_in(&server_url(&server));
        let cases: [(&str, &[u8]); 10] = [
            (
                "/api/item/0a1b2c3d4e5f60718293a4b5c6d7e8f9/played",
                b"not json",
            ),
            ("/api/item/0a1b2c3d4e5f60718293a4b5c6d7e8f9/played", b"{}"),
            (
                "/api/item/0a1b2c3d4e5f60718293a4b5c6d7e8f9/played",
                br#"{"played":"false"}"#,
            ),
            (
                "/api/item/0a1b2c3d4e5f60718293a4b5c6d7e8f9/favorite",
                b"true",
            ),
            ("/api/play", br#"{"resume":true}"#),
            ("/api/play", br#"{"itemId":"item-1","quality":"8k"}"#),
            ("/api/auth/connect", b"{"),
            ("/api/player/command", br#"{"command":"set-mute"}"#),
            ("/api/player/command", br#"{"command":"toggle-pause"}"#),
            (
                "/api/player/command",
                br#"{"command":"set-video-aspect","aspect":"5:4"}"#,
            ),
        ];
        for (path, body) in cases {
            let response = send(&fixture, &post(path, body));
            let body = String::from_utf8_lossy(body);
            assert_eq!(response.status, 400, "{path} {body}");
            assert!(
                error_of(&response).starts_with("invalid request body"),
                "{path} {body}: {}",
                error_of(&response)
            );
        }
        assert_eq!(
            server.accept().expect_err("no request").kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn a_well_formed_unwatched_request_clears_the_played_state() {
        use std::io::{BufRead, BufReader, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let fixture = TestServices::signed_in(&server_url(&listener));
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(&mut stream);
            let mut request_line = String::new();
            let _ = reader.read_line(&mut request_line);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if line == "\r\n" => break,
                    Ok(_) => {}
                }
            }
            drop(reader);
            let _ = stream.write_all(
                b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            let _ = sender.send(request_line);
        });

        let response = send(
            &fixture,
            &post(
                "/api/item/0a1b2c3d4e5f60718293a4b5c6d7e8f9/played",
                br#"{"played":false}"#,
            ),
        );
        assert_eq!(response.status, 200, "{}", error_of(&response));
        let body: Value = serde_json::from_slice(&response.body).expect("json");
        assert_eq!(body, json!({ "played": false }));
        let request_line = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("Jellyfin request");
        assert!(
            request_line.starts_with("DELETE /UserPlayedItems/0a1b2c3d4e5f60718293a4b5c6d7e8f9"),
            "{request_line}"
        );
    }

    fn read_request(stream: &mut std::net::TcpStream) -> String {
        use std::io::{BufRead, BufReader, Read};

        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        let _ = reader.read_line(&mut request_line);
        let mut length = 0;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) if line == "\r\n" => break,
                Ok(_) => {
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                }
            }
        }
        let mut body = vec![0; length];
        let _ = reader.read_exact(&mut body);
        request_line
    }

    fn respond(stream: &mut std::net::TcpStream, status: &str, body: &str) {
        use std::io::Write;
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    }

    fn movie_row(fixture: &TestServices, id: &str) {
        let dto = serde_json::from_value(json!({ "Id": id, "Name": "Kept", "Type": "Movie" }))
            .expect("dto");
        fixture
            .services
            .library
            .ingest_page(&crate::jellyfin::session::Live::for_test(), &[dto])
            .expect("ingest");
    }

    #[test]
    fn a_watch_state_answer_after_sign_out_leaves_the_kept_cache_alone() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let fixture = TestServices::signed_in(&server_url(&listener));
        movie_row(&fixture, "f9e8d7c6b5a4938271605f4e3d2c1b0a");
        let (reached_tx, reached_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("write request");
            let request_line = read_request(&mut stream);
            reached_tx.send(()).expect("signal");
            release_rx.recv().expect("release");
            respond(&mut stream, "204 No Content", "");
            request_line
        });
        let services = fixture.services.clone();
        let worker = std::thread::spawn(move || {
            dispatch(
                &post(
                    "/api/item/f9e8d7c6b5a4938271605f4e3d2c1b0a/played",
                    br#"{"played":true}"#,
                ),
                || Some(services.clone()),
            )
        });
        reached_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the write reached the server");
        fixture
            .services
            .session
            .clear_local(false)
            .expect("sign out, keeping the cache");
        release_tx.send(()).expect("release");

        assert_eq!(worker.join().expect("worker").status, 200);
        assert!(
            server
                .join()
                .expect("server")
                .starts_with("POST /UserPlayedItems/f9e8d7c6b5a4938271605f4e3d2c1b0a")
        );
        let row = fixture
            .services
            .library
            .item("f9e8d7c6b5a4938271605f4e3d2c1b0a")
            .expect("item")
            .expect("kept row");
        assert!(!row.summary.played);
    }

    #[test]
    fn a_late_401_from_the_previous_account_does_not_expire_the_next_one() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let url = server_url(&listener);
        let fixture = TestServices::signed_in(&url);
        let (reached_tx, reached_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let (mut stale, _) = listener.accept().expect("stale request");
            let stale_line = read_request(&mut stale);
            reached_tx.send(()).expect("signal");
            let (mut login, _) = listener.accept().expect("login request");
            let login_line = read_request(&mut login);
            respond(
                &mut login,
                "200 OK",
                r#"{"AccessToken":"bob-token","ServerId":"server","User":{"Id":"bob","Name":"Bob","Policy":{"IsAdministrator":true}}}"#,
            );
            release_rx.recv().expect("release");
            respond(&mut stale, "401 Unauthorized", "");
            (stale_line, login_line)
        });
        let services = fixture.services.clone();
        let worker = std::thread::spawn(move || {
            let request = ApiRequest {
                method: "GET".to_string(),
                ..post("/api/item/f9e8d7c6b5a4938271605f4e3d2c1b0a/trailer", b"")
            };
            dispatch(&request, || Some(services.clone()))
        });
        reached_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the stale request reached the server");
        fixture
            .services
            .session
            .login(&url, "bob", "secret")
            .expect("switch to Bob");
        release_tx.send(()).expect("release");

        assert_eq!(worker.join().expect("worker").status, 401);
        let (stale_line, login_line) = server.join().expect("server");
        assert!(
            stale_line.starts_with("GET /Items/f9e8d7c6b5a4938271605f4e3d2c1b0a/LocalTrailers"),
            "{stale_line}"
        );
        assert!(
            login_line.starts_with("POST /Users/AuthenticateByName"),
            "{login_line}"
        );
        let session = &fixture.services.session;
        assert!(session.is_authenticated());
        assert_eq!(session.user_id().as_deref(), Some("bob"));
        assert!(!session.status().expired);
    }

    #[test]
    fn player_commands_are_decoded_before_the_player_is_asked() {
        let fixture = TestServices::signed_out();
        let command = |body: &[u8]| send(&fixture, &post("/api/player/command", body));
        let response = command(br#"{"command":"set-mute","mute":true}"#);
        assert_eq!(response.status, 503, "{}", error_of(&response));
        let response = command(br#"{"command":"set-subtitle-track","subtitleTrack":null}"#);
        assert_eq!(response.status, 503, "{}", error_of(&response));
        let response = command(br#"{"command":"set-subtitle-scale","scale":0}"#);
        assert_eq!(response.status, 400);
    }

    #[test]
    fn a_request_without_a_body_takes_the_endpoint_defaults() {
        let fixture = TestServices::signed_out();
        let response = send(&fixture, &post("/api/auth/logout", b""));
        assert_eq!(response.status, 200, "{}", error_of(&response));
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
