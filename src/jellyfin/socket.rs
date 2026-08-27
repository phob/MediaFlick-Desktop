//! Live Jellyfin server events over the `/socket` WebSocket.
//!
//! One worker thread owns the connection: it idles until credentials exist,
//! answers the server's keep-alive probes, and translates pushed
//! `UserDataChanged` and `LibraryChanged` notifications into the same cache
//! upserts, evictions, and UI invalidations the sync thread produces. The
//! socket is an acceleration layer only — dropped connections lose events, so
//! every successful (re)connect asks the sync worker for one requested cycle
//! to reconcile the gap, and the periodic sweeps remain the backstop.

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::http::header::AUTHORIZATION;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::app::urls::join_url;
use crate::library::sync::SyncHandle;
use crate::library::{Library, LibraryChangeBatch, UserDataRecord};

use super::api::ApiError;
use super::api::items;
use super::api::model::UserItemDataDto;
use super::api::sessions;
use super::remote;
use super::session::Session;

/// How often the worker re-checks for a signed-in session while idle. Cheap —
/// one in-process state read — so sign-in starts the stream promptly.
const IDLE_INTERVAL: Duration = Duration::from_secs(5);
/// TCP connect budget per resolved address.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Socket read/write budget while the TLS and WebSocket handshakes run.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Read timeout once connected. Each expiry is one loop tick: the worker
/// checks the stop flag and the session, and sends a due keep-alive.
const READ_TICK: Duration = Duration::from_secs(5);
/// Ping cadence until the server announces its own timeout via ForceKeepAlive.
const DEFAULT_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
/// Reconnect backoff bounds for an unreachable or refusing server.
const RECONNECT_MIN: Duration = Duration::from_secs(5);
const RECONNECT_MAX: Duration = Duration::from_secs(10 * 60);
/// A connection that lasted this long proves the server healthy, so the next
/// reconnect starts from the minimum delay again.
const STABLE_CONNECTION: Duration = Duration::from_secs(60);
/// Ids per `/Items?ids=` fetch when a LibraryChanged burst names many items.
const FETCH_CHUNK: usize = 100;

const KEEPALIVE_MESSAGE: &str = r#"{"MessageType":"KeepAlive"}"#;

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

struct Signal {
    stopped: Mutex<bool>,
    condvar: Condvar,
}

/// Handle used by the shell to stop the event-stream thread.
#[derive(Clone)]
pub struct SocketHandle {
    signal: Arc<Signal>,
}

impl SocketHandle {
    pub fn stop(&self) {
        if let Ok(mut stopped) = self.signal.stopped.lock() {
            *stopped = true;
        }
        self.signal.condvar.notify_all();
    }

    fn is_stopped(&self) -> bool {
        self.signal
            .stopped
            .lock()
            .map(|stopped| *stopped)
            .unwrap_or(true)
    }

    /// Sleeps for `timeout` unless stopped; returns whether to keep running.
    fn wait(&self, timeout: Duration) -> bool {
        let Ok(stopped) = self.signal.stopped.lock() else {
            return false;
        };
        if *stopped {
            return false;
        }
        let (stopped, _) = self
            .signal
            .condvar
            .wait_timeout(stopped, timeout)
            .unwrap_or_else(|error| error.into_inner());
        !*stopped
    }
}

pub fn spawn(library: Arc<Library>, session: Arc<Session>, sync: SyncHandle) -> SocketHandle {
    let handle = SocketHandle {
        signal: Arc::new(Signal {
            stopped: Mutex::new(false),
            condvar: Condvar::new(),
        }),
    };
    let worker = handle.clone();
    if let Err(error) = thread::Builder::new()
        .name("jellyfin-socket".to_string())
        .spawn(move || run(&library, &session, &sync, &worker))
    {
        tracing::warn!(target: "jellyfin.socket", "failed to start the Jellyfin event thread: {error}");
    }
    handle
}

