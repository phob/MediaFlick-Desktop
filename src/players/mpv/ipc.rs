//! mpv IPC endpoint allocation, command transport, and event decoding.

use std::io::{self, BufRead, BufReader, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::app::logger;

use super::commands::next_request_id;
use super::controller::IPC_COMMAND_CONNECT_TIMEOUT;

static IPC_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn make_ipc_path() -> String {
    let counter = IPC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    #[cfg(target_os = "windows")]
    {
        format!(
            r"\\.\pipe\mediaflick-desktop-{}-{timestamp}-{counter}",
            std::process::id()
        )
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::temp_dir()
            .join(format!(
                "mediaflick-desktop-{}-{timestamp}-{counter}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(not(target_os = "windows"))]
pub fn cleanup_ipc_path(path: &str) {
    let _ = std::fs::remove_file(path);
}

#[cfg(target_os = "windows")]
pub fn cleanup_ipc_path(_path: &str) {}

#[derive(Debug)]
pub(super) struct MpvEvent {
    pub(super) name: String,
    pub(super) reason: Option<String>,
    pub(super) property: Option<String>,
    pub(super) data: Option<Value>,
    pub(super) args: Vec<String>,
    pub(super) raw: Value,
}

impl MpvEvent {
    pub(super) fn summary(&self) -> String {
        match self.name.as_str() {
            "property-change" => format!(
                "property-change name={} data={}",
                self.property.as_deref().unwrap_or("unknown"),
                self.data
                    .as_ref()
                    .map(logger::redacted_json)
                    .unwrap_or_else(|| "null".to_string())
            ),
            "end-file" => format!(
                "end-file reason={}",
                self.reason.as_deref().unwrap_or("unknown")
            ),
            "client-message" => format!("client-message args={:?}", self.args),
            name => name.to_string(),
        }
    }

    pub(super) fn is_position_property_change(&self) -> bool {
        self.name == "property-change"
            && matches!(self.property.as_deref(), Some("time-pos" | "playback-time"))
    }
}

pub(super) struct IpcWorker {
    path: String,
    command_tx: Sender<IpcCommand>,
    reader_thread: thread::JoinHandle<()>,
    writer_thread: thread::JoinHandle<()>,
    writer_alive: Arc<AtomicBool>,
}

type IpcCommand = (Value, Duration, Sender<Result<(), IpcCommandFailure>>);

#[derive(Debug)]
pub(super) enum IpcCommandFailure {
    Transport(io::Error),
    Rejected(io::Error),
}

impl IpcCommandFailure {
    pub(super) fn is_transport(&self) -> bool {
        matches!(self, Self::Transport(_))
    }

    pub(super) fn with_context(self, context: impl std::fmt::Display) -> Self {
        match self {
            Self::Transport(error) => {
                Self::Transport(io::Error::new(error.kind(), format!("{context}: {error}")))
            }
            Self::Rejected(error) => {
                Self::Rejected(io::Error::new(error.kind(), format!("{context}: {error}")))
            }
        }
    }
}

impl std::fmt::Display for IpcCommandFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) => write!(formatter, "IPC transport failure: {error}"),
            Self::Rejected(error) => write!(formatter, "mpv rejected command: {error}"),
        }
    }
}

impl std::error::Error for IpcCommandFailure {}

struct IpcCommandWriter {
    stream: BufReader<IpcConnection>,
    alive: Arc<AtomicBool>,
}

impl Drop for IpcCommandWriter {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
    }
}

