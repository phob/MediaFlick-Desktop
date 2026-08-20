use std::collections::VecDeque;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::app::logger;
use crate::jellyfin::playback_reporter::PlaybackReporter;
use crate::playback::model::allocate_playback_id;
use crate::playback::segments::SkipSegment;
use crate::playback::{
    PlaybackContext, PlaybackEvent, PlaybackRequest, PlayerCommand, PlayerSnapshot, ReportingState,
};
use crate::players::mpv::ipc::{IpcCommandFailure, IpcWorker, MpvEvent};
use crate::preferences::{FullscreenBehavior, SegmentSkipConfig};

pub use super::commands::{control_command, loadfile_command};
use session::{is_completion_reason, normalized_stop_reason};

#[path = "playback_transition.rs"]
mod playback_transition;
mod segment_skip;
mod session;
#[cfg(test)]
mod test_support;

const IPC_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IPC_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const IPC_COMMAND_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
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
const CONTROLLER_SHUTDOWN_ACK_TIMEOUT: Duration = Duration::from_secs(60);
const PLAYSTATE_SHUTDOWN_FLUSH_TIMEOUT: Duration = Duration::from_secs(11);
const STARTUP_SEEK_DELAY: Duration = Duration::from_millis(500);
const STARTUP_SEEK_RETRY_DELAY: Duration = Duration::from_secs(1);
const STARTUP_SEEK_POSITION_TOLERANCE: i64 = 30_000_000;
const SEGMENT_SKIP_OSD_DURATION_MS: i64 = 3000;
const SEGMENT_SKIP_OSD_DEBOUNCE: Duration = Duration::from_secs(3);
const SEGMENT_AUTO_SKIP_DELAY: Duration = Duration::from_secs(3);
const SEGMENT_AUTO_SKIP_COUNTDOWN_INTERVAL: Duration = Duration::from_secs(1);
const SEGMENT_AUTO_SKIP_COUNTDOWN_OSD_DURATION_MS: i64 = 1200;
const CHAPTER_MARKER_RETRY_INTERVAL: Duration = Duration::from_millis(1000);
const CHAPTER_MARKER_MAX_ATTEMPTS: u32 = 15;
const IPC_SUBTITLE_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct MpvController {
    tx: Sender<ControllerMessage>,
    snapshot: Arc<Mutex<PlayerSnapshot>>,
    shutdown_requested: Arc<AtomicBool>,
}

