use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use crate::app::threads::spawn_named;
use crate::preferences::{FullscreenBehavior, PlayerPreferences};

use super::model::{AudioNowPlaying, AudioPhase, allocate_playback_id, ticks_to_milliseconds};
use super::queue::{AudioQueue, AudioTrack};
use super::{
    AudioSource, AudioStart, BackendEvent, NativeWindowHandle, NowPlaying, PlaybackContent,
    PlaybackEvent, PlaybackRequest, PlayerBackend, PlayerCommand, PlayerSnapshot, PreparedAudio,
    StopReason,
};

/// Previous within this much of a track's start goes to the previous entry;
/// later it restarts the track.
const PREVIOUS_RESTARTS_AFTER_MS: f64 = 3000.0;
/// How often an idle actor checks that the queue's account is still signed in.
const ACCOUNT_CHECK_INTERVAL: Duration = Duration::from_millis(500);

static NEXT_QUEUE_ID: AtomicU64 = AtomicU64::new(1);

/// A validated audio queue waiting to replace whatever plays.
pub struct AudioPlan {
    queue: AudioQueue,
    start: AudioStart,
}

impl AudioPlan {
    /// `None` for an empty list, a start outside it, or more entries than
    /// [`super::MAX_QUEUE_LENGTH`].
    pub fn new(tracks: Vec<AudioTrack>, start_index: usize, start: AudioStart) -> Option<Self> {
        let queue_id = NEXT_QUEUE_ID.fetch_add(1, Ordering::Relaxed);
        Some(Self {
            queue: AudioQueue::new(queue_id, tracks, start_index)?,
            start,
        })
    }

    pub fn first_item_id(&self) -> &str {
        self.queue.current().track.item_id()
    }
}

/// Application-facing playback service.
///
/// One actor thread owns the playback session: it orders UI and remote
/// commands, audio preparation results and the player's events, keeps the
/// audio queue, and is the only source of the events the shell sees. Native
/// window and overlay calls go straight to the installed backend, so they
/// never wait behind a negotiation.
pub struct PlaybackCoordinator {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    /// The installation of the newest adapter; its relay tags every event with it.
    installed: AtomicU64,
}

struct Shared {
    backend: Mutex<Arc<dyn PlayerBackend>>,
    player_path: Mutex<Option<String>>,
    published: Mutex<Published>,
}

