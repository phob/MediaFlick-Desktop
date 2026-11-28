use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::playback::NativeOverlayFrame;
use crate::preferences::FullscreenBehavior;

use super::LibmpvProfile;
use super::helper::{
    HelperOverlayError, HelperRestartBudget, HelperRuntime, parse_helper_args,
    read_control_json_line,
};

#[test]
fn helper_restart_budget_caps_restarts_within_window_and_recovers() {
    let start = Instant::now();
    let mut budget = HelperRestartBudget::default();
    for offset in 0..5 {
        assert!(
            budget.record_and_allow(start + Duration::from_secs(offset)),
            "restart #{offset} must be allowed"
        );
    }
    assert!(
        !budget.record_and_allow(start + Duration::from_secs(10)),
        "6th restart inside 30s window must trip circuit breaker"
    );
    assert!(
        budget.record_and_allow(start + Duration::from_secs(35)),
        "window expiry after 30s must allow restarts again"
    );
}

#[test]
fn parse_helper_args_extracts_all_flags_and_rejects_incomplete() {
    let args = [
        "mediaflick-desktop",
        "--player-helper",
        "--libmpv-path",
        "/usr/lib/libmpv.so",
        "--ipc-path",
        "/tmp/mpv.sock",
        "--wid",
        "123456",
        "--overlay-path",
        "/tmp/ovr.bgra",
        "--control-path",
        "/tmp/ctl.sock",
        "--parent-pid",
        "4242",
        "--fullscreen",
        "fullscreen",
        "--profile",
        "standard",
    ];
    let parsed = parse_helper_args(args).expect("valid helper args");
    assert_eq!(parsed.libmpv_path, PathBuf::from("/usr/lib/libmpv.so"));
    assert_eq!(parsed.ipc_path, "/tmp/mpv.sock");
    assert_eq!(parsed.wid, 123_456);
    assert_eq!(parsed.overlay_path, PathBuf::from("/tmp/ovr.bgra"));
    assert_eq!(parsed.control_path, PathBuf::from("/tmp/ctl.sock"));
    assert_eq!(parsed.parent_pid, 4242);
    assert_eq!(parsed.fullscreen, FullscreenBehavior::Fullscreen);
    assert_eq!(parsed.profile, LibmpvProfile::Standard);

    let windowed = parse_helper_args([
        "mediaflick-desktop",
        "--player-helper",
        "--libmpv-path",
        "/usr/lib/libmpv.so",
        "--ipc-path",
        "/tmp/mpv.sock",
        "--wid",
        "123456",
        "--overlay-path",
        "/tmp/ovr.bgra",
        "--control-path",
        "/tmp/ctl.sock",
        "--parent-pid",
        "4242",
        "--fullscreen",
        FullscreenBehavior::Windowed.fullscreen_arg(),
        "--profile",
        "standard",
    ])
    .expect("windowed fullscreen_arg");
    assert_eq!(windowed.fullscreen, FullscreenBehavior::Windowed);

    assert!(parse_helper_args(["mediaflick-desktop", "--player-helper"]).is_err());
    assert!(
        parse_helper_args([
            "mediaflick-desktop",
            "--player-helper",
            "--libmpv-path",
            "/x",
            "--ipc-path",
            "/y",
            "--wid",
            "not-a-number",
        ])
        .is_err()
    );
}

#[test]
fn parse_helper_args_handles_wayland_and_x11_backends() {
    let wayland = parse_helper_args([
        "mediaflick-desktop",
        "--player-helper",
        "--backend",
        "wayland",
        "--libmpv-path",
        "/usr/lib/libmpv.so",
        "--ipc-path",
        "/tmp/mpv.sock",
        "--wid",
        "0",
        "--control-path",
        "/tmp/ctl.sock",
        "--parent-pid",
        "4242",
        "--fullscreen",
        "no",
        "--profile",
        "standard",
    ])
    .expect("wayland helper args");
    assert_eq!(
        wayland.backend,
        super::wayland::LinuxDisplayBackend::Wayland
    );
    assert!(
        parse_helper_args([
            "mediaflick-desktop",
            "--player-helper",
            "--backend",
            "invalid",
            "--libmpv-path",
            "/x",
            "--ipc-path",
            "/y",
            "--wid",
            "0",
            "--control-path",
            "/c",
            "--parent-pid",
            "1",
            "--fullscreen",
            "no",
            "--profile",
            "standard",
        ])
        .is_err()
    );
}

