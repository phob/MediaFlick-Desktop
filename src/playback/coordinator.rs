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

    pub fn refresh_input_bindings(&self) {
        self.backend().refresh_input_bindings();
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::playback::Capabilities;

    struct RecordingBackend {
        binding_refreshes: Arc<AtomicUsize>,
    }

    impl PlayerBackend for RecordingBackend {
        fn warm(&self, _path: String, _fullscreen: FullscreenBehavior) {}

        fn load(&self, _path: String, _fullscreen: FullscreenBehavior, _request: PlaybackRequest) {}

        fn control(&self, _command: PlayerCommand) {}

        fn refresh_input_bindings(&self) {
            self.binding_refreshes.fetch_add(1, Ordering::SeqCst);
        }

        fn set_segment_skip_config(&self, _config: SegmentSkipConfig) {}

        fn update_playback_context(&self, _context: PlaybackContext) {}

        fn snapshot(&self) -> PlayerSnapshot {
            PlayerSnapshot::default()
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }

        fn shutdown(&self) {}
    }

    #[test]
    fn input_binding_refresh_reaches_the_running_backend() {
        let refreshes = Arc::new(AtomicUsize::new(0));
        let coordinator = PlaybackCoordinator::new(Box::new(RecordingBackend {
            binding_refreshes: refreshes.clone(),
        }));

        coordinator.refresh_input_bindings();

        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    }
}
