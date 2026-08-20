mod segment_skip;

use std::collections::VecDeque;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::app::logger;
use crate::jellyfin::playback_reporter::{PlaybackReporter, flush_playstate_reports};
use crate::playback::model::allocate_playback_id;
use crate::playback::segments::SkipSegment;
use crate::playback::{
    PlaybackContext, PlaybackEvent, PlaybackRequest, PlayerCommand, PlayerSnapshot, ReportingState,
    seconds_to_ticks,
};
use crate::preferences::{FullscreenBehavior, SegmentSkipConfig};

use self::segment_skip::PendingAutoSkip;
use super::protocol::{self, Inbound};
use super::request::{
    TrackSelection, audio_index as mpchc_audio_index, media_url as mpchc_media_url,
    subtitle_index as mpchc_subtitle_index, track_selection,
};
use super::transport::MpcHcTransport;

const RECV_TIMEOUT: Duration = Duration::from_millis(200);
const POSITION_POLL_INTERVAL: Duration = Duration::from_secs(1);
const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);
const DUPLICATE_DEBOUNCE: Duration = Duration::from_secs(2);
const SHUTDOWN_ACK_TIMEOUT: Duration = Duration::from_secs(25);
const PLAYSTATE_SHUTDOWN_FLUSH_TIMEOUT: Duration = Duration::from_secs(11);
const VOLUME_STEP_PERCENT: f64 = 5.0;
const SEEKING_OSD_DURATION_MS: i32 = 60_000;

#[derive(Clone)]
pub struct MpcHcController {
    tx: Sender<Msg>,
    snapshot: Arc<Mutex<PlayerSnapshot>>,
    shutdown_requested: Arc<AtomicBool>,
}

enum Msg {
    Warm {
        path: String,
        fullscreen: FullscreenBehavior,
    },
    Load {
        path: String,
        fullscreen: FullscreenBehavior,
        launch: Box<PlaybackRequest>,
    },
    Control(PlayerCommand),
    PlaybackContext(Box<PlaybackContext>),
    SegmentSkipConfig(SegmentSkipConfig),
    MediaSegments {
        playback_id: i64,
        result: Result<Vec<SkipSegment>, String>,
    },
    Shutdown {
        ack: Sender<()>,
    },
}

#[derive(Clone)]
struct Identity {
    playback_id: i64,
    item_id: Option<String>,
    media_source_id: Option<String>,
    play_session_id: Option<String>,
}

impl Identity {
    fn from_launch(playback_id: i64, launch: &PlaybackRequest) -> Self {
        Self {
            playback_id,
            item_id: launch.item_id.clone(),
            media_source_id: launch.media_source_id.clone(),
            play_session_id: launch.play_session_id.clone(),
        }
    }
}

fn identity_matches_context(identity: &Identity, context: &PlaybackContext) -> bool {
    let mut matched = false;
    for (expected, actual) in [
        (identity.item_id.as_deref(), context.item_id.as_deref()),
        (
            identity.media_source_id.as_deref(),
            context.media_source_id.as_deref(),
        ),
        (
            identity.play_session_id.as_deref(),
            context.play_session_id.as_deref(),
        ),
    ] {
        let expected = expected.map(str::trim).filter(|value| !value.is_empty());
        let actual = actual.map(str::trim).filter(|value| !value.is_empty());
        let (Some(expected), Some(actual)) = (expected, actual) else {
            continue;
        };
        if expected != actual {
            return false;
        }
        matched = true;
    }
    matched
}

fn update_identity_from_context(identity: &mut Identity, context: &PlaybackContext) {
    if identity.item_id.is_none() {
        identity.item_id.clone_from(&context.item_id);
    }
    if identity.media_source_id.is_none() {
        identity
            .media_source_id
            .clone_from(&context.media_source_id);
    }
    if identity.play_session_id.is_none() {
        identity
            .play_session_id
            .clone_from(&context.play_session_id);
    }
}

struct Pending {
    identity: Identity,
    launch: PlaybackRequest,
    reporter: Option<PlaybackReporter>,
}

