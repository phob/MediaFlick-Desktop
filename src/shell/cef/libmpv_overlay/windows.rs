mod close_hooks;
mod compositor;
mod display;
mod input;

use std::cell::{Cell, RefCell};
use std::io;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use cef::*;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
    GetWindowLongPtrW, HTCLIENT, HWND_TOP, IDC_ARROW, IsWindow, LoadCursorW, PostMessageW,
    RegisterClassW, SW_MINIMIZE, SW_SHOW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, WM_APP, WM_CHAR, WM_CLOSE, WM_ERASEBKGND, WM_KEYDOWN, WM_KEYUP,
    WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN,
    WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY,
    WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SETFOCUS, WM_SYSCHAR,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSW,
    WS_CHILD, WS_VISIBLE,
};

use crate::playback::{NativeWindowHandle, PlaybackCoordinator, PlayerCommand};
use crate::preferences::{AppSettings, PlayerBackend, WebUiWindowSettings};
use crate::shell::cef::{BrowserState, app_scheme, request_main_window_close};

use self::close_hooks::{HostCloseHooks, install_host_close_hooks, remove_host_close_hooks};
use self::compositor::Compositor;
use self::display::{
    apply_saved_window_settings, display_frame_rate, logical_to_physical, physical_to_logical,
    sample_metrics, sample_window_settings,
};
use self::input::{
    apply_cursor, is_alt_f4, key_event, low_word, mouse_button, mouse_modifiers,
    navigation_mouse_button, right_button_pause, signed_high_word, signed_low_word,
    track_mouse_leave,
};

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_HOST_CLOSE_REQUESTED: u32 = WM_APP + 0x2A1;
const DEFAULT_DPI: u32 = 96;
const SYNC_INTERVAL_MS: i64 = 50;
const INPUT_CLASS: &[u16] = &[
    77, 101, 100, 105, 97, 70, 108, 105, 99, 107, 67, 101, 102, 73, 110, 112, 117, 116, 0,
];

static ACTIVE: AtomicBool = AtomicBool::new(false);
static CLASS_REGISTERED: OnceLock<bool> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ViewMetrics {
    logical_width: i32,
    logical_height: i32,
    physical_width: i32,
    physical_height: i32,
    screen_x: i32,
    screen_y: i32,
    dpi: u32,
}

impl ViewMetrics {
    fn from_logical_size(width: i32, height: i32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            logical_width: width,
            logical_height: height,
            physical_width: width,
            physical_height: height,
            dpi: DEFAULT_DPI,
            ..Self::default()
        }
    }
}

pub(crate) struct LibmpvOverlaySurface {
    state: BrowserState,
    playback: Arc<PlaybackCoordinator>,
    host: Cell<HWND>,
    input: Cell<HWND>,
    host_close_hooks: Cell<HostCloseHooks>,
    metrics: Cell<ViewMetrics>,
    frame_rate: Cell<i32>,
    display_checked_at: Cell<Instant>,
    window_settings: Cell<WebUiWindowSettings>,
    browser: RefCell<Option<Browser>>,
    compositor: RefCell<Option<Compositor>>,
    popup_rect: RefCell<Rect>,
    cursor: Cell<CursorType>,
    accelerated_paint_seen: Cell<bool>,
    software_paint_seen: Cell<bool>,
    closing: Cell<bool>,
}

impl LibmpvOverlaySurface {
    pub(in crate::shell::cef) fn select(
        settings: &AppSettings,
        playback: Arc<PlaybackCoordinator>,
        state: BrowserState,
    ) -> Option<Rc<Self>> {
        if !is_configured(settings) {
            return None;
        }
        let (width, height) = settings.webui_window.size();
        Some(Rc::new(Self {
            state,
            playback,
            host: Cell::new(null_mut()),
            input: Cell::new(null_mut()),
            host_close_hooks: Cell::new(HostCloseHooks::default()),
            metrics: Cell::new(ViewMetrics::from_logical_size(width, height)),
            frame_rate: Cell::new(60),
            display_checked_at: Cell::new(Instant::now()),
            window_settings: Cell::new(settings.webui_window),
            browser: RefCell::new(None),
            compositor: RefCell::new(None),
            popup_rect: RefCell::new(Rect::default()),
            cursor: Cell::new(CursorType::POINTER),
            accelerated_paint_seen: Cell::new(false),
            software_paint_seen: Cell::new(false),
            closing: Cell::new(false),
        }))
    }

