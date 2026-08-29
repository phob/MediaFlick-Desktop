mod controller;
mod protocol;
mod request;
mod transport;

pub use controller::MpcHcController;

use crate::playback::{
    Capabilities, MPCHC_CAPABILITIES, NativeWindowHandle, PlaybackContext, PlaybackRequest,
    PlayerBackend, PlayerCommand, PlayerSnapshot,
};
use crate::preferences::{FullscreenBehavior, SegmentSkipConfig};
use std::time::Duration;

impl PlayerBackend for MpcHcController {
    fn warm(&self, path: String, fullscreen: FullscreenBehavior) {
        self.warm(path, fullscreen);
    }

    fn native_window(&self, _timeout: Duration) -> Option<NativeWindowHandle> {
        None
    }

    fn load(&self, path: String, fullscreen: FullscreenBehavior, launch: PlaybackRequest) {
        self.load(path, fullscreen, launch);
    }

    fn control(&self, command: PlayerCommand) {
        self.control(command);
    }

    fn refresh_input_bindings(&self) {
        // MPC-HC does not install MediaFlick's mpv input section.
    }

    fn set_segment_skip_config(&self, config: SegmentSkipConfig) {
        self.set_segment_skip_config(config);
    }

    fn update_playback_context(&self, context: PlaybackContext) {
        self.update_playback_context(context);
    }

    fn snapshot(&self) -> PlayerSnapshot {
        self.snapshot()
    }

    fn capabilities(&self) -> Capabilities {
        MPCHC_CAPABILITIES
    }

    fn shutdown(&self) {
        self.shutdown();
    }
}
