use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::external_mpv::{ExternalMpv, HttpHeader, MpvLaunch};
use crate::jellyfin_bridge;
use crate::playback_reporter::{
    MpvPlaybackState, PlaybackReporter, cleanup_ipc_path, make_mpv_ipc_path, seconds_to_ticks,
};

const IPC_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IPC_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);
const DUPLICATE_DEBOUNCE: Duration = Duration::from_secs(2);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

static REQUEST_COUNTER: AtomicI64 = AtomicI64::new(100);

#[derive(Clone)]
pub struct MpvController {
    tx: Sender<ControllerMessage>,
}

enum ControllerMessage {
    Load {
        mpv_path: String,
        launch: Box<MpvLaunch>,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
struct RecentLoad {
    key: String,
    seen_at: Instant,
}

struct ControllerState {
    rx: Receiver<ControllerMessage>,
    child: Option<Child>,
    ipc_path: Option<String>,
    ipc_worker: Option<IpcWorker>,
    event_rx: Option<Receiver<MpvEvent>>,
    active: Option<ActivePlayback>,
    pending: Option<PendingPlayback>,
    last_state: MpvPlaybackState,
    recent_loads: VecDeque<RecentLoad>,
}

struct PendingPlayback {
    key: String,
    launch: MpvLaunch,
    reporter: Option<PlaybackReporter>,
    requested_at: Instant,
}

struct ActivePlayback {
    reporter: PlaybackReporter,
    last_progress_sent: Instant,
    last_pause: bool,
}

#[derive(Debug)]
struct MpvEvent {
    name: String,
    reason: Option<String>,
    property: Option<String>,
    data: Option<Value>,
}

struct IpcWorker {
    path: String,
    reader_thread: thread::JoinHandle<()>,
}

impl MpvController {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || ControllerState::new(rx).run());
        Self { tx }
    }

    pub fn load(&self, mpv_path: impl Into<String>, launch: MpvLaunch) {
        let _ = self.tx.send(ControllerMessage::Load {
            mpv_path: mpv_path.into(),
            launch: Box::new(launch),
        });
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(ControllerMessage::Shutdown);
    }
}

impl ControllerState {
    fn new(rx: Receiver<ControllerMessage>) -> Self {
        Self {
            rx,
            child: None,
            ipc_path: None,
            ipc_worker: None,
            event_rx: None,
            active: None,
            pending: None,
            last_state: MpvPlaybackState {
                volume: Some(100),
                ..Default::default()
            },
            recent_loads: VecDeque::new(),
        }
    }

    fn run(mut self) {
        loop {
            match self.rx.recv_timeout(Duration::from_millis(200)) {
                Ok(ControllerMessage::Load { mpv_path, launch }) => self.load(mpv_path, *launch),
                Ok(ControllerMessage::Shutdown) => {
                    self.shutdown();
                    return;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.shutdown();
                    return;
                }
            }

            self.drain_events();
            self.poll_child();
            self.maybe_report_progress();
            self.prune_recent_loads();
        }
    }

    fn load(&mut self, mpv_path: String, launch: MpvLaunch) {
        let key = launch.dedupe_key();
        if self.is_duplicate(&key) {
            return;
        }

        if !self.ensure_mpv(&mpv_path) {
            return;
        };

        let reporter = PlaybackReporter::from_launch(&launch);
        if let Some(active) = self.active.take() {
            active.reporter.report_stopped(&self.last_state, false);
        }

        match self.send_mpv_command(loadfile_command(&launch)) {
            Ok(()) => {
                eprintln!(
                    "Loaded Jellyfin stream in mpv: item={} url={}",
                    launch.item_id.as_deref().unwrap_or("unknown"),
                    jellyfin_bridge::redact_url_secrets(&launch.media_url)
                );
                self.recent_loads.push_back(RecentLoad {
                    key: key.clone(),
                    seen_at: Instant::now(),
                });
                self.pending = Some(PendingPlayback {
                    key,
                    launch,
                    reporter,
                    requested_at: Instant::now(),
                });
            }
            Err(error) => {
                eprintln!("Failed to send mpv loadfile command: {error}");
                self.reset_mpv();
            }
        }
    }

