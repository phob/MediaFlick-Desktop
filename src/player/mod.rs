#[cfg(windows)]
mod mpchc;
pub mod segments;

use std::sync::mpsc::Sender;

use crate::app::settings::{
    AppSettings, MpvFullscreenBehavior, PlayerBackend as PlayerBackendKind, SegmentSkipConfig,
};
use crate::jellyfin::bridge::PlaybackContext;
use crate::mpv::{
    MpvControlCommand, MpvController, MpvLaunch, MpvPlaybackEvent, MpvPlayerSnapshot,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub chapter_markers: bool,
    pub external_subtitles: bool,
    pub injected_hotkeys: bool,
    pub absolute_volume: bool,
    pub pushes_position: bool,
}

pub const MPV_CAPABILITIES: Capabilities = Capabilities {
    chapter_markers: true,
    external_subtitles: true,
    injected_hotkeys: true,
    absolute_volume: true,
    pushes_position: true,
};

#[allow(dead_code)]
pub const MPCHC_CAPABILITIES: Capabilities = Capabilities {
    chapter_markers: false,
    external_subtitles: false,
    injected_hotkeys: false,
    absolute_volume: false,
    pushes_position: false,
};

pub trait PlayerBackend: Send {
    fn warm(&self, path: String, fullscreen: MpvFullscreenBehavior);
    fn load(&self, path: String, fullscreen: MpvFullscreenBehavior, launch: MpvLaunch);
    fn control(&self, command: MpvControlCommand);
    fn set_segment_skip_config(&self, config: SegmentSkipConfig);
    fn update_playback_context(&self, context: PlaybackContext);
    fn snapshot(&self) -> MpvPlayerSnapshot;
    fn capabilities(&self) -> Capabilities;
    fn shutdown(&self);
}

pub fn build_backend(
    settings: &AppSettings,
    event_tx: Sender<MpvPlaybackEvent>,
) -> Box<dyn PlayerBackend> {
    match settings.effective_backend() {
        PlayerBackendKind::Mpv => Box::new(MpvController::new(
            Some(event_tx),
            settings.segment_skip_config(),
        )),
        PlayerBackendKind::Mpchc => {
            #[cfg(windows)]
            {
                Box::new(mpchc::MpcHcController::new(
                    Some(event_tx),
                    settings.segment_skip_config(),
                ))
            }
            #[cfg(not(windows))]
            {
                Box::new(MpvController::new(
                    Some(event_tx),
                    settings.segment_skip_config(),
                ))
            }
        }
    }
}
