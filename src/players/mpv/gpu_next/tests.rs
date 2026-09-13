use super::super::{LibmpvProfile, MpvRuntime, MpvRuntimeKind};
use super::*;
use crate::preferences::FullscreenBehavior;
use std::path::Path;
use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt, ImageFormat};
use x11rb::rust_connection::RustConnection;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
#[ignore = "requires MEDIAFLICK_DESKTOP_LIBMPV_PATH and an isolated X11 display"]
fn configured_gpu_next_composes_and_resizes_browser_frames() -> TestResult {
    let path = std::env::var_os("MEDIAFLICK_DESKTOP_LIBMPV_PATH").ok_or("libmpv path missing")?;
    let ipc = crate::players::mpv::ipc::make_ipc_path();
    let mut runtime = MpvRuntime::start(
        MpvRuntimeKind::Library,
        LibmpvProfile::Standard,
        Path::new(&path),
        &ipc,
        FullscreenBehavior::Windowed,
    )?;
    let shutdown = std::sync::atomic::AtomicBool::new(false);
    let (worker, _events) = crate::players::mpv::ipc::start_ipc_worker(
        &ipc,
        Duration::from_secs(5),
        &shutdown,
        || runtime.is_alive(),
    )?;
    let (conn, _) = x11rb::connect(None)?;
    let native = runtime.native_window().ok_or("native window missing")?;
    let window = native.raw() as u32;
    let content = native.content() as u32;
    conn.map_window(window)?.check()?;
    let MpvRuntime::Library(lib) = &runtime else {
        return Err("library missing".into());
    };
    let renderer = lib.renderer.as_ref().ok_or("renderer missing")?;
    let context = renderer.property(lib.handle, c"current-gpu-context");
    eprintln!("Embedded gpu-next context: {context}");
    if let Ok(expected) = std::env::var("MEDIAFLICK_DESKTOP_EXPECT_GPU_CONTEXT") {
        assert_eq!(context, expected);
    }
    assert_eq!(renderer.property(lib.handle, c"current-vo"), "gpu-next");

    for size in [(320, 180), (480, 270), (320, 180)] {
        resize(&conn, window, content, size)?;
        let frame = pattern();
        let mut returned = runtime.present_overlay(frame)?;
        // Reuse immediately: mpv must have copied the previous frame before
        // returning ownership to CEF, including the raw-address command path.
        returned.pixels.fill(0);
        wait_pixel(&conn, content, (size.0 / 4, size.1 / 4), |p| {
            p[2] > 180 && p[1] < 20
        })?;
        wait_pixel(&conn, content, (size.0 * 3 / 4, size.1 * 3 / 4), |p| {
            p[1] > 180 && p[2] < 20
        })?;
        runtime.present_overlay(returned)?;
        wait_pixel(&conn, content, (size.0 / 4, size.1 / 4), |p| {
            p[..3].iter().all(|c| *c < 20)
        })?;
    }
    assert_paused_composition(&mut runtime, &worker, &conn, content)?;
    runtime.stop();
    worker.shutdown();
    crate::players::mpv::ipc::cleanup_ipc_path(&ipc);
    Ok(())
}

fn pattern() -> NativeOverlayFrame {
    let mut pixels = Vec::with_capacity(160 * 90 * 4);
    for y in 0..90 {
        for x in 0..160 {
            pixels.extend_from_slice(if x < 80 && y < 45 {
                &[0, 0, 220, 255]
            } else if x >= 80 && y >= 45 {
                &[0, 220, 0, 255]
            } else {
                &[0, 0, 0, 0]
            });
        }
    }
    NativeOverlayFrame {
        pixels,
        width: 160,
        height: 90,
    }
}

fn resize(conn: &RustConnection, window: u32, content: u32, size: (u16, u16)) -> TestResult {
    for id in [window, content] {
        conn.configure_window(
            id,
            &ConfigureWindowAux::new()
                .width(u32::from(size.0))
                .height(u32::from(size.1)),
        )?
        .check()?;
    }
    Ok(())
}

fn wait_pixel(
    conn: &RustConnection,
    content: u32,
    point: (u16, u16),
    matches: impl Fn(&[u8]) -> bool,
) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let pixel = conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                content,
                point.0 as i16,
                point.1 as i16,
                1,
                1,
                u32::MAX,
            )?
            .reply()?
            .data;
        if pixel.len() >= 3 && matches(&pixel) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("unexpected pixel at {point:?}: {pixel:?}").into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn assert_paused_composition(
    runtime: &mut MpvRuntime,
    worker: &crate::players::mpv::ipc::IpcWorker,
    conn: &RustConnection,
    content: u32,
) -> TestResult {
    let media =
        std::env::temp_dir().join(format!("mediaflick-gpu-next-{}.y4m", std::process::id()));
    let mut data = b"YUV4MPEG2 W16 H16 F24:1 Ip A1:1 C420jpeg\nFRAME\n".to_vec();
    data.extend(vec![128; 16 * 16 * 3 / 2]);
    std::fs::write(&media, data)?;
    for command in [
        serde_json::json!(["set_property", "pause", true]),
        serde_json::json!(["loadfile", media.to_string_lossy()]),
    ] {
        worker
            .send_with_timeout(
                serde_json::json!({"command": command}),
                Duration::from_secs(5),
            )
            .map_err(|e| e.to_string())?;
    }
    wait_pixel(conn, content, (240, 45), |p| {
        p[..3].iter().all(|c| (80..180).contains(c))
    })?;
    let frame = runtime.present_overlay(pattern())?;
    wait_pixel(conn, content, (80, 45), |p| p[2] > 180 && p[1] < 20)?;
    wait_pixel(conn, content, (240, 45), |p| {
        p[..3].iter().all(|c| (80..180).contains(c))
    })?;
    let mut blended = frame;
    blended.pixels = [0, 0, 100, 128].repeat(160 * 90);
    runtime.present_overlay(blended)?;
    wait_pixel(conn, content, (240, 45), |p| p[2] > p[1] + 30 && p[1] > 20)?;
    std::fs::remove_file(media)?;
    Ok(())
}
