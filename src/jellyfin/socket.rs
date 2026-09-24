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

use crate::app::services::ShellBridge;
use crate::app::urls::join_url;
use crate::library::sync::SyncHandle;
use crate::library::{Library, LibraryChangeBatch, UserDataRecord};

use super::api::ApiError;
use super::api::items;
use super::api::model::UserItemDataDto;
use super::api::sessions;
use super::remote::RemoteCommand;
use super::session::{Session, SessionScope};

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
    shell: Arc<ShellBridge>,
    remote: Arc<Mutex<Option<RemoteHandler>>>,
}

/// Carries out a remote-control command for the application.
pub type RemoteHandler = Arc<dyn Fn(RemoteCommand) + Send + Sync>;

impl SocketHandle {
    fn new(shell: Arc<ShellBridge>) -> Self {
        Self {
            signal: Arc::new(Signal {
                stopped: Mutex::new(false),
                condvar: Condvar::new(),
            }),
            shell,
            remote: Arc::new(Mutex::new(None)),
        }
    }

    /// A handle with no event thread behind it, for tests.
    #[cfg(test)]
    pub fn detached() -> Self {
        Self::new(Arc::default())
    }

    /// Registers who carries out Play, Playstate and GeneralCommand messages.
    /// Until then they are dropped, as there is no player to drive.
    pub fn on_remote_command(&self, handler: RemoteHandler) {
        if let Ok(mut slot) = self.remote.lock() {
            *slot = Some(handler);
        }
    }

