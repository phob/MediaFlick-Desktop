use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;

use crate::preferences::{FullscreenBehavior, SegmentSkipConfig};

use super::{
    Capabilities, PlaybackContext, PlaybackRequest, PlayerBackend, PlayerCommand, PlayerSnapshot,
};

/// Application-facing playback service.
///
/// It gives the shell a stable, shareable handle while concrete adapters can be
/// replaced after a preference change. Player calls never require holding CEF
/// browser-state locks.
pub struct PlaybackCoordinator {
    backend: Mutex<Box<dyn PlayerBackend>>,
}

impl PlaybackCoordinator {
    pub fn new(backend: Box<dyn PlayerBackend>) -> Self {
        Self {
            backend: Mutex::new(backend),
        }
    }

    fn backend(&self) -> MutexGuard<'_, Box<dyn PlayerBackend>> {
        self.backend.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Publish `backend` as the active player and retire the previous one.
    ///
    /// The retired backend's bounded teardown runs on a detached thread so the
    /// calling thread (usually CEF's UI thread) never waits on player shutdown.
    pub fn replace(&self, backend: Box<dyn PlayerBackend>) {
        let retired = std::mem::replace(&mut *self.backend(), backend);
        if let Err(error) = thread::Builder::new()
            .name("playback-retire".to_string())
            .spawn(move || retired.shutdown())
        {
            tracing::warn!(target: "playback", "failed to spawn retired-backend shutdown thread: {error}");
        }
    }

    pub fn warm(&self, path: String, fullscreen: FullscreenBehavior) {
        self.backend().warm(path, fullscreen);
    }

    pub fn open(&self, path: String, fullscreen: FullscreenBehavior, request: PlaybackRequest) {
        self.backend().load(path, fullscreen, request);
    }

    pub fn control(&self, command: PlayerCommand) {
        self.backend().control(command);
    }

    pub fn configure_segments(&self, config: SegmentSkipConfig) {
        self.backend().set_segment_skip_config(config);
    }

    pub fn update_context(&self, context: PlaybackContext) {
        self.backend().update_playback_context(context);
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.backend().snapshot()
    }

    pub fn capabilities(&self) -> Capabilities {
        self.backend().capabilities()
    }

    pub fn shutdown(&self) {
        self.backend().shutdown();
    }
}
