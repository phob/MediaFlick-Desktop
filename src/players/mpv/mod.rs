mod commands;
pub mod controller;
pub mod external;
pub mod input;
pub mod ipc;
mod runtime;

pub use controller::MpvController;
pub use external::ExternalMpv;

use crate::playback::{
    Capabilities, MPV_CAPABILITIES, PlaybackContext, PlaybackRequest, PlayerBackend, PlayerCommand,
    PlayerSnapshot,
};
use crate::preferences::{FullscreenBehavior, SegmentSkipConfig};

impl PlayerBackend for MpvController {
    fn warm(&self, path: String, fullscreen: FullscreenBehavior) {
        self.warm(path, fullscreen);
    }

    fn load(&self, path: String, fullscreen: FullscreenBehavior, launch: PlaybackRequest) {
        self.load(path, fullscreen, launch);
    }

    fn control(&self, command: PlayerCommand) {
        self.control(command);
    }

    fn refresh_input_bindings(&self) {
        self.refresh_input_bindings();
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
        MPV_CAPABILITIES
    }

    fn shutdown(&self) {
        self.shutdown();
    }
}