struct Active {
    identity: Identity,
    reporter: Option<PlaybackReporter>,
    last_progress: Instant,
}

#[derive(Clone)]
struct RecentLoad {
    key: String,
    seen_at: Instant,
}

impl MpcHcController {
    pub fn new(
        event_tx: Option<Sender<PlaybackEvent>>,
        segment_skip_config: SegmentSkipConfig,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let snapshot = Arc::new(Mutex::new(PlayerSnapshot::default()));
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let state = State {
            tx: tx.clone(),
            rx,
            snapshot: snapshot.clone(),
            event_tx,
            shutdown_requested: shutdown_requested.clone(),
            transport: None,
            inbound: None,
            child: None,
            connected: false,
            last_state: ReportingState {
                volume: Some(100),
                ..ReportingState::default()
            },
            pending: None,
            active: None,
            identity: None,
            playback_active: false,
            awaiting_open: false,
            resume_seconds: None,
            last_position_poll: Instant::now(),
            skip_segments: Vec::new(),
            segment_skip_config,
            current_skip_segment: None,
            pending_auto_skip: None,
            last_skip_osd_at: None,
            recent_loads: VecDeque::new(),
            fullscreen_pref: FullscreenBehavior::default(),
            fullscreen_state: false,
            target_volume: 100.0,
            believed_output: 100.0,
            muted: false,
            seeking_osd: false,
        };
        thread::spawn(move || state.run());
        Self {
            tx,
            snapshot,
            shutdown_requested,
        }
    }

    pub fn warm(&self, path: String, fullscreen: FullscreenBehavior) {
        let _ = self.tx.send(Msg::Warm { path, fullscreen });
    }

    pub fn load(&self, path: String, fullscreen: FullscreenBehavior, launch: PlaybackRequest) {
        let _ = self.tx.send(Msg::Load {
            path,
            fullscreen,
            launch: Box::new(launch),
        });
    }

    pub fn control(&self, command: PlayerCommand) {
        let _ = self.tx.send(Msg::Control(command));
    }

    pub fn set_segment_skip_config(&self, config: SegmentSkipConfig) {
        let _ = self.tx.send(Msg::SegmentSkipConfig(config));
    }

    pub fn update_playback_context(&self, context: PlaybackContext) {
        let _ = self.tx.send(Msg::PlaybackContext(Box::new(context)));
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
        if self.tx.send(Msg::Shutdown { ack }).is_err() {
            return;
        }
        if ack_rx.recv_timeout(SHUTDOWN_ACK_TIMEOUT).is_err() {
            tracing::warn!(target: "mpchc", "timed out waiting for MPC-HC controller shutdown");
        }
    }
}

struct State {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    snapshot: Arc<Mutex<PlayerSnapshot>>,
    event_tx: Option<Sender<PlaybackEvent>>,
    shutdown_requested: Arc<AtomicBool>,
    transport: Option<MpcHcTransport>,
    inbound: Option<Receiver<Inbound>>,
    child: Option<Child>,
    connected: bool,
    last_state: ReportingState,
    pending: Option<Pending>,
    active: Option<Active>,
    identity: Option<Identity>,
    playback_active: bool,
    awaiting_open: bool,
    resume_seconds: Option<f64>,
    last_position_poll: Instant,
    skip_segments: Vec<SkipSegment>,
    segment_skip_config: SegmentSkipConfig,
    current_skip_segment: Option<usize>,
    pending_auto_skip: Option<PendingAutoSkip>,
    last_skip_osd_at: Option<Instant>,
    recent_loads: VecDeque<RecentLoad>,
    fullscreen_pref: FullscreenBehavior,
    fullscreen_state: bool,
    target_volume: f64,
    believed_output: f64,
    muted: bool,
    seeking_osd: bool,
}

