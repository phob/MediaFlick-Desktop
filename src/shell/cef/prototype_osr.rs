#[cfg(not(any(target_os = "windows", target_os = "linux")))]
use std::rc::Rc;
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
use std::sync::Arc;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
use cef::{Browser, Client, CursorType, RenderHandler};

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
use crate::playback::{NativeWindowHandle, PlaybackCoordinator};
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
use crate::preferences::AppSettings;

#[cfg(target_os = "windows")]
#[path = "prototype_osr/windows.rs"]
mod platform;

#[cfg(target_os = "linux")]
#[path = "prototype_osr/linux.rs"]
mod platform;

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(super) use platform::{PrototypeOsrSurface, is_active, is_configured};

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) struct PrototypeOsrSurface;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl PrototypeOsrSurface {
    pub(super) fn select(
        _settings: &AppSettings,
        _playback: Arc<PlaybackCoordinator>,
    ) -> Option<Rc<Self>> {
        None
    }

    pub(super) fn render_handler(&self) -> Option<RenderHandler> {
        None
    }

    pub(super) fn set_cursor(&self, _cursor: CursorType) {}

    pub(super) fn bind(&self, _parent: NativeWindowHandle) -> Result<(), String> {
        Err("the integrated libmpv overlay is not compiled for this build".to_string())
    }

    pub(super) fn create_browser(&self, _client: &mut Client) -> Option<Browser> {
        None
    }

    pub(super) fn show(&self) {}

    pub(super) fn destroy(&self) {}
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) fn is_configured(_settings: &AppSettings) -> bool {
    false
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) fn is_active() -> bool {
    false
}

#[cfg(target_os = "linux")]
pub(super) use platform::reveal;
