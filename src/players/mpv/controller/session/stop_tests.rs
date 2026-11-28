use std::io::Write;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::jellyfin::playback_reporter::{
    PlaybackReporter, flush_playstate_reports, read_test_http_request,
};
use crate::playback::{BackendEvent, HttpHeader, NativeWindowHandle, PlaybackRequest, StopReason};
use crate::players::mpv::ipc::MpvEvent;
use crate::players::mpv::ipc::test_server::FakeMpv;
use crate::players::mpv::runtime::{LibmpvProfile, MpvRuntime, MpvRuntimeKind};
use crate::preferences::{AppSettings, FullscreenBehavior};

use super::super::{
    ControllerState, PendingPlayback, PlaybackIdentity, PlaybackPhase, RuntimeSelection,
};

struct PlaystateCapture {
    url: String,
    entries: Arc<Mutex<Vec<(String, Value)>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl PlaystateCapture {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind playstate capture");
        let url = format!("http://{}", listener.local_addr().expect("capture address"));
        listener.set_nonblocking(true).expect("nonblocking capture");
        let entries = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let server_entries = Arc::clone(&entries);
        let server_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !server_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let request = read_test_http_request(&mut stream);
                        if let Ok(parsed) = serde_json::from_str::<Value>(&request.body) {
                            server_entries
                                .lock()
                                .expect("captured entries")
                                .push((request.request_line, parsed));
                        }
                        let _ = stream.write_all(
                            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                        let _ = stream.flush();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url,
            entries,
            stop,
            handle: Some(handle),
        }
    }

    fn stopped_reports(&self) -> Vec<Value> {
        self.entries
            .lock()
            .expect("captured entries")
            .iter()
            .filter(|(line, _)| line.starts_with("POST /Sessions/Playing/Stopped"))
            .map(|(_, body)| body.clone())
            .collect()
    }
}

impl Drop for PlaystateCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn active_controller(
    runtime_kind: MpvRuntimeKind,
    server_url: &str,
    fake: Option<&FakeMpv>,
) -> (
    ControllerState,
    Arc<AtomicBool>,
    mpsc::Receiver<BackendEvent>,
) {
    let (tx, rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let mut state = ControllerState::new(
        tx,
        rx,
        Arc::new(Mutex::new(Default::default())),
        Some(event_tx),
        Arc::new(AtomicBool::new(false)),
        AppSettings::default().player_preferences(),
        RuntimeSelection {
            kind: runtime_kind,
            libmpv_profile: LibmpvProfile::Standard,
        },
    );
    let stopped = Arc::new(AtomicBool::new(false));
    state.runtime = Some(MpvRuntime::TestRuntime {
        alive: Arc::new(AtomicBool::new(true)),
        stopped: Arc::clone(&stopped),
        native_window: Some(NativeWindowHandle::test_handle(99)),
    });
    state.current_mpv_path = Some("test-mpv".to_string());
    state.remember_configured_mpv("test-mpv", FullscreenBehavior::Windowed);
    if let Some(fake) = fake {
        let (worker, event_stream) = fake.connect();
        state.ipc.path = Some(fake.path().to_string());
        state.ipc.worker = Some(worker);
        state.ipc.active_id = Some(1);
        state.ipc.next_id = 2;
        state.start_event_relay(1, event_stream);
    } else {
        state.ipc.path = Some("disconnected-ipc".to_string());
    }

    let mut launch = PlaybackRequest::new(format!("{server_url}/Videos/item-stop/stream.mkv"));
    launch.item_id = Some("item-stop".to_string());
    launch.play_session_id = Some("session-stop".to_string());
    launch.headers = vec![HttpHeader {
        name: "X-Emby-Token".to_string(),
        value: "secret".to_string(),
    }];
    let reporter = PlaybackReporter::from_launch(&launch);
    state.phase = PlaybackPhase::loading(PendingPlayback {
        key: "stop-test".to_string(),
        identity: PlaybackIdentity::from_launch(5, &launch),
        launch,
        reporter,
        requested_at: Instant::now(),
    });
    state.activate_pending();
    state.last_state.position_ticks = 600_000_000;
    (state, stopped, event_rx)
}

#[test]
fn stop_sync_queues_stopped_report_sends_mpv_stop_and_ignores_later_end_file() {
    for runtime_kind in [MpvRuntimeKind::External, MpvRuntimeKind::Library] {
        let capture = PlaystateCapture::start();
        let stop_commands = Arc::new(AtomicUsize::new(0));
        let stop_commands_for_mpv = Arc::clone(&stop_commands);
        let fake = FakeMpv::start(move |command| {
            if command["command"][0] == "stop" {
                stop_commands_for_mpv.fetch_add(1, Ordering::SeqCst);
            }
            vec![json!({
                "request_id": command["request_id"].clone(),
                "error": "success",
            })]
        });
        let (mut state, _stopped, event_rx) =
            active_controller(runtime_kind, &capture.url, Some(&fake));

        let (ack_tx, ack_rx) = mpsc::channel();
        state.stop_sync(&ack_tx, Duration::from_secs(2));
        assert!(ack_rx.try_recv().is_ok());
        assert!(!state.mpv_playback_active);
        assert!(state.phase.is_idle());
        assert_eq!(stop_commands.load(Ordering::SeqCst), 1);

        let mut stopped_events = 0usize;
        while let Ok(event) = event_rx.try_recv() {
            if let BackendEvent::Stopped(snapshot) = event {
                assert_eq!(snapshot.stop_reason, Some(StopReason::Stop));
                assert!(!snapshot.active);
                stopped_events += 1;
            }
        }
        assert_eq!(stopped_events, 1);

        assert!(flush_playstate_reports(Duration::from_secs(2)));
        let reports = capture.stopped_reports();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0]["ItemId"], "item-stop");
        assert_eq!(reports[0]["PositionTicks"], 600_000_000);

        state.handle_session_event(
            1,
            &MpvEvent {
                name: "end-file".to_string(),
                reason: Some("stop".to_string()),
                property: None,
                data: None,
                args: Vec::new(),
                playlist_entry_id: None,
                raw: json!({ "event": "end-file", "reason": "stop" }),
            },
        );
        assert!(flush_playstate_reports(Duration::from_secs(2)));
        assert_eq!(capture.stopped_reports().len(), 1);
        while let Ok(event) = event_rx.try_recv() {
            assert!(
                !matches!(event, BackendEvent::Stopped(_)),
                "late end-file must not emit a duplicate Stopped event"
            );
        }

        if let Some(worker) = state.ipc.worker.take() {
            fake.finish(worker);
        } else {
            fake.stop();
        }
    }
}

