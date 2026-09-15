#[path = "linux/display.rs"]
mod display;
#[path = "linux/input.rs"]
mod input;
#[path = "linux/scale.rs"]
mod scale;
#[path = "linux/window.rs"]
mod window;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cef::*;
use x11rb::connection::Connection;
use x11rb::protocol::{Event, xproto::*};

use crate::playback::{NativeWindowHandle, PlaybackCoordinator, PlayerCommand};
use crate::preferences::{AppSettings, PlayerBackend, WebUiWindowSettings};
use crate::shell::cef::app_scheme;
use scale::Scale;
use window::OverlayWindow;

static ACTIVE: AtomicBool = AtomicBool::new(false);
const SYNC_MS: i64 = 16;
thread_local! {
    static SURFACE: RefCell<Weak<PrototypeOsrSurface>> = const { RefCell::new(Weak::new()) };
}

pub(crate) fn reveal() {
    let surface = SURFACE.with(|slot| slot.borrow().upgrade());
    if let Some(surface) = surface {
        surface.reveal();
    }
}

pub(crate) fn is_configured(settings: &AppSettings) -> bool {
    settings.effective_backend() == PlayerBackend::Libmpv
}

pub(crate) fn is_active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Geometry {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    visible: bool,
    fullscreen: bool,
    maximized: bool,
}

pub(crate) struct PrototypeOsrSurface {
    playback: Arc<PlaybackCoordinator>,
    window: RefCell<Option<OverlayWindow>>,
    browser: RefCell<Option<Browser>>,
    geometry: Cell<Geometry>,
    scale: Cell<Scale>,
    outputs: RefCell<Vec<display::Output>>,
    closing: Cell<bool>,
    frame: RefCell<Vec<u8>>,
    frame_size: Cell<(u16, u16)>,
    popup: RefCell<Vec<u8>>,
    popup_rect: RefCell<Rect>,
    popup_visible: Cell<bool>,
    painted: Cell<bool>,
    window_settings: Cell<WebUiWindowSettings>,
    pressed: RefCell<HashSet<u8>>,
    last_click: Cell<(u8, u32, i16, i16, i32)>,
    placement_invalidated: Cell<bool>,
}

impl PrototypeOsrSurface {
    pub(crate) fn select(
        settings: &AppSettings,
        playback: Arc<PlaybackCoordinator>,
    ) -> Option<Rc<Self>> {
        if !is_configured(settings) {
            return None;
        }
        let (width, height) = settings.webui_window.size();
        Some(Rc::new(Self {
            playback,
            window: RefCell::new(None),
            browser: RefCell::new(None),
            geometry: Cell::new(Geometry {
                width: width.clamp(1, 16384) as u16,
                height: height.clamp(1, 16384) as u16,
                ..Geometry::default()
            }),
            closing: Cell::new(false),
            scale: Cell::new(Scale::default()),
            outputs: RefCell::new(Vec::new()),
            frame: RefCell::new(Vec::new()),
            frame_size: Cell::new((0, 0)),
            popup: RefCell::new(Vec::new()),
            popup_rect: RefCell::new(Rect::default()),
            popup_visible: Cell::new(false),
            painted: Cell::new(false),
            window_settings: Cell::new(settings.webui_window),
            pressed: RefCell::new(HashSet::new()),
            last_click: Cell::new((0, 0, 0, 0, 0)),
            placement_invalidated: Cell::new(true),
        }))
    }

    pub(crate) fn bind(&self, parent: NativeWindowHandle) -> Result<(), String> {
        let content = u32::try_from(parent.content()).map_err(|e| e.to_string())?;
        let parent = u32::try_from(parent.raw()).map_err(|e| e.to_string())?;
        let window = OverlayWindow::new(parent, content).map_err(|e| e.to_string())?;
        let geometry = self.geometry.get();
        window
            .conn
            .unmap_window(parent)
            .map_err(|e| e.to_string())?;
        let position = self.window_settings.get().position().unwrap_or((0, 0));
        window
            .conn
            .configure_window(
                parent,
                &ConfigureWindowAux::new()
                    .x(position.0)
                    .y(position.1)
                    .width(u32::from(geometry.width))
                    .height(u32::from(geometry.height)),
            )
            .map_err(|e| e.to_string())?;
        window
            .restore(self.window_settings.get())
            .map_err(|e| e.to_string())?;
        window
            .resize_content(geometry.width, geometry.height)
            .map_err(|e| e.to_string())?;
        window.set_icon();
        *self.window.borrow_mut() = Some(window);
        self.refresh_scale();
        let _ = crate::players::mpv::take_window_close_request();
        ACTIVE.store(true, Ordering::Release);
        Ok(())
    }

