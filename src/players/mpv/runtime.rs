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
mod gpu_next;
#[cfg(target_os = "linux")]
pub(crate) mod helper;
#[cfg(target_os = "linux")]
pub(super) mod linux_window;
#[cfg(target_os = "linux")]
pub(crate) mod wayland;

#[cfg(target_os = "linux")]
pub(crate) use helper::{is_player_helper_invocation, run_player_helper};

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
pub(crate) enum LibmpvProfile {
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
    #[cfg(target_os = "linux")]
    LibraryHelper(Box<helper::HelperRuntime>),
    #[cfg(test)]
    TestRuntime {
        alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
        stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
        native_window: Option<NativeWindowHandle>,
    },
}

impl MpvRuntime {
    #[cfg(target_os = "linux")]
    pub(super) fn ui_exposes_video(&self) -> bool {
        match self {
            Self::Library(runtime) => runtime.ui_exposes_video,
            Self::LibraryHelper(runtime) => runtime.ui_exposes_video,
            Self::External(_) => false,
            #[cfg(test)]
            Self::TestRuntime { .. } => false,
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn present_helper_overlay(
        &mut self,
        frame: crate::playback::NativeOverlayFrame,
        display: (u16, u16),
    ) -> Result<crate::playback::NativeOverlayFrame, helper::HelperOverlayError> {
        match self {
            Self::LibraryHelper(runtime) => runtime.present_overlay(frame, display),
            Self::Library(runtime) => {
                runtime
                    .renderer
                    .as_ref()
                    .ok_or_else(|| {
                        helper::HelperOverlayError::Composition(
                            "native renderer is unavailable".into(),
                        )
                    })?
                    .submit(runtime.handle, &frame, display)
                    .map_err(helper::HelperOverlayError::Composition)?;
                runtime.ui_exposes_video = frame.exposes_video();
                Ok(frame)
            }
            Self::External(_) => Err(helper::HelperOverlayError::Composition(
                "UI composition requires built-in libmpv".into(),
            )),
            #[cfg(test)]
            Self::TestRuntime { .. } => Ok(frame),
        }
    }

    #[cfg(all(target_os = "linux", test))]
    pub(super) fn present_overlay(
        &mut self,
        frame: crate::playback::NativeOverlayFrame,
    ) -> Result<crate::playback::NativeOverlayFrame, String> {
        frame.validate()?;
        match self {
            Self::Library(runtime) => {
                let display = runtime
                    ._window
                    .as_ref()
                    .ok_or("native host window is unavailable")?
                    .content_size()
                    .map_err(|e| e.to_string())?;
                runtime
                    .renderer
                    .as_ref()
                    .ok_or("native renderer is unavailable")?
                    .submit(runtime.handle, &frame, display)?;
                runtime.ui_exposes_video = frame.exposes_video();
                Ok(frame)
            }
            Self::LibraryHelper(runtime) => runtime
                .present_overlay(frame, (1280, 720))
                .map_err(|e| format!("{e:?}")),
            Self::External(_) => Err("UI composition requires built-in libmpv".into()),
            Self::TestRuntime { .. } => Ok(frame),
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn start_linux_helper(
        path: &Path,
        ipc_path: &str,
        wid: usize,
        fullscreen: FullscreenBehavior,
        libmpv_profile: LibmpvProfile,
    ) -> io::Result<Self> {
        helper::HelperRuntime::start(path, ipc_path, wid, fullscreen, libmpv_profile)
            .map(Box::new)
            .map(Self::LibraryHelper)
    }

    #[cfg(target_os = "linux")]
    pub(super) fn start_linux_wayland_helper(
        path: &Path,
        ipc_path: &str,
        wayland_socket: std::os::fd::OwnedFd,
        fullscreen: FullscreenBehavior,
        libmpv_profile: LibmpvProfile,
    ) -> io::Result<Self> {
        helper::HelperRuntime::start_wayland(
            path,
            ipc_path,
            wayland_socket,
            fullscreen,
            libmpv_profile,
        )
        .map(Box::new)
        .map(Self::LibraryHelper)
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
            #[cfg(target_os = "linux")]
            Self::LibraryHelper(_) => None,
            #[cfg(test)]
            Self::TestRuntime { native_window, .. } => *native_window,
        }
    }

    pub(super) fn is_alive(&mut self) -> io::Result<bool> {
        match self {
            Self::External(child) => child.try_wait().map(|status| status.is_none()),
            Self::Library(runtime) => Ok(runtime.is_alive()),
            #[cfg(target_os = "linux")]
            Self::LibraryHelper(runtime) => runtime.is_alive(),
            #[cfg(test)]
            Self::TestRuntime { alive, .. } => Ok(alive.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }

    #[cfg(windows)]
    pub(super) fn process_id(&self) -> Option<u32> {
        match self {
            Self::External(child) => Some(child.id()),
            Self::Library(_) => None,
            #[cfg(test)]
            Self::TestRuntime { .. } => None,
        }
    }

    pub(super) fn request_graceful_stop(&mut self) {
        #[cfg(target_os = "linux")]
        if let Self::LibraryHelper(runtime) = self {
            runtime.request_graceful_stop();
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
            #[cfg(target_os = "linux")]
            Self::LibraryHelper(runtime) => runtime.stop(),
            #[cfg(test)]
            Self::TestRuntime { alive, stopped, .. } => {
                alive.store(false, std::sync::atomic::Ordering::SeqCst);
                stopped.store(true, std::sync::atomic::Ordering::SeqCst);
            }
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

#[derive(Clone, Copy)]
struct LibMpvStartSpec<'a> {
    path: &'a Path,
    ipc_path: &'a str,
    fullscreen: FullscreenBehavior,
    libmpv_profile: LibmpvProfile,
    #[cfg(target_os = "linux")]
    linux_wid: Option<usize>,
    #[cfg(target_os = "linux")]
    use_wayland: bool,
    #[cfg(target_os = "linux")]
    native_window: Option<NativeWindowHandle>,
}

struct MpvSymbols {
    create: MpvCreate,
    set_option_string: MpvSetOptionString,
    initialize: MpvInitialize,
    #[cfg(target_os = "windows")]
    get_property: MpvGetProperty,
    wait_event: MpvWaitEvent,
    terminate_destroy: MpvTerminateDestroy,
    client_api_version: MpvClientApiVersion,
    error_string: MpvErrorString,
}

impl MpvSymbols {
    fn load(library: &Library) -> io::Result<Self> {
        Ok(Self {
            create: load_symbol(library, b"mpv_create\0")?,
            set_option_string: load_symbol(library, b"mpv_set_option_string\0")?,
            initialize: load_symbol(library, b"mpv_initialize\0")?,
            #[cfg(target_os = "windows")]
            get_property: load_symbol(library, b"mpv_get_property\0")?,
            wait_event: load_symbol(library, b"mpv_wait_event\0")?,
            terminate_destroy: load_symbol(library, b"mpv_terminate_destroy\0")?,
            client_api_version: load_symbol(library, b"mpv_client_api_version\0")?,
            error_string: load_symbol(library, b"mpv_error_string\0")?,
        })
    }
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
    _window: Option<linux_window::HostWindow>,
    #[cfg(target_os = "linux")]
    renderer: Option<gpu_next::Compositor>,
    library: Option<Library>,
    #[cfg(target_os = "windows")]
    _svp_environment: Option<super::svp::RuntimeEnvironment>,
}

fn default_platform_options(
    target_windows: bool,
    target_linux: bool,
) -> &'static [(&'static str, &'static str)] {
    if target_windows {
        &[
            ("vo", "gpu-next"),
            ("gpu-api", "d3d11,vulkan"),
            ("gpu-context", "d3d11,winvk"),
            ("d3d11-flip", "yes"),
            ("target-colorspace-hint", "auto"),
            ("osd-level", "0"),
            ("osc", "no"),
        ]
    } else if target_linux {
        &[
            ("vo", "gpu-next"),
            ("gpu-api", "vulkan,opengl"),
            ("gpu-context", "x11vk,x11egl"),
            ("gpu-sw", "yes"),
            ("osd-level", "0"),
            ("x11-name", "io.github.phob.MediaFlickDesktop"),
            ("osc", "no"),
        ]
    } else {
        &[]
    }
}

#[cfg(any(target_os = "linux", test))]
const fn wayland_platform_options() -> &'static [(&'static str, &'static str)] {
    &[
        ("vo", "gpu-next"),
        ("gpu-api", "vulkan,opengl"),
        ("gpu-context", "waylandvk,wayland"),
        ("ao", "pulse,alsa"),
        ("target-colorspace-hint", "auto"),
        ("gpu-sw", "yes"),
        ("osd-level", "0"),
        ("osc", "no"),
        ("clipboard-backends", "none"),
        ("clipboard-monitor", "no"),
    ]
}

fn configure_libmpv_options(
    handle: *mut MpvHandle,
    set_option_string: MpvSetOptionString,
    error_string: MpvErrorString,
    ipc_path: &str,
    fullscreen: FullscreenBehavior,
    libmpv_profile: LibmpvProfile,
    #[cfg(target_os = "linux")] use_wayland: bool,
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
    let platform = if use_wayland {
        wayland_platform_options()
    } else {
        default_platform_options(false, true)
    };
    #[cfg(not(target_os = "linux"))]
    let platform = default_platform_options(cfg!(target_os = "windows"), false);
    options.extend_from_slice(platform);
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
        #[cfg(target_os = "linux")]
        let window = Some(linux_window::HostWindow::new()?);
        #[cfg(target_os = "linux")]
        let host_handle = window
            .as_ref()
            .ok_or_else(|| io::Error::other("native host window missing"))?
            .handle()?;
        #[cfg(target_os = "linux")]
        let wid = host_handle.content();
        #[cfg(target_os = "linux")]
        let native_window = Some(host_handle);

        Self::start_with_linux_spec(
            &LibMpvStartSpec {
                path,
                ipc_path,
                fullscreen,
                libmpv_profile,
                #[cfg(target_os = "linux")]
                linux_wid: Some(wid),
                #[cfg(target_os = "linux")]
                use_wayland: false,
                #[cfg(target_os = "linux")]
                native_window,
            },
            #[cfg(target_os = "linux")]
            window,
        )
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn start_embedded_linux(
        path: &Path,
        ipc_path: &str,
        wid: usize,
        fullscreen: FullscreenBehavior,
        libmpv_profile: LibmpvProfile,
    ) -> io::Result<Self> {
        Self::start_with_linux_spec(
            &LibMpvStartSpec {
                path,
                ipc_path,
                fullscreen,
                libmpv_profile,
                linux_wid: Some(wid),
                use_wayland: false,
                native_window: None,
            },
            None,
        )
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn start_wayland_linux(
        path: &Path,
        ipc_path: &str,
        fullscreen: FullscreenBehavior,
        libmpv_profile: LibmpvProfile,
    ) -> io::Result<Self> {
        Self::start_with_linux_spec(
            &LibMpvStartSpec {
                path,
                ipc_path,
                fullscreen,
                libmpv_profile,
                linux_wid: None,
                use_wayland: true,
                native_window: None,
            },
            None,
        )
    }

    fn create_and_init_handle(
        spec: &LibMpvStartSpec<'_>,
        syms: &MpvSymbols,
        #[cfg(target_os = "linux")] window_id: Option<&str>,
    ) -> io::Result<*mut MpvHandle> {
        // SAFETY: mpv_create takes no arguments; a null result is handled below.
        let handle = unsafe { (syms.create)() };
        if handle.is_null() {
            return Err(io::Error::other("libmpv could not create a client handle"));
        }
        let configured = configure_libmpv_options(
            handle,
            syms.set_option_string,
            syms.error_string,
            spec.ipc_path,
            spec.fullscreen,
            spec.libmpv_profile,
            #[cfg(target_os = "linux")]
            spec.use_wayland,
        );
        #[cfg(target_os = "linux")]
        let configured = configured.and_then(|()| {
            if let Some(wid) = window_id {
                set_option(
                    handle,
                    syms.set_option_string,
                    syms.error_string,
                    "wid",
                    wid,
                )
            } else {
                Ok(())
            }
        });
        if let Err(error) = configured {
            // SAFETY: `handle` is live and destroyed once on this error path.
            unsafe { (syms.terminate_destroy)(handle) };
            return Err(error);
        }
        // SAFETY: `handle` is the live, configured handle created above.
        let status = unsafe { (syms.initialize)(handle) };
        if status < 0 {
            let message = mpv_error(syms.error_string, status);
            // SAFETY: `handle` is live and destroyed once on this error path.
            unsafe { (syms.terminate_destroy)(handle) };
            return Err(io::Error::other(format!(
                "libmpv initialization failed: {message}"
            )));
        }
        Ok(handle)
    }

    fn start_with_linux_spec(
        spec: &LibMpvStartSpec<'_>,
        #[cfg(target_os = "linux")] window: Option<linux_window::HostWindow>,
    ) -> io::Result<Self> {
        #[cfg(target_os = "windows")]
        let svp_environment = if spec.libmpv_profile == LibmpvProfile::Svp {
            super::svp::prepare_runtime_environment()?
        } else {
            None
        };
        // SAFETY: the resolved symbols below use libmpv's published C ABI,
        // and `library` keeps the DLL loaded for every copied function pointer.
        let library = unsafe { Library::new(spec.path) }.map_err(io::Error::other)?;
        let syms = MpvSymbols::load(&library)?;
        // SAFETY: mpv_client_api_version takes no arguments and only returns a value.
        let (major, minor) = validate_client_api(unsafe { (syms.client_api_version)() })?;
        #[cfg(target_os = "linux")]
        let window_id = if spec.use_wayland {
            None
        } else {
            Some(
                spec.linux_wid
                    .ok_or_else(|| io::Error::other("missing X11 content window id"))?
                    .to_string(),
            )
        };

        let handle = Self::create_and_init_handle(
            spec,
            &syms,
            #[cfg(target_os = "linux")]
            window_id.as_deref(),
        )?;
        #[cfg(target_os = "linux")]
        let (renderer, native_window) = match gpu_next::Compositor::new(&library, handle) {
            Ok(renderer) => (Some(renderer), spec.native_window),
            Err(error) => {
                // SAFETY: `handle` is live and destroyed once on this error path.
                unsafe { (syms.terminate_destroy)(handle) };
                return Err(error);
            }
        };
        #[cfg(target_os = "windows")]
        let native_window = match wait_for_native_window(handle, syms.get_property) {
            Ok(player_window) => Some(player_window),
            Err(error) => {
                // SAFETY: `handle` is live and destroyed once on this error path.
                unsafe { (syms.terminate_destroy)(handle) };
                return Err(error);
            }
        };
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        let native_window = None;

        tracing::info!(
            target: "mpv.library",
            path = %spec.path.display(),
            client_api = %format_args!("{major}.{minor}"),
            "initialized libmpv"
        );
        Ok(Self {
            handle,
            wait_event: syms.wait_event,
            terminate_destroy: syms.terminate_destroy,
            alive: true,
            native_window,
            #[cfg(target_os = "linux")]
            ui_exposes_video: false,
            #[cfg(target_os = "linux")]
            _window: window,
            #[cfg(target_os = "linux")]
            renderer,
            library: Some(library),
            #[cfg(target_os = "windows")]
            _svp_environment: svp_environment,
        })
    }

    pub(crate) fn is_alive(&mut self) -> bool {
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

    #[cfg(target_os = "linux")]
    pub(crate) fn submit_overlay_pixels(
        &mut self,
        pixels: &[u8],
        width: u16,
        height: u16,
        display: (u16, u16),
    ) -> Result<(), String> {
        self.renderer
            .as_ref()
            .ok_or_else(|| "native renderer is unavailable".to_string())?
            .submit_pixels(self.handle, pixels, width, height, display)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn current_vo(&self) -> String {
        self.renderer
            .as_ref()
            .map_or_else(String::new, |r| r.current_vo(self.handle))
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn current_gpu_context(&self) -> String {
        self.renderer
            .as_ref()
            .map_or_else(String::new, |r| r.current_gpu_context(self.handle))
    }

    pub(crate) fn terminate(&mut self) {
        if self.handle.is_null() {
            return;
        }
        #[cfg(target_os = "linux")]
        self.renderer.take();
        let handle = std::mem::replace(&mut self.handle, std::ptr::null_mut());
        self.alive = false;
        #[cfg(target_os = "windows")]
        {
            struct SendHandle(*mut MpvHandle);
            // SAFETY: `LibMpvRuntime` cleared `self.handle` and exclusively transfers ownership to `mpv-destroy`.
            unsafe impl Send for SendHandle {}

            let destroy = self.terminate_destroy;
            let raw = SendHandle(handle);
            let (tx, rx) = std::sync::mpsc::channel();
            if let Ok(join) = thread::Builder::new()
                .name("mpv-destroy".to_string())
                .spawn(move || {
                    let send = raw;
                    // SAFETY: `send.0` is exclusively owned by this thread and destroyed once.
                    unsafe { (destroy)(send.0) };
                    let _ = tx.send(());
                })
            {
                match rx.recv_timeout(Duration::from_millis(1500)) {
                    Ok(()) => {
                        let _ = join.join();
                        self.library.take();
                    }
                    Err(_) => {
                        tracing::warn!(
                            target: "mpv.library",
                            "libmpv terminate_destroy exceeded 1500ms shutdown budget; detaching destructor thread"
                        );
                        if let Some(lib) = self.library.take() {
                            std::mem::forget(lib);
                        }
                    }
                }
                return;
            }
        }
        // SAFETY: `handle` was taken out of the runtime, which now holds null,
        // so it is destroyed exactly once.
        unsafe { (self.terminate_destroy)(handle) };
        self.library.take();
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

#[cfg(all(test, target_os = "linux"))]
mod helper_tests;
#[cfg(test)]
mod tests;