impl State {
    fn run(mut self) {
        tracing::debug!(target: "mpchc", "MPC-HC controller thread started");
        loop {
            match self.rx.recv_timeout(RECV_TIMEOUT) {
                Ok(Msg::Warm { path, fullscreen }) => self.warm(path, fullscreen),
                Ok(Msg::Load {
                    path,
                    fullscreen,
                    launch,
                }) => self.load(&path, fullscreen, *launch),
                Ok(Msg::Control(command)) => self.control(&command),
                Ok(Msg::PlaybackContext(context)) => self.update_context(context.as_ref()),
                Ok(Msg::SegmentSkipConfig(config)) => self.apply_segment_skip_config(config),
                Ok(Msg::MediaSegments {
                    playback_id,
                    result,
                }) => self.handle_media_segments(playback_id, result),
                Ok(Msg::Shutdown { ack }) => {
                    self.shutdown();
                    let _ = ack.send(());
                    return;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.shutdown();
                    return;
                }
            }

            self.drain_inbound();
            self.poll_child();
            self.maybe_poll_position();
            self.maybe_report_progress();
            self.maybe_update_auto_skip();
        }
    }

    fn warm(&mut self, _path: String, _fullscreen: FullscreenBehavior) {
        tracing::debug!(target: "mpchc", "skipping MPC-HC warmup; it launches on first playback");
    }

    fn ensure_transport(&mut self) -> bool {
        if self.transport.is_some() {
            return true;
        }
        match MpcHcTransport::spawn() {
            Ok((transport, inbound)) => {
                self.transport = Some(transport);
                self.inbound = Some(inbound);
                true
            }
            Err(error) => {
                tracing::warn!(target: "mpchc", "failed to start MPC-HC slave transport: {error}");
                false
            }
        }
    }

