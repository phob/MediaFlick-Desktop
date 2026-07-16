pub mod controller;
pub mod external;
pub mod input;
pub mod ipc;

pub use controller::MpvController;
pub use external::ExternalMpv;

use crate::playback::{
    PlaybackRequest as MpvLaunch, PlayerCommand as MpvControlCommand,
    PlayerSnapshot as MpvPlayerSnapshot,
};

use crate::playback::{Capabilities, MPV_CAPABILITIES, PlaybackContext, PlayerBackend};
use crate::preferences::{MpvFullscreenBehavior, SegmentSkipConfig};

impl PlayerBackend for MpvController {
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
        MPV_CAPABILITIES
    }

    fn shutdown(&self) {
        self.shutdown();
    }
}
