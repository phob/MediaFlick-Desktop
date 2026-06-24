use std::collections::VecDeque;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::app::settings::{MpvFullscreenBehavior, SegmentSkipConfig, SegmentSkipMode};
use crate::jellyfin::bridge::{self as jellyfin_bridge, PlaybackContext};
use crate::jellyfin::media_segments::{self, SkipSegment};
use crate::jellyfin::playback_reporter::{
    MpvPlaybackState, PlaybackReporter, TICKS_PER_SECOND, seconds_to_ticks,
};
use crate::mpv::{MpvControlCommand, MpvLaunch, MpvPlaybackEvent, MpvPlayerSnapshot};
use crate::player::segments;

use super::protocol::{self, Inbound};
use super::transport::MpcHcTransport;

const RECV_TIMEOUT: Duration = Duration::from_millis(200);
const POSITION_POLL_INTERVAL: Duration = Duration::from_secs(1);
const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);
const DUPLICATE_DEBOUNCE: Duration = Duration::from_secs(2);
const SHUTDOWN_ACK_TIMEOUT: Duration = Duration::from_secs(10);
const SEGMENT_SKIP_OSD_DURATION_MS: i32 = 3000;
const SEGMENT_SKIP_OSD_DEBOUNCE: Duration = Duration::from_secs(3);
const SEGMENT_AUTO_SKIP_DELAY: Duration = Duration::from_secs(3);
const SEGMENT_AUTO_SKIP_COUNTDOWN_INTERVAL: Duration = Duration::from_secs(1);
const SEGMENT_AUTO_SKIP_COUNTDOWN_OSD_DURATION_MS: i32 = 1200;
const VOLUME_STEP_PERCENT: f64 = 5.0;
const SEEKING_OSD_DURATION_MS: i32 = 60_000;

static PLAYBACK_COUNTER: AtomicI64 = AtomicI64::new(1);