fn run(library: &Arc<Library>, session: &Arc<Session>, sync: &SyncHandle, handle: &SocketHandle) {
    let mut backoff = RECONNECT_MIN;
    while !handle.is_stopped() {
        if !session.is_authenticated() {
            if !handle.wait(IDLE_INTERVAL) {
                return;
            }
            continue;
        }

        match connect(session) {
            Ok((mut socket, authorization)) => {
                tracing::info!(target: "jellyfin.socket", "listening for Jellyfin server events");
                announce_capabilities(session);
                // Whatever happened while no connection existed was never
                // pushed; one requested cycle reconciles the gap.
                sync.request();
                let connected_at = Instant::now();
                match listen(&mut socket, library, session, sync, handle, &authorization) {
                    Disconnect::Stopped => return,
                    Disconnect::SessionChanged => {
                        tracing::debug!(
                            target: "jellyfin.socket",
                            "the session changed; reconnecting the event stream"
                        );
                    }
                    Disconnect::Closed => {
                        tracing::info!(
                            target: "jellyfin.socket",
                            "the Jellyfin server closed the event stream"
                        );
                    }
                    Disconnect::Failed(reason) => {
                        tracing::debug!(
                            target: "jellyfin.socket",
                            "the Jellyfin event stream dropped: {reason}"
                        );
                    }
                }
                if connected_at.elapsed() >= STABLE_CONNECTION {
                    backoff = RECONNECT_MIN;
                }
            }
            Err(reason) => {
                tracing::debug!(
                    target: "jellyfin.socket",
                    "could not open the Jellyfin event stream: {reason}"
                );
            }
        }

        if !handle.wait(backoff) {
            return;
        }
        backoff = (backoff * 2).min(RECONNECT_MAX);
    }
}

/// Announces media-control capabilities for the freshly connected session on
/// its own thread: the announcement is a bounded HTTP POST that must not hold
/// up event handling, and a failure only means "Play On" menus skip this
/// device until the next reconnect.
fn announce_capabilities(session: &Arc<Session>) {
    let session = session.clone();
    let spawned = thread::Builder::new()
        .name("jellyfin-capabilities".to_string())
        .spawn(move || {
            let Ok(client) = session.client() else {
                return;
            };
            match sessions::announce_capabilities(&client) {
                Ok(()) => {
                    tracing::debug!(
                        target: "jellyfin.socket",
                        "announced remote-control capabilities"
                    );
                }
                Err(error) => {
                    session.note_error(&error);
                    tracing::debug!(
                        target: "jellyfin.socket",
                        "could not announce remote-control capabilities: {error}"
                    );
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(
            target: "jellyfin.socket",
            "failed to start the capabilities announcement thread: {error}"
        );
    }
}

/// Why [`listen`] returned.
enum Disconnect {
    /// The shell asked the worker to exit.
    Stopped,
    /// The signed-in session no longer matches the connected one.
    SessionChanged,
    /// The server ended the connection in an orderly way.
    Closed,
    /// The connection died.
    Failed(String),
}

fn connect(session: &Session) -> Result<(Socket, String), String> {
    let client = session.client().map_err(|error| error.to_string())?;
    let authorization = client.authorization_header();
    let endpoint = Endpoint::parse(client.base_url())
        .ok_or_else(|| format!("unsupported server URL {}", client.base_url()))?;
    let stream = endpoint.open()?;

    let mut request = endpoint
        .socket_url
        .as_str()
        .into_client_request()
        .map_err(|error| error.to_string())?;
    // The token travels in the same MediaBrowser header as every REST call;
    // it must never appear in the URL.
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&authorization).map_err(|error| error.to_string())?,
    );

    let (mut socket, _response) = tungstenite::client_tls(request, stream).map_err(|error| {
        if let tungstenite::handshake::HandshakeError::Failure(tungstenite::Error::Http(response)) =
            &error
            && response.status().as_u16() == 401
        {
            session.note_error(&ApiError::Unauthorized);
        }
        error.to_string()
    })?;
    set_read_timeout(&mut socket, READ_TICK);
    Ok((socket, authorization))
}

fn listen(
    socket: &mut Socket,
    library: &Library,
    session: &Session,
    sync: &SyncHandle,
    handle: &SocketHandle,
    authorization: &str,
) -> Disconnect {
    let mut keepalive_interval = DEFAULT_KEEPALIVE_INTERVAL;
    let mut keepalive_sent = Instant::now();
    loop {
        if handle.is_stopped() {
            let _ = socket.close(None);
            let _ = socket.flush();
            return Disconnect::Stopped;
        }
        // Sign-out and account switches revoke the token this connection
        // authenticated with; keep listening only while it is still current.
        if current_authorization(session).as_deref() != Some(authorization) {
            let _ = socket.close(None);
            let _ = socket.flush();
            return Disconnect::SessionChanged;
        }
        if keepalive_sent.elapsed() >= keepalive_interval {
            if let Err(error) = socket.send(Message::text(KEEPALIVE_MESSAGE)) {
                return Disconnect::Failed(error.to_string());
            }
            keepalive_sent = Instant::now();
        }

        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Some(interval) = handle_message(text.as_str(), library, session, sync) {
                    keepalive_interval = interval;
                    // Answer immediately so the server's lost-connection
                    // timer resets from a known point.
                    if socket.send(Message::text(KEEPALIVE_MESSAGE)).is_err() {
                        return Disconnect::Failed("keep-alive answer failed".to_string());
                    }
                    keepalive_sent = Instant::now();
                }
            }
            Ok(Message::Close(_)) => return Disconnect::Closed,
            Ok(_) => {}
            // The read timeout elapsing is the loop's tick, not a failure.
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Disconnect::Closed;
            }
            Err(error) => return Disconnect::Failed(error.to_string()),
        }
    }
}

