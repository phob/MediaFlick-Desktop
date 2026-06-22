mod controller;
mod protocol;
mod transport;

pub use controller::MpcHcController;

use crate::app::settings::{MpvFullscreenBehavior, SegmentSkipConfig};
use crate::jellyfin::bridge::PlaybackContext;
use crate::mpv::{MpvControlCommand, MpvLaunch, MpvPlayerSnapshot};
use crate::player::{Capabilities, MPCHC_CAPABILITIES, PlayerBackend};

impl PlayerBackend for MpcHcController {
    fn warm(&self, path: String, fullscreen: MpvFullscreenBehavior) {
        self.warm(path, fullscreen);
    }

    fn load(&self, path: String, fullscreen: MpvFullscreenBehavior, launch: MpvLaunch) {
        self.load(path, fullscreen, launch);
    }

    fn control(&self, command: MpvControlCommand) {
        self.control(command);
    }

    fn set_segment_skip_config(&self, config: SegmentSkipConfig) {
        self.set_segment_skip_config(config);
    }

    fn update_playback_context(&self, context: PlaybackContext) {
        self.update_playback_context(context);
    }

    fn snapshot(&self) -> MpvPlayerSnapshot {
        self.snapshot()
    }

    fn capabilities(&self) -> Capabilities {
        MPCHC_CAPABILITIES
    }

    fn shutdown(&self) {
        self.shutdown();
    }
}
