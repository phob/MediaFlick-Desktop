//! Backend-neutral playback domain.
//!
//! This module owns the contracts and policies used by every external player.
//! Concrete mpv and MPC-HC protocol implementations live under `players`.

pub mod coordinator;
pub mod model;
pub mod segments;

pub use coordinator::PlaybackCoordinator;
pub use model::{
    HttpHeader, PlaybackContext, PlaybackEvent, PlaybackRequest, PlayerCommand, PlayerSnapshot,
    ReportingState, TICKS_PER_SECOND, seconds_to_ticks,
};

use crate::preferences::{FullscreenBehavior, SegmentSkipConfig};

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
    external_subtitles: true,
    injected_hotkeys: false,
    absolute_volume: true,
    pushes_position: false,
};

/// Port implemented by each external player adapter.
pub trait PlayerBackend: Send {
    fn warm(&self, path: String, fullscreen: FullscreenBehavior);
    fn load(&self, path: String, fullscreen: FullscreenBehavior, request: PlaybackRequest);
    fn control(&self, command: PlayerCommand);
    fn set_segment_skip_config(&self, config: SegmentSkipConfig);
    fn update_playback_context(&self, context: PlaybackContext);
    fn snapshot(&self) -> PlayerSnapshot;
    fn capabilities(&self) -> Capabilities;
    fn shutdown(&self);
}