    pub(crate) fn render_handler(self: &Rc<Self>) -> Option<RenderHandler> {
        Some(OverlayRenderHandler::new(Rc::clone(self)))
    }

    pub(crate) fn set_cursor(&self, cursor: CursorType) {
        self.cursor.set(cursor);
        let input = self.input.get();
        if input.is_null() {
            return;
        }
        // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
        unsafe {
            PostMessageW(input, WM_SETCURSOR, input as WPARAM, HTCLIENT as LPARAM);
        }
    }

    pub(crate) fn bind(self: &Rc<Self>, parent: NativeWindowHandle) -> Result<(), String> {
        if !input_class_registered() {
            return Err("failed to register the CEF input window class".to_string());
        }
        let host = parent.raw() as HWND;
        // SAFETY: IsWindow only inspects the handle value.
        if host.is_null() || unsafe { IsWindow(host) } == 0 {
            return Err("libmpv returned an invalid native window".to_string());
        }

        let seeded = self.metrics.get();
        apply_saved_window_settings(host, self.window_settings.get());
        let metrics = sample_metrics(host).unwrap_or(seeded);
        crate::windows::set_native_window_icon(parent.raw());
        let compositor = Compositor::new(parent.raw()).map_err(|error| error.to_string())?;
        // SAFETY: a null name asks for this process's own module handle.
        let module = unsafe { GetModuleHandleW(null()) };
        if module.is_null() {
            return Err(io::Error::last_os_error().to_string());
        }
        let retained = Rc::into_raw(Rc::clone(self));
        // SAFETY: INPUT_CLASS is a registered, NUL-terminated class name and
        // `retained` is a live surface pointer; input_wndproc stores it at
        // WM_NCCREATE and releases it at WM_NCDESTROY.
        let input = unsafe {
            CreateWindowExW(
                0,
                INPUT_CLASS.as_ptr(),
                null(),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                metrics.physical_width,
                metrics.physical_height,
                host,
                null_mut(),
                module,
                retained.cast(),
            )
        };
        if input.is_null() {
            // SAFETY: creation failed before any window message could store the
            // pointer, so this reclaims the only use of `retained`.
            unsafe { drop(Rc::from_raw(retained)) };
            return Err(io::Error::last_os_error().to_string());
        }
        let host_close_hooks = match install_host_close_hooks(host, input) {
            Ok(hooks) => hooks,
            Err(error) => {
                // SAFETY: `input` was created on this thread just above; destroying
                // it runs WM_NCDESTROY, which releases the retained surface.
                unsafe { DestroyWindow(input) };
                return Err(error.to_string());
            }
        };

        self.host.set(host);
        self.input.set(input);
        self.host_close_hooks.set(host_close_hooks);
        self.metrics.set(metrics);
        *self.compositor.borrow_mut() = Some(compositor);
        ACTIVE.store(true, Ordering::Release);
        tracing::info!(target: "cef.osr", hwnd = parent.raw(), "bound the GPU-composited React surface to libmpv");
        self.sync();
        Ok(())
    }

    pub(crate) fn create_browser(self: &Rc<Self>, client: &mut Client) -> Option<Browser> {
        let host = self.host.get();
        if host.is_null() {
            return None;
        }
        let cef_parent = sys::HWND(host.cast::<sys::HWND__>());
        let mut window_info = WindowInfo::default().set_as_windowless(cef_parent);
        window_info.shared_texture_enabled = 1;
        let frame_rate = display_frame_rate(host);
        self.frame_rate.set(frame_rate);
        let settings = BrowserSettings {
            background_color: 0,
            windowless_frame_rate: frame_rate,
            ..BrowserSettings::default()
        };
        let url = CefString::from(app_scheme::APP_URL);
        let browser = browser_host_create_browser_sync(
            Some(&window_info),
            Some(client),
            Some(&url),
            Some(&settings),
            None,
            None,
        )?;
        *self.browser.borrow_mut() = Some(browser.clone());
        if let Some(browser_host) = browser.host() {
            browser_host.set_focus(1);
        }
        let input = self.input.get();
        if !input.is_null() {
            // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
            unsafe { SetFocus(input) };
        }
        self.schedule_sync();
        Some(browser)
    }