impl IpcWorker {
    fn start(path: &str) -> io::Result<(Self, Receiver<MpvEvent>)> {
        tracing::trace!(target: "mpv.ipc", ipc_path = path, "connecting mpv IPC reader");
        let mut reader = connect_ipc(path)?;
        write_observe_commands(&mut reader)?;
        // Keep command writes on a dedicated pipe opened while mpv is still idle.
        // Opening fresh command pipes after load can hang on Windows, and writing
        // commands through a clone of the event reader can prevent loadfile from
        // reaching mpv. The separate persistent writer is the known-good shape.
        tracing::trace!(target: "mpv.ipc", ipc_path = path, "connecting mpv IPC command writer");
        let writer_alive = Arc::new(AtomicBool::new(true));
        let writer = IpcCommandWriter {
            stream: BufReader::new(connect_ipc_for_commands_with_timeout(
                path,
                IPC_COMMAND_CONNECT_TIMEOUT,
            )?),
            alive: writer_alive.clone(),
        };
        let (event_tx, event_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel();
        let reader_thread = thread::spawn(move || read_events(&reader, &event_tx));
        let writer_thread = thread::spawn(move || writer.write_commands(&command_rx));
        Ok((
            Self {
                path: path.to_string(),
                command_tx,
                reader_thread,
                writer_thread,
                writer_alive,
            },
            event_rx,
        ))
    }

    pub(super) fn is_writer_alive(&self) -> bool {
        self.writer_alive.load(Ordering::SeqCst)
    }

    pub(super) fn send_with_timeout(
        &self,
        command: Value,
        timeout: Duration,
    ) -> Result<(), IpcCommandFailure> {
        if !self.writer_alive.load(Ordering::SeqCst) {
            return Err(IpcCommandFailure::Transport(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "mpv IPC writer stopped",
            )));
        }
        let (ack, ack_rx) = mpsc::channel();
        self.command_tx.send((command, timeout, ack)).map_err(|_| {
            IpcCommandFailure::Transport(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "mpv IPC writer stopped",
            ))
        })?;
        match ack_rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(_) => {
                // A reply timeout leaves the synchronous reader's state
                // unknowable. Poison this worker so no later command queues
                // behind it; the controller poll will replace the session.
                self.writer_alive.store(false, Ordering::SeqCst);
                Err(IpcCommandFailure::Transport(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "mpv IPC command reply timed out after {}ms",
                        timeout.as_millis()
                    ),
                )))
            }
        }
    }

    pub(super) fn shutdown(self) {
        let Self {
            path,
            command_tx,
            reader_thread,
            writer_thread,
            writer_alive: _,
        } = self;
        tracing::trace!(target: "mpv.ipc", ipc_path = %path, "joining mpv IPC reader thread");
        drop(command_tx);
        let _ = writer_thread.join();
        let _ = reader_thread.join();
    }
}

impl IpcCommandWriter {
    fn write_commands(mut self, rx: &Receiver<IpcCommand>) {
        while let Ok((command, timeout, ack)) = rx.recv() {
            tracing::trace!(
                target: "mpv.ipc",
                command = %logger::mpv_command_summary(&command),
                "writing mpv IPC command"
            );
            let asynchronous = command
                .get("async")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let result = if asynchronous {
                // mpv's async IPC mode explicitly permits later commands to run
                // while this one is still executing. Acknowledge the successful
                // write now; a later validated command drains and logs the
                // eventual completion reply by request id.
                write_command(self.stream.get_mut(), &command).map_err(IpcCommandFailure::Transport)
            } else {
                self.write_command_and_wait_for_reply(&command, timeout)
            };
            match result {
                Ok(()) => {
                    let _ = ack.send(Ok(()));
                }
                Err(IpcCommandFailure::Rejected(error)) => {
                    let _ = ack.send(Err(IpcCommandFailure::Rejected(error)));
                }
                Err(IpcCommandFailure::Transport(error)) => {
                    let _ = ack.send(Err(IpcCommandFailure::Transport(error)));
                    break;
                }
            }
        }
        tracing::trace!(target: "mpv.ipc", "mpv IPC writer stopped");
    }

    fn write_command_and_wait_for_reply(
        &mut self,
        command: &Value,
        timeout: Duration,
    ) -> Result<(), IpcCommandFailure> {
        let request_id = command
            .get("request_id")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        set_ipc_command_read_timeout(self.stream.get_mut(), timeout)
            .map_err(IpcCommandFailure::Transport)?;
        write_command(self.stream.get_mut(), command).map_err(IpcCommandFailure::Transport)?;

        let mut line = String::new();
        loop {
            line.clear();
            match self.stream.read_line(&mut line) {
                Ok(0) => {
                    return Err(IpcCommandFailure::Transport(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "mpv IPC command connection closed before its reply",
                    )));
                }
                Ok(_) => {}
                Err(error) => return Err(IpcCommandFailure::Transport(error)),
            }

            let Ok(reply) = serde_json::from_str::<Value>(&line) else {
                tracing::trace!(
                    target: "mpv.ipc",
                    line = %logger::redact_text(&line),
                    "ignored malformed mpv IPC command reply"
                );
                continue;
            };
            if reply.get("event").is_some() {
                tracing::trace!(
                    target: "mpv.ipc",
                    event = %logger::redacted_json(&reply),
                    "ignored duplicate event on mpv command connection"
                );
                continue;
            }
            let reply_id = reply
                .get("request_id")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            if reply_id != request_id {
                log_command_reply(&reply);
                tracing::trace!(
                    target: "mpv.ipc",
                    request_id,
                    reply_id,
                    "ignored unrelated mpv IPC command reply"
                );
                continue;
            }

            log_command_reply(&reply);
            return command_reply_result(&reply, request_id).map_err(IpcCommandFailure::Rejected);
        }
    }
}

