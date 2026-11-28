pub mod coordinator;
pub mod inhibit;
pub mod model;
pub mod os_media;
mod queue;
pub mod segments;

pub use coordinator::{AudioPlan, PlaybackCoordinator};
pub use model::{
    ArtworkRef, AudioDisplay, AudioKind, BackendEvent, HttpHeader, NowPlaying, PlaybackContent,
    PlaybackDiagnostics, PlaybackEvent, PlaybackRequest, PlayerChapter, PlayerCommand,
    PlayerSnapshot, PlayerTrack, PlayerTrackKind, QueueReport, ReportingState, StopReason,
    TICKS_PER_MILLISECOND, TICKS_PER_SECOND, TOKEN_QUERY_KEYS, ToneMapping, VideoAspect, VideoFit,
    non_empty, query_auth_token, seconds_to_ticks, ticks_to_milliseconds,
};
pub use queue::{AudioTrack, MAX_QUEUE_LENGTH};

use crate::preferences::{FullscreenBehavior, PlayerPreferences};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeWindowHandle {
    raw: usize,
    #[cfg(target_os = "linux")]
    content: usize,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
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

#[cfg(target_os = "linux")]
pub enum OverlayPresentation {
    Presented(NativeOverlayFrame),
    TimedOut(NativeOverlayReply),
}

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

    #[cfg(test)]
    pub fn test_handle(raw: usize) -> Self {
        Self {
            raw,
            #[cfg(target_os = "linux")]
            content: raw + 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioStart {
    Default,
    Beginning,
    At(i64),
}

pub struct PreparedAudio {
    pub player_path: String,
    pub request: PlaybackRequest,
}

pub trait PlaybackOwner: Send + Sync {
    fn is_current(&self) -> bool;
}

pub trait AudioSource: PlaybackOwner {
    fn prepare(&self, track: &AudioTrack, start: AudioStart) -> Result<PreparedAudio, String>;
    fn dispatch_if_current(&self, dispatch: &mut dyn FnMut()) -> bool;
}

pub trait PlayerBackend: Send + Sync {
    fn warm(&self, path: String, fullscreen: FullscreenBehavior);
    fn native_window(&self, timeout: Duration) -> Option<NativeWindowHandle>;
    #[cfg(target_os = "linux")]
    fn wayland_ui(&self, _timeout: Duration) -> Option<crate::players::mpv::WaylandUiHandle> {
        None
    }
    #[cfg(target_os = "linux")]
    fn submit_overlay(&self, _frame: NativeOverlayFrame) -> Result<NativeOverlayReply, String> {
        Err("this player does not support native UI composition".into())
    }
    fn load(&self, path: String, fullscreen: FullscreenBehavior, request: PlaybackRequest);
    fn control(&self, command: PlayerCommand);
    fn stop_and_wait(&self, timeout: Duration) -> bool;
    fn set_preferences(&self, preferences: PlayerPreferences);
    fn snapshot(&self) -> PlayerSnapshot;
    fn release_external_runtime(&self) {}
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
}