    pub(crate) fn sync(&self) {
        let _ = self.sync_live_window();
    }

    pub(crate) fn show(&self) {
        let input = self.input.get();
        if !input.is_null() {
            // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
            unsafe { ShowWindow(input, SW_SHOW) };
        }
    }

    pub(crate) fn destroy(&self) {
        self.closing.set(true);
        self.save_window_settings();
        remove_host_close_hooks(self.host_close_hooks.replace(HostCloseHooks::default()));
        let input = self.input.replace(null_mut());
        // SAFETY: IsWindow only inspects the handle value, and a live `input`
        // is our own child window, destroyed on the thread that created it.
        if !input.is_null() && unsafe { IsWindow(input) } != 0 {
            // SAFETY: as above; WM_NCDESTROY releases the retained surface.
            unsafe { DestroyWindow(input) };
        }
        self.browser.borrow_mut().take();
        self.compositor.borrow_mut().take();
        self.host.set(null_mut());
        ACTIVE.store(false, Ordering::Release);
    }

    fn schedule_sync(self: &Rc<Self>) {
        let mut task = SurfaceSyncTask::new(Rc::clone(self));
        if post_delayed_task(ThreadId::UI, Some(&mut task), SYNC_INTERVAL_MS) == 0 {
            tracing::warn!(target: "cef.osr", "failed to schedule native-window synchronization");
        }
    }

