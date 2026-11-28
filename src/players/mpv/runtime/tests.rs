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
        assert!(validate_client_api(version).is_err(), "major {major}");
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
            #[cfg(target_os = "linux")]
            false,
        )
    };
    assert!(configure(LibmpvProfile::Standard).is_ok());
    assert!(configure(LibmpvProfile::Svp).is_err());
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

#[test]
fn default_platform_options_enable_gpu_next_and_colorspace_on_windows_and_linux() {
    let win = default_platform_options(true, false);
    for pair in [
        ("vo", "gpu-next"),
        ("gpu-api", "d3d11,vulkan"),
        ("gpu-context", "d3d11,winvk"),
        ("d3d11-flip", "yes"),
        ("target-colorspace-hint", "auto"),
        ("osd-level", "0"),
        ("osc", "no"),
    ] {
        assert!(win.contains(&pair), "missing Windows option {pair:?}");
    }
    let lin = default_platform_options(false, true);
    for pair in [
        ("vo", "gpu-next"),
        ("gpu-api", "vulkan,opengl"),
        ("gpu-context", "x11vk,x11egl"),
        ("gpu-sw", "yes"),
        ("osd-level", "0"),
        ("x11-name", "io.github.phob.MediaFlickDesktop"),
        ("osc", "no"),
    ] {
        assert!(lin.contains(&pair), "missing Linux option {pair:?}");
    }
    assert!(default_platform_options(false, false).is_empty());
}

#[test]
fn wayland_platform_options_use_wayland_vulkan_pulse_and_hdr_hint() {
    let opts = wayland_platform_options();
    for pair in [
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
    ] {
        assert!(opts.contains(&pair), "missing Wayland option {pair:?}");
    }
}
