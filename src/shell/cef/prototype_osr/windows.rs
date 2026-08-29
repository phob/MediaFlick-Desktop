#[path = "windows/compositor.rs"]
mod compositor;

use std::cell::{Cell, RefCell};
use std::io;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use cef::*;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_CAPITAL, VK_CONTROL, VK_F4, VK_MENU, VK_NUMLOCK, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CS_DBLCLKS, CWPRETSTRUCT, CallNextHookEx, CreateWindowExW, DefWindowProcW,
    DestroyWindow, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW, GetWindowThreadProcessId,
    HHOOK, HTCLIENT, HWND_TOP, IDC_APPSTARTING, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_HELP,
    IDC_IBEAM, IDC_NO, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IDC_WAIT,
    IsWindow, LoadCursorW, MSG, PostMessageW, RegisterClassW, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOZORDER, SWP_SHOWWINDOW, SetCursor, SetWindowLongPtrW, SetWindowPos, SetWindowsHookExW,
    ShowWindow, UnhookWindowsHookEx, WH_CALLWNDPROCRET, WH_GETMESSAGE, WM_APP, WM_CHAR, WM_CLOSE,
    WM_ERASEBKGND, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SETCURSOR, WM_SETFOCUS, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSW, WS_CHILD,
    WS_VISIBLE,
};
use windows_sys::core::PCWSTR;

use crate::playback::{NativeWindowHandle, PlaybackCoordinator, PlayerCommand};
use crate::preferences::{AppSettings, PlayerBackend};
use crate::shell::cef::app_scheme;

use self::compositor::Compositor;

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_HOST_CLOSE_REQUESTED: u32 = WM_APP + 0x2A1;
const DEFAULT_DPI: u32 = 96;
const SYNC_INTERVAL_MS: i64 = 50;
const VK_Q: WPARAM = b'Q' as WPARAM;
const VK_V: WPARAM = b'V' as WPARAM;
const CAPTURED_Q: u8 = 1;
const CAPTURED_V: u8 = 1 << 1;
const INPUT_CLASS: &[u16] = &[
    77, 101, 100, 105, 97, 70, 108, 105, 99, 107, 67, 101, 102, 73, 110, 112, 117, 116, 0,
];

static ACTIVE: AtomicBool = AtomicBool::new(false);
static CLASS_REGISTERED: OnceLock<bool> = OnceLock::new();
static HOOKED_HOST: AtomicUsize = AtomicUsize::new(0);
static CLOSE_TARGET: AtomicUsize = AtomicUsize::new(0);

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

#[derive(Clone, Copy)]
struct HostCloseHooks {
    dispatched: HHOOK,
    queued: HHOOK,
}

impl HostCloseHooks {
    fn uninstall(self) {
        for hook in [self.dispatched, self.queued] {
            if !hook.is_null() {
                unsafe { UnhookWindowsHookEx(hook) };
            }
        }
    }
}

impl Default for HostCloseHooks {
    fn default() -> Self {
        Self {
            dispatched: null_mut(),
            queued: null_mut(),
        }
    }
}