    fn sync_live_window(&self) -> bool {
        if self.closing.get() {
            return false;
        }
        let host = self.host.get();
        // SAFETY: IsWindow only inspects the handle value.
        if host.is_null() || unsafe { IsWindow(host) } == 0 {
            self.close_browser();
            return false;
        }
        let input = self.input.get();
        // SAFETY: IsWindow only inspects the handle value.
        if input.is_null() || unsafe { IsWindow(input) } == 0 {
            self.close_browser();
            return false;
        }
        if self.display_checked_at.get().elapsed() >= Duration::from_secs(1) {
            self.display_checked_at.set(Instant::now());
            let rate = display_frame_rate(host);
            if self.frame_rate.replace(rate) != rate {
                self.with_browser_host(|browser_host| browser_host.set_windowless_frame_rate(rate));
            }
        }
        self.capture_window_settings(host);
        let Some(metrics) = sample_metrics(host) else {
            return true;
        };
        let previous = self.metrics.replace(metrics);
        // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
        unsafe {
            SetWindowPos(
                input,
                HWND_TOP,
                0,
                0,
                metrics.physical_width,
                metrics.physical_height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        if previous != metrics {
            self.with_browser_host(|browser_host| {
                browser_host.notify_screen_info_changed();
                if previous.logical_width != metrics.logical_width
                    || previous.logical_height != metrics.logical_height
                    || previous.dpi != metrics.dpi
                {
                    browser_host.was_resized();
                }
            });
        }
        true
    }

    fn request_close(&self) {
        let browser = self.browser.borrow().clone();
        request_main_window_close(Some(&self.state), browser, || {
            // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
            unsafe { ShowWindow(self.host.get(), SW_MINIMIZE) };
        });
    }

    /// Closes after the native window vanished; nothing may cancel it.
    fn close_browser(&self) {
        if self.closing.replace(true) {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            state.force_close_requested = true;
        }
        self.with_browser_host(|browser_host| browser_host.close_browser(1));
    }

    fn capture_window_settings(&self, host: HWND) {
        if let Some(settings) = sample_window_settings(host, self.window_settings.get()) {
            self.window_settings.set(settings);
        }
    }

    fn save_window_settings(&self) {
        let host = self.host.get();
        if host.is_null() {
            return;
        }
        // SAFETY: IsWindow only inspects the handle value.
        if unsafe { IsWindow(host) } != 0 {
            self.capture_window_settings(host);
        }
        let Some(services) = crate::app::services::services() else {
            return;
        };
        if let Err(error) = services
            .preferences
            .record_window(self.window_settings.get())
        {
            tracing::warn!(target: "config", "failed to save libmpv window settings: {error}");
        }
    }

    fn handle_playback_right_button(&self, window: HWND, message: u32) -> bool {
        let snapshot = self.playback.snapshot();
        if !snapshot.active {
            return false;
        }
        if matches!(message, WM_RBUTTONDOWN | WM_RBUTTONDBLCLK) {
            // SAFETY: `window` is the input window receiving this message on its
            // own thread. Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
            unsafe {
                SetFocus(window);
                SetCapture(window);
            }
        } else if message == WM_RBUTTONUP {
            // SAFETY: releases this thread's own capture, if any.
            unsafe { ReleaseCapture() };
        }
        if let Some(pause) = right_button_pause(message, snapshot.paused) {
            self.playback.control(PlayerCommand::SetPause(pause));
        }
        true
    }

    fn handle_navigation_mouse_button(&self, message: u32, wparam: WPARAM) {
        // CEF's off-screen mouse API only accepts left/middle/right. Handle
        // navigation once on release; consume down events in the window
        // procedure so DefWindowProc cannot issue a second browser command.
        let Some(back) = navigation_mouse_button(message, wparam) else {
            return;
        };
        let browser = self.browser.borrow().clone();
        if let Some(browser) = browser {
            if back {
                browser.go_back();
            } else {
                browser.go_forward();
            }
        }
    }

    fn with_browser_host(&self, action: impl FnOnce(BrowserHost)) {
        if let Some(browser_host) = self
            .browser
            .borrow()
            .clone()
            .and_then(|browser| browser.host())
        {
            action(browser_host);
        }
    }

    fn view_rect(&self, rect: &mut Rect) {
        let metrics = self.metrics.get();
        *rect = Rect {
            x: 0,
            y: 0,
            width: metrics.logical_width.max(1),
            height: metrics.logical_height.max(1),
        };
    }

    fn screen_info(&self, info: &mut ScreenInfo) -> i32 {
        let metrics = self.metrics.get();
        let rect = Rect {
            x: 0,
            y: 0,
            width: metrics.logical_width.max(1),
            height: metrics.logical_height.max(1),
        };
        info.device_scale_factor = metrics.dpi.max(DEFAULT_DPI) as f32 / DEFAULT_DPI as f32;
        info.rect = rect.clone();
        info.available_rect = rect;
        1
    }

    fn screen_point(&self, view_x: i32, view_y: i32, screen_x: &mut i32, screen_y: &mut i32) {
        let metrics = self.metrics.get();
        *screen_x = metrics.screen_x + logical_to_physical(view_x, metrics.dpi);
        *screen_y = metrics.screen_y + logical_to_physical(view_y, metrics.dpi);
    }

    fn popup_visibility(&self, visible: bool) {
        if let Ok(mut compositor) = self.compositor.try_borrow_mut()
            && let Some(compositor) = compositor.as_mut()
        {
            compositor.set_popup_visible(visible);
        }
    }

    fn popup_rect(&self, rect: &Rect) {
        *self.popup_rect.borrow_mut() = rect.clone();
        let dpi = self.metrics.get().dpi;
        if let Ok(mut compositor) = self.compositor.try_borrow_mut()
            && let Some(compositor) = compositor.as_mut()
        {
            compositor.set_popup_position(
                logical_to_physical(rect.x, dpi) as f32,
                logical_to_physical(rect.y, dpi) as f32,
            );
        }
    }

    fn accelerated_paint(&self, type_: PaintElementType, info: &AcceleratedPaintInfo) {
        let Some(popup) = paint_part(type_) else {
            return;
        };
        if !self.accelerated_paint_seen.replace(true) {
            tracing::info!(target: "cef.osr", "received the first CEF shared-texture frame");
        }
        let Ok(mut compositor) = self.compositor.try_borrow_mut() else {
            tracing::warn!(target: "cef.osr", "dropped a reentrant accelerated paint callback");
            return;
        };
        let Some(compositor) = compositor.as_mut() else {
            return;
        };
        if let Err(error) = compositor.present_shared(popup, info.shared_texture_handle) {
            tracing::warn!(target: "cef.osr", "failed to present CEF shared texture: {error}");
        }
    }

    fn software_paint(&self, type_: PaintElementType, buffer: *const u8, width: i32, height: i32) {
        let Some(popup) = paint_part(type_) else {
            return;
        };
        if !self.software_paint_seen.replace(true) {
            tracing::warn!(target: "cef.osr", "CEF used the software paint fallback instead of a shared texture");
        }
        let Some(length) = paint_buffer_len(width, height) else {
            return;
        };
        if buffer.is_null() {
            return;
        }
        // SAFETY: CEF guarantees a tightly packed width * height * 4 BGRA
        // buffer that remains valid for the duration of on_paint.
        let pixels = unsafe { slice::from_raw_parts(buffer, length) };
        let Ok(mut compositor) = self.compositor.try_borrow_mut() else {
            tracing::warn!(target: "cef.osr", "dropped a reentrant software paint callback");
            return;
        };
        let Some(compositor) = compositor.as_mut() else {
            return;
        };
        if let Err(error) = compositor.present_software(popup, pixels, width, height) {
            tracing::warn!(target: "cef.osr", "failed to upload CEF software texture: {error}");
        }
    }

    fn mouse_event(&self, x: i32, y: i32, modifiers: u32) -> MouseEvent {
        let dpi = self.metrics.get().dpi;
        MouseEvent {
            x: physical_to_logical(x, dpi),
            y: physical_to_logical(y, dpi),
            modifiers,
        }
    }
}

impl Drop for LibmpvOverlaySurface {
    fn drop(&mut self) {
        self.destroy();
    }
}

pub(crate) fn is_active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

pub(crate) fn is_configured(settings: &AppSettings) -> bool {
    settings.effective_backend() == PlayerBackend::Libmpv
}

wrap_task! {
    struct SurfaceSyncTask {
        surface: Rc<LibmpvOverlaySurface>,
    }

    impl Task {
        fn execute(&self) {
            if self.surface.sync_live_window() {
                self.surface.schedule_sync();
            }
        }
    }
}

wrap_render_handler! {
    struct OverlayRenderHandler {
        surface: Rc<LibmpvOverlaySurface>,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                self.surface.view_rect(rect);
            }
        }

        fn screen_point(
            &self,
            _browser: Option<&mut Browser>,
            view_x: i32,
            view_y: i32,
            screen_x: Option<&mut i32>,
            screen_y: Option<&mut i32>,
        ) -> i32 {
            let (Some(screen_x), Some(screen_y)) = (screen_x, screen_y) else {
                return 0;
            };
            self.surface.screen_point(view_x, view_y, screen_x, screen_y);
            1
        }

        fn screen_info(
            &self,
            _browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> i32 {
            screen_info.map_or(0, |info| self.surface.screen_info(info))
        }

        fn on_popup_show(&self, _browser: Option<&mut Browser>, show: i32) {
            self.surface.popup_visibility(show != 0);
        }

        fn on_popup_size(&self, _browser: Option<&mut Browser>, rect: Option<&Rect>) {
            if let Some(rect) = rect {
                self.surface.popup_rect(rect);
            }
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
            self.surface.software_paint(type_, buffer, width, height);
        }

        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            if let Some(info) = info {
                self.surface.accelerated_paint(type_, info);
            }
        }
    }
}