    fn child_alive(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    fn ensure_process(&mut self, path: &str, fullscreen: FullscreenBehavior) -> bool {
        if self.shutdown_requested.load(Ordering::SeqCst) {
            return false;
        }
        if !self.ensure_transport() {
            return false;
        }
        if self.child_alive() {
            return true;
        }
        self.connected = false;
        if let Some(transport) = &self.transport {
            transport.clear_target();
        }
        self.launch(path, fullscreen)
    }

    fn launch(&mut self, path: &str, fullscreen: FullscreenBehavior) -> bool {
        let Some(transport) = &self.transport else {
            return false;
        };
        let mut command = Command::new(path);
        command.arg("/slave").arg(transport.our_hwnd_arg());
        command.arg("/new");
        if fullscreen == FullscreenBehavior::Fullscreen {
            command.arg("/fullscreen");
        }
        match command.spawn() {
            Ok(child) => {
                tracing::info!(target: "mpchc", path = %path, "launched MPC-HC slave process");
                crate::windows::confine_to_app_lifetime(&child);
                self.child = Some(child);
                self.fullscreen_state = fullscreen == FullscreenBehavior::Fullscreen;
                true
            }
            Err(error) => {
                tracing::warn!(target: "mpchc", path = %path, "failed to launch MPC-HC: {error}");
                false
            }
        }
    }

    fn load(&mut self, path: &str, fullscreen: FullscreenBehavior, launch: PlaybackRequest) {
        let key = launch.dedupe_key();
        if self.is_duplicate(&key) {
            tracing::debug!(target: "mpchc", dedupe_key = %key, "ignored duplicate playback load");
            return;
        }
        self.fullscreen_pref = fullscreen;
        if !self.ensure_process(path, fullscreen) {
            tracing::warn!(target: "mpchc", "cannot load playback because MPC-HC is unavailable");
            self.report_playback_failure(
                "Could not start MPC-HC. Check that the MPC-HC path in Settings is correct.",
            );
            return;
        }

        self.finish_active("replaced", false);

        let identity = Identity::from_launch(allocate_playback_id(), &launch);
        let playback_id = identity.playback_id;
        let reporter = PlaybackReporter::from_launch(&launch);
        self.resume_seconds = launch.start_seconds().filter(|seconds| *seconds > 0.0);
        self.last_state = ReportingState {
            volume: Some(self.target_volume.round() as i64),
            mute: Some(self.muted),
            position_ticks: self
                .resume_seconds
                .and_then(seconds_to_ticks)
                .unwrap_or_default(),
            ..ReportingState::default()
        };
        self.skip_segments.clear();
        self.current_skip_segment = None;
        self.pending_auto_skip = None;
        self.last_skip_osd_at = None;
        self.seeking_osd = false;
        self.identity = Some(identity.clone());
        self.playback_active = true;

        self.pending = Some(Pending {
            identity,
            launch: launch.clone(),
            reporter,
        });
        self.recent_loads.push_back(RecentLoad {
            key,
            seen_at: Instant::now(),
        });

        if self.connected {
            self.send_open();
        } else {
            self.awaiting_open = true;
        }

        self.fetch_media_segments(playback_id, launch);
        self.publish_snapshot();
    }

    fn send_open(&mut self) {
        self.awaiting_open = false;
        let Some(pending) = &self.pending else {
            return;
        };
        let url = mpchc_media_url(&pending.launch);
        tracing::info!(
            target: "mpchc",
            url = %logger::redact_url_secrets(&url),
            "opening Jellyfin stream in MPC-HC"
        );
        self.send_command(protocol::CMD_OPENFILE, &url);
    }

    fn on_connected(&mut self, hwnd: isize) {
        if let Some(transport) = &self.transport {
            transport.set_target(hwnd);
        }
        self.connected = true;
        tracing::info!(target: "mpchc", target_hwnd = format!("{hwnd:#x}"), "MPC-HC connected");
        if self.awaiting_open && self.pending.is_some() {
            self.send_open();
        }
    }

    fn on_loaded(&mut self) {
        let mut selection = None;
        if let Some(pending) = self.pending.take() {
            selection = Some(track_selection(&pending.launch));
            if let Some(reporter) = &pending.reporter {
                reporter.report_start(&self.last_state);
            }
            self.active = Some(Active {
                identity: pending.identity,
                reporter: pending.reporter,
                last_progress: Instant::now(),
            });
        }
        self.send_command_empty(protocol::CMD_GETNOWPLAYING);
        if let Some(selection) = selection {
            self.apply_track_selection(&selection);
        }
        self.apply_default_fullscreen();
        if let Some(target) = self.resume_seconds.take().filter(|seconds| *seconds > 0.0) {
            self.send_seek(target);
        }
        self.last_position_poll = Instant::now();
        self.publish_snapshot();
    }

    fn apply_track_selection(&mut self, selection: &TrackSelection) {
        if let Some(index) = selection.audio_index {
            self.send_command(protocol::CMD_SETAUDIOTRACK, &index.to_string());
        }
        if let Some(index) = selection.subtitle_index {
            self.send_command(protocol::CMD_SETSUBTITLETRACK, &index.to_string());
        }
    }

    fn apply_default_fullscreen(&mut self) {
        if !self.connected {
            return;
        }
        let want = self.fullscreen_pref == FullscreenBehavior::Fullscreen;
        if self.fullscreen_state == want {
            return;
        }
        self.send_command_empty(protocol::CMD_TOGGLEFULLSCREEN);
        self.fullscreen_state = want;
    }

    fn control(&mut self, command: &PlayerCommand) {
        match command {
            PlayerCommand::SetPause(true) => {
                self.send_command_empty(protocol::CMD_PAUSE);
            }
            PlayerCommand::SetPause(false) => {
                self.send_command_empty(protocol::CMD_PLAY);
            }
            PlayerCommand::SeekMilliseconds(position_ms) => {
                if !self.handle_prompt_skip(*position_ms) {
                    self.send_seek(*position_ms / 1000.0);
                }
            }
            PlayerCommand::SetPlaybackRate(rate) => {
                self.send_command(protocol::CMD_SETSPEED, &format!("{rate}"));
            }
            PlayerCommand::SetAudioTrack(index) => {
                if let Some(track) = mpchc_audio_index(*index) {
                    self.send_command(protocol::CMD_SETAUDIOTRACK, &track.to_string());
                }
            }
            PlayerCommand::SetSubtitleTrack(index) => {
                self.send_command(
                    protocol::CMD_SETSUBTITLETRACK,
                    &mpchc_subtitle_index(*index).to_string(),
                );
            }
            PlayerCommand::AddSubtitle(_) => {
                tracing::debug!(target: "mpchc", "external subtitles are delivered burned-in, not via runtime sub-add");
            }
            PlayerCommand::SetVolume(volume) => self.set_volume(*volume),
            PlayerCommand::SetMute(mute) => self.set_mute(*mute),
            PlayerCommand::Stop => {
                self.finish_active("stop", false);
                self.send_command_empty(protocol::CMD_STOP);
            }
        }
    }

    fn set_volume(&mut self, volume: f64) {
        if !volume.is_finite() {
            return;
        }
        let target = volume.clamp(0.0, 100.0);
        self.target_volume = target;
        if !self.muted {
            self.step_output_to(target);
        }
        self.last_state.volume = Some(target.round() as i64);
        self.publish_snapshot();
    }

    fn set_mute(&mut self, mute: bool) {
        if self.muted != mute {
            self.muted = mute;
            let target = if mute { 0.0 } else { self.target_volume };
            self.step_output_to(target);
        }
        self.last_state.mute = Some(mute);
        self.publish_snapshot();
    }

    fn step_output_to(&mut self, target: f64) {
        let target = target.clamp(0.0, 100.0);
        let delta = target - self.believed_output;
        let steps = volume_step_count(delta);
        if steps == 0 {
            return;
        }
        let (command, applied) = if delta > 0.0 {
            (
                protocol::CMD_INCREASEVOLUME,
                steps as f64 * VOLUME_STEP_PERCENT,
            )
        } else {
            (
                protocol::CMD_DECREASEVOLUME,
                -(steps as f64) * VOLUME_STEP_PERCENT,
            )
        };
        for _ in 0..steps {
            self.send_command_empty(command);
        }
        self.believed_output = (self.believed_output + applied).clamp(0.0, 100.0);
    }

    fn update_context(&mut self, context: &PlaybackContext) {
        if let Some(active) = &mut self.active
            && identity_matches_context(&active.identity, context)
        {
            update_identity_from_context(&mut active.identity, context);
            if let Some(reporter) = &mut active.reporter {
                reporter.merge_context(context);
            }
        }
        if let Some(pending) = &mut self.pending
            && identity_matches_context(&pending.identity, context)
        {
            context.merge_into_request(&mut pending.launch);
            update_identity_from_context(&mut pending.identity, context);
            if let Some(reporter) = &mut pending.reporter {
                reporter.merge_context(context);
            }
        }
        if let Some(identity) = &mut self.identity
            && identity_matches_context(identity, context)
        {
            update_identity_from_context(identity, context);
        }
        self.publish_snapshot();
    }

    fn drain_inbound(&mut self) {
        let mut messages = Vec::new();
        if let Some(inbound) = &self.inbound {
            while let Ok(message) = inbound.try_recv() {
                messages.push(message);
            }
        }
        for message in messages {
            self.handle_inbound(&message);
        }
    }

    fn handle_inbound(&mut self, message: &Inbound) {
        match message {
            Inbound::Connect { hwnd } => self.on_connected(*hwnd),
            Inbound::State(state) if *state == protocol::MLS_LOADED => self.on_loaded(),
            Inbound::State(state) if *state == protocol::MLS_FAILING => {
                self.finish_active("error", true)
            }
            Inbound::State(_) => {}
            Inbound::PlayMode(mode) => self.handle_play_mode(*mode),
            Inbound::NowPlaying { duration_seconds } => {
                self.last_state.duration_ticks = (*duration_seconds).and_then(seconds_to_ticks);
                self.publish_snapshot();
            }
            Inbound::CurrentPosition(seconds) => self.handle_position(*seconds, false),
            Inbound::NotifySeek(seconds) => self.handle_position(*seconds, true),
            Inbound::EndOfStream => self.finish_active("eof", false),
            Inbound::Disconnect => {
                self.connected = false;
                if let Some(transport) = &self.transport {
                    transport.clear_target();
                }
                self.finish_active("quit", false);
            }
            Inbound::Ignored(command) => {
                tracing::trace!(target: "mpchc", command = format!("{command:#x}"), "ignored MPC-HC message")
            }
        }
    }

    fn handle_play_mode(&mut self, mode: i64) {
        let paused = mode == protocol::PS_PAUSE;
        if mode == protocol::PS_PLAY || mode == protocol::PS_PAUSE {
            if self.last_state.pause != paused {
                self.last_state.pause = paused;
                if let Some(active) = &self.active
                    && let Some(reporter) = &active.reporter
                {
                    reporter.report_progress(&self.last_state);
                }
            }
            self.publish_snapshot();
        } else if mode == protocol::PS_STOP {
            tracing::trace!(target: "mpchc", "MPC-HC reported stop play mode");
        }
    }

    fn handle_position(&mut self, seconds: f64, user_seek: bool) {
        if user_seek && self.seeking_osd {
            self.clear_seeking_osd();
        }
        let Some(ticks) = seconds_to_ticks(seconds) else {
            return;
        };
        let previous = self.last_state.position_ticks;
        self.last_state.position_ticks = ticks;
        if user_seek {
            self.maybe_accept_seek_skip(previous, ticks);
        }
        self.update_skip_state(ticks);
        self.publish_snapshot();
    }

    fn send_seek(&mut self, seconds: f64) {
        self.show_osd("Seeking...", SEEKING_OSD_DURATION_MS);
        self.seeking_osd = true;
        self.send_command(protocol::CMD_SETPOSITION, &format!("{seconds:.3}"));
    }

    fn clear_seeking_osd(&mut self) {
        self.seeking_osd = false;
        if let Some(transport) = &self.transport {
            transport.send_osd(protocol::OSD_TOPLEFT, 1, "");
        }
    }

    fn maybe_poll_position(&mut self) {
        if !self.connected || !self.playback_active {
            return;
        }
        if self.last_position_poll.elapsed() < POSITION_POLL_INTERVAL {
            return;
        }
        self.last_position_poll = Instant::now();
        self.send_command_empty(protocol::CMD_GETCURRENTPOSITION);
    }

    fn maybe_report_progress(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };
        if active.last_progress.elapsed() < PROGRESS_INTERVAL {
            return;
        }
        active.last_progress = Instant::now();
        if let Some(reporter) = &active.reporter {
            reporter.report_progress(&self.last_state);
        }
    }