impl Shared {
    fn backend(&self) -> Arc<dyn PlayerBackend> {
        Arc::clone(&self.backend.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn player_path(&self, prepared: String) -> String {
        self.player_path
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or(prepared)
    }

    fn publish(&self, published: Published) {
        *self
            .published
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = published;
    }

    fn project(&self, snapshot: PlayerSnapshot) -> PlayerSnapshot {
        project(
            snapshot,
            &self
                .published
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }
}

/// What snapshots read about the session.
#[derive(Clone)]
enum Published {
    /// Whatever plays is video.
    Video,
    /// The audio queue's view. `load` is the dispatched load of its current
    /// entry; until the player reports that load, snapshots describe the
    /// entry rather than the file still playing.
    Audio {
        view: Box<AudioNowPlaying>,
        load: Option<i64>,
    },
}

fn project(mut snapshot: PlayerSnapshot, published: &Published) -> PlayerSnapshot {
    match published {
        Published::Video => {
            snapshot.now_playing = snapshot.active.then_some(NowPlaying::Video);
        }
        Published::Audio { view, load } => {
            if load.is_none() || snapshot.playback_id != *load {
                snapshot = PlayerSnapshot {
                    active: true,
                    item_id: Some(view.current.item_id.clone()),
                    duration_ms: view.current.runtime_ticks.map(ticks_to_milliseconds),
                    volume: snapshot.volume,
                    mute: snapshot.mute,
                    ..PlayerSnapshot::default()
                };
            }
            snapshot.now_playing = Some(NowPlaying::Audio(view.clone()));
        }
    }
    snapshot
}

enum Command {
    Video {
        path: String,
        fullscreen: FullscreenBehavior,
        request: Box<PlaybackRequest>,
    },
    Audio {
        plan: Box<AudioPlan>,
        source: Arc<dyn AudioSource>,
    },
    Control(PlayerCommand),
    Prepared {
        ticket: u64,
        result: Result<Box<PreparedAudio>, String>,
    },
    Backend {
        installation: u64,
        event: Box<BackendEvent>,
    },
    /// Sent before the adapter of `installation` exists, so its events all
    /// arrive after this.
    BackendReplaced {
        installation: u64,
    },
}

impl PlaybackCoordinator {
    /// `shell` receives every event the UI sees; `make_backend` builds the
    /// first adapter around the sender its events go to.
    pub fn new(
        shell: Sender<PlaybackEvent>,
        make_backend: impl FnOnce(Sender<BackendEvent>) -> Box<dyn PlayerBackend>,
    ) -> Self {
        let (commands, inbox) = mpsc::channel();
        let backend = make_backend(relay_backend_events(commands.clone(), 0));
        let shared = Arc::new(Shared {
            backend: Mutex::new(Arc::from(backend)),
            player_path: Mutex::new(None),
            published: Mutex::new(Published::Video),
        });
        let actor = Actor {
            shared: shared.clone(),
            shell,
            worker: spawn_preparation_worker(commands.clone()),
            installation: 0,
            session: Session::Idle,
            live: None,
            ending: None,
            ticket: 0,
        };
        spawn_named("playback-coordinator", move || actor.run(&inbox));
        Self {
            shared,
            commands,
            installed: AtomicU64::new(0),
        }
    }

    /// Publish a new adapter and retire the previous one. Whatever plays ends
    /// with it; the retired adapter's own events, its shutdown's Stopped
    /// included, never reach the shell.
    ///
    /// The retired backend's bounded teardown runs on a detached thread so the
    /// calling thread (usually CEF's UI thread) never waits on player shutdown.
    pub fn replace(
        &self,
        make_backend: impl FnOnce(Sender<BackendEvent>) -> Box<dyn PlayerBackend>,
    ) {
        let installation = self.installed.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self
            .commands
            .send(Command::BackendReplaced { installation });
        let backend = make_backend(relay_backend_events(self.commands.clone(), installation));
        let retired = std::mem::replace(
            &mut *self
                .shared
                .backend
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
            Arc::from(backend),
        );
        *self
            .shared
            .player_path
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        if let Err(error) = thread::Builder::new()
            .name("playback-retire".to_string())
            .spawn(move || retired.shutdown())
        {
            tracing::warn!(target: "playback", "failed to spawn retired-backend shutdown thread: {error}");
        }
    }

    pub fn warm(&self, path: String, fullscreen: FullscreenBehavior) {
        *self
            .shared
            .player_path
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(path.clone());
        self.shared.backend().warm(path, fullscreen);
    }

    pub fn native_window(&self, timeout: Duration) -> Option<NativeWindowHandle> {
        self.shared.backend().native_window(timeout)
    }

    #[cfg(target_os = "linux")]
    pub fn present_overlay(
        &self,
        frame: super::NativeOverlayFrame,
    ) -> Result<super::NativeOverlayFrame, String> {
        let response = self.shared.backend().submit_overlay(frame)?;
        response
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| format!("native UI presentation did not complete: {error}"))?
    }

    /// Plays a video, replacing whatever plays, an audio queue included.
    pub fn open(&self, path: String, fullscreen: FullscreenBehavior, request: PlaybackRequest) {
        let _ = self.commands.send(Command::Video {
            path,
            fullscreen,
            request: Box::new(request),
        });
    }

    /// Replaces whatever plays with `plan`, whose entries `source` prepares
    /// one at a time.
    pub fn play_audio(&self, plan: AudioPlan, source: Arc<dyn AudioSource>) {
        let _ = self.commands.send(Command::Audio {
            plan: Box::new(plan),
            source,
        });
    }

    pub fn control(&self, command: PlayerCommand) {
        let _ = self.commands.send(Command::Control(command));
    }

    pub fn configure(&self, preferences: PlayerPreferences) {
        self.shared.backend().set_preferences(preferences);
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.shared.project(self.shared.backend().snapshot())
    }

    pub fn shutdown(&self) {
        self.shared.backend().shutdown();
    }
}

/// Each adapter gets its own sender, whose events carry the adapter's
/// installation; the relay ends once the adapter is gone.
fn relay_backend_events(commands: Sender<Command>, installation: u64) -> Sender<BackendEvent> {
    let (events, received) = mpsc::channel();
    spawn_named("playback-backend-relay", move || {
        while let Ok(event) = received.recv() {
            let command = Command::Backend {
                installation,
                event: Box::new(event),
            };
            if commands.send(command).is_err() {
                return;
            }
        }
    });
    events
}

struct Job {
    ticket: u64,
    track: AudioTrack,
    start: AudioStart,
    source: Arc<dyn AudioSource>,
}

/// One worker prepares the most recent selection only, so a run of Next
/// presses negotiates the entry the user stopped on, not every one passed.
fn spawn_preparation_worker(commands: Sender<Command>) -> Sender<Job> {
    let (jobs, received) = mpsc::channel::<Job>();
    spawn_named("playback-prepare", move || {
        while let Ok(mut job) = received.recv() {
            while let Ok(newer) = received.try_recv() {
                job = newer;
            }
            let result = job.source.prepare(&job.track, job.start).map(Box::new);
            if commands
                .send(Command::Prepared {
                    ticket: job.ticket,
                    result,
                })
                .is_err()
            {
                return;
            }
        }
    });
    jobs
}

enum Session {
    Idle,
    Video {
        load_id: i64,
        item_id: Option<String>,
    },
    Audio {
        queue: Box<AudioQueue>,
        source: Arc<dyn AudioSource>,
        /// Where the current entry starts; only the queue's first entry can
        /// carry anything but the default.
        start: AudioStart,
        phase: Phase,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Preparing { ticket: u64 },
    Loading { load_id: i64 },
    Playing { load_id: i64 },
}

impl Phase {
    fn load_id(self) -> Option<i64> {
        match self {
            Self::Preparing { .. } => None,
            Self::Loading { load_id } | Self::Playing { load_id } => Some(load_id),
        }
    }

    fn view(self) -> AudioPhase {
        match self {
            Self::Preparing { .. } => AudioPhase::Preparing,
            Self::Loading { .. } => AudioPhase::Loading,
            Self::Playing { .. } => AudioPhase::Playing,
        }
    }
}

/// An audio load stopped by the user or an account change: its final
/// Stopped event still describes the audio that ended.
struct Ending {
    load_id: i64,
    view: AudioNowPlaying,
}

struct Actor {
    shared: Arc<Shared>,
    shell: Sender<PlaybackEvent>,
    worker: Sender<Job>,
    /// The installed adapter's; events of a retired one are dropped.
    installation: u64,
    session: Session,
    /// The load the player was handed last and has not reported stopped.
    live: Option<i64>,
    ending: Option<Ending>,
    ticket: u64,
}

impl Actor {
    fn run(mut self, inbox: &Receiver<Command>) {
        loop {
            match inbox.recv_timeout(ACCOUNT_CHECK_INTERVAL) {
                Ok(command) => self.handle(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if let Session::Audio { source, .. } = &self.session
                && !source.is_current()
            {
                tracing::info!(target: "playback", "the account that started the audio queue is no longer signed in; stopping it");
                self.stop_audio(StopReason::Stop);
            }
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Video {
                path,
                fullscreen,
                mut request,
            } => {
                self.abandon_audio();
                let load_id = allocate_playback_id();
                request.load_id = load_id;
                request.content = PlaybackContent::Video;
                request.queue = None;
                self.session = Session::Video {
                    load_id,
                    item_id: request.item_id.clone(),
                };
                self.live = Some(load_id);
                self.shared.publish(Published::Video);
                let path = self.shared.player_path(path);
                self.shared.backend().load(path, fullscreen, *request);
            }
            Command::Audio { plan, source } => {
                self.abandon_audio();
                tracing::info!(
                    target: "playback",
                    first = plan.first_item_id(),
                    entries = plan.queue.report().entries.len(),
                    "starting an audio queue"
                );
                let AudioPlan { queue, start } = *plan;
                self.session = Session::Audio {
                    queue: Box::new(queue),
                    source,
                    start,
                    phase: Phase::Preparing { ticket: 0 },
                };
                self.prepare_current();
            }
            Command::Control(command) => self.control(command),
            Command::Prepared { ticket, result } => self.prepared(ticket, result),
            Command::Backend {
                installation,
                event,
            } => {
                if installation == self.installation {
                    self.backend_event(*event);
                } else {
                    tracing::debug!(target: "playback", installation, ?event, "dropped an event of a retired player");
                }
            }
            Command::BackendReplaced { installation } => {
                self.installation = installation;
                self.live = None;
                let ending = self.ending.take();
                let ended = match &self.session {
                    Session::Audio { queue, phase, .. } => {
                        Some(synthetic_stop(queue.view(phase.view()), StopReason::Quit))
                    }
                    Session::Video { load_id, item_id } => Some(PlayerSnapshot {
                        playback_id: Some(*load_id),
                        item_id: item_id.clone(),
                        stop_reason: Some(StopReason::Quit),
                        now_playing: Some(NowPlaying::Video),
                        ..PlayerSnapshot::default()
                    }),
                    Session::Idle => {
                        ending.map(|ending| synthetic_stop(ending.view, StopReason::Quit))
                    }
                };
                self.end_session();
                if let Some(ended) = ended {
                    self.emit_stopped(ended);
                }
            }
        }
    }

    fn control(&mut self, command: PlayerCommand) {
        let Session::Audio {
            queue,
            start,
            phase,
            ..
        } = &mut self.session
        else {
            if !matches!(
                command,
                PlayerCommand::NextTrack | PlayerCommand::PreviousTrack
            ) {
                self.shared.backend().control(command);
            }
            return;
        };
        match command {
            PlayerCommand::NextTrack => {
                if queue.advance() {
                    *start = AudioStart::Default;
                    self.prepare_current();
                }
            }
            PlayerCommand::PreviousTrack => {
                let load = phase.load_id();
                let backend = self.shared.backend();
                let position = load
                    .map(|_| backend.snapshot())
                    .filter(|snapshot| snapshot.playback_id == load)
                    .map_or(0.0, |snapshot| snapshot.position_ms);
                if position > PREVIOUS_RESTARTS_AFTER_MS || !queue.has_previous() {
                    if load.is_some() {
                        backend.control(PlayerCommand::SeekMilliseconds(0.0));
                    }
                } else if queue.retreat() {
                    *start = AudioStart::Default;
                    self.prepare_current();
                }
            }
            PlayerCommand::Stop => self.stop_audio(StopReason::Stop),
            PlayerCommand::MarkWatchedAndPlayNext => {}
            command => self.shared.backend().control(command),
        }
    }

    fn prepare_current(&mut self) {
        self.ticket += 1;
        let ticket = self.ticket;
        let Session::Audio {
            queue,
            source,
            start,
            phase,
        } = &mut self.session
        else {
            return;
        };
        *phase = Phase::Preparing { ticket };
        let _ = self.worker.send(Job {
            ticket,
            track: queue.current().track.clone(),
            start: *start,
            source: source.clone(),
        });
        self.shared.publish(Published::Audio {
            view: Box::new(queue.view(AudioPhase::Preparing)),
            load: None,
        });
        self.emit_state();
    }

    fn prepared(&mut self, ticket: u64, result: Result<Box<PreparedAudio>, String>) {
        let Session::Audio {
            queue,
            source,
            start,
            phase,
        } = &mut self.session
        else {
            return;
        };
        if *phase != (Phase::Preparing { ticket }) {
            tracing::debug!(target: "playback", ticket, "dropped an audio preparation a newer selection replaced");
            return;
        }
        let prepared = match result {
            Ok(prepared) => prepared,
            Err(message) => {
                let title = queue.current().track.display.title.clone();
                tracing::warn!(target: "playback", title, "could not prepare an audio queue entry: {message}");
                let _ = self.shell.send(PlaybackEvent::Failed {
                    message: format!("Could not play {title}: {message}"),
                });
                self.stop_audio(StopReason::Error);
                return;
            }
        };
        let load_id = allocate_playback_id();
        let PreparedAudio {
            player_path,
            mut request,
        } = *prepared;
        request.load_id = load_id;
        request.content = PlaybackContent::Audio(queue.current().track.kind);
        request.queue = Some(queue.report());
        let path = self.shared.player_path(player_path);
        let backend = self.shared.backend();
        let mut pending = Some(request);
        let dispatched = source.dispatch_if_current(&mut || {
            if let Some(request) = pending.take() {
                backend.load(path.clone(), FullscreenBehavior::Windowed, request);
            }
        });
        if !dispatched {
            tracing::info!(target: "playback", "dropped an audio entry prepared for an account that is no longer signed in");
            self.stop_audio(StopReason::Stop);
            return;
        }
        *start = AudioStart::Default;
        *phase = Phase::Loading { load_id };
        self.live = Some(load_id);
        self.shared.publish(Published::Audio {
            view: Box::new(queue.view(AudioPhase::Loading)),
            load: Some(load_id),
        });
    }

    fn backend_event(&mut self, event: BackendEvent) {
        match event {
            BackendEvent::StateChanged(snapshot) => {
                let _ = self
                    .shell
                    .send(PlaybackEvent::StateChanged(self.shared.project(snapshot)));
            }
            BackendEvent::Started { playback_id } => {
                if let Session::Audio { queue, phase, .. } = &mut self.session
                    && *phase
                        == (Phase::Loading {
                            load_id: playback_id,
                        })
                {
                    *phase = Phase::Playing {
                        load_id: playback_id,
                    };
                    self.shared.publish(Published::Audio {
                        view: Box::new(queue.view(AudioPhase::Playing)),
                        load: Some(playback_id),
                    });
                }
            }
            BackendEvent::Stopped(snapshot) => self.stopped(snapshot),
            BackendEvent::TrackRequested { next } => self.control(if next {
                PlayerCommand::NextTrack
            } else {
                PlayerCommand::PreviousTrack
            }),
            BackendEvent::Failed { load_id, message } => {
                let _ = self.shell.send(PlaybackEvent::Failed { message });
                // A refused load never plays, so the player never reports it
                // stopped; it ends here as that report would end it.
                if self.live == Some(load_id) {
                    self.stopped(PlayerSnapshot {
                        playback_id: Some(load_id),
                        stop_reason: Some(StopReason::Error),
                        ..PlayerSnapshot::default()
                    });
                }
            }
        }
    }

    fn stopped(&mut self, mut snapshot: PlayerSnapshot) {
        let load = snapshot.playback_id;
        if self.live.is_some() && self.live == load {
            self.live = None;
        }
        if let Some(ending) = self.ending.take_if(|ending| Some(ending.load_id) == load) {
            snapshot.now_playing = Some(NowPlaying::Audio(Box::new(ending.view)));
            self.emit_stopped(snapshot);
            return;
        }
        match &mut self.session {
            Session::Audio {
                queue,
                start,
                phase,
                ..
            } if phase.load_id().is_some() && phase.load_id() == load => {
                let finished = queue.view(phase.view());
                snapshot.now_playing = Some(NowPlaying::Audio(Box::new(finished)));
                if snapshot.stop_reason == Some(StopReason::Eof) && queue.advance() {
                    *start = AudioStart::Default;
                    let _ = self.shell.send(PlaybackEvent::ItemFinished(snapshot));
                    self.prepare_current();
                    return;
                }
                self.end_session();
                self.emit_stopped(snapshot);
            }
            Session::Audio { .. } => {
                tracing::debug!(target: "playback", ?load, "ignored the stop of an audio load the queue moved past");
            }
            Session::Video { load_id, .. } => {
                if load == Some(*load_id) {
                    self.session = Session::Idle;
                }
                snapshot.now_playing = Some(NowPlaying::Video);
                self.emit_stopped(snapshot);
            }
            Session::Idle => {
                snapshot.now_playing = Some(NowPlaying::Video);
                self.emit_stopped(snapshot);
            }
        }
    }

    /// Ends the audio queue and stops what the player plays for it. The
    /// final Stopped event describes the entry that was current.
    fn stop_audio(&mut self, reason: StopReason) {
        let Session::Audio { queue, phase, .. } = &self.session else {
            return;
        };
        let view = queue.view(phase.view());
        self.end_session();
        match self.live {
            Some(load_id) => {
                self.ending = Some(Ending { load_id, view });
                self.shared.backend().control(PlayerCommand::Stop);
            }
            None => self.emit_stopped(synthetic_stop(view, reason)),
        }
    }

    /// Drops the audio queue without stopping the player, for a load that
    /// replaces it.
    fn abandon_audio(&mut self) {
        if matches!(self.session, Session::Audio { .. }) {
            self.end_session();
        }
        self.ending = None;
    }

    fn end_session(&mut self) {
        self.ticket += 1;
        self.session = Session::Idle;
        self.shared.publish(Published::Video);
    }

    fn emit_state(&self) {
        let snapshot = self.shared.project(self.shared.backend().snapshot());
        let _ = self.shell.send(PlaybackEvent::StateChanged(snapshot));
    }

    fn emit_stopped(&self, snapshot: PlayerSnapshot) {
        let _ = self.shell.send(PlaybackEvent::Stopped(snapshot));
    }
}

/// The Stopped event for an audio entry the player never loaded.
fn synthetic_stop(view: AudioNowPlaying, reason: StopReason) -> PlayerSnapshot {
    PlayerSnapshot {
        item_id: Some(view.current.item_id.clone()),
        stop_reason: Some(reason),
        now_playing: Some(NowPlaying::Audio(Box::new(view))),
        ..PlayerSnapshot::default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, mpsc};

    use super::*;

    struct WaitingBackend {
        entered: Mutex<mpsc::Sender<()>>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl PlayerBackend for WaitingBackend {
        fn warm(&self, _path: String, _fullscreen: FullscreenBehavior) {}

        fn native_window(&self, _timeout: Duration) -> Option<NativeWindowHandle> {
            let _ = self.entered.lock().expect("entered").send(());
            let _ = self.release.lock().expect("release").recv();
            None
        }

        fn load(&self, _path: String, _fullscreen: FullscreenBehavior, _request: PlaybackRequest) {}

        fn control(&self, _command: PlayerCommand) {}

        fn set_preferences(&self, _preferences: PlayerPreferences) {}

        fn snapshot(&self) -> PlayerSnapshot {
            PlayerSnapshot {
                active: true,
                ..PlayerSnapshot::default()
            }
        }

        fn shutdown(&self) {}
    }

    #[test]
    fn a_call_that_waits_does_not_block_the_other_player_calls() {
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let (shell, _events) = mpsc::channel();
        let coordinator = Arc::new(PlaybackCoordinator::new(shell, |_| {
            Box::new(WaitingBackend {
                entered: Mutex::new(entered_tx),
                release: Mutex::new(release_rx),
            })
        }));
        let waiting = Arc::clone(&coordinator);
        let window = thread::spawn(move || waiting.native_window(Duration::from_secs(10)));
        entered
            .recv_timeout(Duration::from_secs(5))
            .expect("native_window started");

        let polling = Arc::clone(&coordinator);
        let (snapshot_tx, snapshot) = mpsc::channel();
        thread::spawn(move || {
            let _ = snapshot_tx.send(polling.snapshot());
        });
        let answered = snapshot.recv_timeout(Duration::from_secs(5));

        release.send(()).expect("release native_window");
        window.join().expect("native_window thread");
        assert!(
            answered
                .expect("snapshot answered while native_window waited")
                .active,
            "the snapshot came from the backend"
        );
    }
}