    fn ensure_mpv(&mut self, mpv_path: &str) -> bool {
        if self.child_is_alive() {
            return self.ipc_worker.is_some();
        }

        self.reset_mpv();
        let ipc_path = make_mpv_ipc_path();
        let mpv = ExternalMpv::new(PathBuf::from(mpv_path));
        let child = match mpv.command_for_idle_with_ipc(&ipc_path).spawn() {
            Ok(child) => child,
            Err(error) => {
                eprintln!(
                    "Failed to launch mpv for Jellyfin stream ({}): {error}",
                    mpv.executable().display()
                );
                cleanup_ipc_path(&ipc_path);
                return false;
            }
        };

        let (ipc_worker, event_rx) = match start_ipc_worker(&ipc_path, IPC_CONNECT_TIMEOUT) {
            Ok(worker) => worker,
            Err(error) => {
                eprintln!("Failed to connect mpv IPC: {error}");
                let mut child = child;
                let _ = child.kill();
                cleanup_ipc_path(&ipc_path);
                return false;
            }
        };
        self.child = Some(child);
        self.ipc_path = Some(ipc_path.clone());
        self.ipc_worker = Some(ipc_worker);
        self.event_rx = Some(event_rx);
        true
    }

    fn drain_events(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.event_rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: MpvEvent) {
        match event.name.as_str() {
            "file-loaded" => self.activate_pending(),
            "end-file" => self.finish_active(event.reason.as_deref()),
            "shutdown" => {
                self.finish_active(Some("quit"));
                self.reset_mpv();
            }
            "property-change" => self.apply_property(event.property.as_deref(), event.data),
            _ => {}
        }
    }

    fn activate_pending(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.last_state.position_ticks = self.last_state.position_ticks.max(
            pending
                .launch
                .start_seconds()
                .and_then(seconds_to_ticks)
                .unwrap_or_default(),
        );
        if let Some(reporter) = pending.reporter {
            reporter.report_start(&self.last_state);
            self.active = Some(ActivePlayback {
                reporter,
                last_progress_sent: Instant::now(),
                last_pause: self.last_state.pause,
            });
        }
    }

    fn finish_active(&mut self, reason: Option<&str>) {
        if self.pending.is_some() {
            let failed = matches!(reason, Some("error"));
            if failed
                && let Some(pending) = self.pending.take()
                && let Some(reporter) = pending.reporter
            {
                reporter.report_stopped(&self.last_state, true);
            }
        }

        let Some(active) = self.active.take() else {
            return;
        };
        let failed = matches!(reason, Some("error"));
        if matches!(reason, Some("eof"))
            && let Some(duration) = self.last_state.duration_ticks
        {
            self.last_state.position_ticks = duration;
        }
        active.reporter.report_stopped(&self.last_state, failed);
    }

    fn apply_property(&mut self, property: Option<&str>, data: Option<Value>) {
        match property {
            Some("time-pos") | Some("playback-time") => {
                if let Some(ticks) = data
                    .and_then(|value| value.as_f64())
                    .and_then(seconds_to_ticks)
                {
                    self.last_state.position_ticks = ticks;
                }
            }
            Some("pause") => {
                if let Some(value) = data.and_then(|value| value.as_bool()) {
                    self.last_state.pause = value;
                }
            }
            Some("duration") => {
                self.last_state.duration_ticks = data
                    .and_then(|value| value.as_f64())
                    .and_then(seconds_to_ticks);
            }
            Some("volume") => {
                self.last_state.volume = data
                    .and_then(|value| value.as_f64())
                    .map(|value| value.round() as i64);
            }
            Some("mute") => {
                self.last_state.mute = data.and_then(|value| value.as_bool());
            }
            Some("eof-reached") => {
                self.last_state.eof_reached =
                    data.and_then(|value| value.as_bool()).unwrap_or(false);
            }
            Some("playback-abort") => {
                if data.and_then(|value| value.as_bool()).unwrap_or(false) {
                    self.finish_active(Some("error"));
                }
            }
            _ => {}
        }
    }

