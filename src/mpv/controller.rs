use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::app::logger;
use crate::app::settings::{MpvFullscreenBehavior, SegmentSkipConfig, SegmentSkipMode};
use crate::jellyfin::bridge::{self as jellyfin_bridge, PlaybackContext};
use crate::jellyfin::media_segments::{self, SegmentType, SkipSegment};
use crate::jellyfin::playback_reporter::{
    MpvPlaybackState, PlaybackReporter, TICKS_PER_SECOND, cleanup_ipc_path, make_mpv_ipc_path,
    seconds_to_ticks,
};
use crate::mpv::input::{INPUT_SECTION_NAME, MARK_WATCHED_NEXT_COMMAND, MpvInputBindings};
use crate::mpv::{ExternalMpv, HttpHeader, MpvLaunch};

#[path = "playback_transition.rs"]
mod playback_transition;

const IPC_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IPC_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const IPC_COMMAND_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const PENDING_FILE_LOADED_TIMEOUT: Duration = Duration::from_secs(60);
const NEXT_PLAYBACK_HANDOFF_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(windows)]
const MPV_RAISE_PULSE_DELAY: Duration = Duration::from_millis(150);
#[cfg(not(windows))]
const MPV_RAISE_PULSE_DELAY: Duration = Duration::from_millis(1200);
const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);
const DUPLICATE_DEBOUNCE: Duration = Duration::from_secs(2);
const MPV_SESSION_POLL_INTERVAL: Duration = Duration::from_secs(2);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);
const CONTROLLER_SHUTDOWN_ACK_TIMEOUT: Duration = Duration::from_secs(25);
const STARTUP_SEEK_DELAY: Duration = Duration::from_millis(500);
const STARTUP_SEEK_RETRY_DELAY: Duration = Duration::from_secs(1);
const STARTUP_SEEK_POSITION_TOLERANCE: i64 = 30_000_000;
const SEGMENT_SKIP_OSD_DURATION_MS: i64 = 3000;
const SEGMENT_SKIP_OSD_DEBOUNCE: Duration = Duration::from_secs(3);
const SEGMENT_AUTO_SKIP_DELAY: Duration = Duration::from_secs(3);
const SEGMENT_AUTO_SKIP_COUNTDOWN_INTERVAL: Duration = Duration::from_secs(1);
const SEGMENT_AUTO_SKIP_COUNTDOWN_OSD_DURATION_MS: i64 = 1200;

static REQUEST_COUNTER: AtomicI64 = AtomicI64::new(100);
static PLAYBACK_COUNTER: AtomicI64 = AtomicI64::new(1);

#[derive(Clone)]
pub struct MpvController {
    tx: Sender<ControllerMessage>,
    snapshot: Arc<Mutex<MpvPlayerSnapshot>>,
    shutdown_requested: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Default)]
pub struct MpvPlayerSnapshot {
    pub active: bool,
    pub playback_id: Option<i64>,
    pub item_id: Option<String>,
    pub media_source_id: Option<String>,
    pub play_session_id: Option<String>,
    pub position_ms: f64,
    pub duration_ms: Option<f64>,
    pub paused: bool,
    pub volume: Option<i64>,
    pub mute: Option<bool>,
    pub stop_reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub enum MpvPlaybackEvent {
    Stopped(MpvPlayerSnapshot),
}

#[derive(Debug, Clone)]
pub enum MpvControlCommand {
    SetPause(bool),
    SeekMilliseconds(f64),
    SetVolume(f64),
    SetMute(bool),
    SetPlaybackRate(f64),
    SetAudioTrack(i64),
    SetSubtitleTrack(Option<i64>),
    AddSubtitle(String),
    Stop,
}

enum ControllerMessage {
    Warm {
        mpv_path: String,
        fullscreen: MpvFullscreenBehavior,
    },
    Load {
        mpv_path: String,
        fullscreen: MpvFullscreenBehavior,
        launch: Box<MpvLaunch>,
    },
    PlaybackContext(Box<PlaybackContext>),
    Control(MpvControlCommand),
    SegmentSkipConfig(SegmentSkipConfig),
    MediaSegmentsFetched {
        playback_id: i64,
        result: Result<Vec<SkipSegment>, String>,
    },
    Shutdown {
        ack: Sender<()>,
    },
}

#[derive(Debug, Clone)]
struct RecentLoad {
    key: String,
    seen_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct PendingAutoSkip {
    segment_index: usize,
    due_at: Instant,
    next_countdown_at: Instant,
}

struct ControllerState {
    tx: Sender<ControllerMessage>,
    rx: Receiver<ControllerMessage>,
    snapshot: Arc<Mutex<MpvPlayerSnapshot>>,
    child: Option<Child>,
    configured_mpv: Option<ConfiguredMpv>,
    current_mpv_path: Option<String>,
    ipc_path: Option<String>,
    ipc_worker: Option<IpcWorker>,
    event_rx: Option<Receiver<MpvEvent>>,
    active: Option<ActivePlayback>,
    pending: Option<PendingPlayback>,
    playback_identity: Option<PlaybackIdentity>,
    startup_seek: Option<StartupSeek>,
    mpv_playback_active: bool,
    playback_runtime_ticks: Option<i64>,
    last_state: MpvPlaybackState,
    last_position_log_bucket: Option<i64>,
    skip_segments: Vec<SkipSegment>,
    current_skip_segment: Option<usize>,
    pending_auto_skip: Option<PendingAutoSkip>,
    last_skip_osd_at: Option<Instant>,
    seek_started_at_ticks: Option<i64>,
    segment_skip_config: SegmentSkipConfig,
    recent_loads: VecDeque<RecentLoad>,
    next_playback_handoff_until: Option<Instant>,
    pending_raise_pulse_reset_at: Option<Instant>,
    last_session_poll: Instant,
    event_tx: Option<Sender<MpvPlaybackEvent>>,
    shutdown_requested: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
struct ConfiguredMpv {
    mpv_path: String,
    fullscreen: MpvFullscreenBehavior,
}

#[derive(Debug, Clone)]
struct PlaybackIdentity {
    playback_id: i64,
    item_id: Option<String>,
    media_source_id: Option<String>,
    play_session_id: Option<String>,
}

impl PlaybackIdentity {
    fn from_launch(playback_id: i64, launch: &MpvLaunch) -> Self {
        Self {
            playback_id,
            item_id: launch.item_id.clone(),
            media_source_id: launch.media_source_id.clone(),
            play_session_id: launch.play_session_id.clone(),
        }
    }
}

struct PendingPlayback {
    key: String,
    identity: PlaybackIdentity,
    launch: MpvLaunch,
    reporter: Option<PlaybackReporter>,
    requested_at: Instant,
}

struct ActivePlayback {
    identity: PlaybackIdentity,
    reporter: PlaybackReporter,
    runtime_ticks: Option<i64>,
    last_progress_sent: Instant,
    last_pause: bool,
}

#[derive(Debug, Clone, Copy)]
struct StartupSeek {
    position_ms: f64,
    due_at: Instant,
    sent_at: Option<Instant>,
}

#[derive(Debug)]
struct MpvEvent {
    name: String,
    reason: Option<String>,
    property: Option<String>,
    data: Option<Value>,
    args: Vec<String>,
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
            "client-message" => format!("client-message args={:?}", self.args),
            name => name.to_string(),
        }
    }

    fn is_position_property_change(&self) -> bool {
        self.name == "property-change"
            && matches!(self.property.as_deref(), Some("time-pos" | "playback-time"))
    }
}

struct IpcWorker {
    path: String,
    command_tx: Sender<IpcCommand>,
    reader_thread: thread::JoinHandle<()>,
    writer_thread: thread::JoinHandle<()>,
    writer_alive: Arc<AtomicBool>,
}

type IpcCommand = (Value, Sender<io::Result<()>>);

struct IpcCommandWriter {
    stream: IpcConnection,
    alive: Arc<AtomicBool>,
}

impl Drop for IpcCommandWriter {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
    }
}

