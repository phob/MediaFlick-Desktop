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
use crate::logger;
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

#[derive(Debug, Clone, Copy)]
pub enum MpvControlCommand {
    SetPause(bool),
    SeekMilliseconds(f64),
    SetVolume(f64),
    SetMute(bool),
    SetPlaybackRate(f64),
    Stop,
}

enum ControllerMessage {
    Load {
        mpv_path: String,
        launch: Box<MpvLaunch>,
    },
    Control(MpvControlCommand),
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
    last_position_log_bucket: Option<i64>,
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
    raw: Value,
}

impl MpvEvent {
    fn summary(&self) -> String {
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
            name => name.to_string(),
        }
    }
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

    pub fn control(&self, command: MpvControlCommand) {
        let _ = self.tx.send(ControllerMessage::Control(command));
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
            last_position_log_bucket: None,
            recent_loads: VecDeque::new(),
        }
    }

    fn run(mut self) {
        tracing::debug!(target: "playback", "mpv controller thread started");
        loop {
            match self.rx.recv_timeout(Duration::from_millis(200)) {
                Ok(ControllerMessage::Load { mpv_path, launch }) => {
                    tracing::debug!(target: "playback", "received playback load request");
                    self.load(mpv_path, *launch);
                }
                Ok(ControllerMessage::Control(command)) => {
                    tracing::debug!(target: "playback", ?command, "received playback control request");
                    self.control(command);
                }
                Ok(ControllerMessage::Shutdown) => {
                    tracing::debug!(target: "playback", "received playback shutdown request");
                    self.shutdown();
                    return;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    tracing::debug!(target: "playback", "controller channel disconnected");
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

    fn control(&mut self, command: MpvControlCommand) {
        let Some(command_json) = control_command(command) else {
            tracing::debug!(target: "mpv.ipc", ?command, "ignored invalid mpv control command");
            return;
        };
        if let Err(error) = self.send_mpv_command(command_json) {
            tracing::warn!(target: "mpv.ipc", ?command, "failed to send mpv control command: {error}");
        }
    }

    fn load(&mut self, mpv_path: String, launch: MpvLaunch) {
        let key = launch.dedupe_key();
        tracing::debug!(
            target: "playback",
            dedupe_key = %key,
            launch = %logger::launch_summary(&launch),
            "handling playback load"
        );
        if self.is_duplicate(&key) {
            tracing::debug!(
                target: "playback",
                dedupe_key = %key,
                "ignored duplicate playback load"
            );
            return;
        }

        if !self.ensure_mpv(&mpv_path) {
            tracing::warn!(
                target: "playback",
                mpv_path = %mpv_path,
                "cannot load playback because mpv is unavailable"
            );
            return;
        };

        let reporter = PlaybackReporter::from_launch(&launch);
        if let Some(active) = self.active.take() {
            tracing::info!(
                target: "playback",
                state = %self.last_state,
                "stopping previous active playback before loading replacement"
            );
            active.reporter.report_stopped(&self.last_state, false);
        }

        match self.send_mpv_command(loadfile_command(&launch)) {
            Ok(()) => {
                tracing::info!(
                    target: "playback",
                    item_id = %launch.item_id.as_deref().unwrap_or("unknown"),
                    url = %jellyfin_bridge::redact_url_secrets(&launch.media_url),
                    "loaded Jellyfin stream in mpv"
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
                tracing::warn!(target: "mpv.ipc", "failed to send mpv loadfile command: {error}");
                self.reset_mpv();
            }
        }
    }

    fn ensure_mpv(&mut self, mpv_path: &str) -> bool {
        if self.child_is_alive() {
            tracing::trace!(
                target: "mpv.ipc",
                connected = self.ipc_worker.is_some(),
                "reusing existing mpv process"
            );
            return self.ipc_worker.is_some();
        }

        self.reset_mpv();
        let ipc_path = make_mpv_ipc_path();
        let mpv = ExternalMpv::new(PathBuf::from(mpv_path));
        tracing::info!(
            target: "mpv.ipc",
            executable = %mpv.executable().display(),
            ipc_path = %ipc_path,
            "starting idle mpv process"
        );
        let child = match mpv.command_for_idle_with_ipc(&ipc_path).spawn() {
            Ok(child) => child,
            Err(error) => {
                tracing::warn!(
                    target: "mpv.ipc",
                    executable = %mpv.executable().display(),
                    "failed to launch mpv for Jellyfin stream: {error}"
                );
                cleanup_ipc_path(&ipc_path);
                return false;
            }
        };

        let (ipc_worker, event_rx) = match start_ipc_worker(&ipc_path, IPC_CONNECT_TIMEOUT) {
            Ok(worker) => worker,
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", ipc_path = %ipc_path, "failed to connect mpv IPC: {error}");
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
        tracing::info!(target: "mpv.ipc", ipc_path = %ipc_path, "mpv IPC connected");
        true
    }

    fn drain_events(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.event_rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        if !events.is_empty() {
            tracing::trace!(target: "mpv.ipc", count = events.len(), "drained mpv events");
        }
        for event in events {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: MpvEvent) {
        tracing::debug!(target: "mpv.ipc", event = %event.summary(), "received mpv event");
        tracing::trace!(
            target: "mpv.ipc",
            event = %logger::redacted_json(&event.raw),
            "received raw mpv event"
        );
        match event.name.as_str() {
            "file-loaded" => self.activate_pending(),
            "end-file" => self.finish_active(event.reason.as_deref()),
            "shutdown" => {
                self.finish_active(Some("quit"));
                self.reset_mpv();
            }
            "property-change" => self.apply_property(event.property.as_deref(), event.data),
            _ => tracing::trace!(target: "mpv.ipc", name = %event.name, "ignored mpv event"),
        }
    }

    fn activate_pending(&mut self) {
        let Some(pending) = self.pending.take() else {
            tracing::debug!(target: "playback", "mpv reported file-loaded without pending playback");
            return;
        };
        tracing::info!(
            target: "playback",
            dedupe_key = %pending.key,
            launch = %logger::launch_summary(&pending.launch),
            state = %self.last_state,
            "activating pending playback"
        );
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
        } else {
            tracing::debug!(
                target: "jellyfin.playstate",
                "activated playback without Jellyfin reporter"
            );
        }
    }

    fn finish_active(&mut self, reason: Option<&str>) {
        tracing::debug!(
            target: "playback",
            reason = reason.unwrap_or("unknown"),
            state = %self.last_state,
            "finishing playback"
        );
        if self.pending.is_some() {
            let failed = matches!(reason, Some("error"));
            if failed
                && let Some(pending) = self.pending.take()
                && let Some(reporter) = pending.reporter
            {
                tracing::warn!(
                    target: "playback",
                    reason = reason.unwrap_or("unknown"),
                    "pending playback failed before activation"
                );
                reporter.report_stopped(&self.last_state, true);
            }
        }

        let Some(active) = self.active.take() else {
            tracing::trace!(target: "playback", "no active playback to finish");
            return;
        };
        let failed = matches!(reason, Some("error"));
        if matches!(reason, Some("eof"))
            && let Some(duration) = self.last_state.duration_ticks
        {
            self.last_state.position_ticks = duration;
        }
        tracing::info!(
            target: "playback",
            failed,
            reason = reason.unwrap_or("unknown"),
            state = %self.last_state,
            "reporting active playback stopped"
        );
        active.reporter.report_stopped(&self.last_state, failed);
    }

    fn apply_property(&mut self, property: Option<&str>, data: Option<Value>) {
        tracing::trace!(
            target: "mpv.ipc",
            property = property.unwrap_or("unknown"),
            data = %data
                .as_ref()
                .map(logger::redacted_json)
                .unwrap_or_else(|| "null".to_string()),
            "applying mpv property"
        );
        match property {
            Some("time-pos") | Some("playback-time") => {
                if let Some(ticks) = data
                    .as_ref()
                    .and_then(Value::as_f64)
                    .and_then(seconds_to_ticks)
                {
                    let previous = self.last_state.position_ticks;
                    self.last_state.position_ticks = ticks;
                    self.log_position_change(property.unwrap_or("time"), previous, ticks);
                }
            }
            Some("pause") => {
                if let Some(value) = data.as_ref().and_then(Value::as_bool) {
                    let previous = self.last_state.pause;
                    self.last_state.pause = value;
                    if previous != value {
                        tracing::debug!(
                            target: "playback",
                            previous,
                            current = value,
                            state = %self.last_state,
                            "mpv pause state changed"
                        );
                    }
                }
            }
            Some("duration") => {
                self.last_state.duration_ticks = data
                    .as_ref()
                    .and_then(Value::as_f64)
                    .and_then(seconds_to_ticks);
                tracing::debug!(
                    target: "playback",
                    state = %self.last_state,
                    "mpv duration changed"
                );
            }
            Some("volume") => {
                let previous = self.last_state.volume;
                self.last_state.volume = data
                    .as_ref()
                    .and_then(|value| value.as_f64())
                    .map(|value| value.round() as i64);
                if previous != self.last_state.volume {
                    tracing::debug!(
                        target: "playback",
                        previous = ?previous,
                        current = ?self.last_state.volume,
                        state = %self.last_state,
                        "mpv volume changed"
                    );
                }
            }
            Some("mute") => {
                let previous = self.last_state.mute;
                self.last_state.mute = data.as_ref().and_then(Value::as_bool);
                if previous != self.last_state.mute {
                    tracing::debug!(
                        target: "playback",
                        previous = ?previous,
                        current = ?self.last_state.mute,
                        state = %self.last_state,
                        "mpv mute state changed"
                    );
                }
            }
            Some("eof-reached") => {
                let previous = self.last_state.eof_reached;
                self.last_state.eof_reached =
                    data.as_ref().and_then(Value::as_bool).unwrap_or(false);
                if previous != self.last_state.eof_reached {
                    tracing::debug!(
                        target: "playback",
                        previous,
                        current = self.last_state.eof_reached,
                        state = %self.last_state,
                        "mpv eof state changed"
                    );
                }
            }
            Some("playback-abort") => {
                if data.as_ref().and_then(Value::as_bool).unwrap_or(false) {
                    tracing::debug!(
                        target: "playback",
                        pending = self.pending.is_some(),
                        active = self.active.is_some(),
                        state = %self.last_state,
                        "mpv playback-abort is true; waiting for end-file before finishing playback"
                    );
                }
            }
            Some(other) => {
                tracing::trace!(target: "mpv.ipc", property = other, "ignored mpv property")
            }
            None => tracing::trace!(target: "mpv.ipc", "ignored mpv property with no name"),
        }
    }

    fn log_position_change(&mut self, property: &str, previous: i64, current: i64) {
        tracing::trace!(
            target: "playback",
            property,
            previous_ticks = previous,
            current_ticks = current,
            state = %self.last_state,
            "mpv playback position changed"
        );

        let bucket = current / 100_000_000;
        if self.last_position_log_bucket != Some(bucket) {
            self.last_position_log_bucket = Some(bucket);
            tracing::debug!(
                target: "playback",
                property,
                previous_ticks = previous,
                current_ticks = current,
                state = %self.last_state,
                "mpv playback position sample"
            );
        }
    }

    fn maybe_report_progress(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };
        let now = Instant::now();
        let due = now.saturating_duration_since(active.last_progress_sent) >= PROGRESS_INTERVAL;
        if due || active.last_pause != self.last_state.pause {
            tracing::debug!(
                target: "jellyfin.playstate",
                due,
                pause_changed = active.last_pause != self.last_state.pause,
                state = %self.last_state,
                "Jellyfin playback progress report due"
            );
            active.reporter.report_progress(&self.last_state);
            active.last_progress_sent = now;
            active.last_pause = self.last_state.pause;
        }
    }

    fn poll_child(&mut self) {
        let Some(child) = &mut self.child else {
            return;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                tracing::info!(target: "mpv.ipc", %status, "mpv process exited");
                self.finish_active(Some("quit"));
                self.reset_mpv();
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", "failed to poll mpv process: {error}");
            }
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
            tracing::warn!(
                target: "playback",
                timeout_ms = IPC_CONNECT_TIMEOUT.as_millis(),
                "pending playback timed out waiting for mpv file-loaded"
            );
            self.finish_active(Some("error"));
            self.pending = None;
        }
    }

    fn shutdown(&mut self) {
        tracing::debug!(target: "playback", state = %self.last_state, "shutting down mpv controller");
        self.finish_active(Some("quit"));
        if let Err(error) = self.send_mpv_command(json!({ "command": ["quit"] })) {
            tracing::debug!(target: "mpv.ipc", "failed to send mpv quit during shutdown: {error}");
        }
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        while Instant::now() < deadline {
            if !self.child_is_alive() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let still_alive = self.child_is_alive();
        if still_alive && let Some(child) = &mut self.child {
            tracing::warn!(target: "mpv.ipc", "mpv did not exit before shutdown deadline; killing process");
            let _ = child.kill();
        }
        self.reset_mpv();
    }

    fn reset_mpv(&mut self) {
        tracing::debug!(target: "mpv.ipc", "resetting mpv process and IPC state");
        if let Some(mut child) = self.child.take() {
            if matches!(child.try_wait(), Ok(None)) {
                tracing::debug!(target: "mpv.ipc", "killing live mpv process during reset");
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        if let Some(path) = self.ipc_path.take() {
            tracing::trace!(target: "mpv.ipc", ipc_path = %path, "cleaning mpv IPC path");
            cleanup_ipc_path(&path);
        }
        self.event_rx = None;
        if let Some(worker) = self.ipc_worker.take() {
            worker.shutdown();
        }
        self.last_position_log_bucket = None;
    }

    fn send_mpv_command(&self, command: Value) -> io::Result<()> {
        let Some(worker) = &self.ipc_worker else {
            tracing::warn!(
                target: "mpv.ipc",
                command = %logger::mpv_command_summary(&command),
                "cannot send mpv command because IPC worker is not connected"
            );
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "mpv IPC worker is not connected",
            ));
        };
        tracing::debug!(
            target: "mpv.ipc",
            command = %logger::mpv_command_summary(&command),
            "sending mpv command"
        );
        tracing::trace!(
            target: "mpv.ipc",
            command = %logger::redacted_json(&command),
            "sending raw mpv command"
        );
        let result = worker.send(command);
        match &result {
            Ok(()) => tracing::debug!(target: "mpv.ipc", "sent mpv command"),
            Err(error) => tracing::warn!(target: "mpv.ipc", "mpv command send failed: {error}"),
        }
        result
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

pub fn control_command(command: MpvControlCommand) -> Option<Value> {
    let command = match command {
        MpvControlCommand::SetPause(pause) => {
            json!(["set_property", "pause", pause])
        }
        MpvControlCommand::SeekMilliseconds(position_ms) => {
            if !position_ms.is_finite() {
                return None;
            }
            let seconds = (position_ms / 1000.0).max(0.0);
            json!(["seek", seconds, "absolute+exact"])
        }
        MpvControlCommand::SetVolume(volume) => {
            if !volume.is_finite() {
                return None;
            }
            json!(["set_property", "volume", volume.clamp(0.0, 100.0)])
        }
        MpvControlCommand::SetMute(mute) => {
            json!(["set_property", "mute", mute])
        }
        MpvControlCommand::SetPlaybackRate(rate) => {
            if !rate.is_finite() {
                return None;
            }
            json!(["set_property", "speed", rate.clamp(0.1, 10.0)])
        }
        MpvControlCommand::Stop => json!(["stop"]),
    };

    Some(json!({
        "command": command,
        "request_id": next_request_id(),
    }))
}

fn next_request_id() -> i64 {
    REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
}

impl IpcWorker {
    fn start(path: &str) -> io::Result<(Self, Receiver<MpvEvent>)> {
        tracing::trace!(target: "mpv.ipc", ipc_path = path, "connecting mpv IPC reader");
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
        tracing::trace!(target: "mpv.ipc", ipc_path = %self.path, "joining mpv IPC reader thread");
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

fn write_command_to_new_connection(path: &str, command: Value) -> io::Result<()> {
    tracing::trace!(
        target: "mpv.ipc",
        ipc_path = path,
        command = %logger::mpv_command_summary(&command),
        "opening mpv IPC command connection"
    );
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
                    tracing::trace!(
                        target: "mpv.ipc",
                        line = %logger::redact_text(&line),
                        "ignored malformed mpv IPC line"
                    );
                    continue;
                };
                let Some(name) = value.get("event").and_then(Value::as_str) else {
                    tracing::trace!(
                        target: "mpv.ipc",
                        value = %logger::redacted_json(&value),
                        "ignored mpv IPC reply without event name"
                    );
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
                    raw: value,
                };
                if tx.send(event).is_err() {
                    tracing::trace!(target: "mpv.ipc", "mpv event receiver dropped");
                    break;
                }
            }
            Err(error) => {
                tracing::trace!(target: "mpv.ipc", "mpv IPC read failed: {error}");
                break;
            }
        }
    }
    tracing::trace!(target: "mpv.ipc", "mpv IPC reader stopped");
}

fn start_ipc_worker(path: &str, timeout: Duration) -> io::Result<(IpcWorker, Receiver<MpvEvent>)> {
    let deadline = Instant::now() + timeout;
    let mut last_error = None;
    tracing::debug!(
        target: "mpv.ipc",
        ipc_path = path,
        timeout_ms = timeout.as_millis(),
        "waiting for mpv IPC"
    );
    while Instant::now() < deadline {
        match IpcWorker::start(path) {
            Ok(worker) => return Ok(worker),
            Err(error) => {
                tracing::trace!(target: "mpv.ipc", ipc_path = path, "mpv IPC not ready yet: {error}");
                last_error = Some(error);
            }
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
    use super::{
        ControllerState, MpvControlCommand, PendingPlayback, control_command, loadfile_command,
        mpv_string_list,
    };
    use crate::external_mpv::{HttpHeader, MpvLaunch};
    use serde_json::json;
    use std::sync::mpsc;
    use std::time::Instant;

    fn controller_with_pending_load(start_time_ticks: Option<i64>) -> ControllerState {
        let (_tx, rx) = mpsc::channel();
        let mut launch = MpvLaunch::new("https://example.test/video.mkv?ApiKey=secret");
        launch.start_time_ticks = start_time_ticks;

        let mut state = ControllerState::new(rx);
        state.pending = Some(PendingPlayback {
            key: "test-load".to_string(),
            launch,
            reporter: None,
            requested_at: Instant::now(),
        });
        state
    }

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

    #[test]
    fn control_commands_map_to_mpv_ipc_commands() {
        let pause = control_command(MpvControlCommand::SetPause(true)).expect("pause command");
        assert_eq!(pause["command"], json!(["set_property", "pause", true]));

        let seek =
            control_command(MpvControlCommand::SeekMilliseconds(12_345.0)).expect("seek command");
        assert_eq!(seek["command"], json!(["seek", 12.345, "absolute+exact"]));

        let volume = control_command(MpvControlCommand::SetVolume(250.0)).expect("volume command");
        assert_eq!(volume["command"], json!(["set_property", "volume", 100.0]));

        assert!(control_command(MpvControlCommand::SetPlaybackRate(f64::NAN)).is_none());
    }

    #[test]
    fn playback_abort_snapshot_does_not_fail_pending_load() {
        let mut state = controller_with_pending_load(Some(20_000_000));

        state.apply_property(Some("playback-abort"), Some(json!(true)));

        assert!(state.pending.is_some());
        state.activate_pending();
        assert!(state.pending.is_none());
        assert_eq!(state.last_state.position_ticks, 20_000_000);
    }

    #[test]
    fn end_file_error_still_fails_pending_load() {
        let mut state = controller_with_pending_load(None);

        state.finish_active(Some("error"));

        assert!(state.pending.is_none());
    }
}