/// Applies one server message. Returns a new keep-alive interval when the
/// server announced its timeout via ForceKeepAlive.
fn handle_message(
    text: &str,
    library: &Library,
    session: &Session,
    sync: &SyncHandle,
) -> Option<Duration> {
    match parse_message(text) {
        ServerMessage::KeepAliveInterval(interval) => return Some(interval),
        ServerMessage::UserData(records) => apply_user_data(library, &records),
        ServerMessage::LibraryChanged { changed, removed } => {
            apply_library_change(library, session, sync, &changed, &removed);
        }
        ServerMessage::Play(data) => remote::handle_play(&data),
        ServerMessage::Playstate(data) => remote::handle_playstate(&data),
        ServerMessage::GeneralCommand(data) => remote::handle_general_command(&data),
        ServerMessage::Ignored => {}
    }
    None
}

/// The subset of Jellyfin's WebSocket traffic this client acts on.
#[derive(Debug, Clone, PartialEq)]
enum ServerMessage {
    /// ForceKeepAlive: the server's lost-connection timeout, already halved
    /// into the cadence our pings must keep.
    KeepAliveInterval(Duration),
    /// UserDataChanged: pushed watch-state rows for the signed-in user.
    UserData(Vec<UserDataRecord>),
    /// LibraryChanged: item ids added/updated on the server, and ids removed.
    LibraryChanged {
        changed: Vec<String>,
        removed: Vec<String>,
    },
    /// Play: another client asked this session to start something.
    Play(Value),
    /// Playstate: pause/stop/seek/next from a remote client.
    Playstate(Value),
    /// GeneralCommand: volume and mute from a remote client.
    GeneralCommand(Value),
    /// Anything else — session chatter, keep-alive acks, refresh progress.
    Ignored,
}

fn parse_message(text: &str) -> ServerMessage {
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        return ServerMessage::Ignored;
    };
    let data = &message["Data"];
    match message["MessageType"].as_str().unwrap_or_default() {
        "ForceKeepAlive" => {
            let timeout = data.as_u64().filter(|seconds| *seconds > 0);
            match timeout {
                // Ping at half the announced timeout so one lost frame does
                // not end the connection.
                Some(seconds) => {
                    ServerMessage::KeepAliveInterval(Duration::from_secs((seconds / 2).max(5)))
                }
                None => ServerMessage::Ignored,
            }
        }
        "UserDataChanged" => {
            let records = data["UserDataList"]
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| {
                            let item_id = entry["ItemId"].as_str().map(str::trim)?;
                            if item_id.is_empty() {
                                return None;
                            }
                            let dto =
                                serde_json::from_value::<UserItemDataDto>(entry.clone()).ok()?;
                            Some(UserDataRecord::from_dto(item_id, &dto))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            ServerMessage::UserData(records)
        }
        "LibraryChanged" => {
            let mut changed = id_list(data, "ItemsAdded");
            changed.extend(id_list(data, "ItemsUpdated"));
            changed.sort_unstable();
            changed.dedup();
            ServerMessage::LibraryChanged {
                changed,
                removed: id_list(data, "ItemsRemoved"),
            }
        }
        "Play" => ServerMessage::Play(data.clone()),
        "Playstate" => ServerMessage::Playstate(data.clone()),
        "GeneralCommand" => ServerMessage::GeneralCommand(data.clone()),
        _ => ServerMessage::Ignored,
    }
}