    pub(crate) fn render_handler(self: &Rc<Self>) -> Option<RenderHandler> {
        Some(LinuxRenderHandler::new(Rc::clone(self)))
    }

    pub(crate) fn create_browser(self: &Rc<Self>, client: &mut Client) -> Option<Browser> {
        self.sync_geometry().ok()?;
        let parent = self.window.borrow().as_ref()?.parent;
        let info = WindowInfo::default().set_as_windowless(u64::from(parent));
        let settings = BrowserSettings {
            background_color: 0,
            windowless_frame_rate: 60,
            ..BrowserSettings::default()
        };
        let browser = browser_host_create_browser_sync(
            Some(&info),
            Some(client),
            Some(&CefString::from(app_scheme::APP_URL)),
            Some(&settings),
            None,
            None,
        )?;
        *self.browser.borrow_mut() = Some(browser.clone());
        SURFACE.with(|slot| *slot.borrow_mut() = Rc::downgrade(self));
        self.schedule();
        Some(browser)
    }

    pub(crate) fn show(&self) {
        let _ = self.sync_geometry();
        if let Some(host) = self.browser_host() {
            host.set_focus(1);
        }
    }

    fn reveal(&self) {
        if let Some(window) = self.window.borrow().as_ref() {
            let _ = window.conn.map_window(window.parent);
            let _ = window.activate();
        }
        let _ = self.sync_geometry();
        if let Some(window) = self.window.borrow().as_ref() {
            let _ =
                window
                    .conn
                    .set_input_focus(InputFocus::PARENT, window.window, x11rb::CURRENT_TIME);
            let _ = window.conn.flush();
        }
        if let Some(host) = self.browser_host() {
            host.set_focus(1);
        }
    }

    pub(crate) fn set_title(&self, title: &str) {
        if let Some(window) = self.window.borrow().as_ref() {
            let _ = window.set_title(title);
        }
    }

    pub(crate) fn destroy(&self) {
        self.closing.set(true);
        self.browser.borrow_mut().take();
        self.window.borrow_mut().take();
        if let Some(services) = crate::app::services::services()
            && let Err(error) = services
                .preferences
                .record_window(self.window_settings.get())
        {
            tracing::warn!(target: "config", "failed to save Linux window bounds: {error}");
        }
        ACTIVE.store(false, Ordering::Release);
    }

    pub(crate) fn set_cursor(&self, cursor: CursorType) {
        let glyph = if cursor == CursorType::IBEAM {
            152
        } else if cursor == CursorType::HAND {
            60
        } else if cursor == CursorType::CROSS {
            34
        } else {
            68
        };
        if let Some(window) = self.window.borrow().as_ref() {
            let _ = window.cursor((cursor != CursorType::NONE).then_some(glyph));
        }
    }

    fn browser_host(&self) -> Option<BrowserHost> {
        self.browser.borrow().as_ref()?.host()
    }

    fn refresh_scale(&self) {
        *self.outputs.borrow_mut() = display::outputs();
        let next = self
            .window
            .borrow()
            .as_ref()
            .and_then(|window| Scale::query(&window.conn).ok());
        if let Some(next) = next {
            self.update_scale(self.scale_for_geometry(next, self.geometry.get()));
        }
    }

    fn scale_for_geometry(&self, scale: Scale, geometry: Geometry) -> Scale {
        let x = scale.logical(i32::from(geometry.x) + i32::from(geometry.width) / 2);
        let y = scale.logical(i32::from(geometry.y) + i32::from(geometry.height) / 2);
        scale.with_native_density(display::density_at(&self.outputs.borrow(), x, y))
    }

