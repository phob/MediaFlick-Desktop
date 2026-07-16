use std::sync::Mutex;

use crate::preferences::{MpvFullscreenBehavior, SegmentSkipConfig};

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

    pub fn replace(&self, backend: Box<dyn PlayerBackend>) {
        let Ok(mut current) = self.backend.lock() else {
            tracing::warn!(target: "playback", "failed to lock player while replacing backend");
            return;
        };
        let retired = std::mem::replace(&mut *current, backend);
        drop(current);
        retired.shutdown();
    }

    pub fn warm(&self, path: String, fullscreen: MpvFullscreenBehavior) {
        if let Ok(backend) = self.backend.lock() {
            backend.warm(path, fullscreen);
        }
    }

    pub fn open(&self, path: String, fullscreen: MpvFullscreenBehavior, request: PlaybackRequest) {
        if let Ok(backend) = self.backend.lock() {
            backend.load(path, fullscreen, request);
        }
    }

    pub fn control(&self, command: PlayerCommand) {
        if let Ok(backend) = self.backend.lock() {
            backend.control(command);
        }
    }

    pub fn configure_segments(&self, config: SegmentSkipConfig) {
        if let Ok(backend) = self.backend.lock() {
            backend.set_segment_skip_config(config);
        }
    }

    pub fn update_context(&self, context: PlaybackContext) {
        if let Ok(backend) = self.backend.lock() {
            backend.update_playback_context(context);
        }
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.backend
            .lock()
            .map(|backend| backend.snapshot())
            .unwrap_or_default()
    }

    pub fn capabilities(&self) -> Capabilities {
        self.backend
            .lock()
            .map(|backend| backend.capabilities())
            .unwrap_or_default()
    }

    pub fn shutdown(&self) {
        if let Ok(backend) = self.backend.lock() {
            backend.shutdown();
        }
    }
}