fn id_list(data: &Value, key: &str) -> Vec<String> {
    data[key]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn apply_user_data(library: &Library, records: &[UserDataRecord]) {
    if records.is_empty() {
        return;
    }
    match library.apply_user_data(records) {
        Ok(changes) if !changes.is_empty() => {
            tracing::debug!(
                target: "jellyfin.socket",
                items = changes.item_ids.len(),
                "applied pushed watch-state changes"
            );
            crate::app::services::notify_library_changed(changes);
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(target: "jellyfin.socket", "failed to store pushed user data: {error}");
        }
    }
}

fn apply_library_change(
    library: &Library,
    session: &Session,
    sync: &SyncHandle,
    changed: &[String],
    removed: &[String],
) {
    let mut batch = LibraryChangeBatch::default();
    for item_id in removed {
        match library.forget(item_id) {
            Ok(changes) => batch.merge(changes),
            Err(error) => {
                tracing::warn!(
                    target: "jellyfin.socket",
                    "failed to drop removed item {item_id}: {error}"
                );
            }
        }
    }

    if !changed.is_empty() {
        match session.client_and_user() {
            Ok((client, user_id)) => {
                for chunk in changed.chunks(FETCH_CHUNK) {
                    match items::fetch_items(&client, &user_id, chunk) {
                        // Non-library kinds (music, folders) come back too;
                        // ingest_page already filters them out.
                        Ok(response) => match library.ingest_page(&response.items) {
                            Ok(changes) => batch.merge(changes),
                            Err(error) => {
                                tracing::warn!(
                                    target: "jellyfin.socket",
                                    "failed to cache pushed items: {error}"
                                );
                            }
                        },
                        Err(error) => {
                            session.note_error(&error);
                            tracing::debug!(
                                target: "jellyfin.socket",
                                "could not fetch pushed items ({error}); asking for a sync cycle"
                            );
                            sync.request();
                            break;
                        }
                    }
                }
            }
            Err(_) => sync.request(),
        }
    }

    if !batch.is_empty() {
        tracing::debug!(
            target: "jellyfin.socket",
            items = batch.item_ids.len(),
            "applied a pushed library change"
        );
        crate::app::services::notify_library_changed(batch);
    }
}

fn current_authorization(session: &Session) -> Option<String> {
    session
        .client()
        .ok()
        .map(|client| client.authorization_header())
}

fn set_read_timeout(socket: &mut Socket, timeout: Duration) {
    let stream = match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream,
        MaybeTlsStream::Rustls(stream) => &mut stream.sock,
        _ => return,
    };
    let _ = stream.set_read_timeout(Some(timeout));
}

/// Where the server's `/socket` endpoint lives, derived from the session's
/// HTTP(S) base URL.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Endpoint {
    host: String,
    port: u16,
    socket_url: String,
}

impl Endpoint {
    fn parse(base_url: &str) -> Option<Self> {
        let (tls, rest) = if let Some(rest) = base_url.strip_prefix("https://") {
            (true, rest)
        } else {
            (false, base_url.strip_prefix("http://")?)
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let authority = authority
            .rsplit_once('@')
            .map(|(_, host)| host)
            .unwrap_or(authority);
        let default_port = if tls { 443 } else { 80 };
        let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
            let (host, tail) = bracketed.split_once(']')?;
            let port = match tail.strip_prefix(':') {
                Some(port) => port.parse::<u16>().ok()?,
                None => default_port,
            };
            (host, port)
        } else if let Some((host, port)) = authority.rsplit_once(':') {
            (host, port.parse::<u16>().ok()?)
        } else {
            (authority, default_port)
        };
        if host.is_empty() {
            return None;
        }

        let joined = join_url(base_url, "/socket");
        let socket_url = if tls {
            format!("wss{}", &joined["https".len()..])
        } else {
            format!("ws{}", &joined["http".len()..])
        };
        Some(Self {
            host: host.to_string(),
            port,
            socket_url,
        })
    }