    fn update_scale(&self, next: Scale) -> bool {
        if next == self.scale.get() {
            return false;
        }
        self.scale.set(next);
        *self.frame.borrow_mut() = Vec::new();
        self.frame_size.set((0, 0));
        *self.popup.borrow_mut() = Vec::new();
        if let Some(host) = self.browser_host() {
            host.notify_screen_info_changed();
            host.was_resized();
        }
        true
    }

    fn schedule(self: &Rc<Self>) {
        let mut task = LinuxSurfaceTask::new(Rc::clone(self));
        if post_delayed_task(ThreadId::UI, Some(&mut task), SYNC_MS) == 0 {
            tracing::warn!(target: "cef.osr", "failed to schedule the Linux overlay");
        }
    }

    fn close(&self) {
        if !self.closing.replace(true)
            && let Some(host) = self.browser_host()
        {
            host.close_browser(1);
        }
    }

    fn sync_geometry(&self) -> Result<(), String> {
        let (geometry, resize_settled) = {
            let window = self.window.borrow();
            let window = window.as_ref().ok_or("Linux overlay is detached")?;
            let (x, y, width, height, visible) = window.geometry().map_err(|e| e.to_string())?;
            let (fullscreen, maximized) = window.state().map_err(|e| e.to_string())?;
            let next = Geometry {
                x,
                y,
                width,
                height,
                visible,
                fullscreen,
                maximized,
            };
            let misaligned = self.placement_invalidated.replace(false)
                && !window.is_aligned(&next).map_err(|e| e.to_string())?;
            if next != self.geometry.get() || misaligned {
                window
                    .place(width, height, visible)
                    .map_err(|e| e.to_string())?;
            }
            (next, window.resize_settled())
        };
        let old = self.geometry.replace(geometry);
        let scale_changed = self.update_scale(self.scale_for_geometry(self.scale.get(), geometry));
        if geometry.visible && !geometry.fullscreen && resize_settled {
            let mut settings = self.window_settings.get();
            settings.record_bounds(
                i32::from(geometry.x),
                i32::from(geometry.y),
                i32::from(geometry.width),
                i32::from(geometry.height),
                geometry.maximized,
            );
            self.window_settings.set(settings);
        }
        if let Some(host) = self.browser_host() {
            if !scale_changed && (old.width != geometry.width || old.height != geometry.height) {
                // Release the retained old-size frame before CEF allocates the
                // replacement. In particular, don't hold a fullscreen buffer
                // while returning to a small window.
                *self.frame.borrow_mut() = Vec::new();
                self.frame_size.set((0, 0));
                host.was_resized();
            }
            if old.x != geometry.x || old.y != geometry.y {
                host.notify_move_or_resize_started();
            }
            if old.visible != geometry.visible {
                host.was_hidden(i32::from(!geometry.visible));
            }
        }
        Ok(())
    }

    fn tick(&self) -> bool {
        if self.closing.get() {
            return false;
        }
        if crate::players::mpv::take_window_close_request() || self.sync_geometry().is_err() {
            self.close();
            return false;
        }
        // Drain a bounded batch so continuous input cannot starve CEF tasks.
        for _ in 0..256 {
            let event = self
                .window
                .borrow()
                .as_ref()
                .and_then(|w| w.conn.poll_for_event().ok().flatten());
            let Some(event) = event else {
                break;
            };
            self.handle_event(&event);
            if self.closing.get() {
                return false;
            }
        }
        true
    }