    fn maybe_report_progress(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };
        let now = Instant::now();
        let due = now.saturating_duration_since(active.last_progress_sent) >= PROGRESS_INTERVAL;
        if due || active.last_pause != self.last_state.pause {
            active.reporter.report_progress(&self.last_state);
            active.last_progress_sent = now;
            active.last_pause = self.last_state.pause;
        }
    }

    fn poll_child(&mut self) {
        let Some(child) = &mut self.child else {
            return;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            self.finish_active(Some("quit"));
            self.reset_mpv();
        }
    }

    fn child_is_alive(&mut self) -> bool {
        self.child
            .as_mut()
            .map(|child| matches!(child.try_wait(), Ok(None)))
            .unwrap_or(false)
    }

    fn is_duplicate(&mut self, key: &str) -> bool {
        self.prune_recent_loads();
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.key == key)
            || self
                .recent_loads
                .iter()
                .any(|load| load.key == key && load.seen_at.elapsed() <= DUPLICATE_DEBOUNCE)
    }

    fn prune_recent_loads(&mut self) {
        while self
            .recent_loads
            .front()
            .is_some_and(|load| load.seen_at.elapsed() > DUPLICATE_DEBOUNCE)
        {
            self.recent_loads.pop_front();
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.requested_at.elapsed() > IPC_CONNECT_TIMEOUT)
        {
            self.finish_active(Some("error"));
            self.pending = None;
        }
    }

    fn shutdown(&mut self) {
        self.finish_active(Some("quit"));
        let _ = self.send_mpv_command(json!({ "command": ["quit"] }));
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        while Instant::now() < deadline {
            if !self.child_is_alive() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let still_alive = self.child_is_alive();
        if still_alive && let Some(child) = &mut self.child {
            let _ = child.kill();
        }
        self.reset_mpv();
    }

    fn reset_mpv(&mut self) {
        if let Some(mut child) = self.child.take() {
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        if let Some(path) = self.ipc_path.take() {
            cleanup_ipc_path(&path);
        }
        self.event_rx = None;
        if let Some(worker) = self.ipc_worker.take() {
            worker.shutdown();
        }
    }

    fn send_mpv_command(&self, command: Value) -> io::Result<()> {
        let Some(worker) = &self.ipc_worker else {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "mpv IPC worker is not connected",
            ));
        };
        worker.send(command)
    }
}

pub fn loadfile_command(launch: &MpvLaunch) -> Value {
    let mut options = Map::new();
    if let Some(start_seconds) = launch.start_seconds() {
        options.insert("start".to_string(), json!(format!("{start_seconds:.3}")));
    }
    if let Some(title) = non_empty(launch.title.as_deref()) {
        options.insert(
            "force-media-title".to_string(),
            json!(sanitize_option(title)),
        );
    }
    let headers = mpv_headers(launch);
    if !headers.is_empty() {
        options.insert(
            "http-header-fields".to_string(),
            json!(mpv_string_list(
                headers
                    .iter()
                    .map(|header| format!("{}: {}", header.name, header.value))
            )),
        );
    }

    json!({
        "command": ["loadfile", launch.media_url, "replace", -1, Value::Object(options)],
        "request_id": next_request_id(),
    })
}

fn next_request_id() -> i64 {
    REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
}

impl IpcWorker {
    fn start(path: &str) -> io::Result<(Self, Receiver<MpvEvent>)> {
        let mut reader = connect_ipc(path)?;
        write_observe_commands(&mut reader)?;

        let (event_tx, event_rx) = mpsc::channel();
        let reader_thread = thread::spawn(move || read_events(reader, event_tx));
        Ok((
            Self {
                path: path.to_string(),
                reader_thread,
            },
            event_rx,
        ))
    }

    fn send(&self, command: Value) -> io::Result<()> {
        let (ack, ack_rx) = mpsc::channel();
        let path = self.path.clone();
        thread::spawn(move || {
            let result = write_command_to_new_connection(&path, command);
            let _ = ack.send(result);
        });
        ack_rx
            .recv_timeout(IPC_COMMAND_TIMEOUT)
            .unwrap_or_else(|_| {
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "mpv IPC write timed out",
                ))
            })
    }

    fn shutdown(self) {
        let _ = self.reader_thread.join();
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
        "playback-abort",
    ] {
        let command = json!({
            "command": ["observe_property", next_request_id(), property],
            "request_id": next_request_id(),
        });
        serde_json::to_writer(&mut *stream, &command)?;
        stream.write_all(b"\n")?;
    }
    stream.flush()
}