fn command_reply_result(reply: &Value, request_id: i64) -> io::Result<()> {
    match reply.get("error").and_then(Value::as_str) {
        Some("success") => Ok(()),
        Some(error) => Err(io::Error::other(format!(
            "mpv rejected request {request_id}: {error}"
        ))),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("mpv reply for request {request_id} did not contain an error status"),
        )),
    }
}

fn write_observe_commands<W: Write>(stream: &mut W) -> io::Result<()> {
    for property in [
        "pause",
        "time-pos",
        "playback-time",
        "duration",
        "volume",
        "mute",
        "eof-reached",
        "seeking",
        "playback-abort",
        "chapter-list",
    ] {
        let command = json!({
            "command": ["observe_property", next_request_id(), property],
            "request_id": next_request_id(),
        });
        tracing::debug!(target: "mpv.ipc", property, "registering mpv property observer");
        tracing::trace!(
            target: "mpv.ipc",
            command = %logger::redacted_json(&command),
            "sending mpv observe_property command"
        );
        serde_json::to_writer(&mut *stream, &command)?;
        stream.write_all(b"\n")?;
    }
    stream.flush()
}

fn write_command<W: Write>(stream: &mut W, command: &Value) -> io::Result<()> {
    serde_json::to_writer(&mut *stream, command)?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn connect_ipc_for_commands_with_timeout(
    path: &str,
    timeout: Duration,
) -> io::Result<IpcConnection> {
    let deadline = Instant::now() + timeout;
    let mut last_error = None;
    while Instant::now() < deadline {
        match connect_ipc_for_commands(path) {
            Ok(stream) => return Ok(stream),
            Err(error) => {
                tracing::trace!(
                    target: "mpv.ipc",
                    ipc_path = path,
                    "mpv IPC command writer not ready yet: {error}"
                );
                last_error = Some(error);
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "mpv IPC command writer did not become ready",
        )
    }))
}

fn log_command_reply(value: &Value) {
    match value.get("error").and_then(Value::as_str) {
        Some("success") | None => tracing::trace!(
            target: "mpv.ipc",
            value = %logger::redacted_json(value),
            "received mpv IPC command reply"
        ),
        Some(error) => tracing::warn!(
            target: "mpv.ipc",
            request_id = value.get("request_id").and_then(|id| id.as_i64()),
            error,
            "mpv rejected command"
        ),
    }
}

fn read_events(stream: &IpcConnection, tx: &Sender<MpvEvent>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    tracing::trace!(
                        target: "mpv.ipc",
                        line = %logger::redact_text(&line),
                        "ignored malformed mpv IPC line"
                    );
                    continue;
                };
                let Some(name) = value.get("event").and_then(Value::as_str) else {
                    log_command_reply(&value);
                    continue;
                };
                let event = MpvEvent {
                    name: name.to_string(),
                    reason: value
                        .get("reason")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    property: value
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    data: value.get("data").cloned(),
                    args: value
                        .get("args")
                        .and_then(Value::as_array)
                        .map(|args| {
                            args.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                    raw: value,
                };
                if tx.send(event).is_err() {
                    tracing::trace!(target: "mpv.ipc", "mpv event receiver dropped");
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                tracing::trace!(target: "mpv.ipc", "mpv IPC read timed out while waiting for events");
                continue;
            }
            Err(error) => {
                tracing::trace!(target: "mpv.ipc", "mpv IPC read failed: {error}");
                break;
            }
        }
    }
    tracing::trace!(target: "mpv.ipc", "mpv IPC reader stopped");
}