impl MpvController {
    pub fn new(
        event_tx: Option<Sender<MpvPlaybackEvent>>,
        segment_skip_config: SegmentSkipConfig,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let snapshot = Arc::new(Mutex::new(MpvPlayerSnapshot::default()));
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let controller_snapshot = snapshot.clone();
        let controller_shutdown_requested = shutdown_requested.clone();
        let controller_tx = tx.clone();
        thread::spawn(move || {
            ControllerState::new(
                controller_tx,
                rx,
                controller_snapshot,
                event_tx,
                controller_shutdown_requested,
                segment_skip_config,
            )
            .run()
        });
        Self {
            tx,
            snapshot,
            shutdown_requested,
        }
    }

    pub fn warm(&self, mpv_path: impl Into<String>, fullscreen: MpvFullscreenBehavior) {
        let _ = self.tx.send(ControllerMessage::Warm {
            mpv_path: mpv_path.into(),
            fullscreen,
        });
    }

    pub fn load(
        &self,
        mpv_path: impl Into<String>,
        fullscreen: MpvFullscreenBehavior,
        launch: MpvLaunch,
    ) {
        let _ = self.tx.send(ControllerMessage::Load {
            mpv_path: mpv_path.into(),
            fullscreen,
            launch: Box::new(launch),
        });
    }

    pub fn control(&self, command: MpvControlCommand) {
        let _ = self.tx.send(ControllerMessage::Control(command));
    }

    pub fn set_segment_skip_config(&self, config: SegmentSkipConfig) {
        let _ = self.tx.send(ControllerMessage::SegmentSkipConfig(config));
    }

    pub fn update_playback_context(&self, context: PlaybackContext) {
        let _ = self
            .tx
            .send(ControllerMessage::PlaybackContext(Box::new(context)));
    }

    pub fn snapshot(&self) -> MpvPlayerSnapshot {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }

    pub fn shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
        let (ack, ack_rx) = mpsc::channel();
        if self.tx.send(ControllerMessage::Shutdown { ack }).is_err() {
            return;
        }
        if ack_rx
            .recv_timeout(CONTROLLER_SHUTDOWN_ACK_TIMEOUT)
            .is_err()
        {
            tracing::warn!(target: "mpv.ipc", "timed out waiting for mpv controller shutdown acknowledgement");
        }
    }
}

impl ControllerState {
    fn new(
        tx: Sender<ControllerMessage>,
        rx: Receiver<ControllerMessage>,
        snapshot: Arc<Mutex<MpvPlayerSnapshot>>,
        event_tx: Option<Sender<MpvPlaybackEvent>>,
        shutdown_requested: Arc<AtomicBool>,
        segment_skip_config: SegmentSkipConfig,
    ) -> Self {
        Self {
            tx,
            rx,
            snapshot,
            child: None,
            configured_mpv: None,
            current_mpv_path: None,
            ipc_path: None,
            ipc_worker: None,
            event_rx: None,
            active: None,
            pending: None,
            playback_identity: None,
            startup_seek: None,
            mpv_playback_active: false,
            playback_runtime_ticks: None,
            last_state: MpvPlaybackState {
                volume: Some(100),
                ..Default::default()
            },
            last_position_log_bucket: None,
            skip_segments: Vec::new(),
            current_skip_segment: None,
            pending_auto_skip: None,
            last_skip_osd_at: None,
            seek_started_at_ticks: None,
            segment_skip_config,
            recent_loads: VecDeque::new(),
            next_playback_handoff_until: None,
            pending_raise_pulse_reset_at: None,
            last_session_poll: Instant::now(),
            event_tx,
            shutdown_requested,
        }
    }