fn write_command_to_new_connection(path: &str, command: Value) -> io::Result<()> {
    let mut stream = connect_ipc(path)?;
    serde_json::to_writer(&mut stream, &command)?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn read_events(stream: IpcConnection, tx: Sender<MpvEvent>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let Some(name) = value.get("event").and_then(Value::as_str) else {
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
                };
                if tx.send(event).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn start_ipc_worker(path: &str, timeout: Duration) -> io::Result<(IpcWorker, Receiver<MpvEvent>)> {
    let deadline = Instant::now() + timeout;
    let mut last_error = None;
    while Instant::now() < deadline {
        match IpcWorker::start(path) {
            Ok(worker) => return Ok(worker),
            Err(error) => last_error = Some(error),
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(last_error
        .unwrap_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "mpv IPC did not become ready")))
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

#[cfg(all(unix, not(target_os = "windows")))]
type IpcConnection = std::os::unix::net::UnixStream;

#[cfg(all(unix, not(target_os = "windows")))]
fn connect_ipc(path: &str) -> io::Result<IpcConnection> {
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(stream)
}

fn mpv_headers(launch: &MpvLaunch) -> Vec<HttpHeader> {
    let mut headers = Vec::<HttpHeader>::new();
    for header in &launch.headers {
        let name = sanitize_header_name(&header.name);
        let value = sanitize_option(header.value.trim());
        if name.is_empty() || value.is_empty() || !is_forwarded_header(&name) {
            continue;
        }
        if !headers
            .iter()
            .any(|existing| existing.name.eq_ignore_ascii_case(&name))
        {
            headers.push(HttpHeader { name, value });
        }
    }
    if !headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case("X-Emby-Token"))
        && let Some(token) = query_auth_token(&launch.media_url)
            .map(|value| sanitize_option(&value))
            .filter(|value| !value.is_empty())
    {
        headers.push(HttpHeader {
            name: "X-Emby-Token".to_string(),
            value: token,
        });
    }
    headers
}

fn is_forwarded_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "x-emby-authorization"
            | "x-emby-token"
            | "x-mediabrowser-token"
            | "cookie"
            | "user-agent"
            | "referer"
            | "origin"
    )
}

fn mpv_string_list(values: impl IntoIterator<Item = String>) -> String {
    values
        .into_iter()
        .map(|value| value.replace('\\', "\\\\").replace(',', "\\,"))
        .collect::<Vec<_>>()
        .join(",")
}

fn sanitize_header_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>()
}

fn sanitize_option(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '\0' | '\r' | '\n'))
        .collect::<String>()
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn query_auth_token(url: &str) -> Option<String> {
    [
        "api_key",
        "apikey",
        "access_token",
        "accesstoken",
        "x-emby-token",
        "x-mediabrowser-token",
    ]
    .into_iter()
    .find_map(|key| query_param_ci(url, key))
}

fn query_param_ci(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next().unwrap_or_default();
    query.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        percent_decode(raw_key)
            .eq_ignore_ascii_case(key)
            .then(|| percent_decode(raw_value))
    })
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{loadfile_command, mpv_string_list};
    use crate::external_mpv::{HttpHeader, MpvLaunch};

    #[test]
    fn loadfile_command_contains_url_replace_options_and_request_id() {
        let mut launch = MpvLaunch::new("https://example.test/video.mkv");
        launch.start_time_ticks = Some(20_000_000);
        launch.title = Some("A Movie".to_string());

        let command = loadfile_command(&launch);
        let args = command["command"].as_array().expect("command array");
        assert_eq!(args[0], "loadfile");
        assert_eq!(args[1], "https://example.test/video.mkv");
        assert_eq!(args[2], "replace");
        assert_eq!(args[3], -1);
        assert!(command["request_id"].as_i64().is_some());
        assert_eq!(args[4]["start"], "2.000");
        assert_eq!(args[4]["force-media-title"], "A Movie");
    }

    #[test]
    fn loadfile_filters_and_escapes_headers_for_mpv_string_list() {
        let mut launch = MpvLaunch::new("https://example.test/video.mkv");
        launch.headers = vec![
            HttpHeader {
                name: "Authorization".to_string(),
                value: "MediaBrowser Client=\"Jellyfin Web\", Token=\"abc,def\"".to_string(),
            },
            HttpHeader {
                name: "Host".to_string(),
                value: "evil.test".to_string(),
            },
        ];

        let command = loadfile_command(&launch);
        let headers = command["command"][4]["http-header-fields"]
            .as_str()
            .expect("header list");
        assert!(headers.contains(
            "Authorization: MediaBrowser Client=\"Jellyfin Web\"\\, Token=\"abc\\,def\""
        ));
        assert!(!headers.contains("Host:"));
    }

    #[test]
    fn loadfile_adds_token_header_from_url_when_missing() {
        let launch = MpvLaunch::new("https://example.test/video.mkv?ApiKey=secret");
        let command = loadfile_command(&launch);
        let headers = command["command"][4]["http-header-fields"]
            .as_str()
            .expect("header list");
        assert_eq!(headers, "X-Emby-Token: secret");
    }

    #[test]
    fn escapes_mpv_string_list_commas_and_backslashes() {
        assert_eq!(
            mpv_string_list(["a,b".to_string(), r"c\d".to_string()]),
            r"a\,b,c\\d"
        );
    }
}