    /// Connects with explicit timeouts; `tungstenite::connect` would otherwise
    /// let a stalling handshake block the worker indefinitely.
    fn open(&self) -> Result<TcpStream, String> {
        let addresses = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|error| error.to_string())?;
        let mut last_error = "the server address did not resolve".to_string();
        for address in addresses {
            match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                Ok(stream) => {
                    let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
                    let _ = stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT));
                    return Ok(stream);
                }
                Err(error) => last_error = error.to_string(),
            }
        }
        Err(last_error)
    }
}

#[cfg(test)]
mod tests {
    use super::{Duration, Endpoint, ServerMessage, parse_message, spawn};
    use crate::jellyfin::session::Session;
    use crate::library::{Library, sync};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;
    use tungstenite::Message;

    #[test]
    fn endpoints_map_http_bases_to_socket_urls() {
        assert_eq!(
            Endpoint::parse("http://server:8096"),
            Some(Endpoint {
                host: "server".to_string(),
                port: 8096,
                socket_url: "ws://server:8096/socket".to_string(),
            })
        );
        assert_eq!(
            Endpoint::parse("https://media.example.com/jellyfin"),
            Some(Endpoint {
                host: "media.example.com".to_string(),
                port: 443,
                socket_url: "wss://media.example.com/jellyfin/socket".to_string(),
            })
        );
        assert_eq!(
            Endpoint::parse("http://[::1]:8096"),
            Some(Endpoint {
                host: "::1".to_string(),
                port: 8096,
                socket_url: "ws://[::1]:8096/socket".to_string(),
            })
        );
        assert_eq!(Endpoint::parse("file:///etc/passwd"), None);
        assert_eq!(Endpoint::parse("http://:8096"), None);
    }

