use std::cell::{Cell, RefCell};
use std::os::fd::BorrowedFd;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cef::*;

use crate::playback::{NowPlaying, PlaybackCoordinator, PlayerCommand};
use crate::players::mpv::{
    WaylandDmaBufPlane, WaylandFramePayload, WaylandModifiers, WaylandSurfaceRole,
    WaylandUiCommand, WaylandUiEvent, WaylandUiHandle, drm_fourcc_for_cef_color, physical_extent,
};
use crate::preferences::{AppSettings, WebUiWindowSettings};
use crate::shell::cef::{BrowserState, request_main_window_close};

use super::input;

const CLICK_WINDOW: Duration = Duration::from_millis(500);
const CLICK_SLOP_PX: i32 = 4;

thread_local! {
    static WAYLAND_SURFACE: RefCell<Weak<WaylandSurface>> = const { RefCell::new(Weak::new()) };
}

pub(super) fn reveal() {
    let surface = WAYLAND_SURFACE.with(|slot| slot.borrow().upgrade());
    if let Some(surface) = surface {
        surface.reveal();
    }
}

pub(super) struct WaylandSurface {
    state: BrowserState,
    playback: Arc<PlaybackCoordinator>,
    ui: RefCell<Option<WaylandUiHandle>>,
    browser: RefCell<Option<Browser>>,
    logical_width: Cell<u16>,
    logical_height: Cell<u16>,
    scale_120: Cell<u32>,
    fullscreen: Cell<bool>,
    maximized: Cell<bool>,
    visible: Cell<bool>,
    closing: Cell<bool>,
    window_settings: Cell<WebUiWindowSettings>,
    popup_visible: Cell<bool>,
    popup_rect: RefCell<Rect>,
    last_click: Cell<(u32, Instant, i32, i32, i32)>,
    dmabuf_logged: Cell<bool>,
    shm_logged: Cell<bool>,
    wake_queued: Arc<AtomicBool>,
}

impl WaylandSurface {
    pub(super) fn new(
        settings: &AppSettings,
        playback: Arc<PlaybackCoordinator>,
        state: BrowserState,
    ) -> Rc<Self> {
        let (width, height) = settings.webui_window.size();
        Rc::new(Self {
            state,
            playback,
            ui: RefCell::new(None),
            browser: RefCell::new(None),
            logical_width: Cell::new(width.clamp(320, 16384) as u16),
            logical_height: Cell::new(height.clamp(240, 16384) as u16),
            scale_120: Cell::new(120),
            fullscreen: Cell::new(false),
            maximized: Cell::new(settings.webui_window.maximized),
            visible: Cell::new(true),
            closing: Cell::new(false),
            window_settings: Cell::new(settings.webui_window),
            popup_visible: Cell::new(false),
            popup_rect: RefCell::new(Rect::default()),
            last_click: Cell::new((0, Instant::now(), 0, 0, 0)),
            dmabuf_logged: Cell::new(false),
            shm_logged: Cell::new(false),
            wake_queued: Arc::new(AtomicBool::new(false)),
        })
    }

    pub(super) fn bind(&self, ui: WaylandUiHandle) -> Result<(), String> {
        let title = self
            .state
            .lock()
            .map(|s| s.title.clone())
            .unwrap_or_else(|_| "MediaFlick Desktop".to_string());
        ui.send(WaylandUiCommand::ConfigureWindow {
            title,
            width: self.logical_width.get(),
            height: self.logical_height.get(),
            maximized: self.maximized.get(),
        });
        let flag = Arc::clone(&self.wake_queued);
        ui.set_waker(Arc::new(move || {
            if !flag.swap(true, Ordering::AcqRel) {
                let mut task = WaylandWakeTask::new(Arc::clone(&flag));
                let _ = crate::shell::cef::post_ui_task(&mut task);
            }
        }));
        *self.ui.borrow_mut() = Some(ui);
        super::ACTIVE.store(true, Ordering::Release);
        Ok(())
    }

    pub(super) fn render_handler(self: &Rc<Self>) -> Option<RenderHandler> {
        Some(WaylandRenderHandler::new(Rc::clone(self)))
    }

    pub(super) fn create_browser(self: &Rc<Self>, client: &mut Client) -> Option<Browser> {
        let browser = super::create_windowless_browser(client, 0, true)?;
        *self.browser.borrow_mut() = Some(browser.clone());
        WAYLAND_SURFACE.with(|slot| *slot.borrow_mut() = Rc::downgrade(self));
        self.drain_events();
        Some(browser)
    }

