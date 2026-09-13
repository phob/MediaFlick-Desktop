//! Backend-neutral playback domain.
//!
//! This module owns the contracts and policies used by every player backend.
//! Concrete mpv and MPC-HC protocol implementations live under `players`.

pub mod coordinator;
pub mod model;
pub mod segments;

pub use coordinator::PlaybackCoordinator;
pub use model::{
    HttpHeader, PlaybackContext, PlaybackDiagnostics, PlaybackEvent, PlaybackRequest,
    PlayerChapter, PlayerCommand, PlayerSnapshot, PlayerTrack, PlayerTrackKind, ReportingState,
    TICKS_PER_SECOND, ToneMapping, VideoAspect, VideoFit, seconds_to_ticks,
};

use crate::preferences::{FullscreenBehavior, SegmentSkipConfig};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub chapter_markers: bool,
    pub external_subtitles: bool,
    pub injected_hotkeys: bool,
    pub absolute_volume: bool,
    pub pushes_position: bool,
    pub fullscreen: bool,
    pub playback_tuning: bool,
}

/// Opaque native window identity owned by a playback backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeWindowHandle {
    raw: usize,
    #[cfg(target_os = "linux")]
    content: usize,
}

/// An owned, premultiplied BGRA browser frame for native video composition.
#[cfg(target_os = "linux")]
pub struct NativeOverlayFrame {
    pub pixels: Vec<u8>,
    pub width: u16,
    pub height: u16,
}

#[cfg(target_os = "linux")]
impl NativeOverlayFrame {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=16384).contains(&self.width)
            || !(1..=16384).contains(&self.height)
            || self.pixels.len() != usize::from(self.width) * usize::from(self.height) * 4
        {
            return Err("invalid native UI frame dimensions".into());
        }
        Ok(())
    }
    pub fn exposes_video(&self) -> bool {
        self.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] == 0)
    }
}

#[cfg(target_os = "linux")]
pub type NativeOverlayReply = std::sync::mpsc::Receiver<Result<NativeOverlayFrame, String>>;

impl NativeWindowHandle {
    #[cfg(target_os = "windows")]
    pub fn new(raw: usize) -> Option<Self> {
        (raw != 0).then_some(Self {
            raw,
            #[cfg(target_os = "linux")]
            content: raw,
        })
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub fn raw(self) -> usize {
        self.raw
    }

    #[cfg(target_os = "linux")]
    pub fn with_content(raw: usize, content: usize) -> Option<Self> {
        (raw != 0 && content != 0).then_some(Self { raw, content })
    }

    #[cfg(target_os = "linux")]
    pub fn content(self) -> usize {
        self.content
    }
}

pub const MPV_CAPABILITIES: Capabilities = Capabilities {
    chapter_markers: true,
    external_subtitles: true,
    injected_hotkeys: true,
    absolute_volume: true,
    pushes_position: true,
    fullscreen: true,
    playback_tuning: true,
};

#[allow(dead_code)]
pub const MPCHC_CAPABILITIES: Capabilities = Capabilities {
    chapter_markers: false,
    external_subtitles: true,
    injected_hotkeys: false,
    absolute_volume: true,
    pushes_position: false,
    fullscreen: true,
    playback_tuning: false,
};

/// Port implemented by each player adapter.
pub trait PlayerBackend: Send {
    fn warm(&self, path: String, fullscreen: FullscreenBehavior);
    /// Return the native window owned by this backend, if it exposes one.
    fn native_window(&self, timeout: Duration) -> Option<NativeWindowHandle>;
    #[cfg(target_os = "linux")]
    fn submit_overlay(&self, _frame: NativeOverlayFrame) -> Result<NativeOverlayReply, String> {
        Err("this player does not support native UI composition".into())
    }
    fn load(&self, path: String, fullscreen: FullscreenBehavior, request: PlaybackRequest);
    fn control(&self, command: PlayerCommand);
    /// Re-read backend-specific hotkeys without restarting a running player.
    fn refresh_input_bindings(&self);
    fn set_segment_skip_config(&self, config: SegmentSkipConfig);
    fn update_playback_context(&self, context: PlaybackContext);
    fn snapshot(&self) -> PlayerSnapshot;
    fn capabilities(&self) -> Capabilities;
    fn shutdown(&self);
}

#[cfg(all(test, target_os = "linux"))]
mod native_frame_tests {
    use super::NativeOverlayFrame;

    #[test]
    fn native_frame_rejects_invalid_dimensions_and_truncated_buffers() {
        for (width, height, length) in [(0, 1, 0), (1, 0, 0), (2, 2, 15), (16385, 1, 0)] {
            let frame = NativeOverlayFrame {
                width,
                height,
                pixels: vec![0; length],
            };
            assert!(frame.validate().is_err());
        }
    }

    #[test]
    fn fullscreen_gate_distinguishes_catalog_from_transparent_playback() {
        let mut frame = NativeOverlayFrame {
            width: 2,
            height: 1,
            pixels: vec![255; 8],
        };
        assert!(frame.validate().is_ok());
        assert!(!frame.exposes_video());
        frame.pixels[4..8].fill(0);
        assert!(frame.exposes_video());
    }
}