    fn handle_event(&self, event: &Event) {
        let Some(host) = self.browser_host() else {
            return;
        };
        match event {
            Event::MotionNotify(e) => host.send_mouse_move_event(
                Some(&MouseEvent {
                    x: self.scale.get().logical(i32::from(e.event_x)),
                    y: self.scale.get().logical(i32::from(e.event_y)),
                    modifiers: input::modifiers(e.state),
                }),
                0,
            ),
            Event::LeaveNotify(e) => host.send_mouse_move_event(
                Some(&MouseEvent {
                    x: self.scale.get().logical(i32::from(e.event_x)),
                    y: self.scale.get().logical(i32::from(e.event_y)),
                    modifiers: input::modifiers(e.state),
                }),
                1,
            ),
            Event::ButtonPress(e) => self.button(&host, e, false),
            Event::ButtonRelease(e) => self.button(&host, e, true),
            Event::KeyPress(e) => self.key(&host, e, false),
            Event::KeyRelease(e) => self.key(&host, e, true),
            Event::FocusIn(_) | Event::FocusOut(_) => self.sync_focus(&host),
            Event::Expose(_) => self.present(),
            Event::ConfigureNotify(e)
                if self
                    .window
                    .borrow()
                    .as_ref()
                    .is_some_and(|w| e.window == w.root) =>
            {
                self.refresh_scale();
            }
            Event::ConfigureNotify(_) | Event::MapNotify(_) => self.placement_invalidated.set(true),
            Event::PropertyNotify(e) if e.atom == u32::from(AtomEnum::RESOURCE_MANAGER) => {
                self.refresh_scale()
            }
            Event::ClientMessage(e) => {
                let close = self
                    .window
                    .borrow()
                    .as_ref()
                    .is_some_and(|w| e.data.as_data32()[0] == w.delete);
                if close {
                    self.close();
                }
            }
            _ => {}
        }
    }

    fn sync_focus(&self, host: &BrowserHost) {
        let focused = self
            .window
            .borrow()
            .as_ref()
            .map(OverlayWindow::focus_browser);
        match focused {
            Some(Ok(focused)) => {
                if !focused {
                    self.pressed.borrow_mut().clear();
                }
                host.set_focus(i32::from(focused));
            }
            Some(Err(error)) => {
                tracing::warn!(target: "cef.osr", "failed to synchronize Linux browser focus: {error}");
            }
            None => {}
        }
    }

    fn button(&self, host: &BrowserHost, e: &ButtonPressEvent, up: bool) {
        let mouse = MouseEvent {
            x: self.scale.get().logical(i32::from(e.event_x)),
            y: self.scale.get().logical(i32::from(e.event_y)),
            modifiers: input::modifiers(e.state),
        };
        if !up {
            if let Some(window) = self.window.borrow().as_ref() {
                let _ = window
                    .conn
                    .set_input_focus(InputFocus::PARENT, window.window, e.time);
                let _ = window.conn.flush();
            }
            host.set_focus(1);
        }
        match e.detail {
            4..=7 if !up => {
                let (x, y) = match e.detail {
                    4 => (0, 120),
                    5 => (0, -120),
                    6 => (120, 0),
                    _ => (-120, 0),
                };
                host.send_mouse_wheel_event(Some(&mouse), x, y);
            }
            1..=3 => {
                if e.detail == 3 && self.playback.snapshot().active {
                    if !up {
                        self.playback
                            .control(PlayerCommand::SetPause(!self.playback.snapshot().paused));
                    }
                    return;
                }
                let button = match e.detail {
                    1 => MouseButtonType::LEFT,
                    2 => MouseButtonType::MIDDLE,
                    _ => MouseButtonType::RIGHT,
                };
                let (previous, time, x, y, count) = self.last_click.get();
                let count = if up {
                    count
                } else if previous == e.detail
                    && e.time.wrapping_sub(time) <= 500
                    && (i32::from(x) - i32::from(e.event_x)).abs() <= self.scale.get().physical(4)
                    && (i32::from(y) - i32::from(e.event_y)).abs() <= self.scale.get().physical(4)
                {
                    count % 3 + 1
                } else {
                    1
                };
                if !up {
                    self.last_click
                        .set((e.detail, e.time, e.event_x, e.event_y, count));
                }
                host.send_mouse_click_event(Some(&mouse), button, i32::from(up), count);
            }
            _ => {}
        }
    }

    fn key(&self, host: &BrowserHost, e: &KeyPressEvent, up: bool) {
        let repeated = if up {
            self.pressed.borrow_mut().remove(&e.detail);
            false
        } else {
            !self.pressed.borrow_mut().insert(e.detail)
        };
        let symbols = self
            .window
            .borrow()
            .as_ref()
            .and_then(|w| input::keysym(&w.conn, e));
        let Some(symbols) = symbols else {
            return;
        };
        if e.state.contains(KeyButMask::MOD1) && symbols.0 == 0xffc1 {
            if !up {
                self.close();
            }
            return;
        }
        input::send_key(host, e, symbols, up, repeated);
    }