    #[test]
    fn force_keep_alive_halves_the_announced_timeout() {
        assert_eq!(
            parse_message(r#"{"MessageType":"ForceKeepAlive","Data":60}"#),
            ServerMessage::KeepAliveInterval(Duration::from_secs(30))
        );
        // A tiny or missing timeout still leaves a sane cadence.
        assert_eq!(
            parse_message(r#"{"MessageType":"ForceKeepAlive","Data":4}"#),
            ServerMessage::KeepAliveInterval(Duration::from_secs(5))
        );
        assert_eq!(
            parse_message(r#"{"MessageType":"ForceKeepAlive"}"#),
            ServerMessage::Ignored
        );
    }

    #[test]
    fn user_data_messages_become_records_and_skip_blank_ids() {
        let message = parse_message(
            r#"{"MessageType":"UserDataChanged","Data":{"UserId":"u1","UserDataList":[
                {"ItemId":"ep1","Played":true,"PlayCount":3,"PlaybackPositionTicks":0,
                 "IsFavorite":false,"LastPlayedDate":"2026-08-18T10:00:00Z"},
                {"ItemId":"  ","Played":true}]}}"#,
        );
        let ServerMessage::UserData(records) = message else {
            panic!("expected user data, got {message:?}");
        };
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].jellyfin_id, "ep1");
        assert!(records[0].played);
        assert_eq!(records[0].play_count, 3);
        assert_eq!(
            records[0].last_played_date.as_deref(),
            Some("2026-08-18T10:00:00Z")
        );
    }

    #[test]
    fn library_changed_merges_added_and_updated_and_keeps_removed_separate() {
        let message = parse_message(
            r#"{"MessageType":"LibraryChanged","Data":{
                "ItemsAdded":["a","b"],"ItemsUpdated":["b","c",""],"ItemsRemoved":["gone"]}}"#,
        );
        assert_eq!(
            message,
            ServerMessage::LibraryChanged {
                changed: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                removed: vec!["gone".to_string()],
            }
        );
    }

    #[test]
    fn remote_control_messages_are_routed_with_their_payloads() {
        let play = parse_message(r#"{"MessageType":"Play","Data":{"ItemIds":["a"]}}"#);
        assert_eq!(
            play,
            ServerMessage::Play(serde_json::json!({ "ItemIds": ["a"] }))
        );
        let playstate = parse_message(r#"{"MessageType":"Playstate","Data":{"Command":"Pause"}}"#);
        assert_eq!(
            playstate,
            ServerMessage::Playstate(serde_json::json!({ "Command": "Pause" }))
        );
        let general =
            parse_message(r#"{"MessageType":"GeneralCommand","Data":{"Name":"ToggleMute"}}"#);
        assert_eq!(
            general,
            ServerMessage::GeneralCommand(serde_json::json!({ "Name": "ToggleMute" }))
        );
    }

    #[test]
    fn unrelated_and_malformed_messages_are_ignored() {
        assert_eq!(
            parse_message(r#"{"MessageType":"Sessions","Data":[]}"#),
            ServerMessage::Ignored
        );
        assert_eq!(
            parse_message(r#"{"MessageType":"KeepAlive"}"#),
            ServerMessage::Ignored
        );
        assert_eq!(parse_message("not json"), ServerMessage::Ignored);
    }

    #[test]
    fn applied_user_data_updates_cached_items_and_their_contexts() {
        let library = Library::open_in_memory().expect("library");
        library
            .ingest_page(&[serde_json::from_str(
                r#"{"Id":"ep1","Name":"Pilot","Type":"Episode",
                    "SeriesId":"show1","SeasonId":"season1","ParentId":"season1"}"#,
            )
            .expect("dto")])
            .expect("ingest");

        let records = vec![crate::library::UserDataRecord {
            jellyfin_id: "ep1".to_string(),
            played: true,
            play_count: 2,
            ..Default::default()
        }];
        let changes = library.apply_user_data(&records).expect("apply");
        assert_eq!(changes.item_ids, vec!["ep1".to_string()]);
        assert_eq!(
            changes.context_ids,
            vec!["season1".to_string(), "show1".to_string()]
        );
        let item = library.item("ep1").expect("item").expect("cached");
        assert_eq!(item["played"], true);
        assert_eq!(item["playCount"], 2);

        // The same state again moves nothing and must not re-notify.
        assert!(
            library
                .apply_user_data(&records)
                .expect("reapply")
                .is_empty()
        );
        // Unknown ids are skipped: the item sweep delivers row and watch
        // state together instead of an orphan user-data row.
        let unknown = vec![crate::library::UserDataRecord {
            jellyfin_id: "never-seen".to_string(),
            played: true,
            ..Default::default()
        }];
        assert!(
            library
                .apply_user_data(&unknown)
                .expect("unknown")
                .is_empty()
        );
    }

    /// End to end over a loopback socket: handshake, ForceKeepAlive,
    /// a pushed watch-state change, and an orderly close.
    #[test]
    fn pushed_watch_state_reaches_the_library_cache() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut socket = tungstenite::accept(stream).expect("handshake");
            socket
                .send(Message::text(
                    r#"{"MessageType":"ForceKeepAlive","Data":60}"#,
                ))
                .expect("force keep alive");
            socket
                .send(Message::text(
                    r#"{"MessageType":"UserDataChanged","Data":{"UserId":"uid","UserDataList":[
                        {"ItemId":"ep1","Played":true,"PlayCount":3,"PlaybackPositionTicks":0,
                         "IsFavorite":false}]}}"#,
                ))
                .expect("user data");
            // The client answers ForceKeepAlive immediately; reading that
            // answer proves both pushes were delivered before closing.
            loop {
                match socket.read() {
                    Ok(Message::Text(_)) => break,
                    Ok(_) => {}
                    Err(error) => panic!("expected a keep-alive answer: {error}"),
                }
            }
            let _ = socket.close(None);
        });

        let library = Arc::new(Library::open_in_memory().expect("library"));
        let mut credentials = library.credentials();
        credentials.server_url = Some(format!("http://{address}"));
        credentials.user_id = Some("uid".to_string());
        credentials.server_id = Some("server".to_string());
        credentials.token = Some("tok".to_string());
        library.save_credentials(&credentials).expect("save");
        library
            .ingest_page(&[
                serde_json::from_str(r#"{"Id":"ep1","Name":"Pilot","Type":"Episode"}"#)
                    .expect("dto"),
            ])
            .expect("ingest");
        let session = Arc::new(Session::restore(library.clone()));

        let handle = spawn(library.clone(), session, sync::detached_handle());
        let deadline = Instant::now() + Duration::from_secs(15);
        let played = loop {
            let item = library.item("ep1").expect("item").expect("cached");
            if item["played"] == true {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(50));
        };
        handle.stop();
        server.join().expect("server");
        assert!(played, "the pushed watch state never reached the cache");

        let item = library.item("ep1").expect("item").expect("cached");
        assert_eq!(item["playCount"], 3);
    }
}