    pub(super) fn show(&self) {
        if let Some(host) = self.browser_host() {
            host.set_focus(1);
        }
        self.drain_events();
    }

    fn reveal(&self) {
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.send(WaylandUiCommand::Reveal);
        }
        if let Some(host) = self.browser_host() {
            host.set_focus(1);
        }
    }

    pub(super) fn set_title(&self, title: &str) {
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.send(WaylandUiCommand::ConfigureWindow {
                title: title.to_string(),
                width: self.logical_width.get(),
                height: self.logical_height.get(),
                maximized: self.maximized.get(),
            });
        }
    }

    pub(super) fn set_cursor(&self, cursor: CursorType) {
        let shape = if cursor == CursorType::IBEAM {
            9
        } else if cursor == CursorType::HAND {
            4
        } else if cursor == CursorType::CROSS {
            8
        } else {
            1
        };
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.send(WaylandUiCommand::SetCursorShape(shape));
        }
    }

    pub(super) fn destroy(&self) {
        self.closing.set(true);
        if let Some(ui) = self.ui.borrow_mut().take() {
            ui.clear_waker();
        }
        self.browser.borrow_mut().take();
        super::save_window_bounds(self.window_settings.get());
        super::ACTIVE.store(false, Ordering::Release);
    }

    fn browser_host(&self) -> Option<BrowserHost> {
        self.browser.borrow().as_ref()?.host()
    }

    fn scale_factor(&self) -> f32 {
        (self.scale_120.get().max(60) as f32) / 120.0
    }

    fn request_close(&self) {
        let browser = self.browser.borrow().clone();
        request_main_window_close(Some(&self.state), browser, || {
            if let Some(ui) = self.ui.borrow().as_ref() {
                ui.send(WaylandUiCommand::Minimize);
            }
        });
    }

    fn drain_events(&self) {
        if self.closing.get() {
            return;
        }
        let Some(ui) = self.ui.borrow().as_ref().cloned() else {
            return;
        };
        for event in ui.drain_events() {
            self.handle_event(event);
            if self.closing.get() {
                return;
            }
        }
    }

    fn handle_event(&self, event: WaylandUiEvent) {
        let Some(host) = self.browser_host() else {
            return;
        };
        match event {
            WaylandUiEvent::Configure { .. } => self.handle_configure(&host, event),
            WaylandUiEvent::PointerMove {
                x,
                y,
                modifiers,
                leave,
            } => host.send_mouse_move_event(
                Some(&MouseEvent {
                    x,
                    y,
                    modifiers: input::wayland_modifiers(modifiers),
                }),
                i32::from(leave),
            ),
            WaylandUiEvent::PointerButton {
                x,
                y,
                button,
                pressed,
                modifiers,
            } => self.handle_pointer_button(&host, x, y, button, pressed, modifiers),
            WaylandUiEvent::PointerWheel {
                x,
                y,
                delta_x,
                delta_y,
                modifiers,
            } => host.send_mouse_wheel_event(
                Some(&MouseEvent {
                    x,
                    y,
                    modifiers: input::wayland_modifiers(modifiers),
                }),
                delta_x,
                delta_y,
            ),
            WaylandUiEvent::KeyboardFocus(focused) => host.set_focus(i32::from(focused)),
            WaylandUiEvent::Key { .. } => self.handle_key(&host, event),
            WaylandUiEvent::CloseRequested => self.request_close(),
        }
    }

    fn handle_configure(&self, host: &BrowserHost, event: WaylandUiEvent) {
        let WaylandUiEvent::Configure {
            width,
            height,
            scale_120,
            fullscreen,
            maximized,
            visible,
        } = event
        else {
            return;
        };
        let old_w = self.logical_width.replace(width.max(1));
        let old_h = self.logical_height.replace(height.max(1));
        let old_scale = self.scale_120.replace(scale_120.max(60));
        self.fullscreen.set(fullscreen);
        self.maximized.set(maximized);
        let old_vis = self.visible.replace(visible);

        if visible && !fullscreen {
            let mut settings = self.window_settings.get();
            settings.record_bounds(0, 0, i32::from(width), i32::from(height), maximized);
            self.window_settings.set(settings);
        }
        if old_scale != scale_120 {
            host.notify_screen_info_changed();
        }
        if old_scale != scale_120 || old_w != width || old_h != height {
            host.was_resized();
        }
        if old_vis != visible {
            host.was_hidden(i32::from(!visible));
        }
    }

    fn handle_pointer_button(
        &self,
        host: &BrowserHost,
        x: i32,
        y: i32,
        button: u32,
        pressed: bool,
        modifiers: WaylandModifiers,
    ) {
        if pressed {
            host.set_focus(1);
        }
        if button == 0x111
            && matches!(
                self.playback.snapshot().now_playing,
                Some(NowPlaying::Video)
            )
        {
            if pressed {
                self.playback
                    .control(PlayerCommand::SetPause(!self.playback.snapshot().paused));
            }
            return;
        }
        let btn = match button {
            0x110 => MouseButtonType::LEFT,
            0x112 => MouseButtonType::MIDDLE,
            0x111 => MouseButtonType::RIGHT,
            _ => return,
        };
        let now = Instant::now();
        let (prev_btn, prev_at, px, py, count) = self.last_click.get();
        let click_count = if !pressed {
            count
        } else if prev_btn == button
            && now.saturating_duration_since(prev_at) <= CLICK_WINDOW
            && (x - px).abs() <= CLICK_SLOP_PX
            && (y - py).abs() <= CLICK_SLOP_PX
        {
            count % 3 + 1
        } else {
            1
        };
        if pressed {
            self.last_click.set((button, now, x, y, click_count));
        }
        host.send_mouse_click_event(
            Some(&MouseEvent {
                x,
                y,
                modifiers: input::wayland_modifiers(modifiers),
            }),
            btn,
            i32::from(!pressed),
            click_count,
        );
    }

    fn handle_key(&self, host: &BrowserHost, event: WaylandUiEvent) {
        let WaylandUiEvent::Key {
            evdev_code,
            base_sym,
            symbol,
            modifiers,
            pressed,
            repeated,
        } = event
        else {
            return;
        };
        if modifiers.alt && base_sym == 0xffc1 {
            if pressed {
                self.request_close();
            }
            return;
        }
        input::send_xkb_key(
            host,
            i32::try_from(evdev_code).unwrap_or(0),
            input::wayland_modifiers(modifiers),
            modifiers.control || modifiers.alt || modifiers.super_key,
            (base_sym, symbol),
            !pressed,
            repeated,
        );
    }

    fn paint_accelerated(&self, kind: PaintElementType, info: Option<&AcceleratedPaintInfo>) {
        if self.closing.get() {
            return;
        }
        let Some(info) = info else {
            return;
        };
        let Ok(w) = u16::try_from(info.extra.coded_size.width) else {
            return;
        };
        let Ok(h) = u16::try_from(info.extra.coded_size.height) else {
            return;
        };
        if w == 0 || h == 0 || w > 16384 || h > 16384 {
            return;
        }
        let plane_count = usize::try_from(info.plane_count).unwrap_or(0);
        let Some(raw_planes) = info.planes.get(..plane_count) else {
            return;
        };
        let mut planes = Vec::with_capacity(raw_planes.len());
        for p in raw_planes {
            if p.fd < 0 || p.stride == 0 {
                return;
            }
            // SAFETY: CEF guarantees `p.fd` is a valid DMA-BUF descriptor during `on_accelerated_paint`.
            let borrowed = unsafe { BorrowedFd::borrow_raw(p.fd) };
            let Ok(owned) = borrowed.try_clone_to_owned() else {
                return;
            };
            planes.push(WaylandDmaBufPlane {
                fd: owned,
                offset: u32::try_from(p.offset).unwrap_or(0),
                stride: p.stride,
            });
        }
        if planes.is_empty() {
            return;
        }
        if !self.dmabuf_logged.replace(true) {
            tracing::info!(
                target: "cef.osr",
                width = w,
                height = h,
                planes = planes.len(),
                format = ?info.format,
                modifier = info.modifier,
                visible_w = info.extra.visible_rect.width,
                visible_h = info.extra.visible_rect.height,
                "received first Wayland DMA-BUF CEF frame"
            );
        }
        let fourcc = drm_fourcc_for_cef_color(info.format == ColorType::RGBA_8888);
        let (role, logical_width, logical_height, x, y) = self.target_surface_geometry(kind, w, h);
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.send(WaylandUiCommand::Present {
                role,
                logical_width,
                logical_height,
                x,
                y,
                payload: WaylandFramePayload::DmaBuf {
                    width: w,
                    height: h,
                    fourcc,
                    modifier: info.modifier,
                    planes,
                },
            });
        }
    }

    fn paint_shm(&self, kind: PaintElementType, buffer: *const u8, width: i32, height: i32) {
        if self.closing.get()
            || buffer.is_null()
            || !(1..=16384).contains(&width)
            || !(1..=16384).contains(&height)
        {
            return;
        }
        let w = width as u16;
        let h = height as u16;
        if kind == PaintElementType::VIEW {
            let expected_w =
                physical_extent(i32::from(self.logical_width.get()), self.scale_120.get());
            let expected_h =
                physical_extent(i32::from(self.logical_height.get()), self.scale_120.get());
            if w != expected_w || h != expected_h {
                return;
            }
        }
        if !self.shm_logged.replace(true) {
            tracing::info!(
                target: "cef.osr",
                width = w,
                height = h,
                "received first Wayland SHM CEF frame"
            );
        }
        let length = usize::from(w) * usize::from(h) * 4;
        // SAFETY: CEF provides `width * height * 4` premultiplied BGRA bytes until `on_paint` returns.
        let pixels = unsafe { std::slice::from_raw_parts(buffer, length) }.to_vec();
        let (role, logical_width, logical_height, x, y) = self.target_surface_geometry(kind, w, h);
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.send(WaylandUiCommand::Present {
                role,
                logical_width,
                logical_height,
                x,
                y,
                payload: WaylandFramePayload::Shm {
                    width: w,
                    height: h,
                    pixels,
                },
            });
        }
    }

    fn target_surface_geometry(
        &self,
        kind: PaintElementType,
        fallback_w: u16,
        fallback_h: u16,
    ) -> (WaylandSurfaceRole, u16, u16, i32, i32) {
        if kind == PaintElementType::POPUP {
            let r = self.popup_rect.borrow();
            let lw = u16::try_from(r.width.clamp(1, 16384)).unwrap_or(fallback_w);
            let lh = u16::try_from(r.height.clamp(1, 16384)).unwrap_or(fallback_h);
            return (WaylandSurfaceRole::Popup, lw, lh, r.x, r.y);
        }
        (
            WaylandSurfaceRole::View,
            self.logical_width.get().max(1),
            self.logical_height.get().max(1),
            0,
            0,
        )
    }
}