#[test]
fn stop_sync_when_idle_acknowledges_without_starting_mpv() {
    let (tx, rx) = mpsc::channel();
    let mut state = ControllerState::new(
        tx,
        rx,
        Arc::new(Mutex::new(Default::default())),
        None,
        Arc::new(AtomicBool::new(false)),
        AppSettings::default().player_preferences(),
        RuntimeSelection {
            kind: MpvRuntimeKind::External,
            libmpv_profile: LibmpvProfile::Standard,
        },
    );
    state.remember_configured_mpv("nonexistent-mpv", FullscreenBehavior::Windowed);

    let (ack_tx, ack_rx) = mpsc::channel();
    state.stop_sync(&ack_tx, Duration::from_secs(2));

    assert!(ack_rx.try_recv().is_ok());
    assert!(state.runtime.is_none());
}

#[test]
fn stop_sync_queues_stopped_report_and_arms_pending_stop_when_libmpv_ipc_is_disconnected() {
    let capture = PlaystateCapture::start();
    let (mut state, stopped, _event_rx) =
        active_controller(MpvRuntimeKind::Library, &capture.url, None);

    let (ack_tx, ack_rx) = mpsc::channel();
    state.stop_sync(&ack_tx, Duration::from_secs(2));

    assert!(ack_rx.try_recv().is_ok());
    assert!(state.ipc.pending_stop);
    assert!(!stopped.load(Ordering::SeqCst));
    assert!(flush_playstate_reports(Duration::from_secs(2)));
    let reports = capture.stopped_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["PositionTicks"], 600_000_000);
}

#[test]
fn stop_sync_stops_external_runtime_when_ipc_writer_is_missing() {
    let capture = PlaystateCapture::start();
    let (mut state, stopped, _event_rx) =
        active_controller(MpvRuntimeKind::External, &capture.url, None);
    state.configured_mpv = None;

    let (ack_tx, ack_rx) = mpsc::channel();
    state.stop_sync(&ack_tx, Duration::from_secs(2));

    assert!(ack_rx.try_recv().is_ok());
    assert!(stopped.load(Ordering::SeqCst));
    assert!(state.runtime.is_none());
    assert!(flush_playstate_reports(Duration::from_secs(2)));
    let reports = capture.stopped_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["PositionTicks"], 600_000_000);
}

#[test]
fn stop_sync_bounds_hung_libmpv_ipc_and_still_queues_stopped_report() {
    let capture = PlaystateCapture::start();
    let stall_stop = Arc::new(AtomicBool::new(true));
    let stall_for_mpv = Arc::clone(&stall_stop);
    let fake = FakeMpv::start(move |command| {
        if command["command"][0] == "stop" && stall_for_mpv.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(200));
        }
        vec![json!({
            "request_id": command["request_id"].clone(),
            "error": "success",
        })]
    });
    let (mut state, stopped, _event_rx) =
        active_controller(MpvRuntimeKind::Library, &capture.url, Some(&fake));
    state.ipc.command_timeout_override = Some(Duration::from_millis(30));

    let (ack_tx, ack_rx) = mpsc::channel();
    let started = Instant::now();
    state.stop_sync(&ack_tx, Duration::from_millis(30));
    let ack_received = ack_rx.recv_timeout(Duration::from_millis(50));
    let elapsed = started.elapsed();

    assert!(ack_received.is_ok());
    assert!(
        elapsed < Duration::from_secs(2),
        "stop_sync must stay bounded on hung IPC, took {elapsed:?}"
    );
    assert!(state.ipc.pending_stop);
    assert!(!stopped.load(Ordering::SeqCst));
    assert!(flush_playstate_reports(Duration::from_secs(2)));
    let reports = capture.stopped_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["PositionTicks"], 600_000_000);

    stall_stop.store(false, Ordering::SeqCst);
    if let Some(worker) = state.ipc.worker.take() {
        fake.finish(worker);
    } else {
        fake.stop();
    }
}
