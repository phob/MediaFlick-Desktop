use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use crate::preferences::{FullscreenBehavior, PlayerPreferences};

use super::{
    NativeWindowHandle, PlaybackContext, PlaybackRequest, PlayerBackend, PlayerCommand,
    PlayerSnapshot,
};

/// Application-facing playback service.
///
/// It gives the shell a stable, shareable handle while concrete adapters can be
/// replaced after a preference change. Player calls never require holding CEF
/// browser-state locks.
pub struct PlaybackCoordinator {
    backend: Mutex<Box<dyn PlayerBackend>>,
    player_path: Mutex<Option<String>>,
}

impl PlaybackCoordinator {
    pub fn new(backend: Box<dyn PlayerBackend>) -> Self {
        Self {
            backend: Mutex::new(backend),
            player_path: Mutex::new(None),
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
        *self
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
            .player_path
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(path.clone());
        self.backend().warm(path, fullscreen);
    }

    pub fn native_window(&self, timeout: Duration) -> Option<NativeWindowHandle> {
        self.backend().native_window(timeout)
    }

    #[cfg(target_os = "linux")]
    pub fn present_overlay(
        &self,
        frame: super::NativeOverlayFrame,
    ) -> Result<super::NativeOverlayFrame, String> {
        let response = self.backend().submit_overlay(frame)?;
        response
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| format!("native UI presentation did not complete: {error}"))?
    }

    pub fn open(&self, path: String, fullscreen: FullscreenBehavior, request: PlaybackRequest) {
        // The backend and executable are one runtime choice. A settings save
        // may persist a different window model for the next launch, but the
        // running coordinator must keep feeding its already-warmed backend.
        let path = self
            .player_path
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or(path);
        self.backend().load(path, fullscreen, request);
    }

    pub fn control(&self, command: PlayerCommand) {
        self.backend().control(command);
    }

    pub fn configure(&self, preferences: PlayerPreferences) {
        self.backend().set_preferences(preferences);
    }

    pub fn update_context(&self, context: PlaybackContext) {
        self.backend().update_playback_context(context);
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.backend().snapshot()
    }

    pub fn shutdown(&self) {
        self.backend().shutdown();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    struct RecordingBackend {
        configured: Arc<AtomicUsize>,
        loaded_paths: Arc<Mutex<Vec<String>>>,
    }

    impl PlayerBackend for RecordingBackend {
        fn warm(&self, _path: String, _fullscreen: FullscreenBehavior) {}

        fn native_window(&self, _timeout: Duration) -> Option<NativeWindowHandle> {
            None
        }

        fn load(&self, path: String, _fullscreen: FullscreenBehavior, _request: PlaybackRequest) {
            self.loaded_paths
                .lock()
                .expect("record loaded path")
                .push(path);
        }

        fn control(&self, _command: PlayerCommand) {}

        fn set_preferences(&self, _preferences: PlayerPreferences) {
            self.configured.fetch_add(1, Ordering::SeqCst);
        }

        fn update_playback_context(&self, _context: PlaybackContext) {}

        fn snapshot(&self) -> PlayerSnapshot {
            PlayerSnapshot::default()
        }

        fn shutdown(&self) {}
    }

    #[test]
    fn preference_changes_reach_the_running_backend() {
        let configured = Arc::new(AtomicUsize::new(0));
        let coordinator = PlaybackCoordinator::new(Box::new(RecordingBackend {
            configured: configured.clone(),
            loaded_paths: Arc::new(Mutex::new(Vec::new())),
        }));

        coordinator.configure(crate::preferences::AppSettings::default().player_preferences());

        assert_eq!(configured.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_pending_restart_does_not_reconfigure_the_running_backend() {
        let loaded_paths = Arc::new(Mutex::new(Vec::new()));
        let coordinator = PlaybackCoordinator::new(Box::new(RecordingBackend {
            configured: Arc::new(AtomicUsize::new(0)),
            loaded_paths: loaded_paths.clone(),
        }));
        coordinator.warm("startup-player".to_string(), FullscreenBehavior::Windowed);

        coordinator.open(
            "saved-for-next-launch".to_string(),
            FullscreenBehavior::Fullscreen,
            PlaybackRequest::default(),
        );

        assert_eq!(
            loaded_paths.lock().expect("read loaded path").as_slice(),
            ["startup-player"]
        );
    }
}