    fn poll_child(&mut self) {
        if self.child.is_none() {
            return;
        }
        if !self.child_alive() {
            tracing::info!(target: "mpchc", "MPC-HC process exited");
            self.child = None;
            self.connected = false;
            if let Some(transport) = &self.transport {
                transport.clear_target();
            }
            self.finish_active("quit", false);
        }
    }

    fn finish_active(&mut self, reason: &'static str, failed: bool) {
        let had_session = self.active.is_some() || self.pending.is_some();
        if let Some(active) = self.active.take()
            && let Some(reporter) = active.reporter
        {
            reporter.report_stopped(&self.last_state, failed);
        }
        self.pending = None;
        self.awaiting_open = false;
        self.skip_segments.clear();
        self.current_skip_segment = None;
        self.pending_auto_skip = None;
        self.resume_seconds = None;
        self.seeking_osd = false;
        if !had_session {
            return;
        }
        self.playback_active = false;
        let snapshot = self.build_snapshot(false, Some(reason));
        if let Ok(mut guard) = self.snapshot.lock() {
            *guard = snapshot.clone();
        }
        if let Some(event_tx) = &self.event_tx {
            let _ = event_tx.send(PlaybackEvent::Stopped(snapshot));
        }
        self.identity = None;
        self.last_state = ReportingState {
            volume: Some(self.target_volume.round() as i64),
            mute: Some(self.muted),
            ..ReportingState::default()
        };
    }

