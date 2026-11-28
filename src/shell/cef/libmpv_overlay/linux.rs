mod display;
mod dmabuf_copier;
mod dmabuf_gl;
mod input;
mod scale;
mod wayland_surface;
mod window;
mod x11;

use x11::Geometry;

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cef::{
    Browser, BrowserSettings, CefString, Client, CursorType, RenderHandler, WindowInfo,
    browser_host_create_browser_sync,
};

use crate::playback::{NativeWindowHandle, PlaybackCoordinator};
use crate::players::mpv::WaylandUiHandle;
use crate::preferences::{AppSettings, WebUiWindowSettings};
use crate::shell::cef::{BrowserState, app_scheme};

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) fn reveal() {
    if crate::players::mpv::use_native_wayland() {
        wayland_surface::reveal();
    } else {
        x11::reveal();
    }
}

pub(crate) fn is_configured(settings: &AppSettings) -> bool {
    settings.effective_backend() == crate::preferences::PlayerBackend::Libmpv
}

pub(crate) fn is_active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

pub(super) fn create_windowless_browser(
    client: &mut Client,
    parent: u64,
    shared_texture_enabled: bool,
) -> Option<Browser> {
    let mut info = WindowInfo::default().set_as_windowless(parent);
    info.shared_texture_enabled = i32::from(shared_texture_enabled);
    let settings = BrowserSettings {
        background_color: 0,
        windowless_frame_rate: 60,
        ..BrowserSettings::default()
    };
    browser_host_create_browser_sync(
        Some(&info),
        Some(client),
        Some(&CefString::from(app_scheme::APP_URL)),
        Some(&settings),
        None,
        None,
    )
}

pub(super) fn save_window_bounds(settings: WebUiWindowSettings) {
    if let Some(services) = crate::shell::locator::services()
        && let Err(error) = services.preferences.record_window(settings)
    {
        tracing::warn!(target: "config", "failed to save Linux window bounds: {error}");
    }
}

enum SurfaceBackend {
    Wayland(Rc<wayland_surface::WaylandSurface>),
    X11(Rc<x11::X11Surface>),
}

pub(crate) struct LibmpvOverlaySurface(SurfaceBackend);

impl LibmpvOverlaySurface {
    pub(in crate::shell::cef) fn select(
        settings: &AppSettings,
        playback: Arc<PlaybackCoordinator>,
        state: BrowserState,
    ) -> Option<Rc<Self>> {
        if !is_configured(settings) {
            return None;
        }
        let backend = if crate::players::mpv::use_native_wayland() {
            SurfaceBackend::Wayland(wayland_surface::WaylandSurface::new(
                settings, playback, state,
            ))
        } else {
            SurfaceBackend::X11(x11::X11Surface::new(settings, playback, state))
        };
        Some(Rc::new(Self(backend)))
    }

    pub(crate) fn bind(&self, parent: NativeWindowHandle) -> Result<(), String> {
        match &self.0 {
            SurfaceBackend::Wayland(_) => {
                Err("Wayland overlay surface cannot bind an X11 window handle".to_string())
            }
            SurfaceBackend::X11(surface) => surface.bind(parent),
        }
    }

    pub(crate) fn bind_wayland(&self, ui: WaylandUiHandle) -> Result<(), String> {
        match &self.0 {
            SurfaceBackend::Wayland(surface) => surface.bind(ui),
            SurfaceBackend::X11(_) => {
                Err("X11 overlay surface cannot bind a Wayland UI handle".to_string())
            }
        }
    }

    pub(crate) fn render_handler(&self) -> Option<RenderHandler> {
        match &self.0 {
            SurfaceBackend::Wayland(surface) => surface.render_handler(),
            SurfaceBackend::X11(surface) => surface.render_handler(),
        }
    }

    pub(crate) fn create_browser(&self, client: &mut Client) -> Option<Browser> {
        match &self.0 {
            SurfaceBackend::Wayland(surface) => surface.create_browser(client),
            SurfaceBackend::X11(surface) => surface.create_browser(client),
        }
    }

    pub(crate) fn show(&self) {
        match &self.0 {
            SurfaceBackend::Wayland(surface) => surface.show(),
            SurfaceBackend::X11(surface) => surface.show(),
        }
    }

    pub(crate) fn set_title(&self, title: &str) {
        match &self.0 {
            SurfaceBackend::Wayland(surface) => surface.set_title(title),
            SurfaceBackend::X11(surface) => surface.set_title(title),
        }
    }

    pub(crate) fn destroy(&self) {
        match &self.0 {
            SurfaceBackend::Wayland(surface) => surface.destroy(),
            SurfaceBackend::X11(surface) => surface.destroy(),
        }
    }

    pub(crate) fn set_cursor(&self, cursor: CursorType) {
        match &self.0 {
            SurfaceBackend::Wayland(surface) => surface.set_cursor(cursor),
            SurfaceBackend::X11(surface) => surface.set_cursor(cursor),
        }
    }
}

#[cfg(test)]
mod tests;