    fn run(mut self) {
        tracing::debug!(target: "playback", "mpv controller thread started");
        loop {
            match self.rx.recv_timeout(Duration::from_millis(200)) {
                Ok(ControllerMessage::Warm {
                    mpv_path,
                    fullscreen,
                }) => {
                    tracing::debug!(target: "playback", "received mpv warm request");
                    self.warm(mpv_path, fullscreen);
                }
                Ok(ControllerMessage::Load {
                    mpv_path,
                    fullscreen,
                    launch,
                }) => {
                    tracing::debug!(target: "playback", "received playback load request");
                    self.load(mpv_path, fullscreen, *launch);
                }
                Ok(ControllerMessage::PlaybackContext(context)) => {
                    tracing::debug!(target: "playback", "received playback context update");
                    self.update_active_playback_context(*context);
                }
                Ok(ControllerMessage::Control(command)) => {
                    tracing::debug!(target: "playback", ?command, "received playback control request");
                    self.control(command);
                }
                Ok(ControllerMessage::SegmentSkipConfig(config)) => {
                    tracing::debug!(target: "playback", ?config, "updated segment skip settings");
                    self.segment_skip_config = config;
                    self.pending_auto_skip = None;
                    if config.all_disabled() {
                        self.clear_skip_segment_state();
                    } else {
                        self.update_skip_segment_state(
                            self.last_state.position_ticks,
                            self.last_state.position_ticks,
                        );
                    }
                }
                Ok(ControllerMessage::MediaSegmentsFetched {
                    playback_id,
                    result,
                }) => self.handle_media_segments_fetched(playback_id, result),
                Ok(ControllerMessage::Shutdown { ack }) => {
                    tracing::debug!(target: "playback", "received playback shutdown request");
                    self.shutdown();
                    let _ = ack.send(());
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
            self.maybe_send_startup_seek();
            self.poll_child();
            self.maybe_poll_mpv_session();
            self.maybe_finish_mpv_raise();
            self.maybe_update_auto_skip_countdown();
            self.maybe_report_progress();
            self.prune_recent_loads();
        }
    }

    fn control(&mut self, command: MpvControlCommand) {
        if matches!(command, MpvControlCommand::Stop)
            && self.should_suppress_stop_during_next_playback_handoff()
        {
            tracing::debug!(
                target: "playback",
                "ignored stop control while next playback handoff is waiting for file-loaded"
            );
            return;
        }

        if self.handle_prompt_skip_control(&command) {
            return;
        }

        let Some(command_json) = control_command(command.clone()) else {
            tracing::debug!(target: "mpv.ipc", ?command, "ignored invalid mpv control command");
            return;
        };

        if !self.ensure_configured_mpv_running("player control preflight") {
            tracing::warn!(target: "mpv.ipc", ?command, "cannot send mpv control command because no session is available");
            return;
        }
        if let Err(error) = self.send_mpv_command(command_json) {
            tracing::warn!(target: "mpv.ipc", ?command, "failed to send mpv control command: {error}");
            self.handle_mpv_session_lost("mpv control command failed");
        }
    }

    fn kick_start_playback(&mut self, launch: &MpvLaunch) {
        // Regression guard: resumed Jellyfin streams must not use mpv's
        // load-time `start` option. On Windows external mpv can show a still
        // frame until a later seek when opened directly at the resume offset.
        // Match shim's safer shape: load normally, then seek after file-loaded.
        if let Some(position_ms) = launch
            .start_seconds()
            .map(|seconds| seconds * 1000.0)
            .filter(|position_ms| position_ms.is_finite() && *position_ms > 0.0)
        {
            tracing::debug!(
                target: "playback",
                position_ms,
                delay_ms = STARTUP_SEEK_DELAY.as_millis(),
                "queued mpv startup seek after file load"
            );
            self.startup_seek = Some(StartupSeek {
                position_ms,
                due_at: Instant::now() + STARTUP_SEEK_DELAY,
                sent_at: None,
            });
            return;
        }

        self.startup_seek = None;
    }

    fn maybe_send_startup_seek(&mut self) {
        let Some(startup_seek) = self.startup_seek else {
            return;
        };
        let now = Instant::now();
        if now < startup_seek.due_at {
            return;
        }

        tracing::debug!(
            target: "playback",
            position_ms = startup_seek.position_ms,
            retry = startup_seek.sent_at.is_some(),
            "sending delayed mpv startup seek"
        );
        if let Some(command) = control_command(MpvControlCommand::SeekMilliseconds(
            startup_seek.position_ms,
        )) {
            match self.send_mpv_command(command) {
                Ok(()) => {
                    if let Some(startup_seek) = &mut self.startup_seek {
                        startup_seek.sent_at = Some(now);
                        startup_seek.due_at = now + STARTUP_SEEK_RETRY_DELAY;
                    }
                }
                Err(error) => {
                    tracing::warn!(target: "mpv.ipc", "failed to send mpv startup seek command: {error}");
                    self.handle_mpv_session_lost("mpv startup seek command failed");
                }
            }
        }
    }

    fn load(&mut self, mpv_path: String, fullscreen: MpvFullscreenBehavior, launch: MpvLaunch) {
        self.remember_configured_mpv(&mpv_path, fullscreen);
        let key = launch.dedupe_key();
        let identity = PlaybackIdentity::from_launch(next_playback_id(), &launch);
        tracing::debug!(
            target: "playback",
            playback_id = identity.playback_id,
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
        if let Some(pending) = &self.pending {
            tracing::debug!(
                target: "playback",
                pending_dedupe_key = %pending.key,
                incoming_dedupe_key = %key,
                "ignored playback load while another load is pending"
            );
            return;
        }

        if !self.ensure_mpv(&mpv_path, fullscreen) {
            tracing::warn!(
                target: "playback",
                mpv_path = %mpv_path,
                "cannot load playback because mpv is unavailable"
            );
            return;
        };
        self.apply_default_fullscreen(fullscreen);

        let reporter = PlaybackReporter::from_launch(&launch);
        self.startup_seek = None;
        if let Some(active) = self.active.take() {
            tracing::info!(
                target: "playback",
                state = %self.last_state,
                "stopping previous active playback before loading replacement"
            );
            active.reporter.report_stopped(&self.last_state, false);
        }

        match self.send_loadfile_with_reconnect(&mpv_path, fullscreen, &launch) {
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
                self.clear_skip_segment_state();
                self.mpv_playback_active = true;
                self.playback_identity = Some(identity.clone());
                let playback_id = identity.playback_id;
                let pending_launch = launch.clone();
                self.pending = Some(PendingPlayback {
                    key,
                    identity,
                    launch,
                    reporter,
                    requested_at: Instant::now(),
                });
                self.fetch_media_segments(playback_id, pending_launch);
                self.prepare_pending_playback_state();
                self.schedule_mpv_raise("loadfile accepted");
                self.publish_snapshot();
            }
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", "failed to send mpv loadfile command after reconnect attempt: {error}");
                self.mpv_playback_active = false;
                self.handle_mpv_session_lost("loadfile command failed");
            }
        }
    }

    fn warm(&mut self, mpv_path: String, fullscreen: MpvFullscreenBehavior) {
        tracing::debug!(
            target: "mpv.ipc",
            mpv_path = %mpv_path,
            fullscreen = %fullscreen.fullscreen_arg(),
            "warming idle mpv process"
        );
        self.remember_configured_mpv(&mpv_path, fullscreen);
        if self.ensure_mpv(&mpv_path, fullscreen) {
            self.apply_default_fullscreen(fullscreen);
        }
    }

    fn remember_configured_mpv(&mut self, mpv_path: &str, fullscreen: MpvFullscreenBehavior) {
        self.configured_mpv = Some(ConfiguredMpv {
            mpv_path: mpv_path.to_string(),
            fullscreen,
        });
    }

    fn apply_default_fullscreen(&self, fullscreen: MpvFullscreenBehavior) {
        let command = json!({
            "command": ["set_property", "fullscreen", fullscreen == MpvFullscreenBehavior::Fullscreen],
            "request_id": next_request_id(),
        });
        if let Err(error) = self.send_mpv_command(command) {
            tracing::warn!(target: "mpv.ipc", "failed to apply default fullscreen behavior: {error}");
        }
    }

    fn send_loadfile_with_reconnect(
        &mut self,
        mpv_path: &str,
        fullscreen: MpvFullscreenBehavior,
        launch: &MpvLaunch,
    ) -> io::Result<()> {
        match self.send_mpv_command(loadfile_command(launch)) {
            Ok(()) => Ok(()),
            Err(first_error) => {
                tracing::warn!(target: "mpv.ipc", "failed to send mpv loadfile command; restarting session and retrying once: {first_error}");
                self.reset_mpv();
                if !self.ensure_mpv(mpv_path, fullscreen) {
                    return Err(first_error);
                }
                self.apply_default_fullscreen(fullscreen);
                self.send_mpv_command(loadfile_command(launch))
                    .map_err(|retry_error| {
                        io::Error::new(
                            retry_error.kind(),
                            format!("initial failure: {first_error}; retry failure: {retry_error}"),
                        )
                    })
            }
        }
    }

    fn ensure_mpv(&mut self, mpv_path: &str, fullscreen: MpvFullscreenBehavior) -> bool {
        if self.shutdown_requested.load(Ordering::SeqCst) {
            tracing::debug!(target: "mpv.ipc", "skipping mpv start because shutdown is requested");
            return false;
        }

        if self.child_is_alive() {
            if !self.current_mpv_path_matches(mpv_path) {
                tracing::info!(
                    target: "mpv.ipc",
                    previous = %self.current_mpv_path.as_deref().unwrap_or("unknown"),
                    next = %mpv_path,
                    "restarting mpv because the configured executable changed"
                );
                self.finish_active(Some("quit"));
                self.reset_mpv();
            } else if self
                .ipc_worker
                .as_ref()
                .is_some_and(IpcWorker::is_writer_alive)
            {
                tracing::trace!(
                    target: "mpv.ipc",
                    "reusing existing mpv process"
                );
                return true;
            } else {
                tracing::warn!(
                    target: "mpv.ipc",
                    "restarting mpv because the tracked process has no live IPC worker"
                );
                self.finish_active(Some("quit"));
                self.reset_mpv();
            }
        } else {
            self.reset_mpv();
        }
        let ipc_path = make_mpv_ipc_path();
        let mpv = ExternalMpv::new(PathBuf::from(mpv_path));
        tracing::info!(
            target: "mpv.ipc",
            executable = %mpv.executable().display(),
            ipc_path = %ipc_path,
            "starting idle mpv process"
        );
        let mut child = match mpv
            .command_for_idle_with_ipc_and_fullscreen(&ipc_path, fullscreen)
            .spawn()
        {
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

        let (ipc_worker, event_rx) = match start_ipc_worker(
            &ipc_path,
            IPC_CONNECT_TIMEOUT,
            &mut child,
            &self.shutdown_requested,
        ) {
            Ok(worker) => worker,
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", ipc_path = %ipc_path, "failed to connect mpv IPC: {error}");
                if matches!(child.try_wait(), Ok(None)) {
                    let _ = child.kill();
                }
                let _ = child.wait();
                cleanup_ipc_path(&ipc_path);
                return false;
            }
        };
        if self.shutdown_requested.load(Ordering::SeqCst) {
            tracing::debug!(target: "mpv.ipc", "closing newly connected mpv because shutdown is requested");
            let _ = child.kill();
            let _ = child.wait();
            cleanup_ipc_path(&ipc_path);
            ipc_worker.shutdown();
            return false;
        }

        self.child = Some(child);
        self.current_mpv_path = Some(mpv_path.to_string());
        self.ipc_path = Some(ipc_path.clone());
        self.ipc_worker = Some(ipc_worker);
        self.event_rx = Some(event_rx);
        tracing::info!(target: "mpv.ipc", ipc_path = %ipc_path, "mpv IPC connected");
        self.install_input_bindings();
        true
    }

    fn current_mpv_path_matches(&self, mpv_path: &str) -> bool {
        self.current_mpv_path
            .as_deref()
            .is_some_and(|current| equivalent_mpv_path(current, mpv_path))
    }

    fn install_input_bindings(&self) {
        let bindings = MpvInputBindings::load();
        let section_contents = bindings.section_contents();

        let define = json!({
            "command": ["define-section", INPUT_SECTION_NAME, section_contents, "force"],
            "request_id": next_request_id(),
        });
        if let Err(error) = self.send_mpv_command(define) {
            tracing::warn!(target: "mpv.ipc", "failed to define mpv input bindings: {error}");
            return;
        }

        let enable = json!({
            "command": ["enable-section", INPUT_SECTION_NAME, "allow-hide-cursor+allow-vo-dragging"],
            "request_id": next_request_id(),
        });
        if let Err(error) = self.send_mpv_command(enable) {
            tracing::warn!(target: "mpv.ipc", "failed to enable mpv input bindings: {error}");
        }
    }

    fn drain_events(&mut self) {
        let mut events = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = &self.event_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => events.push(event),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if !events.is_empty() {
            tracing::trace!(target: "mpv.ipc", count = events.len(), "drained mpv events");
        }
        let saw_shutdown = events.iter().any(|event| event.name == "shutdown");
        for event in events {
            self.handle_event(event);
        }
        if disconnected && !saw_shutdown {
            tracing::warn!(target: "mpv.ipc", "mpv IPC event stream disconnected");
            self.handle_mpv_session_lost("mpv IPC event stream disconnected");
        }
    }

    fn handle_event(&mut self, event: MpvEvent) {
        if event.is_position_property_change() {
            tracing::trace!(target: "mpv.ipc", event = %event.summary(), "received mpv position event");
        } else {
            tracing::debug!(target: "mpv.ipc", event = %event.summary(), "received mpv event");
        }
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
                self.restart_configured_mpv("mpv emitted shutdown");
            }
            "seek" => self.handle_seek_event(),
            "property-change" => self.apply_property(event.property.as_deref(), event.data),
            "client-message" if is_mark_watched_next_message(&event.args) => {
                self.mark_watched_and_play_next();
            }
            _ => tracing::trace!(target: "mpv.ipc", name = %event.name, "ignored mpv event"),
        }
    }

    fn load_external_subtitle(&mut self, launch: &MpvLaunch) {
        let Some(subtitle_url) = non_empty(launch.subtitle_url.as_deref()) else {
            return;
        };
        tracing::debug!(
            target: "mpv.ipc",
            subtitle_url = %jellyfin_bridge::redact_url_secrets(subtitle_url),
            "loading selected external Jellyfin subtitle in mpv"
        );
        if let Some(command) =
            control_command(MpvControlCommand::AddSubtitle(subtitle_url.to_string()))
            && let Err(error) = self.send_mpv_command(command)
        {
            tracing::warn!(target: "mpv.ipc", "failed to load selected external subtitle: {error}");
        }
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
                    if self.defer_startup_position_update(ticks) {
                        return;
                    }
                    let previous = self.last_state.position_ticks;
                    self.last_state.position_ticks = ticks;
                    self.log_position_change(property.unwrap_or("time"), previous, ticks);
                    if !self.maybe_accept_pending_native_seek(ticks) {
                        self.update_skip_segment_state(previous, ticks);
                    }
                    self.publish_snapshot();
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
                    self.publish_snapshot();
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
                self.publish_snapshot();
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
                self.publish_snapshot();
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
                self.publish_snapshot();
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
            Some("seeking") => {
                if let Some(value) = data.as_ref().and_then(Value::as_bool) {
                    self.handle_seeking_property(value);
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

    fn handle_media_segments_fetched(
        &mut self,
        playback_id: i64,
        result: Result<Vec<SkipSegment>, String>,
    ) {
        if !self.playback_id_is_current(playback_id) {
            tracing::debug!(
                target: "jellyfin.media_segments",
                playback_id,
                "ignored stale Jellyfin media segments response"
            );
            return;
        }
        if self.segment_skip_config.all_disabled() {
            tracing::debug!(
                target: "jellyfin.media_segments",
                playback_id,
                "ignored Jellyfin media segments because segment skipping is disabled"
            );
            return;
        }
        match result {
            Ok(segments) => {
                tracing::debug!(
                    target: "jellyfin.media_segments",
                    playback_id,
                    count = segments.len(),
                    "stored Jellyfin media segments"
                );
                self.skip_segments = segments;
                self.current_skip_segment = None;
                self.last_skip_osd_at = None;
                self.update_skip_segment_state(
                    self.last_state.position_ticks,
                    self.last_state.position_ticks,
                );
            }
            Err(error) => tracing::warn!(
                target: "jellyfin.media_segments",
                playback_id,
                "failed to fetch Jellyfin media segments: {error}"
            ),
        }
    }

    fn fetch_media_segments(&self, playback_id: i64, launch: MpvLaunch) {
        if self.segment_skip_config.all_disabled() {
            return;
        }
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = media_segments::fetch_for_launch(&launch);
            let _ = tx.send(ControllerMessage::MediaSegmentsFetched {
                playback_id,
                result,
            });
        });
    }

    fn playback_id_is_current(&self, playback_id: i64) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.identity.playback_id == playback_id)
            || self
                .active
                .as_ref()
                .is_some_and(|active| active.identity.playback_id == playback_id)
            || (self.mpv_playback_active
                && self
                    .playback_identity
                    .as_ref()
                    .is_some_and(|identity| identity.playback_id == playback_id))
    }

    pub(super) fn clear_skip_segment_state(&mut self) {
        self.skip_segments.clear();
        self.current_skip_segment = None;
        self.pending_auto_skip = None;
        self.last_skip_osd_at = None;
        self.seek_started_at_ticks = None;
    }

    fn handle_prompt_skip_control(&mut self, command: &MpvControlCommand) -> bool {
        let MpvControlCommand::SeekMilliseconds(position_ms) = command else {
            return false;
        };
        let Some(requested_ticks) = seconds_to_ticks(position_ms / 1000.0) else {
            return false;
        };
        let current_ticks = self.last_state.position_ticks;
        if requested_ticks <= current_ticks {
            return false;
        }
        let Some(index) = self.prompt_segment_index_at(current_ticks) else {
            return false;
        };
        tracing::debug!(
            target: "playback",
            current_ticks,
            requested_ticks,
            "treating forward seek as segment skip"
        );
        self.skip_segment(index, "web forward seek accepted segment skip prompt")
    }

    fn handle_seek_event(&mut self) {
        let current_ticks = self.last_state.position_ticks;
        if self.prompt_segment_index_at(current_ticks).is_some() {
            self.seek_started_at_ticks = Some(current_ticks);
            tracing::debug!(
                target: "playback",
                current_ticks,
                "recorded native seek start inside prompt segment"
            );
        }
    }

    fn handle_seeking_property(&mut self, seeking: bool) {
        if seeking {
            if self.seek_started_at_ticks.is_none() {
                self.seek_started_at_ticks = Some(self.last_state.position_ticks);
            }
            return;
        }
        self.maybe_accept_pending_native_seek(self.last_state.position_ticks);
    }

    fn maybe_accept_pending_native_seek(&mut self, current_ticks: i64) -> bool {
        let Some(start_ticks) = self.seek_started_at_ticks else {
            return false;
        };
        if current_ticks == start_ticks {
            return false;
        }
        self.seek_started_at_ticks = None;
        if current_ticks < start_ticks {
            tracing::debug!(
                target: "playback",
                start_ticks,
                current_ticks,
                "ignored native backward seek during segment skip prompt"
            );
            return false;
        }
        let Some(index) = self.prompt_segment_index_at(start_ticks) else {
            return false;
        };
        tracing::debug!(
            target: "playback",
            start_ticks,
            current_ticks,
            "treating native forward seek as segment skip"
        );
        self.skip_segment(index, "native forward seek accepted segment skip prompt")
    }

    fn update_skip_segment_state(&mut self, _previous_ticks: i64, current_ticks: i64) {
        if self.skip_segments.is_empty() || self.segment_skip_config.all_disabled() {
            self.current_skip_segment = None;
            self.pending_auto_skip = None;
            return;
        }

        let Some(index) = self.current_segment_index_at(current_ticks) else {
            self.current_skip_segment = None;
            self.pending_auto_skip = None;
            return;
        };
        let was_current = self.current_skip_segment == Some(index);
        self.current_skip_segment = Some(index);
        match self.mode_for_segment(self.skip_segments[index].segment_type) {
            SegmentSkipMode::Disabled => {
                self.pending_auto_skip = None;
            }
            SegmentSkipMode::Prompt => {
                self.pending_auto_skip = None;
                self.maybe_show_skip_prompt(index, !was_current);
            }
            SegmentSkipMode::Always => self.start_auto_skip_countdown(index),
        }
    }

    fn start_auto_skip_countdown(&mut self, index: usize) {
        if self
            .pending_auto_skip
            .is_some_and(|pending| pending.segment_index == index)
        {
            return;
        }
        if self
            .skip_segments
            .get(index)
            .is_none_or(|segment| segment.triggered)
        {
            return;
        }

        let now = Instant::now();
        let due_at = now + SEGMENT_AUTO_SKIP_DELAY;
        self.pending_auto_skip = Some(PendingAutoSkip {
            segment_index: index,
            due_at,
            next_countdown_at: now + SEGMENT_AUTO_SKIP_COUNTDOWN_INTERVAL,
        });
        self.show_auto_skip_countdown(index, SEGMENT_AUTO_SKIP_DELAY.as_secs().max(1));
    }

    fn maybe_update_auto_skip_countdown(&mut self) {
        let Some(pending) = self.pending_auto_skip else {
            return;
        };
        if !self.auto_skip_is_still_valid(pending.segment_index) {
            self.pending_auto_skip = None;
            return;
        }

        let now = Instant::now();
        if now >= pending.due_at {
            self.pending_auto_skip = None;
            self.skip_segment(
                pending.segment_index,
                "automatic segment skip after countdown",
            );
            return;
        }

        if now >= pending.next_countdown_at {
            let remaining = pending
                .due_at
                .saturating_duration_since(now)
                .as_millis()
                .div_ceil(1000)
                .max(1) as u64;
            self.show_auto_skip_countdown(pending.segment_index, remaining);
            if let Some(current) = &mut self.pending_auto_skip
                && current.segment_index == pending.segment_index
            {
                current.next_countdown_at = now + SEGMENT_AUTO_SKIP_COUNTDOWN_INTERVAL;
            }
        }
    }

    fn auto_skip_is_still_valid(&self, index: usize) -> bool {
        let Some(segment) = self.skip_segments.get(index) else {
            return false;
        };
        !segment.triggered
            && self.mode_for_segment(segment.segment_type) == SegmentSkipMode::Always
            && self.last_state.position_ticks >= segment.start_ticks
            && self.last_state.position_ticks < segment.end_ticks
    }

    fn show_auto_skip_countdown(&self, index: usize, remaining_seconds: u64) {
        let Some(segment) = self.skip_segments.get(index) else {
            return;
        };
        let label = match segment.segment_type {
            SegmentType::Intro => "Intro",
            SegmentType::Outro => "Credits",
        };
        let command = json!({
            "command": [
                "show-text",
                format!("Skipping {label} in {remaining_seconds}..."),
                SEGMENT_AUTO_SKIP_COUNTDOWN_OSD_DURATION_MS,
                1
            ],
            "request_id": next_request_id(),
        });
        if let Err(error) = self.send_mpv_command(command) {
            tracing::warn!(target: "mpv.ipc", "failed to show automatic segment skip countdown: {error}");
        }
    }

    fn maybe_show_skip_prompt(&mut self, index: usize, entered_segment: bool) {
        let now = Instant::now();
        if !entered_segment
            && self.last_skip_osd_at.is_some_and(|shown_at| {
                now.saturating_duration_since(shown_at) < SEGMENT_SKIP_OSD_DEBOUNCE
            })
        {
            return;
        }
        let Some(segment) = self.skip_segments.get(index) else {
            return;
        };
        let text = segment.segment_type.prompt_text();
        let command = json!({
            "command": ["show-text", text, SEGMENT_SKIP_OSD_DURATION_MS, 1],
            "request_id": next_request_id(),
        });
        if let Err(error) = self.send_mpv_command(command) {
            tracing::warn!(target: "mpv.ipc", "failed to show segment skip prompt: {error}");
            return;
        }
        self.last_skip_osd_at = Some(now);
    }

    fn skip_segment(&mut self, index: usize, reason: &'static str) -> bool {
        let current_ticks = self.last_state.position_ticks;
        let Some(segment) = self.skip_segments.get(index) else {
            return false;
        };
        if segment.triggered {
            return false;
        }
        let end_ticks = segment.end_ticks;
        let segment_type = segment.segment_type;

        self.current_skip_segment = None;
        self.pending_auto_skip = None;
        self.last_skip_osd_at = Some(Instant::now());
        if end_ticks <= current_ticks {
            self.mark_segment_triggered(index);
            tracing::debug!(
                target: "playback",
                reason,
                current_ticks,
                end_ticks,
                "segment skip target is behind current position"
            );
            return false;
        }

        let seconds = end_ticks as f64 / TICKS_PER_SECOND;
        let command = json!({
            "command": ["seek", seconds, "absolute+exact"],
            "request_id": next_request_id(),
        });
        match self.send_mpv_command(command) {
            Ok(()) => {
                self.mark_segment_triggered(index);
                tracing::info!(
                    target: "playback",
                    reason,
                    current_ticks,
                    end_ticks,
                    "skipped Jellyfin media segment"
                );
                let text = segment_type.skipped_text();
                let command = json!({
                    "command": ["show-text", text, SEGMENT_SKIP_OSD_DURATION_MS, 1],
                    "request_id": next_request_id(),
                });
                if let Err(error) = self.send_mpv_command(command) {
                    tracing::warn!(target: "mpv.ipc", "failed to show segment skipped OSD: {error}");
                }
                true
            }
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", reason, "failed to skip Jellyfin media segment: {error}");
                self.handle_mpv_session_lost("segment skip command failed");
                false
            }
        }
    }

    fn mark_segment_triggered(&mut self, index: usize) {
        if let Some(segment) = self.skip_segments.get_mut(index) {
            segment.triggered = true;
        }
    }

    fn current_segment_index_at(&self, ticks: i64) -> Option<usize> {
        self.skip_segments.iter().position(|segment| {
            !segment.triggered && ticks >= segment.start_ticks && ticks < segment.end_ticks
        })
    }

    fn prompt_segment_index_at(&self, ticks: i64) -> Option<usize> {
        self.skip_segments.iter().position(|segment| {
            !segment.triggered
                && self.mode_for_segment(segment.segment_type) == SegmentSkipMode::Prompt
                && ticks >= segment.start_ticks
                && ticks < segment.end_ticks
        })
    }

    fn mode_for_segment(&self, segment_type: SegmentType) -> SegmentSkipMode {
        match segment_type {
            SegmentType::Intro => self.segment_skip_config.intro,
            SegmentType::Outro => self.segment_skip_config.credits,
        }
    }

    fn defer_startup_position_update(&mut self, ticks: i64) -> bool {
        let Some(startup_seek) = self.startup_seek else {
            return false;
        };
        // mpv reports 0.0 immediately after file-loaded even for resumed media.
        // Do not let that transient sample overwrite Jellyfin/Web's resume
        // position before the delayed startup seek has landed.
        let target_ticks = seconds_to_ticks(startup_seek.position_ms / 1000.0).unwrap_or_default();
        let minimum_resume_tick = target_ticks.saturating_sub(STARTUP_SEEK_POSITION_TOLERANCE);
        if target_ticks > 0 && ticks < minimum_resume_tick {
            tracing::trace!(
                target: "playback",
                current_ticks = ticks,
                target_ticks,
                "holding Jellyfin position while mpv startup seek is pending"
            );
            return true;
        }

        tracing::debug!(
            target: "playback",
            current_ticks = ticks,
            target_ticks,
            seek_sent = startup_seek.sent_at.is_some(),
            "mpv startup seek reached resume range"
        );
        self.startup_seek = None;
        false
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
            tracing::trace!(
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
                self.restart_configured_mpv("mpv process exited");
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", "failed to poll mpv process: {error}");
            }
        }
    }

    fn maybe_poll_mpv_session(&mut self) {
        let now = Instant::now();
        if now.saturating_duration_since(self.last_session_poll) < MPV_SESSION_POLL_INTERVAL {
            return;
        }
        self.last_session_poll = now;
        self.ensure_configured_mpv_running("scheduled mpv session poll");
    }

    fn ensure_configured_mpv_running(&mut self, reason: &'static str) -> bool {
        if self.shutdown_requested.load(Ordering::SeqCst) {
            tracing::trace!(target: "mpv.ipc", reason, "not ensuring mpv session because shutdown is requested");
            return false;
        }
        let Some(config) = self.configured_mpv.clone() else {
            tracing::trace!(target: "mpv.ipc", reason, "no configured mpv executable to supervise");
            return false;
        };
        if self.configured_mpv_session_ready(&config) {
            return true;
        }
        tracing::info!(
            target: "mpv.ipc",
            reason,
            mpv_path = %config.mpv_path,
            "ensuring configured mpv session is running"
        );
        if self.ensure_mpv(&config.mpv_path, config.fullscreen) {
            self.apply_default_fullscreen(config.fullscreen);
            true
        } else {
            false
        }
    }

    fn configured_mpv_session_ready(&mut self, config: &ConfiguredMpv) -> bool {
        self.child_is_alive()
            && self.current_mpv_path_matches(&config.mpv_path)
            && self
                .ipc_worker
                .as_ref()
                .is_some_and(IpcWorker::is_writer_alive)
    }

    fn handle_mpv_session_lost(&mut self, reason: &'static str) {
        self.finish_active(Some("quit"));
        self.reset_mpv();
        self.restart_configured_mpv(reason);
    }

    fn restart_configured_mpv(&mut self, reason: &'static str) {
        if self.shutdown_requested.load(Ordering::SeqCst) {
            tracing::debug!(target: "mpv.ipc", reason, "not restarting mpv because shutdown is requested");
            return;
        }
        let Some(config) = self.configured_mpv.clone() else {
            tracing::debug!(target: "mpv.ipc", reason, "not restarting mpv because no executable is configured");
            return;
        };
        tracing::info!(
            target: "mpv.ipc",
            reason,
            mpv_path = %config.mpv_path,
            "restarting idle mpv process"
        );
        if self.ensure_mpv(&config.mpv_path, config.fullscreen) {
            self.apply_default_fullscreen(config.fullscreen);
        }
    }

    fn schedule_mpv_raise(&mut self, reason: &'static str) {
        if self.begin_mpv_raise(reason) {
            self.pending_raise_pulse_reset_at = Some(Instant::now() + MPV_RAISE_PULSE_DELAY);
        }
    }

    fn maybe_finish_mpv_raise(&mut self) {
        let Some(due_at) = self.pending_raise_pulse_reset_at else {
            return;
        };
        if Instant::now() < due_at {
            return;
        }
        self.pending_raise_pulse_reset_at = None;
        self.finish_mpv_raise();
    }

    #[cfg(windows)]
    fn begin_mpv_raise(&self, reason: &'static str) -> bool {
        self.set_mpv_bool_property("window-minimized", true, reason)
    }

    #[cfg(windows)]
    fn finish_mpv_raise(&self) {
        self.set_mpv_bool_property("window-minimized", false, "raise pulse restore");
    }

    #[cfg(not(windows))]
    fn begin_mpv_raise(&self, reason: &'static str) -> bool {
        self.set_mpv_bool_property("ontop", true, reason)
    }

    #[cfg(not(windows))]
    fn finish_mpv_raise(&self) {
        self.set_mpv_bool_property("ontop", false, "raise pulse reset");
    }

    fn set_mpv_bool_property(&self, property: &str, value: bool, reason: &'static str) -> bool {
        let command = json!({
            "command": ["set_property", property, value],
            "request_id": next_request_id(),
        });
        match self.send_mpv_command(command) {
            Ok(()) => {
                tracing::debug!(target: "mpv.focus", reason, property, value, "set mpv property for window raise");
                true
            }
            Err(error) => {
                tracing::trace!(target: "mpv.focus", reason, property, value, "failed to set mpv property for window raise: {error}");
                false
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
            .is_some_and(|pending| pending.requested_at.elapsed() > PENDING_FILE_LOADED_TIMEOUT)
        {
            tracing::warn!(
                target: "playback",
                timeout_ms = PENDING_FILE_LOADED_TIMEOUT.as_millis(),
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
        self.startup_seek = None;
        self.pending_raise_pulse_reset_at = None;
        self.clear_skip_segment_state();
        if self.active.is_none() && self.pending.is_none() {
            self.playback_runtime_ticks = None;
            self.mpv_playback_active = false;
        }
        if let Some(mut child) = self.child.take() {
            if matches!(child.try_wait(), Ok(None)) {
                tracing::debug!(target: "mpv.ipc", "killing live mpv process during reset");
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        self.current_mpv_path = None;
        if let Some(path) = self.ipc_path.take() {
            tracing::trace!(target: "mpv.ipc", ipc_path = %path, "cleaning mpv IPC path");
            cleanup_ipc_path(&path);
        }
        self.event_rx = None;
        if let Some(worker) = self.ipc_worker.take() {
            worker.shutdown();
        }
        self.last_position_log_bucket = None;
        self.publish_snapshot();
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
    // Intentionally do not set mpv's `start` option here. Resume is performed
    // by a delayed absolute seek after file-loaded; see kick_start_playback.
    if let Some(title) = non_empty(launch.title.as_deref()) {
        options.insert(
            "force-media-title".to_string(),
            json!(sanitize_option(title)),
        );
    }
    if let Some(audio_id) = launch.audio_mpv_id.filter(|id| *id > 0) {
        options.insert("aid".to_string(), json!(audio_id.to_string()));
    }
    if non_empty(launch.subtitle_url.as_deref()).is_some() {
        // Avoid briefly showing an automatically selected embedded subtitle;
        // the selected external Jellyfin subtitle is added with `sub-add select`
        // after mpv reports file-loaded.
        options.insert("sid".to_string(), json!("no"));
    } else if let Some(subtitle_id) = launch.subtitle_mpv_id {
        let value = if subtitle_id > 0 {
            subtitle_id.to_string()
        } else {
            "no".to_string()
        };
        options.insert("sid".to_string(), json!(value));
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
        "command": ["loadfile", media_url_without_fragment(&launch.media_url), "replace", -1, Value::Object(options)],
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
        MpvControlCommand::SetAudioTrack(id) => {
            if id <= 0 {
                return None;
            }
            json!(["set_property", "aid", id])
        }
        MpvControlCommand::SetSubtitleTrack(id) => match id.filter(|id| *id > 0) {
            Some(id) => json!(["set_property", "sid", id]),
            None => json!(["set_property", "sid", "no"]),
        },
        MpvControlCommand::AddSubtitle(url) => {
            let url = non_empty(Some(url.as_str()))?;
            json!(["sub-add", url, "select"])
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

fn next_playback_id() -> i64 {
    PLAYBACK_COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn media_url_without_fragment(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
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
            stream: connect_ipc_for_commands_with_timeout(path, IPC_COMMAND_CONNECT_TIMEOUT)?,
            alive: writer_alive.clone(),
        };

        let (event_tx, event_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel();
        let reader_thread = thread::spawn(move || read_events(reader, event_tx));
        let writer_thread = thread::spawn(move || writer.write_commands(command_rx));
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

    fn is_writer_alive(&self) -> bool {
        self.writer_alive.load(Ordering::SeqCst)
    }

    fn send(&self, command: Value) -> io::Result<()> {
        let (ack, ack_rx) = mpsc::channel();
        self.command_tx
            .send((command, ack))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "mpv IPC writer stopped"))?;
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
    fn write_commands(mut self, rx: Receiver<IpcCommand>) {
        while let Ok((command, ack)) = rx.recv() {
            tracing::trace!(
                target: "mpv.ipc",
                command = %logger::mpv_command_summary(&command),
                "writing mpv IPC command"
            );
            let result = write_command(&mut self.stream, &command);
            let failed = result.is_err();
            let _ = ack.send(result);
            if failed {
                break;
            }
        }
        tracing::trace!(target: "mpv.ipc", "mpv IPC writer stopped");
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

fn start_ipc_worker(
    path: &str,
    timeout: Duration,
    child: &mut Child,
    shutdown_requested: &AtomicBool,
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
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    format!("mpv exited before IPC became ready: {status}"),
                ));
            }
            Ok(None) => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "failed to poll mpv while waiting for IPC: {error}"
                )));
            }
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
    std::fs::OpenOptions::new().write(true).open(path)
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
    connect_ipc(path)
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

fn equivalent_mpv_path(left: &str, right: &str) -> bool {
    let left = normalize_mpv_path_for_compare(left);
    let right = normalize_mpv_path_for_compare(right);
    left == right
}

fn normalize_mpv_path_for_compare(path: &str) -> String {
    let path = path.trim().trim_matches('"');
    let display = Path::new(path).to_string_lossy();

    #[cfg(windows)]
    {
        display.replace('/', "\\").to_ascii_lowercase()
    }

    #[cfg(not(windows))]
    {
        display.into_owned()
    }
}

fn normalized_stop_reason(reason: Option<&str>) -> Option<&'static str> {
    match reason.map(str::trim).filter(|reason| !reason.is_empty()) {
        Some(reason) if reason.eq_ignore_ascii_case("eof") => Some("eof"),
        Some(reason) if reason.eq_ignore_ascii_case("watched-next") => Some("watched-next"),
        Some(reason) if reason.eq_ignore_ascii_case("stop") => Some("stop"),
        Some(reason) if reason.eq_ignore_ascii_case("quit") => Some("quit"),
        Some(reason) if reason.eq_ignore_ascii_case("error") => Some("error"),
        Some(reason) if reason.eq_ignore_ascii_case("redirect") => Some("redirect"),
        Some(reason) if reason.eq_ignore_ascii_case("shutdown") => Some("shutdown"),
        Some(_) => Some("unknown"),
        None => None,
    }
}

fn is_completion_reason(reason: Option<&str>) -> bool {
    reason.is_some_and(|reason| {
        reason.eq_ignore_ascii_case("eof") || reason.eq_ignore_ascii_case("watched-next")
    })
}

fn is_mark_watched_next_message(args: &[String]) -> bool {
    match args {
        [command] => command == MARK_WATCHED_NEXT_COMMAND,
        [target, command, ..] => {
            target == "mediaflick-desktop" && command == MARK_WATCHED_NEXT_COMMAND
        }
        _ => false,
    }
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
        ControllerState, MpvControlCommand, MpvPlaybackEvent, PendingPlayback, PlaybackIdentity,
        control_command, loadfile_command, mpv_string_list,
    };
    use crate::app::settings::{MpvFullscreenBehavior, SegmentSkipConfig, SegmentSkipMode};
    use crate::jellyfin::bridge::PlaybackContext;
    use crate::jellyfin::media_segments::{SegmentType, SkipSegment};
    use crate::mpv::{HttpHeader, MpvLaunch};
    use serde_json::json;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn controller_with_pending_load(start_time_ticks: Option<i64>) -> ControllerState {
        let (tx, rx) = mpsc::channel();
        let mut launch = MpvLaunch::new("https://example.test/video.mkv?ApiKey=secret");
        launch.start_time_ticks = start_time_ticks;

        let mut state = ControllerState::new(
            tx,
            rx,
            Arc::new(Mutex::new(Default::default())),
            None,
            Arc::new(AtomicBool::new(false)),
            SegmentSkipConfig::default(),
        );
        state.pending = Some(PendingPlayback {
            key: "test-load".to_string(),
            identity: PlaybackIdentity::from_launch(1, &launch),
            launch,
            reporter: None,
            requested_at: Instant::now(),
        });
        state
    }

    fn snapshot_active(state: &ControllerState) -> bool {
        state
            .snapshot
            .lock()
            .map(|snapshot| snapshot.active)
            .unwrap_or(false)
    }

    fn add_prompt_credits_segment(state: &mut ControllerState) {
        state.skip_segments = vec![SkipSegment {
            segment_type: SegmentType::Outro,
            start_ticks: 100_000_000,
            end_ticks: 200_000_000,
            triggered: false,
        }];
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
        assert!(args[4].get("start").is_none());
        assert_eq!(args[4]["force-media-title"], "A Movie");
    }

    #[test]
    fn loadfile_command_applies_selected_tracks() {
        let mut launch = MpvLaunch::new("https://example.test/video.mkv");
        launch.audio_stream_index = Some(3);
        launch.subtitle_stream_index = Some(5);
        launch.audio_mpv_id = Some(2);
        launch.subtitle_mpv_id = Some(1);

        let command = loadfile_command(&launch);
        let options = &command["command"][4];
        assert_eq!(options["aid"], "2");
        assert_eq!(options["sid"], "1");
    }

    #[test]
    fn loadfile_command_disables_embedded_subtitles_for_external_subtitle() {
        let mut launch = MpvLaunch::new("https://example.test/video.mkv");
        launch.subtitle_stream_index = Some(7);
        launch.subtitle_url = Some("https://example.test/subtitle.srt".to_string());

        let command = loadfile_command(&launch);
        let options = &command["command"][4];
        assert_eq!(options["sid"], "no");
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
    fn loadfile_strips_media_fragment_from_url() {
        let mut launch = MpvLaunch::new("https://example.test/video.mkv?ApiKey=secret#t=30");
        launch.start_time_ticks = Some(300_000_000);

        let command = loadfile_command(&launch);
        assert_eq!(
            command["command"][1],
            "https://example.test/video.mkv?ApiKey=secret"
        );
        assert!(command["command"][4].get("start").is_none());
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

        let audio = control_command(MpvControlCommand::SetAudioTrack(2)).expect("audio command");
        assert_eq!(audio["command"], json!(["set_property", "aid", 2]));

        let subtitle = control_command(MpvControlCommand::SetSubtitleTrack(None))
            .expect("subtitle none command");
        assert_eq!(subtitle["command"], json!(["set_property", "sid", "no"]));

        let external_subtitle = control_command(MpvControlCommand::AddSubtitle(
            "https://example.test/sub.srt".to_string(),
        ))
        .expect("external subtitle command");
        assert_eq!(
            external_subtitle["command"],
            json!(["sub-add", "https://example.test/sub.srt", "select"])
        );

        assert!(control_command(MpvControlCommand::SetPlaybackRate(f64::NAN)).is_none());
    }

    #[test]
    fn playback_abort_snapshot_does_not_fail_pending_load() {
        let mut state = controller_with_pending_load(Some(20_000_000));

        state.apply_property(Some("playback-abort"), Some(json!(true)));

        assert!(state.pending.is_some());
        state.activate_pending();
        assert!(state.pending.is_none());
        assert_eq!(
            state.startup_seek.map(|seek| seek.position_ms),
            Some(2000.0)
        );
        assert_eq!(state.last_state.position_ticks, 20_000_000);
    }

    #[test]
    fn zero_start_does_not_queue_startup_seek() {
        let mut state = controller_with_pending_load(None);

        state.activate_pending();

        assert!(state.pending.is_none());
        assert!(state.startup_seek.is_none());
        assert_eq!(state.last_state.position_ticks, 0);
    }

    #[test]
    fn activation_without_start_resets_previous_playback_state() {
        let mut state = controller_with_pending_load(None);
        state.last_state.position_ticks = 42_000_000;
        state.last_state.duration_ticks = Some(120_000_000);
        state.last_state.eof_reached = true;

        state.activate_pending();

        assert_eq!(state.last_state.position_ticks, 0);
        assert_eq!(state.last_state.duration_ticks, None);
        assert!(!state.last_state.eof_reached);
    }

    #[test]
    fn pending_preparation_resets_previous_playback_snapshot_state() {
        let mut state = controller_with_pending_load(None);
        state
            .pending
            .as_mut()
            .expect("pending")
            .launch
            .runtime_ticks = Some(300_000_000);
        state.last_state.position_ticks = 120_000_000;
        state.last_state.duration_ticks = Some(120_000_000);
        state.last_state.pause = true;
        state.last_state.eof_reached = true;

        state.prepare_pending_playback_state();
        let snapshot = state.publish_snapshot();

        assert!(snapshot.active);
        assert_eq!(snapshot.position_ms, 0.0);
        assert_eq!(snapshot.duration_ms, Some(30_000.0));
        assert!(!snapshot.paused);
        assert_eq!(state.last_state.position_ticks, 0);
        assert_eq!(state.last_state.duration_ticks, Some(300_000_000));
        assert!(!state.last_state.eof_reached);
    }

    #[test]
    fn activation_without_reporter_still_marks_mpv_snapshot_active() {
        let mut state = controller_with_pending_load(None);

        state.activate_pending();

        assert!(state.active.is_none());
        assert!(snapshot_active(&state));
    }

    #[test]
    fn finish_without_reporter_marks_mpv_snapshot_inactive() {
        let mut state = controller_with_pending_load(None);

        state.activate_pending();
        state.finish_active(Some("quit"));

        assert!(state.active.is_none());
        assert!(!snapshot_active(&state));
    }

    #[test]
    fn finish_without_reporter_emits_stopped_event() {
        let (tx, rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let mut launch = MpvLaunch::new("https://example.test/video.mkv?ApiKey=secret");
        launch.item_id = Some("item-1".to_string());

        let mut state = ControllerState::new(
            tx,
            rx,
            Arc::new(Mutex::new(Default::default())),
            Some(event_tx),
            Arc::new(AtomicBool::new(false)),
            SegmentSkipConfig::default(),
        );
        state.pending = Some(PendingPlayback {
            key: "test-load".to_string(),
            identity: PlaybackIdentity::from_launch(1, &launch),
            launch,
            reporter: None,
            requested_at: Instant::now(),
        });

        state.activate_pending();
        state.finish_active(Some("quit"));

        let event = event_rx.try_recv().expect("stopped event");
        assert!(matches!(
            event,
            MpvPlaybackEvent::Stopped(snapshot)
                if !snapshot.active
                    && snapshot.stop_reason == Some("quit")
                    && snapshot.playback_id == Some(1)
                    && snapshot.item_id.as_deref() == Some("item-1")
        ));
    }

    #[test]
    fn eof_stop_reason_is_preserved_in_snapshot() {
        let mut state = controller_with_pending_load(None);

        state.activate_pending();
        state.finish_active(Some("eof"));

        let snapshot = state.snapshot.lock().expect("snapshot");
        assert_eq!(snapshot.stop_reason, Some("eof"));
    }

    #[test]
    fn startup_seek_holds_resume_position_until_mpv_reaches_resume_range() {
        let mut state = controller_with_pending_load(Some(1_000_000_000));

        state.activate_pending();
        assert_eq!(state.last_state.position_ticks, 1_000_000_000);
        assert!(state.startup_seek.is_some());

        state.apply_property(Some("time-pos"), Some(json!(0.0)));
        assert_eq!(state.last_state.position_ticks, 1_000_000_000);
        assert!(state.startup_seek.is_some());

        state.apply_property(Some("time-pos"), Some(json!(98.0)));
        assert_eq!(state.last_state.position_ticks, 980_000_000);
        assert!(state.startup_seek.is_none());
    }

    #[test]
    fn end_file_error_still_fails_pending_load() {
        let mut state = controller_with_pending_load(None);

        state.finish_active(Some("error"));

        assert!(state.pending.is_none());
    }

    #[test]
    fn native_seek_event_records_prompt_segment_start_before_seeking_property() {
        let mut state = controller_with_pending_load(None);
        add_prompt_credits_segment(&mut state);
        state.last_state.position_ticks = 150_000_000;

        state.handle_seek_event();
        state.last_state.position_ticks = 155_000_000;
        state.handle_seeking_property(true);

        assert_eq!(state.seek_started_at_ticks, Some(150_000_000));
    }

    #[test]
    fn native_seek_waits_for_position_update_when_seeking_false_is_early() {
        let mut state = controller_with_pending_load(None);
        add_prompt_credits_segment(&mut state);
        state.last_state.position_ticks = 150_000_000;

        state.handle_seek_event();
        state.handle_seeking_property(false);
        assert_eq!(state.seek_started_at_ticks, Some(150_000_000));

        state.maybe_accept_pending_native_seek(145_000_000);
        assert_eq!(state.seek_started_at_ticks, None);
    }

    #[test]
    fn always_skip_starts_countdown_without_immediate_trigger() {
        let mut state = controller_with_pending_load(None);
        state.segment_skip_config.credits = SegmentSkipMode::Always;
        add_prompt_credits_segment(&mut state);

        state.update_skip_segment_state(0, 150_000_000);

        assert_eq!(
            state
                .pending_auto_skip
                .as_ref()
                .map(|pending| pending.segment_index),
            Some(0)
        );
        assert!(!state.skip_segments[0].triggered);
    }

    #[test]
    fn always_skip_countdown_cancels_after_leaving_segment() {
        let mut state = controller_with_pending_load(None);
        state.segment_skip_config.credits = SegmentSkipMode::Always;
        add_prompt_credits_segment(&mut state);

        state.update_skip_segment_state(0, 150_000_000);
        state.update_skip_segment_state(150_000_000, 250_000_000);

        assert!(state.pending_auto_skip.is_none());
        assert!(!state.skip_segments[0].triggered);
    }

    #[test]
    fn late_playback_context_updates_active_identity() {
        let mut state = controller_with_pending_load(None);
        let pending = state.pending.as_mut().expect("pending");
        pending.launch.item_id = Some("item-1".to_string());
        pending.launch.media_source_id = Some("source-1".to_string());
        pending.identity = PlaybackIdentity::from_launch(1, &pending.launch);

        state.activate_pending();
        state.update_active_playback_context(PlaybackContext {
            item_id: Some("item-1".to_string()),
            media_source_id: Some("source-1".to_string()),
            play_session_id: Some("session-1".to_string()),
            ..Default::default()
        });

        assert_eq!(
            state
                .playback_identity
                .as_ref()
                .and_then(|identity| identity.play_session_id.as_deref()),
            Some("session-1")
        );
        assert_eq!(
            state
                .snapshot
                .lock()
                .expect("snapshot")
                .play_session_id
                .as_deref(),
            Some("session-1")
        );
    }

    #[test]
    fn pending_load_blocks_different_replacement_until_file_loaded() {
        let mut state = controller_with_pending_load(None);
        let pending_key = state.pending.as_ref().expect("pending load").key.clone();
        let mut launch = MpvLaunch::new("https://example.test/next-video.mkv?ApiKey=secret");
        launch.item_id = Some("next-item".to_string());
        launch.media_source_id = Some("next-source".to_string());

        state.load(
            "C:\\missing\\mpv.exe".to_string(),
            MpvFullscreenBehavior::Fullscreen,
            launch,
        );

        assert_eq!(
            state.pending.as_ref().map(|pending| pending.key.as_str()),
            Some(pending_key.as_str())
        );
    }

    #[test]
    fn next_playback_handoff_suppresses_stop_while_replacement_is_pending() {
        let mut state = controller_with_pending_load(None);
        state.next_playback_handoff_until = Some(Instant::now() + Duration::from_secs(1));

        assert!(state.should_suppress_stop_during_next_playback_handoff());

        state.activate_pending();

        assert!(!state.should_suppress_stop_during_next_playback_handoff());
    }

    #[test]
    fn next_playback_handoff_ignores_old_end_file_while_replacement_is_pending() {
        let mut state = controller_with_pending_load(None);
        state.next_playback_handoff_until = Some(Instant::now() + Duration::from_secs(1));

        state.finish_active(Some("stop"));

        assert!(state.pending.is_some());
    }

    #[test]
    fn eof_arms_next_playback_handoff() {
        let mut state = controller_with_pending_load(None);
        state.activate_pending();
        state.mpv_playback_active = true;
        state.last_state.duration_ticks = Some(120_000_000);

        state.finish_active(Some("eof"));

        assert!(state.next_playback_handoff_until.is_some());
        assert_eq!(state.last_state.position_ticks, 120_000_000);
    }

    #[test]
    fn eof_uses_runtime_when_mpv_duration_is_missing() {
        let mut state = controller_with_pending_load(None);
        state
            .pending
            .as_mut()
            .expect("pending")
            .launch
            .runtime_ticks = Some(240_000_000);
        state.activate_pending();
        state.mpv_playback_active = true;

        state.finish_active(Some("eof"));

        assert_eq!(state.last_state.duration_ticks, Some(240_000_000));
        assert_eq!(state.last_state.position_ticks, 240_000_000);
    }
}
