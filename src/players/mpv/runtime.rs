use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_int, c_ulong, c_void};
use std::io;
use std::path::Path;
use std::process::Child;
#[cfg(target_os = "windows")]
use std::thread;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

use crate::playback::NativeWindowHandle;
use crate::preferences::FullscreenBehavior;

use super::ExternalMpv;

#[cfg(target_os = "linux")]
#[path = "gpu_next.rs"]
mod gpu_next;
#[cfg(target_os = "linux")]
#[path = "linux_window.rs"]
mod linux_window;

const MPV_EVENT_NONE: c_int = 0;
const MPV_EVENT_SHUTDOWN: c_int = 1;
const MPV_ERROR_OPTION_NOT_FOUND: c_int = -5;
#[cfg(target_os = "windows")]
const MPV_FORMAT_INT64: c_int = 4;
const REQUIRED_CLIENT_API_MAJOR: c_ulong = 2;
const WINDOWED_AUTOFIT: &str = "70%";
#[cfg(target_os = "windows")]
const NATIVE_WINDOW_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpvRuntimeKind {
    External,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LibmpvProfile {
    Standard,
    Svp,
}

impl LibmpvProfile {
    pub(super) fn detected() -> Self {
        #[cfg(target_os = "windows")]
        if super::svp::runtime_directory().is_some() {
            return Self::Svp;
        }
        Self::Standard
    }
}

pub(super) enum MpvRuntime {
    External(Child),
    Library(Box<LibMpvRuntime>),
}

impl MpvRuntime {
    #[cfg(target_os = "linux")]
    pub(super) fn poll_window_events(&self) {
        if let Self::Library(runtime) = self {
            runtime.window.poll_events();
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn set_fullscreen(&self, mode: FullscreenBehavior) -> io::Result<()> {
        match self {
            Self::Library(runtime) => runtime.window.set_fullscreen(mode),
            Self::External(_) => Ok(()),
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn fullscreen(&self) -> io::Result<bool> {
        match self {
            Self::Library(runtime) => runtime.window.fullscreen(),
            Self::External(_) => Ok(false),
        }
    }
    #[cfg(target_os = "linux")]
    pub(super) fn ui_exposes_video(&self) -> bool {
        match self {
            Self::Library(runtime) => runtime.ui_exposes_video,
            Self::External(_) => false,
        }
    }
    #[cfg(target_os = "linux")]
    pub(super) fn present_overlay(
        &mut self,
        frame: crate::playback::NativeOverlayFrame,
    ) -> Result<crate::playback::NativeOverlayFrame, String> {
        frame.validate()?;
        match self {
            Self::Library(runtime) => {
                runtime
                    .renderer
                    .as_ref()
                    .ok_or("native renderer is unavailable")?
                    .submit(
                        runtime.handle,
                        &frame,
                        runtime.window.content_size().map_err(|e| e.to_string())?,
                    )?;
                runtime.ui_exposes_video = frame.exposes_video();
                Ok(frame)
            }
            Self::External(_) => Err("UI composition requires built-in libmpv".into()),
        }
    }
    pub(super) fn start(
        kind: MpvRuntimeKind,
        libmpv_profile: LibmpvProfile,
        path: &Path,
        ipc_path: &str,
        fullscreen: FullscreenBehavior,
    ) -> io::Result<Self> {
        match kind {
            MpvRuntimeKind::External => {
                let mpv = ExternalMpv::new(path);
                let child = mpv
                    .command_for_idle_with_ipc_and_fullscreen(ipc_path, fullscreen)
                    .spawn()?;
                crate::windows::confine_to_app_lifetime(&child);
                Ok(Self::External(child))
            }
            MpvRuntimeKind::Library => {
                LibMpvRuntime::start(path, ipc_path, fullscreen, libmpv_profile)
                    .map(Box::new)
                    .map(Self::Library)
            }
        }
    }

    pub(super) fn native_window(&self) -> Option<NativeWindowHandle> {
        match self {
            Self::External(_) => None,
            Self::Library(runtime) => runtime.native_window,
        }
    }

    pub(super) fn is_alive(&mut self) -> io::Result<bool> {
        match self {
            Self::External(child) => child.try_wait().map(|status| status.is_none()),
            Self::Library(runtime) => Ok(runtime.is_alive()),
        }
    }

    #[cfg(windows)]
    pub(super) fn process_id(&self) -> Option<u32> {
        match self {
            Self::External(child) => Some(child.id()),
            Self::Library(_) => None,
        }
    }

    pub(super) fn stop(&mut self) {
        match self {
            Self::External(child) => {
                if matches!(child.try_wait(), Ok(None)) {
                    let _ = child.kill();
                }
                let _ = child.wait();
            }
            Self::Library(runtime) => runtime.terminate(),
        }
    }
}

type MpvCreate = unsafe extern "C" fn() -> *mut MpvHandle;
type MpvSetOptionString =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
type MpvInitialize = unsafe extern "C" fn(*mut MpvHandle) -> c_int;
#[cfg(target_os = "windows")]
type MpvGetProperty =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, c_int, *mut c_void) -> c_int;
type MpvWaitEvent = unsafe extern "C" fn(*mut MpvHandle, f64) -> *const RawMpvEvent;
type MpvTerminateDestroy = unsafe extern "C" fn(*mut MpvHandle);
type MpvClientApiVersion = unsafe extern "C" fn() -> c_ulong;
type MpvErrorString = unsafe extern "C" fn(c_int) -> *const c_char;

#[repr(C)]
struct MpvHandle {
    _private: [u8; 0],
}

#[repr(C)]
struct RawMpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

pub(super) struct LibMpvRuntime {
    handle: *mut MpvHandle,
    wait_event: MpvWaitEvent,
    terminate_destroy: MpvTerminateDestroy,
    alive: bool,
    native_window: Option<NativeWindowHandle>,
    #[cfg(target_os = "linux")]
    ui_exposes_video: bool,
    #[cfg(target_os = "linux")]
    window: linux_window::HostWindow,
    #[cfg(target_os = "linux")]
    renderer: Option<gpu_next::Compositor>,
    _library: Library,
    #[cfg(target_os = "windows")]
    _svp_environment: Option<super::svp::RuntimeEnvironment>,
}

fn configure_libmpv_options(
    handle: *mut MpvHandle,
    set_option_string: MpvSetOptionString,
    error_string: MpvErrorString,
    ipc_path: &str,
    fullscreen: FullscreenBehavior,
    libmpv_profile: LibmpvProfile,
) -> io::Result<()> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let force_window = "yes";
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let force_window = "no";
    let (load_scripts, hwdec) = match libmpv_profile {
        LibmpvProfile::Standard => ("no", "auto-safe"),
        LibmpvProfile::Svp => ("yes", "auto-copy"),
    };
    let mut options = vec![
        ("config", "no"),
        ("load-scripts", load_scripts),
        ("force-window", force_window),
        ("fullscreen", fullscreen.fullscreen_arg()),
        ("autofit", WINDOWED_AUTOFIT),
        ("hwdec", hwdec),
        ("input-default-bindings", "no"),
        ("input-vo-keyboard", "no"),
        ("input-cursor", "no"),
        ("cursor-autohide", "no"),
        ("idle", "yes"),
        ("input-ipc-server", ipc_path),
        ("title", "MediaFlick Desktop"),
        ("keepaspect-window", "no"),
        ("auto-window-resize", "no"),
        ("border", "yes"),
    ];
    #[cfg(target_os = "linux")]
    options.extend([
        // gpu-next owns a child surface inside the app's X11 container and
        // composites CEF bitmaps itself. No mpv_render_context is created.
        ("vo", "gpu-next"),
        ("gpu-api", "vulkan,opengl"),
        ("gpu-context", "x11vk,x11egl"),
        ("gpu-sw", "yes"),
        ("osd-level", "0"),
        ("x11-name", "io.github.phob.MediaFlickDesktop"),
        ("osc", "no"),
    ]);
    if libmpv_profile == LibmpvProfile::Svp {
        options.extend([
            ("hwdec-codecs", "all"),
            ("hr-seek-framedrop", "no"),
            ("resume-playback", "no"),
        ]);
    }
    for (name, value) in options {
        set_option(handle, set_option_string, error_string, name, value)?;
    }
    Ok(())
}

impl LibMpvRuntime {
    fn start(
        path: &Path,
        ipc_path: &str,
        fullscreen: FullscreenBehavior,
        libmpv_profile: LibmpvProfile,
    ) -> io::Result<Self> {
        #[cfg(target_os = "windows")]
        let svp_environment = if libmpv_profile == LibmpvProfile::Svp {
            super::svp::prepare_runtime_environment()?
        } else {
            None
        };

        // SAFETY: the resolved symbols below use libmpv's published C ABI,
        // and `_library` keeps the DLL loaded for every copied function pointer.
        let library = unsafe { Library::new(path) }.map_err(io::Error::other)?;
        let create: MpvCreate = load_symbol(&library, b"mpv_create\0")?;
        let set_option_string: MpvSetOptionString =
            load_symbol(&library, b"mpv_set_option_string\0")?;
        let initialize: MpvInitialize = load_symbol(&library, b"mpv_initialize\0")?;
        let wait_event: MpvWaitEvent = load_symbol(&library, b"mpv_wait_event\0")?;
        let terminate_destroy: MpvTerminateDestroy =
            load_symbol(&library, b"mpv_terminate_destroy\0")?;
        let client_api_version: MpvClientApiVersion =
            load_symbol(&library, b"mpv_client_api_version\0")?;
        let error_string: MpvErrorString = load_symbol(&library, b"mpv_error_string\0")?;
        #[cfg(target_os = "windows")]
        let get_property: MpvGetProperty = load_symbol(&library, b"mpv_get_property\0")?;

        // SAFETY: mpv_client_api_version takes no arguments and only returns a value.
        let (major, minor) = validate_client_api(unsafe { client_api_version() })?;

        #[cfg(target_os = "linux")]
        let window = linux_window::HostWindow::new()?;
        #[cfg(target_os = "linux")]
        let host_handle = window.handle()?;
        #[cfg(target_os = "linux")]
        let window_id = host_handle.content().to_string();
        // SAFETY: mpv_create takes no arguments; a null result is handled below.
        let handle = unsafe { create() };
        if handle.is_null() {
            return Err(io::Error::other("libmpv could not create a client handle"));
        }

        let configured = configure_libmpv_options(
            handle,
            set_option_string,
            error_string,
            ipc_path,
            fullscreen,
            libmpv_profile,
        );
        #[cfg(target_os = "linux")]
        let configured = configured
            .and_then(|()| set_option(handle, set_option_string, error_string, "wid", &window_id));
        if let Err(error) = configured {
            // SAFETY: `handle` is the live handle created above, destroyed once
            // before it is dropped on this error path.
            unsafe { terminate_destroy(handle) };
            return Err(error);
        }

        // SAFETY: `handle` is the live, configured handle created above.
        let status = unsafe { initialize(handle) };
        if status < 0 {
            let message = mpv_error(error_string, status);
            // SAFETY: `handle` is live and destroyed once on this error path.
            unsafe { terminate_destroy(handle) };
            return Err(io::Error::other(format!(
                "libmpv initialization failed: {message}"
            )));
        }

        #[cfg(target_os = "linux")]
        let renderer = match gpu_next::Compositor::new(&library, handle) {
            Ok(renderer) => Some(renderer),
            Err(error) => {
                // SAFETY: `handle` is live and destroyed once on this error path.
                unsafe { terminate_destroy(handle) };
                return Err(error);
            }
        };
        #[cfg(target_os = "linux")]
        let native_window = Some(host_handle);
        #[cfg(target_os = "windows")]
        let native_window = match wait_for_native_window(handle, get_property) {
            Ok(player_window) => Some(player_window),
            Err(error) => {
                // SAFETY: `handle` is live and destroyed once on this error path.
                unsafe { terminate_destroy(handle) };
                return Err(error);
            }
        };
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        let native_window = None;

        tracing::info!(
            target: "mpv.library",
            path = %path.display(),
            client_api = %format_args!("{major}.{minor}"),
            "initialized libmpv"
        );
        Ok(Self {
            handle,
            wait_event,
            terminate_destroy,
            alive: true,
            native_window,
            #[cfg(target_os = "linux")]
            ui_exposes_video: false,
            #[cfg(target_os = "linux")]
            window,
            #[cfg(target_os = "linux")]
            renderer,
            _library: library,
            #[cfg(target_os = "windows")]
            _svp_environment: svp_environment,
        })
    }

    fn is_alive(&mut self) -> bool {
        if !self.alive || self.handle.is_null() {
            return false;
        }
        loop {
            // SAFETY: `handle` remains owned by this runtime, and libmpv keeps
            // the returned event valid until the next mpv_wait_event call.
            let event = unsafe { (self.wait_event)(self.handle, 0.0) };
            if event.is_null() {
                self.alive = false;
                break;
            }
            // SAFETY: `event` is non-null and valid until the next wait call.
            let event_id = unsafe { (*event).event_id };
            match event_id {
                MPV_EVENT_NONE => break,
                MPV_EVENT_SHUTDOWN => {
                    self.alive = false;
                    break;
                }
                _ => {}
            }
        }
        self.alive
    }

    fn terminate(&mut self) {
        if self.handle.is_null() {
            return;
        }
        #[cfg(target_os = "linux")]
        self.renderer.take();
        let handle = std::mem::replace(&mut self.handle, std::ptr::null_mut());
        self.alive = false;
        // SAFETY: `handle` was taken out of the runtime, which now holds null,
        // so it is destroyed exactly once.
        unsafe { (self.terminate_destroy)(handle) };
    }
}

#[cfg(target_os = "windows")]
fn wait_for_native_window(
    handle: *mut MpvHandle,
    get_property: MpvGetProperty,
) -> io::Result<NativeWindowHandle> {
    let property = CString::new("window-id").map_err(io::Error::other)?;
    let deadline = Instant::now() + NATIVE_WINDOW_TIMEOUT;
    loop {
        let mut raw = 0_i64;
        // SAFETY: libmpv writes one MPV_FORMAT_INT64 into `raw`; `handle` and
        // the property string both remain valid for the duration of the call.
        let status = unsafe {
            get_property(
                handle,
                property.as_ptr(),
                MPV_FORMAT_INT64,
                std::ptr::from_mut(&mut raw).cast(),
            )
        };
        if status >= 0
            && let Ok(raw) = usize::try_from(raw)
            && let Some(window) = NativeWindowHandle::new(raw)
        {
            tracing::info!(target: "mpv.library", window_id = raw, "libmpv native window is ready");
            return Ok(window);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "libmpv did not publish its native window within 10 seconds",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

impl Drop for LibMpvRuntime {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn validate_client_api(version: c_ulong) -> io::Result<(c_ulong, c_ulong)> {
    let major = version >> 16;
    let minor = version & 0xffff;
    if major != REQUIRED_CLIENT_API_MAJOR {
        return Err(io::Error::other(format!(
            "unsupported libmpv client API {major}.{minor}; expected major {REQUIRED_CLIENT_API_MAJOR}"
        )));
    }
    Ok((major, minor))
}

fn load_symbol<T>(library: &Library, name: &[u8]) -> io::Result<T>
where
    T: Copy,
{
    // SAFETY: callers request a named libmpv symbol with its exact published
    // function-pointer type, and the owning Library outlives the result.
    let symbol = unsafe { library.get::<T>(name) }.map_err(io::Error::other)?;
    Ok(*symbol)
}

fn set_option(
    handle: *mut MpvHandle,
    set_option_string: MpvSetOptionString,
    error_string: MpvErrorString,
    name: &str,
    value: &str,
) -> io::Result<()> {
    let c_name = CString::new(name).map_err(io::Error::other)?;
    let c_value = CString::new(value).map_err(io::Error::other)?;
    // SAFETY: `handle` is a live, uninitialized handle, and both strings are
    // NUL-terminated and outlive the call; libmpv copies what it keeps.
    let status = unsafe { set_option_string(handle, c_name.as_ptr(), c_value.as_ptr()) };
    // Lean builds omit these options together with their scripting engines.
    // Disabling an absent engine is already satisfied; enabling it (SVP), or
    // any other failure, must still reach the normal startup error boundary.
    if status == MPV_ERROR_OPTION_NOT_FOUND
        && matches!((name, value), ("load-scripts" | "osc", "no"))
    {
        return Ok(());
    }
    if status < 0 {
        return Err(io::Error::other(format!(
            "libmpv rejected option {name}: {}",
            mpv_error(error_string, status)
        )));
    }
    Ok(())
}

fn mpv_error(error_string: MpvErrorString, status: c_int) -> String {
    // SAFETY: mpv_error_string accepts any status value.
    let message = unsafe { error_string(status) };
    if message.is_null() {
        return format!("error {status}");
    }
    // SAFETY: a non-null result is a static, NUL-terminated string in libmpv.
    unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(target_os = "linux")]
static WINDOW_CLOSE_REQUESTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "linux")]
pub(super) fn request_window_close() {
    WINDOW_CLOSE_REQUESTED.store(true, std::sync::atomic::Ordering::Release);
}

#[cfg(target_os = "linux")]
pub(crate) fn take_window_close_request() -> bool {
    WINDOW_CLOSE_REQUESTED.swap(false, std::sync::atomic::Ordering::AcqRel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    use std::time::{Duration, Instant};

    #[test]
    fn client_api_accepts_supported_major_with_any_minor() {
        for minor in [0, 1, 0xffff] {
            let version = (2 << 16) | minor;
            assert_eq!(
                validate_client_api(version).expect("supported API"),
                (2, minor)
            );
        }
    }

    #[test]
    fn client_api_rejects_incompatible_majors() {
        for major in [0, 1, 3, 0xffff] {
            let version = (major << 16) | 42;
            let error = validate_client_api(version).expect_err("unsupported API");
            assert_eq!(
                error.to_string(),
                format!("unsupported libmpv client API {major}.42; expected major 2")
            );
        }
    }

    unsafe extern "C" fn no_scripting_options(
        _handle: *mut MpvHandle,
        name: *const c_char,
        _value: *const c_char,
    ) -> c_int {
        // SAFETY: configure_libmpv_options passes a live, terminated CString.
        match unsafe { CStr::from_ptr(name) }.to_bytes() {
            b"load-scripts" | b"osc" => MPV_ERROR_OPTION_NOT_FOUND,
            _ => 0,
        }
    }

    unsafe extern "C" fn missing_option(
        _handle: *mut MpvHandle,
        _name: *const c_char,
        _value: *const c_char,
    ) -> c_int {
        MPV_ERROR_OPTION_NOT_FOUND
    }

    unsafe extern "C" fn invalid_option(
        _handle: *mut MpvHandle,
        _name: *const c_char,
        _value: *const c_char,
    ) -> c_int {
        -7 // MPV_ERROR_OPTION_ERROR
    }

    unsafe extern "C" fn test_error(_status: c_int) -> *const c_char {
        c"test option error".as_ptr()
    }

    #[test]
    fn standard_profile_initializes_without_scripting_but_svp_requires_it() {
        let configure = |profile| {
            configure_libmpv_options(
                std::ptr::null_mut(),
                no_scripting_options,
                test_error,
                "test-ipc",
                FullscreenBehavior::Windowed,
                profile,
            )
        };
        assert!(configure(LibmpvProfile::Standard).is_ok());
        assert!(configure(LibmpvProfile::Svp).is_err());
    }

    #[test]
    fn missing_required_options_and_invalid_script_options_still_fail() {
        assert!(
            configure_libmpv_options(
                std::ptr::null_mut(),
                missing_option,
                test_error,
                "test-ipc",
                FullscreenBehavior::Windowed,
                LibmpvProfile::Standard,
            )
            .is_err()
        );
        assert!(
            set_option(
                std::ptr::null_mut(),
                invalid_option,
                test_error,
                "load-scripts",
                "no",
            )
            .is_err()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires MEDIAFLICK_DESKTOP_LIBMPV_PATH"]
    fn configured_svp_library_initializes_the_svp_pipe() {
        let path = std::env::var_os("MEDIAFLICK_DESKTOP_LIBMPV_PATH")
            .expect("MEDIAFLICK_DESKTOP_LIBMPV_PATH must point to libmpv");
        let ipc_path = r"\\.\pipe\mpvpipe";
        let mut runtime = MpvRuntime::start(
            MpvRuntimeKind::Library,
            LibmpvProfile::Svp,
            Path::new(&path),
            ipc_path,
            FullscreenBehavior::Windowed,
        )
        .expect("initialize SVP-compatible libmpv");
        let shutdown = AtomicBool::new(false);
        let (worker, _) = crate::players::mpv::ipc::start_ipc_worker(
            ipc_path,
            Duration::from_secs(5),
            &shutdown,
            || runtime.is_alive(),
        )
        .expect("connect to the SVP mpv pipe");

        assert!(runtime.is_alive().expect("poll SVP-compatible libmpv"));
        runtime.stop();
        worker.shutdown();
    }

    #[test]
    #[ignore = "requires MEDIAFLICK_DESKTOP_LIBMPV_PATH"]
    fn configured_library_initializes_its_ipc_server() {
        let path = std::env::var_os("MEDIAFLICK_DESKTOP_LIBMPV_PATH")
            .expect("MEDIAFLICK_DESKTOP_LIBMPV_PATH must point to libmpv");
        let ipc_path = crate::players::mpv::ipc::make_ipc_path();
        let mut runtime = MpvRuntime::start(
            MpvRuntimeKind::Library,
            LibmpvProfile::Standard,
            Path::new(&path),
            &ipc_path,
            FullscreenBehavior::Windowed,
        )
        .expect("initialize configured libmpv");
        let shutdown = AtomicBool::new(false);
        let (worker, events) = crate::players::mpv::ipc::start_ipc_worker(
            &ipc_path,
            Duration::from_secs(5),
            &shutdown,
            || runtime.is_alive(),
        )
        .expect("connect to libmpv IPC");

        assert!(runtime.is_alive().expect("poll libmpv"));
        if let Some(media_path) = std::env::var_os("MEDIAFLICK_DESKTOP_LIBMPV_MEDIA_PATH") {
            assert_media_playback(&mut runtime, &worker, &events, Path::new(&media_path));
        }
        runtime.stop();
        worker.shutdown();
    }

    fn assert_media_playback(
        runtime: &mut MpvRuntime,
        worker: &crate::players::mpv::ipc::IpcWorker,
        events: &Receiver<crate::players::mpv::ipc::MpvEvent>,
        media_path: &Path,
    ) {
        worker
            .send_with_timeout(
                json!({
                    "command": ["loadfile", media_path.to_string_lossy()],
                    "request_id": 9_001,
                }),
                Duration::from_secs(5),
            )
            .expect("load smoke-test media");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut loaded = false;
        let mut advanced = false;
        while Instant::now() < deadline && !(loaded && advanced) {
            assert!(runtime.is_alive().expect("poll libmpv during playback"));
            match events.recv_timeout(Duration::from_millis(250)) {
                Ok(event) if event.name == "file-loaded" => loaded = true,
                Ok(event)
                    if event.name == "property-change"
                        && matches!(
                            event.property.as_deref(),
                            Some("time-pos" | "playback-time")
                        ) =>
                {
                    advanced = event
                        .data
                        .as_ref()
                        .and_then(serde_json::Value::as_f64)
                        .is_some_and(|position| position > 0.05);
                }
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("libmpv IPC events disconnected during playback")
                }
            }
        }
        assert!(loaded, "libmpv did not report file-loaded");
        assert!(advanced, "libmpv playback time did not advance");
    }
}