pub(super) fn start_ipc_worker(
    path: &str,
    timeout: Duration,
    shutdown_requested: &AtomicBool,
    mut runtime_is_alive: impl FnMut() -> io::Result<bool>,
) -> io::Result<(IpcWorker, Receiver<MpvEvent>)> {
    let deadline = Instant::now() + timeout;
    let mut last_error = None;
    tracing::debug!(
        target: "mpv.ipc",
        ipc_path = path,
        timeout_ms = timeout.as_millis(),
        "waiting for mpv IPC"
    );
    while Instant::now() < deadline {
        if shutdown_requested.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "mpv IPC wait cancelled by shutdown",
            ));
        }
        match IpcWorker::start(path) {
            Ok(worker) => return Ok(worker),
            Err(error) => {
                tracing::trace!(target: "mpv.ipc", ipc_path = path, "mpv IPC not ready yet: {error}");
                last_error = Some(error);
            }
        }
        if !runtime_is_alive()? {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "mpv stopped before IPC became ready",
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
    let reason = last_error
        .map(|error| error.to_string())
        .unwrap_or_else(|| "no IPC connection attempt was made".to_string());
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "mpv IPC did not become ready within {}ms: {reason}",
            timeout.as_millis()
        ),
    ))
}

#[cfg(target_os = "windows")]
type IpcConnection = std::fs::File;

#[cfg(target_os = "windows")]
fn connect_ipc(path: &str) -> io::Result<IpcConnection> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
}

#[cfg(target_os = "windows")]
fn connect_ipc_for_commands(path: &str) -> io::Result<IpcConnection> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
}

#[cfg(target_os = "windows")]
fn set_ipc_command_read_timeout(_stream: &IpcConnection, _timeout: Duration) -> io::Result<()> {
    Ok(())
}

#[cfg(all(unix, not(target_os = "windows")))]
type IpcConnection = std::os::unix::net::UnixStream;

#[cfg(all(unix, not(target_os = "windows")))]
fn connect_ipc(path: &str) -> io::Result<IpcConnection> {
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(stream)
}

#[cfg(all(unix, not(target_os = "windows")))]
fn connect_ipc_for_commands(path: &str) -> io::Result<IpcConnection> {
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(stream)
}

#[cfg(all(unix, not(target_os = "windows")))]
fn set_ipc_command_read_timeout(stream: &IpcConnection, timeout: Duration) -> io::Result<()> {
    stream.set_read_timeout(Some(timeout))
}

#[cfg(test)]
mod tests {
    use super::command_reply_result;
    use serde_json::json;
    #[cfg(unix)]
    use std::time::Duration;

    #[test]
    fn command_replies_surface_mpv_rejections() {
        assert!(command_reply_result(&json!({ "request_id": 42, "error": "success" }), 42).is_ok());
        let error = command_reply_result(
            &json!({ "request_id": 43, "error": "property unavailable" }),
            43,
        )
        .expect_err("rejected command");
        assert!(error.to_string().contains("property unavailable"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_command_ipc_uses_per_command_read_timeout() {
        use super::{connect_ipc, connect_ipc_for_commands, set_ipc_command_read_timeout};
        use std::os::unix::net::UnixListener;

        let path = crate::players::mpv::ipc::make_ipc_path();
        let listener = UnixListener::bind(&path).expect("bind test IPC socket");

        let command = connect_ipc_for_commands(&path).expect("connect command socket");
        let (_command_peer, _) = listener.accept().expect("accept command socket");
        assert_eq!(command.read_timeout().expect("command read timeout"), None);
        assert_eq!(
            command.write_timeout().expect("command write timeout"),
            Some(Duration::from_secs(2))
        );

        let subtitle_timeout = Duration::from_secs(30);
        set_ipc_command_read_timeout(&command, subtitle_timeout)
            .expect("set per-command read timeout");
        assert_eq!(
            command
                .read_timeout()
                .expect("updated command read timeout"),
            Some(subtitle_timeout)
        );

        let event = connect_ipc(&path).expect("connect event socket");
        let (_event_peer, _) = listener.accept().expect("accept event socket");
        assert_eq!(
            event.read_timeout().expect("event read timeout"),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            event.write_timeout().expect("event write timeout"),
            Some(Duration::from_secs(2))
        );

        drop(event);
        drop(command);
        drop(listener);
        let _ = std::fs::remove_file(path);
    }
}