    fn is_duplicate(&self, key: &str) -> bool {
        let now = Instant::now();
        self.recent_loads.iter().any(|load| {
            load.key == key && now.saturating_duration_since(load.seen_at) < DUPLICATE_DEBOUNCE
        })
    }

    fn report_playback_failure(&self, message: impl Into<String>) {
        if let Some(event_tx) = &self.event_tx {
            let _ = event_tx.send(PlaybackEvent::Failed {
                message: message.into(),
            });
        }
    }

    fn show_osd(&self, message: &str, duration_ms: i32) {
        if let Some(transport) = &self.transport {
            transport.send_osd(protocol::OSD_TOPLEFT, duration_ms, message);
        }
    }

    fn send_command(&mut self, command: u32, payload: &str) -> bool {
        self.transport
            .as_ref()
            .is_some_and(|transport| transport.send_command(command, payload))
    }

    fn send_command_empty(&mut self, command: u32) -> bool {
        self.send_command(command, "")
    }

    fn build_snapshot(&self, active: bool, stop_reason: Option<&'static str>) -> PlayerSnapshot {
        let identity = self.identity.as_ref();
        PlayerSnapshot {
            active,
            playback_id: identity.map(|identity| identity.playback_id),
            item_id: identity.and_then(|identity| identity.item_id.clone()),
            media_source_id: identity.and_then(|identity| identity.media_source_id.clone()),
            play_session_id: identity.and_then(|identity| identity.play_session_id.clone()),
            position_ms: self.last_state.position_ticks.max(0) as f64 / 10_000.0,
            duration_ms: self
                .last_state
                .duration_ticks
                .map(|ticks| ticks as f64 / 10_000.0),
            paused: self.last_state.pause,
            volume: self.last_state.volume,
            mute: self.last_state.mute,
            stop_reason,
        }
    }