    fn remote(&self, command: RemoteCommand) {
        let handler = self.remote.lock().ok().and_then(|slot| slot.clone());
        if let Some(handler) = handler {
            handler(command);
        }
    }

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

pub fn spawn(
    library: Arc<Library>,
    session: Arc<Session>,
    sync: SyncHandle,
    shell: Arc<ShellBridge>,
) -> SocketHandle {
    let handle = SocketHandle::new(shell);
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
            Ok((mut socket, scope)) => {
                tracing::info!(target: "jellyfin.socket", "listening for Jellyfin server events");
                announce_capabilities(session, &scope);
                // Whatever happened while no connection existed was never
                // pushed; one requested cycle reconciles the gap.
                sync.request();
                let connected_at = Instant::now();
                match listen(&mut socket, library, session, sync, handle, &scope) {
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
fn announce_capabilities(session: &Arc<Session>, scope: &SessionScope) {
    let session = session.clone();
    let scope = scope.clone();
    let spawned = thread::Builder::new()
        .name("jellyfin-capabilities".to_string())
        .spawn(
            move || match sessions::announce_capabilities(scope.client()) {
                Ok(()) => {
                    tracing::debug!(
                        target: "jellyfin.socket",
                        "announced remote-control capabilities"
                    );
                }
                Err(error) => {
                    session.note_scoped_error(&scope, &error);
                    tracing::debug!(
                        target: "jellyfin.socket",
                        "could not announce remote-control capabilities: {error}"
                    );
                }
            },
        );
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

/// Opens the stream for the current account. The returned scope is the
/// connection's identity: everything the server pushes over it belongs to that
/// account, so every cache write it causes commits against the scope.
fn connect(session: &Session) -> Result<(Socket, SessionScope), String> {
    let scope = session.scope().map_err(|error| error.to_string())?;
    let client = scope.client();
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
            session.note_scoped_error(&scope, &ApiError::Unauthorized);
        }
        error.to_string()
    })?;
    set_read_timeout(&mut socket, READ_TICK);
    Ok((socket, scope))
}

fn listen(
    socket: &mut Socket,
    library: &Library,
    session: &Session,
    sync: &SyncHandle,
    handle: &SocketHandle,
    scope: &SessionScope,
) -> Disconnect {
    let mut keepalive_interval = DEFAULT_KEEPALIVE_INTERVAL;
    let mut keepalive_sent = Instant::now();
    loop {
        if handle.is_stopped() {
            let _ = socket.close(None);
            let _ = socket.flush();
            return Disconnect::Stopped;
        }
        // Sign-out, account switches, and token rejection end the session
        // this connection authenticated as; keep listening only while it is
        // still current.
        if !session.scope_is_current(scope) {
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
                if let Some(interval) =
                    handle_message(text.as_str(), library, session, sync, handle, scope)
                {
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
    handle: &SocketHandle,
    scope: &SessionScope,
) -> Option<Duration> {
    let shell = handle.shell.as_ref();
    match parse_message(text) {
        ServerMessage::KeepAliveInterval(interval) => return Some(interval),
        ServerMessage::UserData(records) => {
            apply_user_data(library, session, shell, scope, &records);
        }
        ServerMessage::LibraryChanged { changed, removed } => {
            apply_library_change(library, session, sync, shell, scope, &changed, &removed);
        }
        ServerMessage::Play(data) => handle.remote(RemoteCommand::Play {
            data,
            scope: scope.clone(),
        }),
        ServerMessage::Playstate(data) => handle.remote(RemoteCommand::Playstate {
            data,
            scope: scope.clone(),
        }),
        ServerMessage::GeneralCommand(data) => handle.remote(RemoteCommand::General { data }),
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

/// Why a pushed change was not written to the catalog cache.
enum ScopedWriteError {
    /// The account the connection belongs to is no longer the signed-in one.
    Stale,
    Storage(rusqlite::Error),
}

/// Writes to the catalog cache only while `scope` is still the signed-in
/// session, atomically with account switches and sign-out, so a push that
/// raced one can never land in the next account's cache.
fn commit_scoped<T>(
    session: &Session,
    scope: &SessionScope,
    write: impl FnOnce() -> rusqlite::Result<T>,
) -> Result<T, ScopedWriteError> {
    session.commit_if_current(
        scope,
        || ScopedWriteError::Stale,
        || write().map_err(ScopedWriteError::Storage),
    )
}

fn log_stale_push() {
    tracing::debug!(
        target: "jellyfin.socket",
        "dropped a pushed change for a session that is no longer signed in"
    );
}

fn apply_user_data(
    library: &Library,
    session: &Session,
    shell: &ShellBridge,
    scope: &SessionScope,
    records: &[UserDataRecord],
) {
    if records.is_empty() {
        return;
    }
    match commit_scoped(session, scope, || library.apply_user_data(records)) {
        Ok(changes) if !changes.is_empty() => {
            tracing::debug!(
                target: "jellyfin.socket",
                items = changes.item_ids.len(),
                "applied pushed watch-state changes"
            );
            shell.library_changed(changes);
        }
        Ok(_) => {}
        Err(ScopedWriteError::Stale) => log_stale_push(),
        Err(ScopedWriteError::Storage(error)) => {
            tracing::warn!(target: "jellyfin.socket", "failed to store pushed user data: {error}");
        }
    }
}

/// Evicts removed items and fetches changed ones as the connection's own
/// account. Fetches run without any lock held; each write then commits only if
/// that session is still current. A stale result is dropped, because the next
/// session's own sync owns its cache.
fn apply_library_change(
    library: &Library,
    session: &Session,
    sync: &SyncHandle,
    shell: &ShellBridge,
    scope: &SessionScope,
    changed: &[String],
    removed: &[String],
) {
    let mut batch = LibraryChangeBatch::default();
    for item_id in removed {
        match commit_scoped(session, scope, || library.forget(item_id)) {
            Ok(changes) => batch.merge(changes),
            Err(ScopedWriteError::Stale) => {
                log_stale_push();
                return;
            }
            Err(ScopedWriteError::Storage(error)) => {
                tracing::warn!(
                    target: "jellyfin.socket",
                    "failed to drop removed item {item_id}: {error}"
                );
            }
        }
    }

    for chunk in changed.chunks(FETCH_CHUNK) {
        match items::fetch_items(scope.client(), scope.user_id(), chunk) {
            // Non-library kinds (music, folders) come back too; ingest_page
            // already filters them out.
            Ok(response) => {
                match commit_scoped(session, scope, || library.ingest_page(&response.items)) {
                    Ok(changes) => batch.merge(changes),
                    Err(ScopedWriteError::Stale) => {
                        log_stale_push();
                        return;
                    }
                    Err(ScopedWriteError::Storage(error)) => {
                        tracing::warn!(
                            target: "jellyfin.socket",
                            "failed to cache pushed items: {error}"
                        );
                    }
                }
            }
            Err(error) => {
                session.note_scoped_error(scope, &error);
                tracing::debug!(
                    target: "jellyfin.socket",
                    "could not fetch pushed items ({error}); asking for a sync cycle"
                );
                sync.request();
                break;
            }
        }
    }

    if !batch.is_empty() {
        tracing::debug!(
            target: "jellyfin.socket",
            items = batch.item_ids.len(),
            "applied a pushed library change"
        );
        shell.library_changed(batch);
    }
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
    use super::{
        Duration, Endpoint, RemoteCommand, ServerMessage, SocketHandle, apply_library_change,
        apply_user_data, handle_message, parse_message, spawn,
    };
    use crate::app::services::ShellBridge;
    use crate::jellyfin::session::Session;
    use crate::library::sync::SyncHandle;
    use crate::library::{Library, UserDataRecord};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, mpsc};
    use std::thread;
    use std::time::Instant;
    use tungstenite::Message;

    fn alice_library(server_url: &str) -> Arc<Library> {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let mut credentials = library.credentials();
        credentials.server_url = Some(server_url.to_string());
        credentials.user_id = Some("alice".to_string());
        credentials.user_name = Some("Alice".to_string());
        credentials.server_id = Some("server".to_string());
        credentials.token = Some("alice-token".to_string());
        library.save_credentials(&credentials).expect("save");
        library
    }

    /// Remote control is the application's job: the socket hands each command
    /// to the handler registered on its handle, with the delivering session.
    #[test]
    fn remote_commands_reach_the_registered_handler_in_order() {
        let library = alice_library("http://127.0.0.1:9");
        let session = Session::restore(library.clone(), Arc::default());
        let scope = session.scope().expect("Alice scope");
        let handle = SocketHandle::detached();
        let sync = SyncHandle::detached();
        let message =
            |kind: &str| format!(r#"{{"MessageType":"{kind}","Data":{{"Command":"x"}}}}"#);

        // Without a handler the command is dropped rather than half-applied.
        handle_message(&message("Play"), &library, &session, &sync, &handle, &scope);

        let (seen_tx, seen) = mpsc::channel();
        handle.on_remote_command(Arc::new(move |command| {
            let _ = seen_tx.send(match command {
                RemoteCommand::Play { data, scope } => {
                    format!("play:{}:{}", scope.user_id(), data["Command"])
                }
                RemoteCommand::Playstate { data, scope } => {
                    format!("playstate:{}:{}", scope.user_id(), data["Command"])
                }
                RemoteCommand::General { data } => format!("general:{}", data["Command"]),
            });
        }));
        for kind in ["Play", "Playstate", "GeneralCommand"] {
            handle_message(&message(kind), &library, &session, &sync, &handle, &scope);
        }

        assert_eq!(
            seen.try_iter().collect::<Vec<_>>(),
            [
                r#"play:alice:"x""#,
                r#"playstate:alice:"x""#,
                r#"general:"x""#
            ]
        );
    }

    /// Reads one whole HTTP request, body included, so answering and closing
    /// never resets a connection that still holds unread request bytes.
    fn receive_request(listener: &TcpListener) -> (TcpStream, String) {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 2_048];
        let header_end = loop {
            let read = stream.read(&mut buffer).expect("read request");
            assert!(read > 0, "connection closed before headers");
            request.extend_from_slice(&buffer[..read]);
            if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let head = String::from_utf8_lossy(&request[..header_end]).into_owned();
        let body_length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        while request.len() < header_end + body_length {
            let read = stream.read(&mut buffer).expect("read request body");
            assert!(read > 0, "connection closed before the body");
            request.extend_from_slice(&buffer[..read]);
        }
        let target = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .expect("request target")
            .to_string();
        (stream, target)
    }

    fn send_json(mut stream: TcpStream, body: &str) {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("write response");
    }

    /// The race this guards: a LibraryChanged fetch for Alice is still in
    /// flight when the user switches to Bob, whose sign-in clears the cache.
    /// Alice's late response must not land in Bob's catalog.
    #[test]
    fn a_pushed_change_fetched_before_an_account_switch_is_not_cached() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let server_url = format!("http://{}", listener.local_addr().expect("address"));
        let (stale_fetch_tx, stale_fetch_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let server = thread::spawn(move || {
            let mut targets = Vec::new();
            let (stream, target) = receive_request(&listener);
            targets.push(target);
            send_json(
                stream,
                r#"{"Items":[{"Id":"alice-first","Name":"First","Type":"Movie"}],"TotalRecordCount":1}"#,
            );

            let (stale, target) = receive_request(&listener);
            targets.push(target);
            stale_fetch_tx.send(()).expect("signal the in-flight fetch");

            let (stream, target) = receive_request(&listener);
            targets.push(target);
            send_json(
                stream,
                r#"{"AccessToken":"bob-token","ServerId":"server",
                    "User":{"Id":"bob","Name":"Bob","Policy":{"IsAdministrator":true}}}"#,
            );

            release_rx.recv().expect("release the stale fetch");
            send_json(
                stale,
                r#"{"Items":[{"Id":"alice-late","Name":"Late","Type":"Movie"}],"TotalRecordCount":1}"#,
            );
            targets
        });

        let library = alice_library(&server_url);
        let session = Arc::new(Session::restore(library.clone(), Arc::default()));
        let alice = session.scope().expect("Alice scope");
        let sync = SyncHandle::detached();

        // While Alice is current, a pushed change is fetched and cached.
        apply_library_change(
            &library,
            &session,
            &sync,
            &ShellBridge::default(),
            &alice,
            &["alice-first".to_string()],
            &[],
        );
        assert!(library.item("alice-first").expect("item").is_some());

        let worker = {
            let library = library.clone();
            let session = session.clone();
            thread::spawn(move || {
                apply_library_change(
                    &library,
                    &session,
                    &sync,
                    &ShellBridge::default(),
                    &alice,
                    &["alice-late".to_string()],
                    &[],
                );
            })
        };
        stale_fetch_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the pushed fetch reached the server");
        session
            .login(&server_url, "bob", "secret")
            .expect("switch to Bob");
        release_tx.send(()).expect("release");
        worker.join().expect("push worker");

        let targets = server.join().expect("server");
        assert!(targets[0].starts_with("/Items?") && targets[0].contains("alice-first"));
        assert!(targets[1].starts_with("/Items?") && targets[1].contains("alice-late"));
        assert_eq!(targets[2], "/Users/AuthenticateByName");
        assert_eq!(session.user_id().as_deref(), Some("bob"));
        assert_eq!(
            library.cache_owner(),
            Some(("server".to_string(), "bob".to_string()))
        );
        assert!(library.item("alice-late").expect("item").is_none());
        assert_eq!(library.stats().total, 0);
    }

    #[test]
    fn pushed_removals_and_watch_state_for_a_signed_out_session_are_dropped() {
        let library = alice_library("http://server:8096");
        library
            .ingest_page(&[
                serde_json::from_str(r#"{"Id":"m1","Name":"Kept","Type":"Movie"}"#).expect("dto"),
            ])
            .expect("ingest");
        let session = Session::restore(library.clone(), Arc::default());
        let alice = session.scope().expect("Alice scope");
        session
            .clear_local(false)
            .expect("sign out, keeping the cache");

        apply_user_data(
            &library,
            &session,
            &ShellBridge::default(),
            &alice,
            &[UserDataRecord {
                jellyfin_id: "m1".to_string(),
                played: true,
                ..Default::default()
            }],
        );
        apply_library_change(
            &library,
            &session,
            &SyncHandle::detached(),
            &ShellBridge::default(),
            &alice,
            &[],
            &["m1".to_string()],
        );

        let item = library.item("m1").expect("item").expect("still cached");
        assert!(!item.summary.played);
    }

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
        assert!(item.summary.played);
        assert_eq!(item.summary.play_count, 2);

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
        let session = Arc::new(Session::restore(library.clone(), Arc::default()));

        let handle = spawn(
            library.clone(),
            session,
            SyncHandle::detached(),
            Arc::default(),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        let played = loop {
            let item = library.item("ep1").expect("item").expect("cached");
            if item.summary.played {
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
        assert_eq!(item.summary.play_count, 3);
    }
}