fn paint_part(type_: PaintElementType) -> Option<bool> {
    let kind: sys::cef_paint_element_type_t = type_.into();
    match kind {
        sys::cef_paint_element_type_t::PET_VIEW => Some(false),
        sys::cef_paint_element_type_t::PET_POPUP => Some(true),
        _ => None,
    }
}

fn input_class_registered() -> bool {
    *CLASS_REGISTERED.get_or_init(|| {
        // SAFETY: a null name asks for this process's own module handle.
        let module = unsafe { GetModuleHandleW(null()) };
        if module.is_null() {
            return false;
        }
        let class = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(input_wndproc),
            hInstance: module,
            // SAFETY: IDC_ARROW names a stock system cursor.
            hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
            lpszClassName: INPUT_CLASS.as_ptr(),
            ..WNDCLASSW::default()
        };
        // SAFETY: `class` is fully initialized, names a NUL-terminated static
        // class string, and points at input_wndproc, which lives for the program.
        unsafe { RegisterClassW(&class) != 0 }
    })
}

unsafe extern "system" fn input_wndproc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        // SAFETY: WM_NCCREATE's lparam is the CREATESTRUCTW of this creation.
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        // SAFETY: CreateWindowExW supplied a live LibmpvOverlaySurface pointer
        // in lpCreateParams; the window-owned Rc keeps it alive until destroy.
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    // SAFETY: reads this window's own user data slot.
    let surface_ptr =
        unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const LibmpvOverlaySurface;
    if surface_ptr.is_null() {
        // SAFETY: default handling for a message before WM_NCCREATE.
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    // SAFETY: a non-null slot holds the surface stored at WM_NCCREATE, kept
    // alive by the retained Rc until WM_NCDESTROY clears the slot below.
    let surface = unsafe { &*surface_ptr };
    let handled = dispatch_window_message(surface, window, message, wparam, lparam);
    if message == WM_NCDESTROY {
        // SAFETY: reclaims exactly the strong reference transferred by
        // Rc::into_raw in bind; the slot is cleared first, so no later window
        // message can observe it.
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            drop(Rc::from_raw(surface_ptr));
        }
    }
    // SAFETY: default handling for messages the surface does not consume.
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(window, message, wparam, lparam) })
}