    fn publish_snapshot(&self) {
        let snapshot = self.build_snapshot(self.playback_active, None);
        if let Ok(mut guard) = self.snapshot.lock() {
            *guard = snapshot;
        }
    }

    fn shutdown(&mut self) {
        self.finish_active("quit", false);
        if let Some(transport) = &self.transport {
            transport.send_now(protocol::CMD_CLOSEAPP, "");
        }
        if let Some(mut child) = self.child.take()
            && matches!(child.try_wait(), Ok(None))
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(mut transport) = self.transport.take() {
            transport.shutdown();
        }
        self.inbound = None;
        if !flush_playstate_reports(PLAYSTATE_SHUTDOWN_FLUSH_TIMEOUT) {
            tracing::warn!(target: "jellyfin.playstate", "timed out flushing queued Jellyfin playback state during MPC-HC shutdown");
        }
    }
}

fn volume_step_count(delta: f64) -> i64 {
    (delta.abs() / VOLUME_STEP_PERCENT).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_context_must_match_the_active_identity() {
        let identity = Identity {
            playback_id: 1,
            item_id: Some("current".to_string()),
            media_source_id: Some("source".to_string()),
            play_session_id: None,
        };
        assert!(identity_matches_context(
            &identity,
            &PlaybackContext {
                item_id: Some("current".to_string()),
                ..Default::default()
            }
        ));
        assert!(!identity_matches_context(
            &identity,
            &PlaybackContext {
                item_id: Some("next".to_string()),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn shutdown_ack_deadline_includes_playstate_flush_and_player_close() {
        assert!(
            SHUTDOWN_ACK_TIMEOUT
                > PLAYSTATE_SHUTDOWN_FLUSH_TIMEOUT
                    .saturating_add(crate::players::mpchc::transport::SHUTDOWN_SEND_TIMEOUT)
        );
    }

    #[test]
    fn volume_step_count_rounds_to_nearest_step() {
        assert_eq!(volume_step_count(0.0), 0);
        assert_eq!(volume_step_count(-25.0), 5);
        assert_eq!(volume_step_count(23.0), 5);
        assert_eq!(volume_step_count(2.0), 0);
    }
}
