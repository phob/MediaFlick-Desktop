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
        if !self.ensure_process(&path, fullscreen) {
            tracing::warn!(target: "mpchc", "cannot load playback because MPC-HC is unavailable");
            return;
        }

        self.finish_active("replaced", false);

        let identity = Identity::from_launch(next_playback_id(), &launch);
        let playback_id = identity.playback_id;
        let reporter = PlaybackReporter::from_launch(&launch);
        self.resume_seconds = launch.start_seconds().filter(|seconds| *seconds > 0.0);
        self.last_state = MpvPlaybackState {
            volume: Some(100),
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
        tracing::info!(target: "mpchc", "MPC-HC connected");
        if self.awaiting_open && self.pending.is_some() {
            self.send_open();
        }
    }

    fn on_loaded(&mut self) {
        if let Some(pending) = self.pending.take() {
            if let Some(reporter) = &pending.reporter {
                reporter.report_start(&self.last_state);
            }
            self.active = Some(Active {
                reporter: pending.reporter,
                last_progress: Instant::now(),
            });
        }
        if let Some(seconds) = self.resume_seconds.take() {
            self.send_command(protocol::CMD_SETPOSITION, &format!("{seconds:.3}"));
        }
        self.send_command_empty(protocol::CMD_GETNOWPLAYING);
        self.last_position_poll = Instant::now();
        self.publish_snapshot();
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
                    self.send_command(
                        protocol::CMD_SETPOSITION,
                        &format!("{:.3}", position_ms / 1000.0),
                    );
                }
            }
            MpvControlCommand::SetPlaybackRate(rate) => {
                self.send_command(protocol::CMD_SETSPEED, &format!("{rate}"));
            }
            MpvControlCommand::SetAudioTrack(index) => {
                self.send_command(protocol::CMD_SETAUDIOTRACK, &index.to_string());
            }
            MpvControlCommand::SetSubtitleTrack(index) => {
                self.send_command(
                    protocol::CMD_SETSUBTITLETRACK,
                    &index.unwrap_or(-1).to_string(),
                );
            }
            MpvControlCommand::AddSubtitle(_) => {
                tracing::debug!(target: "mpchc", "external subtitle URLs are not supported on MPC-HC");
            }
            MpvControlCommand::SetVolume(_) | MpvControlCommand::SetMute(_) => {
                tracing::debug!(target: "mpchc", "absolute volume/mute is not supported on MPC-HC");
            }
            MpvControlCommand::Stop => {
                self.finish_active("stop", false);
                self.send_command_empty(protocol::CMD_STOP);
            }
        }
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
        self.current_skip_segment = None;
        self.pending_auto_skip = None;
        self.last_skip_osd_at = Some(Instant::now());
        if end_ticks <= self.last_state.position_ticks {
            self.mark_triggered(index);
            return false;
        }
        let seconds = end_ticks as f64 / TICKS_PER_SECOND;
        self.send_command(protocol::CMD_SETPOSITION, &format!("{seconds:.3}"));
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
            volume: Some(100),
            ..MpvPlaybackState::default()
        };
    }

    fn is_duplicate(&self, key: &str) -> bool {
        let now = Instant::now();
        self.recent_loads.iter().any(|load| {
            load.key == key && now.saturating_duration_since(load.seen_at) < DUPLICATE_DEBOUNCE
        })
    }

    fn show_osd(&self, message: &str, duration_ms: i32) {
        if let Some(transport) = &self.transport {
            transport.send_osd(protocol::OSD_TOPLEFT, duration_ms, message);
        }
    }

    fn send_command(&mut self, command: u32, payload: &str) {
        let sent = self
            .transport
            .as_ref()
            .is_some_and(|transport| transport.send_command(command, payload));
        if !sent {
            tracing::warn!(target: "mpchc", command = format!("{command:#x}"), "failed to send MPC-HC command");
        }
    }

    fn send_command_empty(&mut self, command: u32) {
        self.send_command(command, "");
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
            transport.send_command(protocol::CMD_CLOSEAPP, "");
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
    let url = launch.media_url.clone();
    if url_has_token(&url) {
        return url;
    }
    let Some(token) = token_from_headers(launch) else {
        return url;
    };
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}api_key={token}")
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