wrap_task! {
    struct WaylandWakeTask {
        wake_queued: Arc<AtomicBool>,
    }

    impl Task {
        fn execute(&self) {
            self.wake_queued.store(false, Ordering::Release);
            let surface = WAYLAND_SURFACE.with(|slot| slot.borrow().upgrade());
            if let Some(surface) = surface {
                surface.drain_events();
            }
        }
    }
}

wrap_render_handler! {
    struct WaylandRenderHandler {
        surface: Rc<WaylandSurface>,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                *rect = Rect {
                    x: 0,
                    y: 0,
                    width: i32::from(self.surface.logical_width.get()),
                    height: i32::from(self.surface.logical_height.get()),
                };
            }
        }

        fn screen_info(&self, _browser: Option<&mut Browser>, info: Option<&mut ScreenInfo>) -> i32 {
            let Some(info) = info else {
                return 0;
            };
            info.device_scale_factor = self.surface.scale_factor();
            info.depth = 32;
            info.depth_per_component = 8;
            info.rect = Rect {
                x: 0,
                y: 0,
                width: i32::from(self.surface.logical_width.get()),
                height: i32::from(self.surface.logical_height.get()),
            };
            info.available_rect = info.rect.clone();
            1
        }

        fn screen_point(
            &self,
            _browser: Option<&mut Browser>,
            view_x: i32,
            view_y: i32,
            screen_x: Option<&mut i32>,
            screen_y: Option<&mut i32>,
        ) -> i32 {
            let (Some(x), Some(y)) = (screen_x, screen_y) else {
                return 0;
            };
            *x = view_x;
            *y = view_y;
            1
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: i32,
            height: i32,
        ) {
            self.surface.paint_shm(type_, buffer, width, height);
        }

        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            self.surface.paint_accelerated(type_, info);
        }

        fn on_popup_show(&self, _browser: Option<&mut Browser>, show: i32) {
            self.surface.popup_visible.set(show != 0);
            if show == 0
                && let Some(ui) = self.surface.ui.borrow().as_ref()
            {
                ui.send(WaylandUiCommand::HidePopup);
            }
        }

        fn on_popup_size(&self, _browser: Option<&mut Browser>, rect: Option<&Rect>) {
            if let Some(rect) = rect {
                *self.surface.popup_rect.borrow_mut() = rect.clone();
            }
        }
    }
}