    fn paint(
        &self,
        kind: PaintElementType,
        dirty: &[Rect],
        buffer: *const u8,
        width: i32,
        height: i32,
    ) {
        if buffer.is_null() || !(1..=16384).contains(&width) || !(1..=16384).contains(&height) {
            return;
        }
        if !self.painted.replace(true) {
            tracing::info!(target: "cef.osr", width, height, "received first Linux CEF frame");
        }
        let length = width as usize * height as usize * 4;
        // SAFETY: CEF provides width * height premultiplied BGRA pixels valid
        // until OnPaint returns. Retain a copy, never its borrowed pointer.
        let pixels = unsafe { std::slice::from_raw_parts(buffer, length) };
        if kind == PaintElementType::VIEW {
            let geometry = self.geometry.get();
            let scale = self.scale.get();
            let size = (
                scale.raster_extent(geometry.width),
                scale.raster_extent(geometry.height),
            );
            if i32::from(size.0) != width || i32::from(size.1) != height {
                return;
            }
            self.frame_size.set(size);
            let mut frame = self.frame.borrow_mut();
            let resized = frame.len() != length;
            if resized {
                frame.clear();
                frame.extend_from_slice(pixels);
            } else {
                update_regions(&mut frame, pixels, size.0, size.1, dirty);
            }
            drop(frame);
            if !self.popup_visible.get() && !resized {
                self.present_regions(dirty);
                return;
            }
        } else if kind == PaintElementType::POPUP {
            let rect = self.scale.get().raster_rect(&self.popup_rect.borrow());
            if rect.width != width || rect.height != height {
                return;
            }
            self.popup.borrow_mut().clear();
            self.popup.borrow_mut().extend_from_slice(pixels);
        }
        self.present();
    }

    fn present(&self) {
        let (width, height) = self.frame_size.get();
        self.present_regions(&[Rect {
            x: 0,
            y: 0,
            width: i32::from(width),
            height: i32::from(height),
        }]);
    }

    fn present_regions(&self, _dirty: &[Rect]) {
        let (width, height) = self.frame_size.get();
        let frame = std::mem::take(&mut *self.frame.borrow_mut());
        if width == 0 || height == 0 || frame.len() != usize::from(width) * usize::from(height) * 4
        {
            *self.frame.borrow_mut() = frame;
            return;
        }
        let popup_visible = self.popup_visible.get();
        let pixels = if popup_visible {
            let mut composed = frame.clone();
            blend_popup(
                &mut composed,
                width,
                height,
                &self.popup.borrow(),
                &self.scale.get().raster_rect(&self.popup_rect.borrow()),
            );
            *self.frame.borrow_mut() = frame;
            composed
        } else {
            frame
        };
        match self
            .playback
            .present_overlay(crate::playback::NativeOverlayFrame {
                pixels,
                width,
                height,
            }) {
            Ok(frame) if !popup_visible => *self.frame.borrow_mut() = frame.pixels,
            Ok(_) => {}
            Err(error) => {
                tracing::error!(target: "cef.osr", "native UI composition failed: {error}");
                self.close();
            }
        }
    }
}