#[test]
fn partial_control_line_survives_read_timeout() {
    let (mut writer, reader_stream) = UnixStream::pair().expect("pair");
    reader_stream
        .set_read_timeout(Some(Duration::from_millis(20)))
        .expect("timeout");
    let mut reader = BufReader::new(reader_stream);
    let mut buffer = String::new();

    writer.write_all(br#"{"cmd":"ov"#).expect("partial");
    writer.flush().expect("flush");
    assert_eq!(
        read_control_json_line(&mut reader, &mut buffer).expect("partial timeout ok"),
        None
    );

    writer
        .write_all(b"erlay\",\"seq\":7}\n")
        .expect("rest of line");
    writer.flush().expect("flush");
    let full = read_control_json_line(&mut reader, &mut buffer)
        .expect("complete ok")
        .expect("line present");
    assert_eq!(full.trim(), r#"{"cmd":"overlay","seq":7}"#);
    assert!(buffer.is_empty());
}

#[test]
fn overlay_timeout_skips_file_overwrite_until_late_reply_is_drained() {
    let temp_path = std::env::temp_dir().join(format!(
        "mf-helper-seq-{}.bgra",
        crate::app::ids::random_hex(6)
    ));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&temp_path)
        .expect("open temp overlay");
    let overlay_reader_file = OpenOptions::new()
        .read(true)
        .open(&temp_path)
        .expect("reopen");
    let _ = overlay_reader_file;

    let (parent_sock, mut peer_sock) = UnixStream::pair().expect("pair");
    peer_sock
        .set_read_timeout(Some(Duration::from_millis(100)))
        .expect("timeout");
    let child = Command::new("sleep")
        .arg("5")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("sleep child");
    let mut runtime =
        HelperRuntime::new_for_test(child, parent_sock, file.try_clone().expect("dup"))
            .expect("runtime");

    let frame1 = NativeOverlayFrame {
        width: 1,
        height: 1,
        pixels: vec![1, 2, 3, 255],
    };
    let err1 = runtime
        .present_overlay(frame1, (1, 1))
        .expect_err("first overlay should time out");
    assert!(matches!(err1, HelperOverlayError::TimedOut(_)));

    let mut peer_reader = BufReader::new(peer_sock.try_clone().expect("clone"));
    let mut cmd1_line = String::new();
    peer_reader.read_line(&mut cmd1_line).expect("read cmd1");
    let cmd1: serde_json::Value = serde_json::from_str(cmd1_line.trim()).expect("json1");
    let seq1 = cmd1["seq"].as_u64().expect("seq1");

    let frame2 = NativeOverlayFrame {
        width: 1,
        height: 1,
        pixels: vec![9, 8, 7, 255],
    };
    let err2 = runtime
        .present_overlay(frame2, (1, 1))
        .expect_err("second overlay while seq1 in flight must time out");
    assert!(matches!(err2, HelperOverlayError::TimedOut(_)));
    let mut buf = [0_u8; 4];
    file.seek(SeekFrom::Start(0)).expect("seek");
    file.read_exact(&mut buf).expect("read");
    assert_eq!(
        buf,
        [1, 2, 3, 255],
        "frame2 must not overwrite frame1 in flight"
    );

    writeln!(
        peer_sock,
        "{}",
        serde_json::json!({ "seq": seq1, "ok": true })
    )
    .expect("late reply");
    peer_sock.flush().expect("flush");

    let responder = std::thread::spawn(move || {
        let mut cmd2_line = String::new();
        peer_reader.read_line(&mut cmd2_line).expect("read cmd2");
        let cmd2: serde_json::Value = serde_json::from_str(cmd2_line.trim()).expect("json2");
        let seq2 = cmd2["seq"].as_u64().expect("seq2");
        assert!(seq2 > seq1);
        writeln!(
            peer_sock,
            "{}",
            serde_json::json!({ "seq": seq2, "ok": true })
        )
        .expect("reply2");
        peer_sock.flush().expect("flush2");
        seq2
    });

    let frame3 = NativeOverlayFrame {
        width: 1,
        height: 1,
        pixels: vec![9, 8, 7, 255],
    };
    runtime
        .present_overlay(frame3, (1, 1))
        .expect("seq2 after draining seq1 succeeds cleanly");
    let _ = responder.join().expect("responder");
    file.seek(SeekFrom::Start(0)).expect("seek");
    file.read_exact(&mut buf).expect("read");
    assert_eq!(buf, [9, 8, 7, 255]);
    runtime.stop();
    let _ = std::fs::remove_file(&temp_path);
}
