use std::ffi::{CStr, CString, c_char, c_int, c_ulong, c_void};
use std::io;
use std::path::Path;
use std::process::Child;

use libloading::Library;

use crate::preferences::FullscreenBehavior;

use super::ExternalMpv;

const MPV_EVENT_NONE: c_int = 0;
const MPV_EVENT_SHUTDOWN: c_int = 1;
const REQUIRED_CLIENT_API_MAJOR: u32 = 2;
const WINDOWED_AUTOFIT: &str = "70%";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpvRuntimeKind {
    External,
    Library,
}

pub(super) enum MpvRuntime {
    External(Child),
    Library(LibMpvRuntime),
}

impl MpvRuntime {
    pub(super) fn start(
        kind: MpvRuntimeKind,
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
                LibMpvRuntime::start(path, ipc_path, fullscreen).map(Self::Library)
            }
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
    _library: Library,
}

impl LibMpvRuntime {
    fn start(path: &Path, ipc_path: &str, fullscreen: FullscreenBehavior) -> io::Result<Self> {
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

        let version = unsafe { client_api_version() } as u32;
        let major = version >> 16;
        let minor = version & 0xffff;
        if major != REQUIRED_CLIENT_API_MAJOR {
            return Err(io::Error::other(format!(
                "unsupported libmpv client API {major}.{minor}; expected major {REQUIRED_CLIENT_API_MAJOR}"
            )));
        }

        let handle = unsafe { create() };
        if handle.is_null() {
            return Err(io::Error::other("libmpv could not create a client handle"));
        }

        let options = [
            ("config", "no"),
            ("load-scripts", "no"),
            ("force-window", "no"),
            ("fullscreen", fullscreen.fullscreen_arg()),
            ("autofit", WINDOWED_AUTOFIT),
            ("hwdec", "auto-safe"),
            ("input-default-bindings", "yes"),
            ("input-vo-keyboard", "yes"),
            ("idle", "yes"),
            ("input-ipc-server", ipc_path),
        ];
        for (name, value) in options {
            if let Err(error) = set_option(handle, set_option_string, error_string, name, value) {
                unsafe { terminate_destroy(handle) };
                return Err(error);
            }
        }

        let status = unsafe { initialize(handle) };
        if status < 0 {
            let message = mpv_error(error_string, status);
            unsafe { terminate_destroy(handle) };
            return Err(io::Error::other(format!(
                "libmpv initialization failed: {message}"
            )));
        }

        tracing::info!(
            target: "mpv.library",
            path = %path.display(),
            client_api = %format_args!("{major}.{minor}"),
            "initialized bundled libmpv"
        );
        Ok(Self {
            handle,
            wait_event,
            terminate_destroy,
            alive: true,
            _library: library,
        })
    }

    fn is_alive(&mut self) -> bool {
        if !self.alive || self.handle.is_null() {
            return false;
        }
        loop {
            let event = unsafe { (self.wait_event)(self.handle, 0.0) };
            if event.is_null() {
                self.alive = false;
                break;
            }
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
        let handle = std::mem::replace(&mut self.handle, std::ptr::null_mut());
        self.alive = false;
        unsafe { (self.terminate_destroy)(handle) };
    }
}

impl Drop for LibMpvRuntime {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn load_symbol<T>(library: &Library, name: &[u8]) -> io::Result<T>
where
    T: Copy,
{
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
    let name = CString::new(name).map_err(io::Error::other)?;
    let value = CString::new(value).map_err(io::Error::other)?;
    let status = unsafe { set_option_string(handle, name.as_ptr(), value.as_ptr()) };
    if status < 0 {
        return Err(io::Error::other(format!(
            "libmpv rejected option {}: {}",
            name.to_string_lossy(),
            mpv_error(error_string, status)
        )));
    }
    Ok(())
}

fn mpv_error(error_string: MpvErrorString, status: c_int) -> String {
    let message = unsafe { error_string(status) };
    if message.is_null() {
        return format!("error {status}");
    }
    unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    use std::time::{Duration, Instant};

    #[test]
    fn runtime_kinds_are_distinct() {
        assert_ne!(MpvRuntimeKind::External, MpvRuntimeKind::Library);
    }

    #[test]
    fn configured_library_initializes_its_ipc_server() {
        let Some(path) = std::env::var_os("MEDIAFLICK_DESKTOP_LIBMPV_PATH") else {
            return;
        };
        let ipc_path = crate::players::mpv::ipc::make_ipc_path();
        let mut runtime = MpvRuntime::start(
            MpvRuntimeKind::Library,
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