fn next_playback_id() -> i64 {
    PLAYBACK_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
pub struct MpcHcController {
    tx: Sender<Msg>,
    snapshot: Arc<Mutex<MpvPlayerSnapshot>>,
    shutdown_requested: Arc<AtomicBool>,
}

enum Msg {
    Warm {
        path: String,
        fullscreen: MpvFullscreenBehavior,
    },
    Load {
        path: String,
        fullscreen: MpvFullscreenBehavior,
        launch: Box<MpvLaunch>,
    },
    Control(MpvControlCommand),
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
    fn from_launch(playback_id: i64, launch: &MpvLaunch) -> Self {
        Self {
            playback_id,
            item_id: launch.item_id.clone(),
            media_source_id: launch.media_source_id.clone(),
            play_session_id: launch.play_session_id.clone(),
        }
    }
}

struct Pending {
    launch: MpvLaunch,
    reporter: Option<PlaybackReporter>,
}

struct Active {
    reporter: Option<PlaybackReporter>,
    last_progress: Instant,
}

#[derive(Clone)]
struct RecentLoad {
    key: String,
    seen_at: Instant,
}

#[derive(Clone, Copy)]
struct PendingAutoSkip {
    segment_index: usize,
    due_at: Instant,
    next_countdown_at: Instant,
}

impl MpcHcController {
    pub fn new(
        event_tx: Option<Sender<MpvPlaybackEvent>>,
        segment_skip_config: SegmentSkipConfig,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let snapshot = Arc::new(Mutex::new(MpvPlayerSnapshot::default()));
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
            last_state: MpvPlaybackState {
                volume: Some(100),
                ..MpvPlaybackState::default()
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
            fullscreen_pref: MpvFullscreenBehavior::default(),
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

    pub fn warm(&self, path: String, fullscreen: MpvFullscreenBehavior) {
        let _ = self.tx.send(Msg::Warm { path, fullscreen });
    }

    pub fn load(&self, path: String, fullscreen: MpvFullscreenBehavior, launch: MpvLaunch) {
        let _ = self.tx.send(Msg::Load {
            path,
            fullscreen,
            launch: Box::new(launch),
        });
    }

    pub fn control(&self, command: MpvControlCommand) {
        let _ = self.tx.send(Msg::Control(command));
    }

    pub fn set_segment_skip_config(&self, config: SegmentSkipConfig) {
        let _ = self.tx.send(Msg::SegmentSkipConfig(config));
    }

    pub fn update_playback_context(&self, context: PlaybackContext) {
        let _ = self.tx.send(Msg::PlaybackContext(Box::new(context)));
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
    snapshot: Arc<Mutex<MpvPlayerSnapshot>>,
    event_tx: Option<Sender<MpvPlaybackEvent>>,
    shutdown_requested: Arc<AtomicBool>,
    transport: Option<MpcHcTransport>,
    inbound: Option<Receiver<Inbound>>,
    child: Option<Child>,
    connected: bool,
    last_state: MpvPlaybackState,
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
    fullscreen_pref: MpvFullscreenBehavior,
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
                }) => self.load(path, fullscreen, *launch),
                Ok(Msg::Control(command)) => self.control(command),
                Ok(Msg::PlaybackContext(context)) => self.update_context(*context),
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

    fn warm(&mut self, _path: String, _fullscreen: MpvFullscreenBehavior) {
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

    fn ensure_process(&mut self, path: &str, fullscreen: MpvFullscreenBehavior) -> bool {
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

    fn launch(&mut self, path: &str, fullscreen: MpvFullscreenBehavior) -> bool {
        let Some(transport) = &self.transport else {
            return false;
        };
        let mut command = Command::new(path);
        command.arg("/slave").arg(transport.our_hwnd_arg());
        command.arg("/new");
        if fullscreen == MpvFullscreenBehavior::Fullscreen {
            command.arg("/fullscreen");
        }
        match command.spawn() {
            Ok(child) => {
                tracing::info!(target: "mpchc", path = %path, "launched MPC-HC slave process");
                crate::windows::confine_to_app_lifetime(&child);
                self.child = Some(child);
                self.fullscreen_state = fullscreen == MpvFullscreenBehavior::Fullscreen;
                true
            }
            Err(error) => {
                tracing::warn!(target: "mpchc", path = %path, "failed to launch MPC-HC: {error}");
                false
            }
        }
    }

    fn load(&mut self, path: String, fullscreen: MpvFullscreenBehavior, launch: MpvLaunch) {
        let key = launch.dedupe_key();
        if self.is_duplicate(&key) {
            tracing::debug!(target: "mpchc", dedupe_key = %key, "ignored duplicate playback load");
            return;
        }
        self.fullscreen_pref = fullscreen;
        if !self.ensure_process(&path, fullscreen) {
            tracing::warn!(target: "mpchc", "cannot load playback because MPC-HC is unavailable");
            self.report_playback_failure(
                "Could not start MPC-HC. Check that the MPC-HC path in Settings is correct.",
            );
            return;
        }

        self.finish_active("replaced", false);

        let identity = Identity::from_launch(next_playback_id(), &launch);
        let playback_id = identity.playback_id;
        let reporter = PlaybackReporter::from_launch(&launch);
        self.resume_seconds = launch.start_seconds().filter(|seconds| *seconds > 0.0);
        self.last_state = MpvPlaybackState {
            volume: Some(self.target_volume.round() as i64),
            mute: Some(self.muted),
            position_ticks: self
                .resume_seconds
                .and_then(seconds_to_ticks)
                .unwrap_or_default(),
            ..MpvPlaybackState::default()
        };
        self.skip_segments.clear();
        self.current_skip_segment = None;
        self.pending_auto_skip = None;
        self.last_skip_osd_at = None;
        self.seeking_osd = false;
        self.identity = Some(identity);
        self.playback_active = true;

        self.pending = Some(Pending {
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
            url = %jellyfin_bridge::redact_url_secrets(&url),
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
                reporter: pending.reporter,
                last_progress: Instant::now(),
            });
        }
        self.send_command_empty(protocol::CMD_GETNOWPLAYING);
        if let Some(selection) = selection {
            self.apply_track_selection(selection);
        }
        self.apply_default_fullscreen();
        if let Some(target) = self.resume_seconds.take().filter(|seconds| *seconds > 0.0) {
            self.send_seek(target);
        }
        self.last_position_poll = Instant::now();
        self.publish_snapshot();
    }

    fn apply_track_selection(&mut self, selection: TrackSelection) {
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
        let want = self.fullscreen_pref == MpvFullscreenBehavior::Fullscreen;
        if self.fullscreen_state == want {
            return;
        }
        self.send_command_empty(protocol::CMD_TOGGLEFULLSCREEN);
        self.fullscreen_state = want;
    }

    fn control(&mut self, command: MpvControlCommand) {
        match command {
            MpvControlCommand::SetPause(true) => {
                self.send_command_empty(protocol::CMD_PAUSE);
            }
            MpvControlCommand::SetPause(false) => {
                self.send_command_empty(protocol::CMD_PLAY);
            }
            MpvControlCommand::SeekMilliseconds(position_ms) => {
                if !self.handle_prompt_skip(position_ms) {
                    self.send_seek(position_ms / 1000.0);
                }
            }
            MpvControlCommand::SetPlaybackRate(rate) => {
                self.send_command(protocol::CMD_SETSPEED, &format!("{rate}"));
            }
            MpvControlCommand::SetAudioTrack(index) => {
                if let Some(track) = mpchc_audio_index(index) {
                    self.send_command(protocol::CMD_SETAUDIOTRACK, &track.to_string());
                }
            }
            MpvControlCommand::SetSubtitleTrack(index) => {
                self.send_command(
                    protocol::CMD_SETSUBTITLETRACK,
                    &mpchc_subtitle_index(index).to_string(),
                );
            }
            MpvControlCommand::AddSubtitle(_) => {
                tracing::debug!(target: "mpchc", "external subtitles are delivered burned-in, not via runtime sub-add");
            }
            MpvControlCommand::SetVolume(volume) => self.set_volume(volume),
            MpvControlCommand::SetMute(mute) => self.set_mute(mute),
            MpvControlCommand::Stop => {
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

    fn update_context(&mut self, context: PlaybackContext) {
        if let Some(active) = &mut self.active
            && let Some(reporter) = &mut active.reporter
        {
            reporter.merge_context(&context);
        }
        if let Some(pending) = &mut self.pending {
            context.merge_into_launch(&mut pending.launch);
            if let Some(reporter) = &mut pending.reporter {
                reporter.merge_context(&context);
            }
        }
    }

    fn apply_segment_skip_config(&mut self, config: SegmentSkipConfig) {
        self.segment_skip_config = config;
        self.pending_auto_skip = None;
        if config.all_disabled() {
            self.skip_segments.clear();
            self.current_skip_segment = None;
        } else {
            self.update_skip_state(self.last_state.position_ticks);
        }
    }

    fn fetch_media_segments(&self, playback_id: i64, launch: MpvLaunch) {
        if self.segment_skip_config.all_disabled() {
            return;
        }
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = media_segments::fetch_for_launch(&launch);
            let _ = tx.send(Msg::MediaSegments {
                playback_id,
                result,
            });
        });
    }

    fn handle_media_segments(
        &mut self,
        playback_id: i64,
        result: Result<Vec<SkipSegment>, String>,
    ) {
        if !self.playback_id_is_current(playback_id) || self.segment_skip_config.all_disabled() {
            return;
        }
        match result {
            Ok(segments) => {
                self.skip_segments = segments;
                self.current_skip_segment = None;
                self.last_skip_osd_at = None;
                self.update_skip_state(self.last_state.position_ticks);
            }
            Err(error) => {
                tracing::warn!(target: "mpchc", playback_id, "failed to fetch media segments: {error}")
            }
        }
    }

    fn playback_id_is_current(&self, playback_id: i64) -> bool {
        self.identity
            .as_ref()
            .is_some_and(|identity| identity.playback_id == playback_id)
    }

    fn drain_inbound(&mut self) {
        let mut messages = Vec::new();
        if let Some(inbound) = &self.inbound {
            while let Ok(message) = inbound.try_recv() {
                messages.push(message);
            }
        }
        for message in messages {
            self.handle_inbound(message);
        }
    }

    fn handle_inbound(&mut self, message: Inbound) {
        match message {
            Inbound::Connect { hwnd } => self.on_connected(hwnd),
            Inbound::State(state) if state == protocol::MLS_LOADED => self.on_loaded(),
            Inbound::State(state) if state == protocol::MLS_FAILING => {
                self.finish_active("error", true)
            }
            Inbound::State(_) => {}
            Inbound::PlayMode(mode) => self.handle_play_mode(mode),
            Inbound::NowPlaying { duration_seconds } => {
                self.last_state.duration_ticks = duration_seconds.and_then(seconds_to_ticks);
                self.publish_snapshot();
            }
            Inbound::CurrentPosition(seconds) => self.handle_position(seconds, false),
            Inbound::NotifySeek(seconds) => self.handle_position(seconds, true),
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

    fn update_skip_state(&mut self, ticks: i64) {
        if self.skip_segments.is_empty() || self.segment_skip_config.all_disabled() {
            self.current_skip_segment = None;
            self.pending_auto_skip = None;
            return;
        }
        let Some(index) = segments::active_segment_at(&self.skip_segments, ticks) else {
            self.current_skip_segment = None;
            self.pending_auto_skip = None;
            return;
        };
        let entered = self.current_skip_segment != Some(index);
        self.current_skip_segment = Some(index);
        match segments::mode_for_segment(
            &self.segment_skip_config,
            self.skip_segments[index].segment_type,
        ) {
            SegmentSkipMode::Disabled => self.pending_auto_skip = None,
            SegmentSkipMode::Prompt => {
                self.pending_auto_skip = None;
                self.maybe_show_prompt(index, entered);
            }
            SegmentSkipMode::Always => self.start_auto_skip(index),
        }
    }

    fn maybe_show_prompt(&mut self, index: usize, entered: bool) {
        let now = Instant::now();
        if !entered
            && self.last_skip_osd_at.is_some_and(|shown| {
                now.saturating_duration_since(shown) < SEGMENT_SKIP_OSD_DEBOUNCE
            })
        {
            return;
        }
        let Some(segment) = self.skip_segments.get(index) else {
            return;
        };
        let text = segment.segment_type.prompt_text();
        self.show_osd(text, SEGMENT_SKIP_OSD_DURATION_MS);
        self.last_skip_osd_at = Some(now);
    }

    fn start_auto_skip(&mut self, index: usize) {
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
        self.pending_auto_skip = Some(PendingAutoSkip {
            segment_index: index,
            due_at: now + SEGMENT_AUTO_SKIP_DELAY,
            next_countdown_at: now + SEGMENT_AUTO_SKIP_COUNTDOWN_INTERVAL,
        });
        self.show_auto_skip_countdown(index, SEGMENT_AUTO_SKIP_DELAY.as_secs().max(1));
    }

    fn maybe_update_auto_skip(&mut self) {
        let Some(pending) = self.pending_auto_skip else {
            return;
        };
        if !self.auto_skip_valid(pending.segment_index) {
            self.pending_auto_skip = None;
            return;
        }
        let now = Instant::now();
        if now >= pending.due_at {
            self.pending_auto_skip = None;
            self.skip_segment(pending.segment_index, "automatic segment skip");
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
            if let Some(current) = &mut self.pending_auto_skip {
                current.next_countdown_at = now + SEGMENT_AUTO_SKIP_COUNTDOWN_INTERVAL;
            }
        }
    }

    fn auto_skip_valid(&self, index: usize) -> bool {
        self.skip_segments.get(index).is_some_and(|segment| {
            !segment.triggered
                && segments::mode_for_segment(&self.segment_skip_config, segment.segment_type)
                    == SegmentSkipMode::Always
                && self.last_state.position_ticks >= segment.start_ticks
                && self.last_state.position_ticks < segment.end_ticks
        })
    }

    fn show_auto_skip_countdown(&self, index: usize, remaining_seconds: u64) {
        let Some(segment) = self.skip_segments.get(index) else {
            return;
        };
        let label = segment.segment_type.countdown_label();
        self.show_osd(
            &format!("Skipping {label} in {remaining_seconds}..."),
            SEGMENT_AUTO_SKIP_COUNTDOWN_OSD_DURATION_MS,
        );
    }

    fn handle_prompt_skip(&mut self, position_ms: f64) -> bool {
        let Some(requested) = seconds_to_ticks(position_ms / 1000.0) else {
            return false;
        };
        let current = self.last_state.position_ticks;
        if requested <= current {
            return false;
        }
        let Some(index) =
            segments::prompt_segment_at(&self.skip_segments, &self.segment_skip_config, current)
        else {
            return false;
        };
        self.skip_segment(index, "forward seek accepted skip prompt")
    }

    fn maybe_accept_seek_skip(&mut self, previous: i64, current: i64) {
        if current <= previous {
            return;
        }
        if let Some(index) =
            segments::prompt_segment_at(&self.skip_segments, &self.segment_skip_config, previous)
        {
            self.skip_segment(index, "native forward seek accepted skip prompt");
        }
    }

    fn skip_segment(&mut self, index: usize, reason: &str) -> bool {
        let Some(segment) = self.skip_segments.get(index) else {
            return false;
        };
        if segment.triggered {
            return false;
        }
        let end_ticks = segment.end_ticks;
        let segment_type = segment.segment_type;
        if end_ticks <= self.last_state.position_ticks {
            self.current_skip_segment = None;
            self.pending_auto_skip = None;
            self.mark_triggered(index);
            return false;
        }
        let seconds = end_ticks as f64 / TICKS_PER_SECOND;
        if !self.send_command(protocol::CMD_SETPOSITION, &format!("{seconds:.3}")) {
            tracing::warn!(target: "mpchc", reason, end_ticks, "skip seek failed to send; leaving segment for retry");
            return false;
        }
        self.current_skip_segment = None;
        self.pending_auto_skip = None;
        self.last_skip_osd_at = Some(Instant::now());
        self.mark_triggered(index);
        tracing::info!(target: "mpchc", reason, end_ticks, "skipped media segment");
        self.show_osd(segment_type.skipped_text(), SEGMENT_SKIP_OSD_DURATION_MS);
        true
    }

    fn mark_triggered(&mut self, index: usize) {
        if let Some(segment) = self.skip_segments.get_mut(index) {
            segment.triggered = true;
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
            let _ = event_tx.send(MpvPlaybackEvent::Stopped(snapshot));
        }
        self.identity = None;
        self.last_state = MpvPlaybackState {
            volume: Some(self.target_volume.round() as i64),
            mute: Some(self.muted),
            ..MpvPlaybackState::default()
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
            let _ = event_tx.send(MpvPlaybackEvent::Failed {
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

    fn build_snapshot(&self, active: bool, stop_reason: Option<&'static str>) -> MpvPlayerSnapshot {
        let identity = self.identity.as_ref();
        MpvPlayerSnapshot {
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
    }
}

fn mpchc_media_url(launch: &MpvLaunch) -> String {
    let url = apply_subtitle_burn_in(launch.media_url.clone(), launch);
    if url_has_token(&url) {
        return url;
    }
    let Some(token) = token_from_headers(launch) else {
        return url;
    };
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}api_key={token}")
}

fn volume_step_count(delta: f64) -> i64 {
    (delta.abs() / VOLUME_STEP_PERCENT).round() as i64
}

struct TrackSelection {
    audio_index: Option<i64>,
    subtitle_index: Option<i64>,
}

fn track_selection(launch: &MpvLaunch) -> TrackSelection {
    let audio_index = launch.audio_mpv_id.and_then(mpchc_audio_index);
    let has_external_subtitle = launch
        .subtitle_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let subtitle_index = if has_external_subtitle {
        Some(-1)
    } else {
        launch
            .subtitle_mpv_id
            .map(|id| mpchc_subtitle_index(Some(id)))
    };
    TrackSelection {
        audio_index,
        subtitle_index,
    }
}

fn mpchc_audio_index(mpv_id: i64) -> Option<i64> {
    (mpv_id > 0).then_some(mpv_id - 1)
}

fn mpchc_subtitle_index(mpv_id: Option<i64>) -> i64 {
    match mpv_id {
        Some(id) if id > 0 => id - 1,
        _ => -1,
    }
}

fn apply_subtitle_burn_in(url: String, launch: &MpvLaunch) -> String {
    let Some(index) = launch.subtitle_stream_index.filter(|index| *index >= 0) else {
        return url;
    };
    let is_external = launch
        .subtitle_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if !is_external || query_has_key(&url, "subtitlestreamindex") {
        return url;
    }
    let url = remove_query_keys(&url, &["static"]);
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}SubtitleStreamIndex={index}&SubtitleMethod=Encode")
}

fn query_has_key(url: &str, key: &str) -> bool {
    let Some((_, rest)) = url.split_once('?') else {
        return false;
    };
    let query = rest.split('#').next().unwrap_or(rest);
    query.split('&').any(|pair| {
        pair.split('=')
            .next()
            .unwrap_or_default()
            .eq_ignore_ascii_case(key)
    })
}

fn remove_query_keys(url: &str, keys: &[&str]) -> String {
    let Some((before, rest)) = url.split_once('?') else {
        return url.to_string();
    };
    let (query, fragment) = rest
        .split_once('#')
        .map(|(query, fragment)| (query, Some(fragment)))
        .unwrap_or((rest, None));
    let kept = query
        .split('&')
        .filter(|pair| {
            let key = pair.split('=').next().unwrap_or_default();
            !keys.iter().any(|removed| key.eq_ignore_ascii_case(removed))
        })
        .collect::<Vec<_>>();
    let mut out = String::from(before);
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

fn url_has_token(url: &str) -> bool {
    let Some(query) = url.split_once('?').map(|(_, rest)| rest) else {
        return false;
    };
    query.split(['&', '#']).any(|pair| {
        let key = pair
            .split('=')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        matches!(
            key.as_str(),
            "api_key"
                | "apikey"
                | "access_token"
                | "accesstoken"
                | "x-emby-token"
                | "x-mediabrowser-token"
        )
    })
}

fn token_from_headers(launch: &MpvLaunch) -> Option<String> {
    for header in &launch.headers {
        if header.name.eq_ignore_ascii_case("X-Emby-Token")
            || header.name.eq_ignore_ascii_case("X-MediaBrowser-Token")
        {
            let value = header.value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mpv::HttpHeader;

    fn external_subtitle_launch() -> MpvLaunch {
        MpvLaunch {
            media_url: "https://host/Videos/abc/stream.mkv?static=true&MediaSourceId=src"
                .to_string(),
            subtitle_stream_index: Some(3),
            subtitle_url: Some("https://host/Videos/abc/sub.srt".to_string()),
            ..MpvLaunch::default()
        }
    }

    #[test]
    fn burn_in_drops_static_and_requests_encoded_subtitle() {
        let url = apply_subtitle_burn_in(
            external_subtitle_launch().media_url.clone(),
            &external_subtitle_launch(),
        );
        assert!(!query_has_key(&url, "static"));
        assert!(url.contains("SubtitleStreamIndex=3"));
        assert!(url.contains("SubtitleMethod=Encode"));
        assert!(url.contains("MediaSourceId=src"));
    }

    #[test]
    fn burn_in_skipped_for_embedded_subtitle() {
        let launch = MpvLaunch {
            subtitle_url: None,
            ..external_subtitle_launch()
        };
        let original = launch.media_url.clone();
        assert_eq!(apply_subtitle_burn_in(original.clone(), &launch), original);
    }

    #[test]
    fn burn_in_skipped_without_subtitle_index() {
        let launch = MpvLaunch {
            subtitle_stream_index: None,
            ..external_subtitle_launch()
        };
        let original = launch.media_url.clone();
        assert_eq!(apply_subtitle_burn_in(original.clone(), &launch), original);
    }

    #[test]
    fn burn_in_is_idempotent() {
        let launch = external_subtitle_launch();
        let once = apply_subtitle_burn_in(launch.media_url.clone(), &launch);
        let twice = apply_subtitle_burn_in(once.clone(), &launch);
        assert_eq!(once, twice);
    }

    #[test]
    fn media_url_appends_token_after_burn_in() {
        let launch = MpvLaunch {
            headers: vec![HttpHeader {
                name: "X-Emby-Token".to_string(),
                value: "secret".to_string(),
            }],
            ..external_subtitle_launch()
        };
        let url = mpchc_media_url(&launch);
        assert!(url.contains("SubtitleMethod=Encode"));
        assert!(url.contains("api_key=secret"));
    }

    #[test]
    fn remove_query_keys_preserves_fragment_and_other_pairs() {
        assert_eq!(
            remove_query_keys("https://host/x?a=1&static=true&b=2#frag", &["static"]),
            "https://host/x?a=1&b=2#frag"
        );
        assert_eq!(
            remove_query_keys("https://host/x?static=true", &["static"]),
            "https://host/x"
        );
    }

    #[test]
    fn volume_step_count_rounds_to_nearest_step() {
        assert_eq!(volume_step_count(0.0), 0);
        assert_eq!(volume_step_count(-25.0), 5);
        assert_eq!(volume_step_count(23.0), 5);
        assert_eq!(volume_step_count(2.0), 0);
    }

    #[test]
    fn audio_index_is_zero_based_and_drops_non_tracks() {
        assert_eq!(mpchc_audio_index(1), Some(0));
        assert_eq!(mpchc_audio_index(3), Some(2));
        assert_eq!(mpchc_audio_index(0), None);
        assert_eq!(mpchc_audio_index(-1), None);
    }

    #[test]
    fn subtitle_index_is_zero_based_with_off_sentinel() {
        assert_eq!(mpchc_subtitle_index(Some(1)), 0);
        assert_eq!(mpchc_subtitle_index(Some(5)), 4);
        assert_eq!(mpchc_subtitle_index(Some(-1)), -1);
        assert_eq!(mpchc_subtitle_index(Some(0)), -1);
        assert_eq!(mpchc_subtitle_index(None), -1);
    }

    #[test]
    fn track_selection_converts_embedded_tracks() {
        let launch = MpvLaunch {
            audio_mpv_id: Some(2),
            subtitle_mpv_id: Some(5),
            ..MpvLaunch::default()
        };
        let selection = track_selection(&launch);
        assert_eq!(selection.audio_index, Some(1));
        assert_eq!(selection.subtitle_index, Some(4));
    }

    #[test]
    fn track_selection_disables_embedded_subtitle_for_external() {
        let launch = MpvLaunch {
            subtitle_mpv_id: Some(3),
            subtitle_url: Some("https://host/sub.srt".to_string()),
            ..MpvLaunch::default()
        };
        assert_eq!(track_selection(&launch).subtitle_index, Some(-1));
    }

    #[test]
    fn track_selection_leaves_unset_tracks_alone() {
        let selection = track_selection(&MpvLaunch::default());
        assert_eq!(selection.audio_index, None);
        assert_eq!(selection.subtitle_index, None);
    }
}