pub(crate) struct PrototypeOsrSurface {
    playback: Arc<PlaybackCoordinator>,
    host: Cell<HWND>,
    input: Cell<HWND>,
    host_close_hooks: Cell<HostCloseHooks>,
    metrics: Cell<ViewMetrics>,
    browser: RefCell<Option<Browser>>,
    compositor: RefCell<Option<Compositor>>,
    popup_rect: RefCell<Rect>,
    cursor: Cell<CursorType>,
    captured_playback_keys: Cell<u8>,
    accelerated_paint_seen: Cell<bool>,
    software_paint_seen: Cell<bool>,
    closing: Cell<bool>,
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
            host: Cell::new(null_mut()),
            input: Cell::new(null_mut()),
            host_close_hooks: Cell::new(HostCloseHooks::default()),
            metrics: Cell::new(ViewMetrics::from_logical_size(width, height)),
            browser: RefCell::new(None),
            compositor: RefCell::new(None),
            popup_rect: RefCell::new(Rect::default()),
            cursor: Cell::new(CursorType::POINTER),
            captured_playback_keys: Cell::new(0),
            accelerated_paint_seen: Cell::new(false),
            software_paint_seen: Cell::new(false),
            closing: Cell::new(false),
        }))
    }

    pub(crate) fn render_handler(self: &Rc<Self>) -> Option<RenderHandler> {
        Some(PrototypeRenderHandler::new(Rc::clone(self)))
    }

    pub(crate) fn set_cursor(&self, cursor: CursorType) {
        self.cursor.set(cursor);
        let input = self.input.get();
        if input.is_null() {
            return;
        }
        unsafe {
            PostMessageW(input, WM_SETCURSOR, input as WPARAM, HTCLIENT as LPARAM);
        }
    }

    pub(crate) fn bind(self: &Rc<Self>, parent: NativeWindowHandle) -> Result<(), String> {
        if !input_class_registered() {
            return Err("failed to register the CEF input window class".to_string());
        }
        let host = parent.raw() as HWND;
        if host.is_null() || unsafe { IsWindow(host) } == 0 {
            return Err("libmpv returned an invalid native window".to_string());
        }

        let seeded = self.metrics.get();
        unsafe {
            SetWindowPos(
                host,
                null_mut(),
                0,
                0,
                seeded.physical_width,
                seeded.physical_height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        let metrics = sample_metrics(host).unwrap_or(seeded);
        crate::windows::set_native_window_icon(parent.raw());
        let compositor = Compositor::new(parent.raw()).map_err(|error| error.to_string())?;
        let module = unsafe { GetModuleHandleW(null()) };
        if module.is_null() {
            return Err(io::Error::last_os_error().to_string());
        }
        // The window owns this strong reference until WM_NCDESTROY. Passing
        // the raw pointer through CREATESTRUCT keeps the Rust surface alive
        // for every message the native child can receive.
        let retained = Rc::into_raw(Rc::clone(self));
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
            unsafe { drop(Rc::from_raw(retained)) };
            return Err(io::Error::last_os_error().to_string());
        }
        let host_close_hooks = match install_host_close_hooks(host, input) {
            Ok(hooks) => hooks,
            Err(error) => {
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
        let settings = BrowserSettings {
            background_color: 0,
            windowless_frame_rate: 60,
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
            unsafe { ShowWindow(input, SW_SHOW) };
        }
    }

    pub(crate) fn destroy(&self) {
        self.closing.set(true);
        CLOSE_TARGET.store(0, Ordering::Release);
        HOOKED_HOST.store(0, Ordering::Release);
        self.host_close_hooks
            .replace(HostCloseHooks::default())
            .uninstall();
        let input = self.input.replace(null_mut());
        if !input.is_null() && unsafe { IsWindow(input) } != 0 {
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
        if host.is_null() || unsafe { IsWindow(host) } == 0 {
            self.close_browser();
            return false;
        }
        let input = self.input.get();
        if input.is_null() || unsafe { IsWindow(input) } == 0 {
            self.close_browser();
            return false;
        }
        let Some(metrics) = sample_metrics(host) else {
            return true;
        };
        let previous = self.metrics.replace(metrics);
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

    fn close_browser(&self) {
        if self.closing.replace(true) {
            return;
        }
        self.with_browser_host(|browser_host| browser_host.close_browser(1));
    }

    fn handle_playback_right_button(&self, window: HWND, message: u32) -> bool {
        let snapshot = self.playback.snapshot();
        if !snapshot.active {
            return false;
        }
        if matches!(message, WM_RBUTTONDOWN | WM_RBUTTONDBLCLK) {
            unsafe {
                SetFocus(window);
                SetCapture(window);
            }
        } else if message == WM_RBUTTONUP {
            unsafe { ReleaseCapture() };
        }
        if let Some(pause) = right_button_pause(message, snapshot.paused) {
            self.playback.control(PlayerCommand::SetPause(pause));
        }
        true
    }

    fn handle_playback_key(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
        let Some(key) = playback_key(message, wparam) else {
            return false;
        };
        let captured = self.captured_playback_keys.get();
        match message {
            WM_KEYDOWN => {
                if !self.playback.snapshot().active {
                    return false;
                }
                self.captured_playback_keys.set(captured | key.mask());
                if !is_repeated_key(lparam) {
                    self.playback.control(key.command());
                }
                true
            }
            WM_KEYUP if captured & key.mask() != 0 => {
                self.captured_playback_keys.set(captured & !key.mask());
                true
            }
            WM_CHAR => captured & key.mask() != 0,
            _ => false,
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

impl Drop for PrototypeOsrSurface {
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
        surface: Rc<PrototypeOsrSurface>,
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
    struct PrototypeRenderHandler {
        surface: Rc<PrototypeOsrSurface>,
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
        let module = unsafe { GetModuleHandleW(null()) };
        if module.is_null() {
            return false;
        }
        let class = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(input_wndproc),
            hInstance: module,
            hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
            lpszClassName: INPUT_CLASS.as_ptr(),
            ..WNDCLASSW::default()
        };
        unsafe { RegisterClassW(&class) != 0 }
    })
}

fn install_host_close_hooks(host: HWND, input: HWND) -> io::Result<HostCloseHooks> {
    let thread_id = unsafe { GetWindowThreadProcessId(host, null_mut()) };
    if thread_id == 0 {
        return Err(io::Error::last_os_error());
    }
    HOOKED_HOST.store(host as usize, Ordering::Release);
    CLOSE_TARGET.store(input as usize, Ordering::Release);
    let dispatched = unsafe {
        SetWindowsHookExW(
            WH_CALLWNDPROCRET,
            Some(dispatched_host_message_hook),
            null_mut(),
            thread_id,
        )
    };
    if dispatched.is_null() {
        CLOSE_TARGET.store(0, Ordering::Release);
        HOOKED_HOST.store(0, Ordering::Release);
        return Err(io::Error::last_os_error());
    }
    let queued = unsafe {
        SetWindowsHookExW(
            WH_GETMESSAGE,
            Some(queued_host_message_hook),
            null_mut(),
            thread_id,
        )
    };
    if queued.is_null() {
        HostCloseHooks { dispatched, queued }.uninstall();
        CLOSE_TARGET.store(0, Ordering::Release);
        HOOKED_HOST.store(0, Ordering::Release);
        return Err(io::Error::last_os_error());
    }
    Ok(HostCloseHooks { dispatched, queued })
}

unsafe extern "system" fn dispatched_host_message_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && lparam != 0 {
        let message = unsafe { &*(lparam as *const CWPRETSTRUCT) };
        forward_host_close(message.hwnd, message.message);
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn queued_host_message_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && lparam != 0 {
        let message = unsafe { &*(lparam as *const MSG) };
        forward_host_close(message.hwnd, message.message);
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

fn forward_host_close(window: HWND, message: u32) {
    let host = HOOKED_HOST.load(Ordering::Acquire) as HWND;
    if host.is_null() || window != host || message != WM_CLOSE {
        return;
    }
    let input = CLOSE_TARGET.load(Ordering::Acquire) as HWND;
    if !input.is_null() {
        unsafe { PostMessageW(input, WM_HOST_CLOSE_REQUESTED, 0, 0) };
    }
}

unsafe extern "system" fn input_wndproc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        // SAFETY: CreateWindowExW supplied a live PrototypeOsrSurface pointer
        // in lpCreateParams; the window-owned Rc keeps it alive until destroy.
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    let surface_ptr =
        unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const PrototypeOsrSurface;
    if surface_ptr.is_null() {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    let surface = unsafe { &*surface_ptr };
    let handled = dispatch_window_message(surface, window, message, wparam, lparam);
    if message == WM_NCDESTROY {
        // Reclaim exactly the strong reference transferred by Rc::into_raw in
        // bind after no later window message can observe GWLP_USERDATA.
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            drop(Rc::from_raw(surface_ptr));
        }
    }
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(window, message, wparam, lparam) })
}

fn dispatch_window_message(
    surface: &PrototypeOsrSurface,
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    match message {
        WM_ERASEBKGND => Some(1),
        WM_HOST_CLOSE_REQUESTED => {
            surface.close_browser();
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
        WM_RBUTTONDOWN | WM_RBUTTONUP | WM_RBUTTONDBLCLK
            if surface.handle_playback_right_button(window, message) =>
        {
            Some(0)
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_RBUTTONUP
        | WM_RBUTTONDBLCLK | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_MBUTTONDBLCLK => {
            let mouse_up = matches!(message, WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP);
            if !mouse_up {
                unsafe {
                    SetFocus(window);
                    SetCapture(window);
                }
            } else {
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
        WM_SETFOCUS => {
            surface.with_browser_host(|host| host.set_focus(1));
            Some(0)
        }
        WM_KILLFOCUS => {
            surface.captured_playback_keys.set(0);
            surface.with_browser_host(|host| host.set_focus(0));
            Some(0)
        }
        WM_SYSKEYDOWN if is_alt_f4(wparam) => {
            let host = surface.host.get();
            if !host.is_null() {
                unsafe { PostMessageW(host, WM_CLOSE, 0, 0) };
            }
            Some(0)
        }
        WM_KEYDOWN | WM_KEYUP | WM_CHAR if surface.handle_playback_key(message, wparam, lparam) => {
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

fn sample_metrics(host: HWND) -> Option<ViewMetrics> {
    let mut rect = RECT::default();
    if unsafe { GetClientRect(host, &raw mut rect) } == 0 {
        return None;
    }
    let physical_width = rect.right - rect.left;
    let physical_height = rect.bottom - rect.top;
    if physical_width <= 0 || physical_height <= 0 {
        return None;
    }
    let mut origin = POINT { x: 0, y: 0 };
    if unsafe { ClientToScreen(host, &raw mut origin) } == 0 {
        return None;
    }
    let dpi = unsafe { GetDpiForWindow(host) }.max(DEFAULT_DPI);
    Some(ViewMetrics {
        logical_width: physical_to_logical(physical_width, dpi).max(1),
        logical_height: physical_to_logical(physical_height, dpi).max(1),
        physical_width,
        physical_height,
        screen_x: origin.x,
        screen_y: origin.y,
        dpi,
    })
}

fn paint_buffer_len(width: i32, height: i32) -> Option<usize> {
    let width = usize::try_from(width).ok()?;
    let height = usize::try_from(height).ok()?;
    width.checked_mul(height)?.checked_mul(4)
}

fn track_mouse_leave(window: HWND) {
    let mut event = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: window,
        dwHoverTime: 0,
    };
    unsafe { TrackMouseEvent(&raw mut event) };
}

fn apply_cursor(cursor: CursorType) {
    let Some(resource) = cursor_resource(cursor) else {
        unsafe { SetCursor(null_mut()) };
        return;
    };
    let handle = unsafe { LoadCursorW(null_mut(), resource) };
    unsafe { SetCursor(handle) };
}

fn cursor_resource(cursor: CursorType) -> Option<PCWSTR> {
    let resource = if cursor == CursorType::NONE {
        return None;
    } else if cursor == CursorType::CROSS {
        IDC_CROSS
    } else if matches!(
        cursor,
        CursorType::HAND | CursorType::GRAB | CursorType::GRABBING
    ) {
        IDC_HAND
    } else if matches!(cursor, CursorType::IBEAM | CursorType::VERTICALTEXT) {
        IDC_IBEAM
    } else if cursor == CursorType::WAIT {
        IDC_WAIT
    } else if cursor == CursorType::HELP {
        IDC_HELP
    } else if matches!(
        cursor,
        CursorType::EASTRESIZE
            | CursorType::WESTRESIZE
            | CursorType::EASTWESTRESIZE
            | CursorType::COLUMNRESIZE
    ) {
        IDC_SIZEWE
    } else if matches!(
        cursor,
        CursorType::NORTHRESIZE
            | CursorType::SOUTHRESIZE
            | CursorType::NORTHSOUTHRESIZE
            | CursorType::ROWRESIZE
    ) {
        IDC_SIZENS
    } else if matches!(
        cursor,
        CursorType::NORTHEASTRESIZE
            | CursorType::SOUTHWESTRESIZE
            | CursorType::NORTHEASTSOUTHWESTRESIZE
    ) {
        IDC_SIZENESW
    } else if matches!(
        cursor,
        CursorType::NORTHWESTRESIZE
            | CursorType::SOUTHEASTRESIZE
            | CursorType::NORTHWESTSOUTHEASTRESIZE
    ) {
        IDC_SIZENWSE
    } else if matches!(
        cursor,
        CursorType::MOVE
            | CursorType::MIDDLEPANNING
            | CursorType::MIDDLE_PANNING_VERTICAL
            | CursorType::MIDDLE_PANNING_HORIZONTAL
    ) {
        IDC_SIZEALL
    } else if cursor == CursorType::PROGRESS {
        IDC_APPSTARTING
    } else if matches!(cursor, CursorType::NODROP | CursorType::NOTALLOWED) {
        IDC_NO
    } else {
        IDC_ARROW
    };
    Some(resource)
}

fn is_alt_f4(wparam: WPARAM) -> bool {
    wparam == usize::from(VK_F4)
}

fn mouse_modifiers(wparam: WPARAM) -> u32 {
    let flags = wparam as u32;
    let mut modifiers = 0;
    if flags & 0x0001 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_LEFT_MOUSE_BUTTON.0 as u32;
    }
    if flags & 0x0010 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_MIDDLE_MOUSE_BUTTON.0 as u32;
    }
    if flags & 0x0002 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_RIGHT_MOUSE_BUTTON.0 as u32;
    }
    if flags & 0x0004 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_SHIFT_DOWN.0 as u32;
    }
    if flags & 0x0008 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0 as u32;
    }
    if key_down(VK_MENU) {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_ALT_DOWN.0 as u32;
    }
    modifiers
}

fn keyboard_modifiers(lparam: LPARAM) -> u32 {
    let mut modifiers = 0;
    if key_down(VK_SHIFT) {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_SHIFT_DOWN.0 as u32;
    }
    if key_down(VK_CONTROL) {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0 as u32;
    }
    if key_down(VK_MENU) {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_ALT_DOWN.0 as u32;
    }
    if unsafe { GetKeyState(VK_NUMLOCK as i32) } & 1 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_NUM_LOCK_ON.0 as u32;
    }
    if unsafe { GetKeyState(VK_CAPITAL as i32) } & 1 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_CAPS_LOCK_ON.0 as u32;
    }
    if (lparam as usize) & (1 << 30) != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_IS_REPEAT.0 as u32;
    }
    modifiers
}

fn key_event(message: u32, wparam: WPARAM, lparam: LPARAM) -> KeyEvent {
    let type_ = match message {
        WM_KEYUP | WM_SYSKEYUP => KeyEventType::KEYUP,
        WM_CHAR | WM_SYSCHAR => KeyEventType::CHAR,
        _ => KeyEventType::RAWKEYDOWN,
    };
    let character = if matches!(message, WM_CHAR | WM_SYSCHAR) {
        wparam as u16
    } else {
        0
    };
    KeyEvent {
        type_,
        modifiers: keyboard_modifiers(lparam),
        windows_key_code: wparam as i32,
        native_key_code: lparam as i32,
        is_system_key: i32::from(matches!(message, WM_SYSKEYDOWN | WM_SYSKEYUP | WM_SYSCHAR)),
        character,
        unmodified_character: character,
        ..KeyEvent::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybackKey {
    Stop,
    ToggleSubtitles,
}

impl PlaybackKey {
    const fn mask(self) -> u8 {
        match self {
            Self::Stop => CAPTURED_Q,
            Self::ToggleSubtitles => CAPTURED_V,
        }
    }

    fn command(self) -> PlayerCommand {
        match self {
            Self::Stop => PlayerCommand::Stop,
            Self::ToggleSubtitles => PlayerCommand::ToggleSubtitleVisibility,
        }
    }
}

fn playback_key(message: u32, wparam: WPARAM) -> Option<PlaybackKey> {
    if !matches!(message, WM_KEYDOWN | WM_KEYUP | WM_CHAR) {
        return None;
    }
    match wparam {
        VK_Q | 0x71 => Some(PlaybackKey::Stop),
        VK_V | 0x76 => Some(PlaybackKey::ToggleSubtitles),
        _ => None,
    }
}

fn is_repeated_key(lparam: LPARAM) -> bool {
    (lparam as usize) & (1 << 30) != 0
}

fn right_button_pause(message: u32, paused: bool) -> Option<bool> {
    (message == WM_RBUTTONDOWN).then_some(!paused)
}

fn mouse_button(message: u32) -> (MouseButtonType, i32) {
    let button = match message {
        WM_RBUTTONDOWN | WM_RBUTTONUP | WM_RBUTTONDBLCLK => MouseButtonType::RIGHT,
        WM_MBUTTONDOWN | WM_MBUTTONUP | WM_MBUTTONDBLCLK => MouseButtonType::MIDDLE,
        _ => MouseButtonType::LEFT,
    };
    let clicks = i32::from(matches!(
        message,
        WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK
    )) + 1;
    (button, clicks)
}

fn key_down(key: u16) -> bool {
    (unsafe { GetKeyState(key as i32) }) < 0
}

fn physical_to_logical(value: i32, dpi: u32) -> i32 {
    let dpi = i64::from(dpi.max(1));
    ((i64::from(value) * i64::from(DEFAULT_DPI) + dpi / 2) / dpi) as i32
}

fn logical_to_physical(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * i64::from(dpi) + i64::from(DEFAULT_DPI) / 2) / i64::from(DEFAULT_DPI))
        as i32
}

fn signed_low_word(value: isize) -> i32 {
    (value as u16 as i16) as i32
}

fn low_word(value: isize) -> u32 {
    u32::from(value as u16)
}

fn signed_high_word(value: isize) -> i32 {
    ((value >> 16) as u16 as i16) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_size_seeds_a_hidden_window_browser() {
        let metrics = ViewMetrics::from_logical_size(1280, 720);

        assert_eq!(metrics.logical_width, 1280);
        assert_eq!(metrics.logical_height, 720);
        assert_eq!(metrics.physical_width, 1280);
        assert_eq!(metrics.physical_height, 720);
        assert_eq!(metrics.dpi, DEFAULT_DPI);
    }

    #[test]
    fn dpi_conversion_round_trips_window_coordinates() {
        assert_eq!(physical_to_logical(150, 144), 100);
        assert_eq!(logical_to_physical(100, 144), 150);
    }

    #[test]
    fn cef_cursor_shapes_map_to_native_windows_cursors() {
        assert_eq!(cursor_resource(CursorType::POINTER), Some(IDC_ARROW));
        assert_eq!(cursor_resource(CursorType::HAND), Some(IDC_HAND));
        assert_eq!(cursor_resource(CursorType::IBEAM), Some(IDC_IBEAM));
        assert_eq!(cursor_resource(CursorType::EASTRESIZE), Some(IDC_SIZEWE));
        assert_eq!(cursor_resource(CursorType::NONE), None);
    }

    #[test]
    fn f4_is_the_system_close_key() {
        assert!(is_alt_f4(usize::from(VK_F4)));
        assert!(!is_alt_f4(usize::from(VK_F4) - 1));
    }

    #[test]
    fn active_playback_keys_match_mpv_controls() {
        assert_eq!(playback_key(WM_KEYDOWN, VK_Q), Some(PlaybackKey::Stop));
        assert_eq!(
            playback_key(WM_KEYDOWN, VK_V),
            Some(PlaybackKey::ToggleSubtitles)
        );
        assert_eq!(
            playback_key(WM_CHAR, b'q' as WPARAM),
            Some(PlaybackKey::Stop)
        );
        assert_eq!(playback_key(WM_SYSKEYDOWN, VK_Q), None);
    }

    #[test]
    fn held_playback_keys_do_not_repeat_commands() {
        assert!(!is_repeated_key(0));
        assert!(is_repeated_key(1 << 30));
    }

    #[test]
    fn right_button_toggles_playback_pause() {
        assert_eq!(right_button_pause(WM_RBUTTONDOWN, false), Some(true));
        assert_eq!(right_button_pause(WM_RBUTTONDOWN, true), Some(false));
        assert_eq!(right_button_pause(WM_RBUTTONUP, false), None);
    }
}