enum ControllerMessage {
    Warm {
        mpv_path: String,
        fullscreen: FullscreenBehavior,
    },
    Load {
        mpv_path: String,
        fullscreen: FullscreenBehavior,
        launch: Box<PlaybackRequest>,
    },
    PlaybackContext(Box<PlaybackContext>),
    Control(PlayerCommand),
    RefreshInputBindings,
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
    snapshot: Arc<Mutex<PlayerSnapshot>>,
    child: Option<Child>,
    configured_mpv: Option<ConfiguredMpv>,
    current_mpv_path: Option<String>,
    ipc_path: Option<String>,
    ipc_worker: Option<IpcWorker>,
    event_rx: Option<Receiver<MpvEvent>>,
    pending_external_subtitle_url: Option<String>,
    active: Option<ActivePlayback>,
    pending: Option<PendingPlayback>,
    playback_identity: Option<PlaybackIdentity>,
    startup_seek: Option<StartupSeek>,
    mpv_playback_active: bool,
    playback_runtime_ticks: Option<i64>,
    last_state: ReportingState,
    last_position_log_bucket: Option<i64>,
    skip_segments: Vec<SkipSegment>,
    current_skip_segment: Option<usize>,
    pending_auto_skip: Option<PendingAutoSkip>,
    original_chapters: Option<Vec<Value>>,
    injected_chapter_markers: Vec<Value>,
    last_sent_chapter_list: Option<Vec<Value>>,
    pending_chapter_markers: Option<Vec<Value>>,
    chapter_marker_attempts: u32,
    chapter_marker_next_attempt_at: Option<Instant>,
    last_skip_osd_at: Option<Instant>,
    seek_started_at_ticks: Option<i64>,
    segment_skip_config: SegmentSkipConfig,
    recent_loads: VecDeque<RecentLoad>,
    next_playback_handoff_until: Option<Instant>,
    replacement_end_file_pending: bool,
    pending_raise_pulse_reset_at: Option<Instant>,
    last_session_poll: Instant,
    event_tx: Option<Sender<PlaybackEvent>>,
    shutdown_requested: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
struct ConfiguredMpv {
    mpv_path: String,
    fullscreen: FullscreenBehavior,
}

#[derive(Debug, Clone)]
struct PlaybackIdentity {
    playback_id: i64,
    item_id: Option<String>,
    media_source_id: Option<String>,
    play_session_id: Option<String>,
}

impl PlaybackIdentity {
    fn from_launch(playback_id: i64, launch: &PlaybackRequest) -> Self {
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
    launch: PlaybackRequest,
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

impl MpvController {
    pub fn new(
        event_tx: Option<Sender<PlaybackEvent>>,
        segment_skip_config: SegmentSkipConfig,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let snapshot = Arc::new(Mutex::new(PlayerSnapshot::default()));
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

    pub fn warm(&self, mpv_path: impl Into<String>, fullscreen: FullscreenBehavior) {
        let _ = self.tx.send(ControllerMessage::Warm {
            mpv_path: mpv_path.into(),
            fullscreen,
        });
    }

    pub fn load(
        &self,
        mpv_path: impl Into<String>,
        fullscreen: FullscreenBehavior,
        launch: PlaybackRequest,
    ) {
        let _ = self.tx.send(ControllerMessage::Load {
            mpv_path: mpv_path.into(),
            fullscreen,
            launch: Box::new(launch),
        });
    }

    pub fn control(&self, command: PlayerCommand) {
        let _ = self.tx.send(ControllerMessage::Control(command));
    }

    pub fn refresh_input_bindings(&self) {
        let _ = self.tx.send(ControllerMessage::RefreshInputBindings);
    }

    pub fn set_segment_skip_config(&self, config: SegmentSkipConfig) {
        let _ = self.tx.send(ControllerMessage::SegmentSkipConfig(config));
    }

    pub fn update_playback_context(&self, context: PlaybackContext) {
        let _ = self
            .tx
            .send(ControllerMessage::PlaybackContext(Box::new(context)));
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
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
        snapshot: Arc<Mutex<PlayerSnapshot>>,
        event_tx: Option<Sender<PlaybackEvent>>,
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
            pending_external_subtitle_url: None,
            active: None,
            pending: None,
            playback_identity: None,
            startup_seek: None,
            mpv_playback_active: false,
            playback_runtime_ticks: None,
            last_state: ReportingState {
                volume: Some(100),
                ..Default::default()
            },
            last_position_log_bucket: None,
            skip_segments: Vec::new(),
            current_skip_segment: None,
            pending_auto_skip: None,
            original_chapters: None,
            injected_chapter_markers: Vec::new(),
            last_sent_chapter_list: None,
            pending_chapter_markers: None,
            chapter_marker_attempts: 0,
            chapter_marker_next_attempt_at: None,
            last_skip_osd_at: None,
            seek_started_at_ticks: None,
            segment_skip_config,
            recent_loads: VecDeque::new(),
            next_playback_handoff_until: None,
            replacement_end_file_pending: false,
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
                    self.warm(&mpv_path, fullscreen);
                }
                Ok(ControllerMessage::Load {
                    mpv_path,
                    fullscreen,
                    launch,
                }) => {
                    tracing::debug!(target: "playback", "received playback load request");
                    self.load(&mpv_path, fullscreen, *launch);
                }
                Ok(ControllerMessage::PlaybackContext(context)) => {
                    tracing::debug!(target: "playback", "received playback context update");
                    self.update_active_playback_context(context.as_ref());
                }
                Ok(ControllerMessage::Control(command)) => {
                    tracing::debug!(target: "playback", ?command, "received playback control request");
                    self.control(&command);
                }
                Ok(ControllerMessage::RefreshInputBindings) => {
                    if self
                        .ipc_worker
                        .as_ref()
                        .is_some_and(IpcWorker::is_writer_alive)
                    {
                        tracing::debug!(target: "mpv.ipc", "refreshing live mpv input bindings");
                        self.install_input_bindings();
                    } else {
                        tracing::debug!(target: "mpv.ipc", "saved mpv input bindings for the next player start");
                    }
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
                    self.refresh_chapter_markers();
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
            self.maybe_apply_chapter_markers();
            self.maybe_update_auto_skip_countdown();
            self.maybe_report_progress();
            self.prune_recent_loads();
        }
    }

    fn control(&mut self, command: &PlayerCommand) {
        if matches!(command, PlayerCommand::Stop)
            && self.should_suppress_stop_during_next_playback_handoff()
        {
            tracing::debug!(
                target: "playback",
                "ignored stop control while next playback handoff is waiting for file-loaded"
            );
            return;
        }

        if self.handle_prompt_skip_control(command) {
            return;
        }

        let Some(command_json) = control_command(command) else {
            tracing::debug!(target: "mpv.ipc", ?command, "ignored invalid mpv control command");
            return;
        };

        if !self.ensure_configured_mpv_running("player control preflight") {
            tracing::warn!(target: "mpv.ipc", ?command, "cannot send mpv control command because no session is available");
            return;
        }
        if let Err(error) = self.send_mpv_command(command_json) {
            tracing::warn!(target: "mpv.ipc", ?command, "failed to send mpv control command: {error}");
            if error.is_transport() {
                self.handle_mpv_session_lost("mpv control command transport failed");
            }
        }
    }

    fn kick_start_playback(&mut self, launch: &PlaybackRequest) {
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
        if let Some(command) =
            control_command(&PlayerCommand::SeekMilliseconds(startup_seek.position_ms))
        {
            match self.send_mpv_command(command) {
                Ok(()) => {
                    if let Some(startup_seek) = &mut self.startup_seek {
                        startup_seek.sent_at = Some(now);
                        startup_seek.due_at = now + STARTUP_SEEK_RETRY_DELAY;
                    }
                }
                Err(IpcCommandFailure::Rejected(error)) => {
                    tracing::warn!(target: "mpv.ipc", "mpv rejected startup seek; retrying after file settles: {error}");
                    if let Some(startup_seek) = &mut self.startup_seek {
                        startup_seek.due_at = now + STARTUP_SEEK_RETRY_DELAY;
                    }
                }
                Err(error) => {
                    tracing::warn!(target: "mpv.ipc", "failed to send mpv startup seek command: {error}");
                    self.handle_mpv_session_lost("mpv startup seek transport failed");
                }
            }
        }
    }

    fn load(&mut self, mpv_path: &str, fullscreen: FullscreenBehavior, launch: PlaybackRequest) {
        self.remember_configured_mpv(mpv_path, fullscreen);
        let key = launch.dedupe_key();
        let identity = PlaybackIdentity::from_launch(allocate_playback_id(), &launch);
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

        if !self.ensure_mpv(mpv_path, fullscreen) {
            tracing::warn!(
                target: "playback",
                mpv_path = %mpv_path,
                "cannot load playback because mpv is unavailable"
            );
            self.report_playback_failure(
                "Could not start mpv. Check that the mpv path in Settings is correct.",
            );
            return;
        };
        self.apply_default_fullscreen(fullscreen);

        let reporter = PlaybackReporter::from_launch(&launch);
        self.startup_seek = None;
        self.reset_chapter_markers();
        let replacing_active_file = self.mpv_playback_active || self.active.is_some();
        if let Some(active) = self.active.take() {
            tracing::info!(
                target: "playback",
                state = %self.last_state,
                "stopping previous active playback before loading replacement"
            );
            active.reporter.report_stopped(&self.last_state, false);
        }

        match self.send_loadfile_with_reconnect(mpv_path, fullscreen, &launch) {
            Ok(()) => {
                tracing::info!(
                    target: "playback",
                    item_id = %launch.item_id.as_deref().unwrap_or("unknown"),
                    url = %logger::redact_url_secrets(&launch.media_url),
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
                self.replacement_end_file_pending = replacing_active_file;
                self.fetch_media_segments(playback_id, pending_launch);
                self.prepare_pending_playback_state();
                #[cfg(not(windows))]
                self.schedule_mpv_raise("loadfile accepted");
                self.publish_snapshot();
            }
            Err(IpcCommandFailure::Rejected(error)) => {
                tracing::warn!(target: "mpv.ipc", "mpv rejected loadfile command: {error}");
                self.handle_rejected_loadfile(replacing_active_file, identity);
            }
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", "failed to send mpv loadfile command after reconnect attempt: {error}");
                self.mpv_playback_active = false;
                self.report_playback_failure("mpv did not accept the video. Try playing again.");
                self.handle_mpv_session_lost("loadfile transport failed");
            }
        }
    }

    fn handle_rejected_loadfile(
        &mut self,
        replacing_active_file: bool,
        identity: PlaybackIdentity,
    ) {
        self.mpv_playback_active = false;
        if replacing_active_file {
            tracing::warn!(
                target: "mpv.ipc",
                "resetting mpv after rejected replacement loadfile command"
            );
            self.playback_identity = Some(identity);
            self.reset_mpv();
            let snapshot = self.publish_snapshot_with_stop_reason(Some("error"));
            self.notify_playback_stopped(snapshot);
        }
        self.report_playback_failure("mpv did not accept the video. Try playing again.");
    }

    fn report_playback_failure(&self, message: impl Into<String>) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(PlaybackEvent::Failed {
                message: message.into(),
            });
        }
    }
}
