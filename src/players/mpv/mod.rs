mod commands;
pub mod controller;
pub mod external;
pub mod input;
pub mod ipc;
mod runtime;
#[cfg(target_os = "windows")]
mod svp;

pub use controller::MpvController;
pub use external::ExternalMpv;

use crate::playback::{
    NativeWindowHandle, PlaybackContext, PlaybackRequest, PlayerBackend, PlayerCommand,
    PlayerSnapshot,
};
use crate::preferences::{FullscreenBehavior, PlayerPreferences};
use std::time::Duration;

impl PlayerBackend for MpvController {
    fn warm(&self, path: String, fullscreen: FullscreenBehavior) {
        self.warm(path, fullscreen);
    }

    fn native_window(&self, timeout: Duration) -> Option<NativeWindowHandle> {
        self.native_window(timeout)
    }

    #[cfg(target_os = "linux")]
    fn submit_overlay(
        &self,
        frame: crate::playback::NativeOverlayFrame,
    ) -> Result<crate::playback::NativeOverlayReply, String> {
        self.submit_overlay(frame)
    }

    fn load(&self, path: String, fullscreen: FullscreenBehavior, launch: PlaybackRequest) {
        self.load(path, fullscreen, launch);
    }

    fn control(&self, command: PlayerCommand) {
        self.control(command);
    }

    fn set_preferences(&self, preferences: PlayerPreferences) {
        self.set_preferences(preferences);
    }

    fn update_playback_context(&self, context: PlaybackContext) {
        self.update_playback_context(context);
    }

    fn snapshot(&self) -> PlayerSnapshot {
        self.snapshot()
    }

    fn shutdown(&self) {
        self.shutdown();
    }
}

#[cfg(target_os = "linux")]
pub(crate) use runtime::take_window_close_request;