fn dispatch_window_message(
    surface: &LibmpvOverlaySurface,
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    match message {
        WM_ERASEBKGND => Some(1),
        WM_HOST_CLOSE_REQUESTED => {
            surface.request_close();
            Some(0)
        }
        WM_SETCURSOR if low_word(lparam) == HTCLIENT => {
            apply_cursor(surface.cursor.get());
            Some(1)
        }
        WM_MOUSEMOVE => {
            track_mouse_leave(window);
            let event = surface.mouse_event(
                signed_low_word(lparam),
                signed_high_word(lparam),
                mouse_modifiers(wparam),
            );
            surface.with_browser_host(|host| host.send_mouse_move_event(Some(&event), 0));
            Some(0)
        }
        WM_MOUSELEAVE => {
            let event = MouseEvent {
                x: -1,
                y: -1,
                modifiers: mouse_modifiers(wparam),
            };
            surface.with_browser_host(|host| host.send_mouse_move_event(Some(&event), 1));
            Some(0)
        }
        WM_XBUTTONDOWN | WM_XBUTTONUP | WM_XBUTTONDBLCLK => {
            surface.handle_navigation_mouse_button(message, wparam);
            Some(1)
        }
        WM_RBUTTONDOWN | WM_RBUTTONUP | WM_RBUTTONDBLCLK
            if surface.handle_playback_right_button(window, message) =>
        {
            Some(0)
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_RBUTTONUP
        | WM_RBUTTONDBLCLK | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_MBUTTONDBLCLK => {
            let mouse_up = matches!(message, WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP);
            if !mouse_up {
                // SAFETY: `window` is the input window receiving this message on
                // its own thread. Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
                unsafe {
                    SetFocus(window);
                    SetCapture(window);
                }
            } else {
                // SAFETY: releases this thread's own capture, if any.
                unsafe { ReleaseCapture() };
            }
            let event = surface.mouse_event(
                signed_low_word(lparam),
                signed_high_word(lparam),
                mouse_modifiers(wparam),
            );
            let (button, click_count) = mouse_button(message);
            surface.with_browser_host(|host| {
                host.send_mouse_click_event(Some(&event), button, i32::from(mouse_up), click_count);
            });
            Some(0)
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let mut point = POINT {
                x: signed_low_word(lparam),
                y: signed_high_word(lparam),
            };
            // SAFETY: `point` is a live local the call converts in place.
            unsafe { ScreenToClient(window, &raw mut point) };
            let event = surface.mouse_event(point.x, point.y, mouse_modifiers(wparam));
            let delta = signed_high_word(wparam as isize);
            let (delta_x, delta_y) = if message == WM_MOUSEHWHEEL {
                (delta, 0)
            } else {
                (0, delta)
            };
            surface.with_browser_host(|host| {
                host.send_mouse_wheel_event(Some(&event), delta_x, delta_y);
            });
            Some(0)
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            surface.with_browser_host(|host| host.set_focus(i32::from(message == WM_SETFOCUS)));
            Some(0)
        }
        WM_SYSKEYDOWN if is_alt_f4(wparam) => {
            let host = surface.host.get();
            if !host.is_null() {
                // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
                unsafe { PostMessageW(host, WM_CLOSE, 0, 0) };
            }
            Some(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP | WM_CHAR | WM_SYSCHAR => {
            let event = key_event(message, wparam, lparam);
            surface.with_browser_host(|host| host.send_key_event(Some(&event)));
            Some(0)
        }
        _ => None,
    }
}

fn paint_buffer_len(width: i32, height: i32) -> Option<usize> {
    let width = usize::try_from(width).ok()?;
    let height = usize::try_from(height).ok()?;
    width.checked_mul(height)?.checked_mul(4)
}
