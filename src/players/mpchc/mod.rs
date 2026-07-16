mod controller;
mod protocol;
mod transport;

pub use controller::MpcHcController;

use crate::playback::{
    Capabilities, MPCHC_CAPABILITIES, PlaybackContext, PlaybackRequest, PlayerBackend,
    PlayerCommand, PlayerSnapshot,
};
use crate::preferences::{MpvFullscreenBehavior, SegmentSkipConfig};

impl PlayerBackend for MpcHcController {
    fn warm(&self, path: String, fullscreen: MpvFullscreenBehavior) {
        self.warm(path, fullscreen);
    }

    fn load(&self, path: String, fullscreen: MpvFullscreenBehavior, launch: PlaybackRequest) {
        self.load(path, fullscreen, launch);
    }

    fn control(&self, command: PlayerCommand) {
        self.control(command);
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