fn clipped_rect(rect: &Rect, width: u16, height: u16) -> Option<Rect> {
    let x = rect.x.max(0);
    let y = rect.y.max(0);
    let right = rect.x.saturating_add(rect.width).min(i32::from(width));
    let bottom = rect.y.saturating_add(rect.height).min(i32::from(height));
    (right > x && bottom > y).then_some(Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

fn update_regions(frame: &mut [u8], pixels: &[u8], width: u16, height: u16, dirty: &[Rect]) {
    for rect in dirty.iter().filter_map(|r| clipped_rect(r, width, height)) {
        for y in rect.y..rect.y + rect.height {
            let offset = (y as usize * usize::from(width) + rect.x as usize) * 4;
            let end = offset + rect.width as usize * 4;
            frame[offset..end].copy_from_slice(&pixels[offset..end]);
        }
    }
}

fn blend_popup(frame: &mut [u8], width: u16, height: u16, popup: &[u8], rect: &Rect) {
    if rect.width <= 0
        || rect.height <= 0
        || popup.len() != rect.width as usize * rect.height as usize * 4
    {
        return;
    }
    for y in rect.y.max(0)..(rect.y.saturating_add(rect.height)).min(i32::from(height)) {
        for x in rect.x.max(0)..(rect.x.saturating_add(rect.width)).min(i32::from(width)) {
            let src = ((y - rect.y) as usize * rect.width as usize + (x - rect.x) as usize) * 4;
            let dst = (y as usize * usize::from(width) + x as usize) * 4;
            let inverse_alpha = 255 - u16::from(popup[src + 3]);
            for channel in 0..4 {
                frame[dst + channel] = (u16::from(popup[src + channel])
                    + (u16::from(frame[dst + channel]) * inverse_alpha + 127) / 255)
                    .min(255) as u8;
            }
        }
    }
}

wrap_task! {
    struct LinuxSurfaceTask { surface: Rc<PrototypeOsrSurface> }
    impl Task { fn execute(&self) { if self.surface.tick() { self.surface.schedule(); } } }
}

wrap_render_handler! {
    struct LinuxRenderHandler { surface: Rc<PrototypeOsrSurface> }
    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect { let g = self.surface.geometry.get();
                let scale = self.surface.scale.get();
                *rect = Rect { x: 0, y: 0, width: scale.logical_extent(g.width), height: scale.logical_extent(g.height) }; }
        }
        fn screen_info(&self, _browser: Option<&mut Browser>, info: Option<&mut ScreenInfo>) -> i32 {
            let Some(info) = info else { return 0; };
            let g = self.surface.geometry.get();
            let scale = self.surface.scale.get();
            info.device_scale_factor = scale.factor();
            info.depth = 32;
            info.depth_per_component = 8;
            info.rect = Rect { x: scale.logical(i32::from(g.x)), y: scale.logical(i32::from(g.y)), width: scale.logical_extent(g.width), height: scale.logical_extent(g.height) };
            info.available_rect = info.rect.clone();
            1
        }
        fn screen_point(&self, _browser: Option<&mut Browser>, view_x: i32, view_y: i32,
            screen_x: Option<&mut i32>, screen_y: Option<&mut i32>) -> i32 {
            let (Some(x), Some(y)) = (screen_x, screen_y) else { return 0; };
            let g = self.surface.geometry.get(); let scale = self.surface.scale.get();
            *x = i32::from(g.x) + scale.physical(view_x); *y = i32::from(g.y) + scale.physical(view_y); 1
        }
        fn on_paint(&self, _browser: Option<&mut Browser>, type_: PaintElementType,
            dirty_rects: Option<&[Rect]>, buffer: *const u8, width: i32, height: i32) {
            self.surface.paint(type_, dirty_rects.unwrap_or_default(), buffer, width, height);
        }
        fn on_popup_show(&self, _browser: Option<&mut Browser>, show: i32) {
            self.surface.popup_visible.set(show != 0); self.surface.present();
        }
        fn on_popup_size(&self, _browser: Option<&mut Browser>, rect: Option<&Rect>) {
            if let Some(rect) = rect { *self.surface.popup_rect.borrow_mut() = rect.clone(); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_updates_clip_to_the_view_and_preserve_unchanged_pixels() {
        let mut frame = vec![0; 24];
        let pixels = vec![255; 24];
        update_regions(
            &mut frame,
            &pixels,
            3,
            2,
            &[
                Rect {
                    x: 1,
                    y: -1,
                    width: 4,
                    height: 2,
                },
                Rect {
                    x: i32::MAX,
                    y: 0,
                    width: i32::MAX,
                    height: 1,
                },
            ],
        );
        assert_eq!(&frame[..4], &[0; 4]);
        assert_eq!(&frame[4..12], &[255; 8]);
        assert_eq!(&frame[12..], &[0; 12]);
    }

    #[test]
    fn popup_blending_clips_and_preserves_premultiplied_alpha() {
        let mut frame = vec![0, 0, 100, 255, 0, 0, 100, 255];
        blend_popup(
            &mut frame,
            2,
            1,
            &[100, 0, 0, 128, 0, 100, 0, 128],
            &Rect {
                x: -1,
                y: 0,
                width: 2,
                height: 1,
            },
        );
        assert_eq!(frame, [0, 100, 50, 255, 0, 0, 100, 255]);
    }
}
