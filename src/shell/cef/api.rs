//! The JSON API and static assets served on `mediaflick-desktop://app/`.
//!
//! Handlers run on a CEF background thread (never the UI or IO thread), so
//! blocking SQLite and HTTP calls are safe here.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::app::services::ShellRequest;
use crate::app::services::{self, Services};
use crate::app::urls::{encode_path_segment, percent_decode, query_param};
use crate::integrations::letterboxd as letterboxd_integration;
use crate::integrations::letterboxd::ExternalProfile;
use crate::jellyfin::api::items;
use crate::jellyfin::api::model::{BaseItemDto, BaseItemPerson, MediaSourceInfo, MediaStream};
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::play::{self, PlayOptions};
use crate::jellyfin::session::SessionScope;
use crate::library::model::technical_media_streams_json;
use crate::library::{
    ItemPlaybackPreference, ItemQuery, ItemSort, Library, resolve_playback_preference, sync,
};
use crate::maintenance::player_setup;
use crate::preferences::{
    AccountKey, AppSettings, AppearanceSettingsPatch, ApplicationSettingsPatch, HomeBuiltIn,
    HomeElement, HomeElementId, HomeSettings, PlaybackSettingsPatch, PlayerSettingsPatch,
    StreamingQuality,
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
    /// Decodes the JSON body into the endpoint's request type. A request sent
    /// without a body decodes like `{}`, so it is accepted exactly when every
    /// field of `T` is optional. A body that is not JSON, or does not match
    /// `T`, is rejected rather than read as defaults: a garbled "mark as
    /// unwatched" must not mark the item watched.
    fn body<T: DeserializeOwned>(&self) -> Result<T, ApiResponse> {
        let body: &[u8] = if self.body.iter().all(u8::is_ascii_whitespace) {
            b"{}"
        } else {
            &self.body
        };
        serde_json::from_slice(body)
            .map_err(|error| ApiResponse::error(400, format!("invalid request body: {error}")))
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
    fn json(status: u16, value: impl Serialize) -> Self {
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

    fn ok(value: impl Serialize) -> Self {
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

/// A failed Jellyfin call, reported against the account that made it.
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
    dispatch(request, services::init)
}

/// [`handle`] with the service lookup injected, so tests route requests
/// against their own services instead of the user's data folder.
fn dispatch(request: &ApiRequest, services: impl FnOnce() -> Option<Arc<Services>>) -> ApiResponse {
    if let Some(response) = assets::static_asset(&request.path) {
        return response;
    }
    let Some(api_path) = request.path.strip_prefix("/api/") else {
        // Unknown non-API paths fall back to the shell so client-side routing
        // survives a reload.
        return assets::index_html();
    };

    let Some(services) = services() else {
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
        ["startup"] if request.is("GET") => startup(services, request),
        ["companion", "info"] if request.is("GET") => companion_info(services, false),
        ["companion", "probe"] if request.is("POST") => companion_info(services, true),
        _ => return None,
    };
    Some(response)
}

fn status(services: &Arc<Services>) -> ApiResponse {
    let mut status = services.session.status();
    let stats = services.library.stats();
    let progress = services
        .sync
        .progress(sync::bootstrap_progress(&services.library));
    let bootstrap = &progress.catalog;
    if let Some(object) = status.as_object_mut() {
        object.insert("library".to_string(), json!(stats));
        object.insert("syncing".to_string(), json!(services.sync.is_running()));
        object.insert(
            "lastSync".to_string(),
            json!(services.library.meta("sync.completed_at").ok().flatten()),
        );
        object.insert("bootstrapped".to_string(), json!(bootstrap.complete));
        object.insert("libraryReady".to_string(), json!(bootstrap.ready));
        object.insert("bootstrap".to_string(), json!(bootstrap));
        object.insert("syncProgress".to_string(), json!(progress));
        object.insert("companion".to_string(), services.companion.status());
    }
    ApiResponse::ok(status)
}

/// Everything the first frame reads, in one request. Without it the UI asks
/// for status, then waits for the answer before it can ask for the account's
/// preferences and Home. Each part is answered by its own route; a part that
/// does not succeed is `null`, so the UI requests it separately and surfaces
/// its error there. `home` adds the local Home and billboard once the catalog
/// is ready, for a launch that opens on Home.
fn startup(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let part = |path: &str| {
        let response = route(
            services,
            path,
            &ApiRequest {
                method: "GET".to_string(),
                path: format!("/api/{path}"),
                query: String::new(),
                body: Vec::new(),
                range: None,
                cancelled: request.cancelled.clone(),
            },
        );
        if response.status != 200 {
            return Value::Null;
        }
        serde_json::from_slice(&response.body).unwrap_or(Value::Null)
    };
    let status = part("status");
    let authenticated = status["authenticated"].as_bool() == Some(true);
    let home = authenticated
        && status["libraryReady"].as_bool() == Some(true)
        && request.param("home").as_deref() == Some("1");
    let account_part = |path: &str| {
        if authenticated {
            part(path)
        } else {
            Value::Null
        }
    };
    let home_part = |path: &str| if home { part(path) } else { Value::Null };
    ApiResponse::ok(json!({
        "settings": part("settings"),
        "viewing": account_part("settings/viewing"),
        "browsing": account_part("settings/browsing"),
        "home": home_part("home"),
        "billboard": home_part("billboard"),
        "status": status,
    }))
}

fn companion_info(services: &Arc<Services>, force: bool) -> ApiResponse {
    match services.companion.probe(force) {
        Ok(_) => ApiResponse::ok(services.companion.status()),
        Err(error) => ApiResponse::from_api_error(&error),
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
/// replacement (Jellyfin re-creates the item with a new id) is picked up. The
/// 404 is only proof for the account that received it.
fn forget_item(services: &Arc<Services>, scope: &SessionScope, item_id: &str) {
    let forgotten =
        services
            .session
            .commit_if_current(scope, || (), || Ok(services.library.forget(item_id)));
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
    use crate::app::services::test_support::TestServices;

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

    fn post(path: &str, body: &[u8]) -> ApiRequest {
        ApiRequest {
            method: "POST".to_string(),
            path: path.to_string(),
            query: String::new(),
            body: body.to_vec(),
            range: None,
            cancelled: Default::default(),
        }
    }

    fn send(fixture: &TestServices, request: &ApiRequest) -> ApiResponse {
        dispatch(request, || Some(fixture.services.clone()))
    }

    fn error_of(response: &ApiResponse) -> String {
        let body: Value = serde_json::from_slice(&response.body).expect("json error");
        body["error"].as_str().unwrap_or_default().to_string()
    }

    /// A Jellyfin address that accepts connections but never answers, so a
    /// test can prove that no request was sent to it.
    fn silent_server() -> std::net::TcpListener {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        listener.set_nonblocking(true).expect("nonblocking");
        listener
    }

    fn server_url(listener: &std::net::TcpListener) -> String {
        format!("http://{}", listener.local_addr().expect("address"))
    }

    #[test]
    fn an_empty_body_decodes_like_an_empty_object() {
        #[derive(Debug, Deserialize)]
        struct Optional {
            #[serde(default)]
            flag: bool,
        }
        #[derive(Debug, Deserialize)]
        struct Required {
            #[expect(dead_code, reason = "decoding is what the test observes")]
            flag: bool,
        }
        for body in [&b""[..], b"  \r\n"] {
            let request = post("/api/test", body);
            assert!(!request.body::<Optional>().expect("defaults").flag);
            assert_eq!(request.body::<Required>().expect_err("missing").status, 400);
        }
        let garbled = post("/api/test", b"not json");
        let response = garbled.body::<Optional>().expect_err("not json");
        assert_eq!(response.status, 400);
        assert!(error_of(&response).starts_with("invalid request body"));
    }

    #[test]
    fn malformed_bodies_are_rejected_before_anything_reaches_jellyfin() {
        let server = silent_server();
        let fixture = TestServices::signed_in(&server_url(&server));
        let cases: [(&str, &[u8]); 10] = [
            // A garbled "mark unwatched" used to decode as `{}` and mark the
            // item watched.
            ("/api/item/item-1/played", b"not json"),
            ("/api/item/item-1/played", b"{}"),
            ("/api/item/item-1/played", br#"{"played":"false"}"#),
            ("/api/item/item-1/favorite", b"true"),
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
            &post("/api/item/item-1/played", br#"{"played":false}"#),
        );
        assert_eq!(response.status, 200, "{}", error_of(&response));
        let body: Value = serde_json::from_slice(&response.body).expect("json");
        assert_eq!(body, json!({ "played": false }));
        let request_line = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("Jellyfin request");
        assert!(
            request_line.starts_with("DELETE /UserPlayedItems/item-1"),
            "{request_line}"
        );
    }

    /// One request read to its end: the request line, then its body, so the
    /// client never sees a reset from unread bytes.
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
            .ingest_page(&[dto])
            .expect("ingest");
    }

    #[test]
    fn a_watch_state_answer_after_sign_out_leaves_the_kept_cache_alone() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let fixture = TestServices::signed_in(&server_url(&listener));
        movie_row(&fixture, "m1");
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
            dispatch(&post("/api/item/m1/played", br#"{"played":true}"#), || {
                Some(services.clone())
            })
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
                .starts_with("POST /UserPlayedItems/m1")
        );
        let row = fixture
            .services
            .library
            .item("m1")
            .expect("item")
            .expect("kept row");
        assert_eq!(row["played"], false);
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
                ..post("/api/item/m1/trailer", b"")
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
            stale_line.starts_with("GET /Items/m1/LocalTrailers"),
            "{stale_line}"
        );
        assert!(
            login_line.starts_with("POST /Users/AuthenticateByName"),
            "{login_line}"
        );
        let session = &fixture.services.session;
        assert!(session.is_authenticated());
        assert_eq!(session.user_id().as_deref(), Some("bob"));
        assert_eq!(session.status()["expired"], false);
    }

    #[test]
    fn player_commands_are_decoded_before_the_player_is_asked() {
        let fixture = TestServices::signed_out();
        let command = |body: &[u8]| send(&fixture, &post("/api/player/command", body));
        // Decoded and valid: only the missing playback coordinator stops it.
        let response = command(br#"{"command":"set-mute","mute":true}"#);
        assert_eq!(response.status, 503, "{}", error_of(&response));
        let response = command(br#"{"command":"set-subtitle-track","subtitleTrack":null}"#);
        assert_eq!(response.status, 503, "{}", error_of(&response));
        // Decoded, but not a value the player can apply.
        let response = command(br#"{"command":"set-playback-rate","rate":0}"#);
        assert_eq!(response.status, 400);
        assert_eq!(error_of(&response), "unsupported player command");
    }

    #[test]
    fn the_player_state_is_a_full_idle_snapshot_before_playback_starts() {
        let fixture = TestServices::signed_out();
        let request = ApiRequest {
            method: "GET".to_string(),
            ..post("/api/player/state", b"")
        };
        let response = send(&fixture, &request);
        assert_eq!(response.status, 200);
        let body: Value = serde_json::from_slice(&response.body).expect("json");
        assert_eq!(body["active"], false);
        assert_eq!(body["tracks"], json!([]));
        assert_eq!(body["diagnostics"]["buffering"], false);
        assert!(body.get("capabilities").is_none());
    }

    #[test]
    fn a_request_without_a_body_takes_the_endpoint_defaults() {
        let fixture = TestServices::signed_out();
        let response = send(&fixture, &post("/api/auth/logout", b""));
        assert_eq!(response.status, 200, "{}", error_of(&response));
        let response = send(
            &fixture,
            &post("/api/auth/logout", br#"{"forgetLibrary":1}"#),
        );
        assert_eq!(response.status, 400);
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
