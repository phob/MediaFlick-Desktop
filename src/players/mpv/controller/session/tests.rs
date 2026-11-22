use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::json;

use crate::playback::{BackendEvent, PlaybackRequest, PlayerCommand, StopReason};

use super::super::StartupSeek;
use super::super::test_support::{controller_with_pending_load, snapshot_active};
use super::super::{
    ControllerState, PendingPlayback, PlaybackIdentity, PlaybackPhase, RuntimeSelection,
};
use crate::players::mpv::ipc::MpvEvent;
use crate::players::mpv::ipc::test_server::{COMMAND_CONNECTION, FakeMpv};

fn next_terminal_event(event_rx: &mpsc::Receiver<BackendEvent>) -> BackendEvent {
    loop {
        match event_rx.try_recv().expect("playback event") {
            BackendEvent::StateChanged(_) | BackendEvent::Started { .. } => {}
            event => return event,
        }
    }
}

#[test]
fn watched_next_command_reports_the_runtime_as_final_position() {
    let mut state = controller_with_pending_load(None);
    state
        .phase
        .pending_mut()
        .expect("pending playback")
        .launch
        .runtime_ticks = Some(300_000_000);
    state.activate_pending();

    state.control(&PlayerCommand::MarkWatchedAndPlayNext);

    assert_eq!(state.last_state.position_ticks, 300_000_000);
}

#[test]
fn external_watched_next_message_uses_the_completion_handoff() {
    let mut state = controller_with_pending_load(None);
    state
        .phase
        .pending_mut()
        .expect("pending playback")
        .launch
        .runtime_ticks = Some(300_000_000);
    state.activate_pending();
    state.ipc.active_id = Some(1);
    let event = MpvEvent {
        name: "client-message".to_string(),
        reason: None,
        property: None,
        data: None,
        args: vec![
            "mediaflick-desktop".to_string(),
            "mark-watched-next".to_string(),
        ],
        playlist_entry_id: None,
        raw: json!({"event": "client-message"}),
    };

    state.handle_session_event(2, &event);
    assert!(state.mpv_playback_active);
    state.handle_session_event(1, &event);

    assert!(!state.mpv_playback_active);
    assert_eq!(
        state
            .snapshot
            .lock()
            .expect("playback snapshot")
            .stop_reason,
        Some(StopReason::WatchedNext)
    );
}

#[test]
fn track_list_keeps_selectable_audio_and_subtitle_tracks() {
    let mut state = controller_with_pending_load(None);

    state.apply_property(
        Some("track-list"),
        Some(&json!([
            { "id": 1, "type": "video", "selected": true },
            {
                "id": 2,
                "type": "audio",
                "lang": "jpn",
                "title": "Surround",
                "codec": "dts",
                "selected": true
            },
            {
                "id": 3,
                "type": "sub",
                "lang": "eng",
                "title": "English SDH",
                "external": true,
                "selected": false
            }
        ])),
    );

    let snapshot = state.publish_snapshot();
    assert_eq!(snapshot.tracks.len(), 2);
    assert_eq!(snapshot.tracks[0].id, 2);
    assert_eq!(
        snapshot.tracks[0].kind,
        crate::playback::PlayerTrackKind::Audio
    );
    assert!(snapshot.tracks[0].selected);
    assert_eq!(snapshot.tracks[1].title.as_deref(), Some("English SDH"));
    assert!(snapshot.tracks[1].external);
}

#[cfg(target_os = "windows")]
#[test]
fn svp_library_uses_the_pipe_name_expected_by_svp() {
    let mut state = controller_with_pending_load(None);
    state.runtime_kind = crate::players::mpv::runtime::MpvRuntimeKind::Library;
    state.libmpv_profile = crate::players::mpv::runtime::LibmpvProfile::Svp;

    assert_eq!(state.next_ipc_path(), r"\\.\pipe\mpvpipe");
}

#[test]
fn finish_without_reporter_emits_stopped_event() {
    let (tx, rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let mut launch = PlaybackRequest::new("https://example.test/video.mkv?ApiKey=secret");
    launch.item_id = Some("item-1".to_string());

    let mut state = ControllerState::new(
        tx,
        rx,
        Arc::new(Mutex::new(Default::default())),
        Some(event_tx),
        Arc::new(AtomicBool::new(false)),
        crate::preferences::AppSettings::default().player_preferences(),
        RuntimeSelection {
            kind: crate::players::mpv::runtime::MpvRuntimeKind::External,
            libmpv_profile: crate::players::mpv::runtime::LibmpvProfile::Standard,
        },
    );
    state.phase = PlaybackPhase::loading(PendingPlayback {
        key: "test-load".to_string(),
        identity: PlaybackIdentity::from_launch(1, &launch),
        launch,
        reporter: None,
        requested_at: Instant::now(),
    });

    state.activate_pending();
    state.finish_active(Some(StopReason::Quit));

    let event = next_terminal_event(&event_rx);
    assert!(matches!(
        event,
        BackendEvent::Stopped(snapshot)
            if !snapshot.active
                && snapshot.stop_reason == Some(StopReason::Quit)
                && snapshot.playback_id == Some(1)
                && snapshot.item_id.as_deref() == Some("item-1")
    ));
}

#[test]
fn startup_seek_holds_resume_position_until_mpv_reaches_resume_range() {
    let mut state = controller_with_pending_load(Some(1_000_000_000));

    state.activate_pending();
    assert_eq!(state.last_state.position_ticks, 1_000_000_000);
    assert!(state.startup_seek.is_some());

    state.apply_property(Some("time-pos"), Some(&json!(0.0)));
    assert_eq!(state.last_state.position_ticks, 1_000_000_000);
    assert!(state.startup_seek.is_some());

    state.apply_property(Some("time-pos"), Some(&json!(98.0)));
    assert_eq!(state.last_state.position_ticks, 980_000_000);
    assert!(state.startup_seek.is_none());
}

#[test]
fn an_end_file_error_ends_a_pending_replacement_load() {
    let mut state = controller_with_pending_load(None);
    let (event_tx, event_rx) = mpsc::channel();
    state.event_tx = Some(event_tx);
    state.ipc.active_id = Some(1);
    state.replacement_end_file_pending = true;
    let end_file = MpvEvent {
        name: "end-file".to_string(),
        reason: Some("error".to_string()),
        property: None,
        data: None,
        args: Vec::new(),
        playlist_entry_id: None,
        raw: json!({ "event": "end-file", "reason": "error" }),
    };

    state.handle_session_event(1, &end_file);

    assert!(state.phase.pending().is_none());
    assert!(matches!(
        next_terminal_event(&event_rx),
        BackendEvent::Stopped(snapshot)
            if !snapshot.active && snapshot.stop_reason == Some(StopReason::Error)
    ));
}

#[test]
fn active_replacement_ignores_old_end_file_without_next_episode_handoff() {
    let mut state = controller_with_pending_load(None);
    state.replacement_end_file_pending = true;

    state.finish_active(Some(StopReason::Stop));

    assert!(state.phase.pending().is_some());
    state.activate_pending();
    assert!(state.phase.pending().is_none());
    assert!(snapshot_active(&state));
}

#[test]
fn eof_reports_the_duration_as_the_final_position() {
    let mut state = controller_with_pending_load(None);
    state.activate_pending();
    state.mpv_playback_active = true;
    state.last_state.duration_ticks = Some(120_000_000);

    state.finish_active(Some(StopReason::Eof));

    assert_eq!(state.last_state.position_ticks, 120_000_000);
}

#[test]
fn eof_uses_runtime_when_mpv_duration_is_missing() {
    let mut state = controller_with_pending_load(None);
    state
        .phase
        .pending_mut()
        .expect("pending")
        .launch
        .runtime_ticks = Some(240_000_000);
    state.activate_pending();
    state.mpv_playback_active = true;

    state.finish_active(Some(StopReason::Eof));

    assert_eq!(state.last_state.duration_ticks, Some(240_000_000));
    assert_eq!(state.last_state.position_ticks, 240_000_000);
}

#[test]
fn library_resume_waits_for_file_loaded_and_holds_reported_position() {
    let mut state = controller_with_pending_load(Some(200_000_000));
    state.runtime_kind = crate::players::mpv::runtime::MpvRuntimeKind::Library;
    assert!(state.startup_seek.is_none());
    state.activate_pending();
    assert_eq!(
        state.startup_seek.as_ref().map(|seek| seek.position_ms),
        Some(20_000.0)
    );
    state.apply_property(Some("time-pos"), Some(&json!(0.5)));
    assert_eq!(state.last_state.position_ticks, 200_000_000);
}

fn scripted_mpv(reject_seeks: bool) -> FakeMpv {
    FakeMpv::start(move |command| {
        let rejected = reject_seeks && command["command"][0] == "seek";
        vec![json!({
            "request_id": command["request_id"].clone(),
            "error": if rejected { "property unavailable" } else { "success" },
        })]
    })
}

fn next_seek(fake: &FakeMpv) -> serde_json::Value {
    loop {
        let command = fake.next_command_on(COMMAND_CONNECTION);
        if command["command"][0] == "seek" {
            return command["command"].clone();
        }
    }
}

fn make_due(state: &mut ControllerState) -> StartupSeek {
    let seek = state.startup_seek.as_mut().expect("startup seek pending");
    seek.due_at = Instant::now();
    *seek
}

const RESUME_TICKS: i64 = 900_000_000;

#[test]
fn startup_seek_waits_for_its_delay_then_retries_until_the_position_arrives() {
    let fake = scripted_mpv(false);
    let (worker, _events) = fake.connect();
    let mut state = controller_with_pending_load(Some(RESUME_TICKS));
    state.ipc.worker = Some(worker);
    state.activate_pending();

    state.maybe_send_startup_seek();
    assert!(
        state.startup_seek.expect("queued").sent_at.is_none(),
        "the seek waits for the file to settle"
    );

    make_due(&mut state);
    state.maybe_send_startup_seek();
    assert_eq!(next_seek(&fake), json!(["seek", 90.0, "absolute+exact"]));

    assert!(state.defer_startup_position_update(0));
    make_due(&mut state);
    state.maybe_send_startup_seek();
    assert_eq!(next_seek(&fake), json!(["seek", 90.0, "absolute+exact"]));

    assert!(!state.defer_startup_position_update(RESUME_TICKS));
    assert!(state.startup_seek.is_none(), "the seek landed");
    fake.finish(state.ipc.worker.take().expect("worker"));
}

#[test]
fn a_rejected_startup_seek_is_retried_without_resetting_mpv() {
    let fake = scripted_mpv(true);
    let (worker, _events) = fake.connect();
    let mut state = controller_with_pending_load(Some(RESUME_TICKS));
    state.ipc.worker = Some(worker);
    state.activate_pending();

    make_due(&mut state);
    state.maybe_send_startup_seek();
    assert_eq!(next_seek(&fake), json!(["seek", 90.0, "absolute+exact"]));

    assert!(
        state.startup_seek.is_some(),
        "still pending after the rejection"
    );
    assert!(
        state.ipc.worker.is_some(),
        "a rejection keeps the mpv session"
    );
    assert!(
        state.defer_startup_position_update(0),
        "the resume position is held"
    );
    fake.finish(state.ipc.worker.take().expect("worker"));
}

fn library_controller_with_fake_mpv(
    fake: &FakeMpv,
) -> (
    ControllerState,
    Arc<AtomicBool>,
    crate::playback::NativeWindowHandle,
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
        crate::preferences::AppSettings::default().player_preferences(),
        RuntimeSelection {
            kind: crate::players::mpv::runtime::MpvRuntimeKind::Library,
            libmpv_profile: crate::players::mpv::runtime::LibmpvProfile::Standard,
        },
    );
    let alive = Arc::new(AtomicBool::new(true));
    let stopped = Arc::new(AtomicBool::new(false));
    let handle = crate::playback::NativeWindowHandle::test_handle(42);
    let (worker, event_stream) = fake.connect();
    state.runtime = Some(crate::players::mpv::runtime::MpvRuntime::TestRuntime {
        alive,
        stopped: Arc::clone(&stopped),
        native_window: Some(handle),
    });
    state.current_mpv_path = Some("bundled-libmpv".to_string());
    state.remember_configured_mpv(
        "bundled-libmpv",
        crate::preferences::FullscreenBehavior::Windowed,
    );
    state.ipc.path = Some(fake.path().to_string());
    state.ipc.worker = Some(worker);
    state.ipc.active_id = Some(1);
    state.ipc.next_id = 2;
    state.start_event_relay(1, event_stream);
    (state, stopped, handle, event_rx)
}

#[test]
fn library_ensure_mpv_recycles_ipc_without_stopping_host_runtime() {
    let stall_first = Arc::new(AtomicBool::new(true));
    let stall_flag = Arc::clone(&stall_first);
    let fake = FakeMpv::start(move |command| {
        if command["command"][0] == "get_property"
            && stall_flag.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        vec![json!({
            "request_id": command["request_id"].clone(),
            "error": "success",
        })]
    });
    let (mut state, stopped, handle, _event_rx) = library_controller_with_fake_mpv(&fake);

    let timed_out = state
        .ipc
        .worker
        .as_ref()
        .expect("worker")
        .send_with_timeout(
            json!({ "command": ["get_property", "chapter-list"], "request_id": 99 }),
            std::time::Duration::from_millis(20),
        );
    assert!(timed_out.is_err());
    assert!(!state.ipc.worker.as_ref().expect("worker").is_writer_alive());

    assert!(state.ensure_mpv(
        "bundled-libmpv",
        crate::preferences::FullscreenBehavior::Windowed
    ));
    assert!(!stopped.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        state
            .runtime
            .as_ref()
            .and_then(crate::players::mpv::runtime::MpvRuntime::native_window),
        Some(handle)
    );
    assert!(
        state
            .ipc
            .worker
            .as_ref()
            .is_some_and(crate::players::mpv::ipc::IpcWorker::is_writer_alive)
    );
    assert_ne!(state.ipc.active_id, Some(1));
    assert_eq!(fake.session_count(), 2);
    assert!(
        state
            .send_mpv_command(json!({ "command": ["get_property", "pause"], "request_id": 100 }))
            .is_ok()
    );

    if let Some(worker) = state.ipc.worker.take() {
        fake.finish(worker);
    } else {
        fake.stop();
    }
}

#[test]
fn library_session_lost_recycles_ipc_without_stopping_host_runtime_and_stops_playback() {
    let fake = scripted_mpv(false);
    let (mut state, stopped, handle, event_rx) = library_controller_with_fake_mpv(&fake);
    let launch = PlaybackRequest::new("https://example.test/video.mkv");
    state.phase = PlaybackPhase::loading(PendingPlayback {
        key: "k".to_string(),
        identity: PlaybackIdentity::from_launch(7, &launch),
        launch,
        reporter: None,
        requested_at: Instant::now(),
    });
    state.activate_pending();

    state.handle_mpv_session_lost("mpv control command transport failed");

    assert!(!stopped.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        state
            .runtime
            .as_ref()
            .and_then(crate::players::mpv::runtime::MpvRuntime::native_window),
        Some(handle)
    );
    assert!(!state.mpv_playback_active);
    assert!(
        state
            .ipc
            .worker
            .as_ref()
            .is_some_and(crate::players::mpv::ipc::IpcWorker::is_writer_alive)
    );
    assert_eq!(fake.session_count(), 2);
    assert!(matches!(
        next_terminal_event(&event_rx),
        BackendEvent::Stopped(snapshot) if snapshot.stop_reason == Some(StopReason::Quit)
    ));
    let mut saw_stop = false;
    for _ in 0..32 {
        let command = fake.next_command_on(COMMAND_CONNECTION);
        if command["command"][0] == "stop" {
            saw_stop = true;
            break;
        }
    }
    assert!(saw_stop, "expected stop command after IPC recycle");

    if let Some(worker) = state.ipc.worker.take() {
        fake.finish(worker);
    } else {
        fake.stop();
    }
}

#[test]
fn library_loadfile_transport_failure_recycles_ipc_and_retries_without_stopping_host_runtime() {
    let fake = FakeMpv::start(|command| {
        if command["command"][0] == "loadfile" {
            vec![json!({
                "request_id": command["request_id"].clone(),
                "error": "success",
                "data": { "playlist_entry_id": 7 },
            })]
        } else {
            vec![json!({
                "request_id": command["request_id"].clone(),
                "error": "success",
            })]
        }
    });
    let (mut state, stopped, handle, _event_rx) = library_controller_with_fake_mpv(&fake);
    state.ipc.worker.take().expect("worker");
    let launch = PlaybackRequest::new("https://example.test/video.mkv");

    let entry = state
        .send_loadfile_with_reconnect(
            "bundled-libmpv",
            crate::preferences::FullscreenBehavior::Windowed,
            &launch,
        )
        .expect("loadfile retry succeeds on recycled IPC");

    assert_eq!(entry, Some(7));
    assert!(!stopped.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        state
            .runtime
            .as_ref()
            .and_then(crate::players::mpv::runtime::MpvRuntime::native_window),
        Some(handle)
    );
    assert_eq!(fake.session_count(), 2);

    if let Some(worker) = state.ipc.worker.take() {
        fake.finish(worker);
    } else {
        fake.stop();
    }
}

#[test]
fn library_rejected_replacement_loadfile_preserves_host_runtime() {
    let fake = scripted_mpv(false);
    let (mut state, stopped, handle, event_rx) = library_controller_with_fake_mpv(&fake);
    let launch = PlaybackRequest::new("https://example.test/video.mkv");
    let identity = PlaybackIdentity::from_launch(9, &launch);

    state.handle_rejected_loadfile(true, identity);

    assert!(!stopped.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        state
            .runtime
            .as_ref()
            .and_then(crate::players::mpv::runtime::MpvRuntime::native_window),
        Some(handle)
    );
    assert!(matches!(
        next_terminal_event(&event_rx),
        BackendEvent::Stopped(snapshot) if snapshot.stop_reason == Some(StopReason::Error)
    ));
    assert!(matches!(
        next_terminal_event(&event_rx),
        BackendEvent::Failed { load_id: 9, .. }
    ));

    if let Some(worker) = state.ipc.worker.take() {
        fake.finish(worker);
    } else {
        fake.stop();
    }
}

#[test]
fn library_shutdown_stops_host_runtime() {
    let fake = scripted_mpv(false);
    let (mut state, stopped, _handle, _event_rx) = library_controller_with_fake_mpv(&fake);
    state.ipc.worker.take().expect("worker");

    state.shutdown();

    assert!(stopped.load(std::sync::atomic::Ordering::SeqCst));
    assert!(state.runtime.is_none());
    fake.stop();
}

#[test]
fn library_event_stream_disconnect_recycles_ipc_without_stopping_host_runtime() {
    let fake = scripted_mpv(false);
    let (mut state, stopped, handle, _event_rx) = library_controller_with_fake_mpv(&fake);

    state.handle_event_stream_disconnected(1);

    assert!(!stopped.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        state
            .runtime
            .as_ref()
            .and_then(crate::players::mpv::runtime::MpvRuntime::native_window),
        Some(handle)
    );
    assert_eq!(fake.session_count(), 2);
    let new_id = state.ipc.active_id.expect("reconnected session id");
    assert_ne!(new_id, 1);

    state.handle_event_stream_disconnected(1);
    assert_eq!(
        fake.session_count(),
        2,
        "stale disconnect from previous session must be ignored"
    );

    if let Some(worker) = state.ipc.worker.take() {
        fake.finish(worker);
    } else {
        fake.stop();
    }
}

#[test]
fn external_session_lost_still_stops_and_resets_runtime() {
    let fake = scripted_mpv(false);
    let (mut state, stopped, _handle, _event_rx) = library_controller_with_fake_mpv(&fake);
    state.runtime_kind = crate::players::mpv::runtime::MpvRuntimeKind::External;
    state.configured_mpv = None;

    state.handle_mpv_session_lost("external control transport failed");

    assert!(stopped.load(std::sync::atomic::Ordering::SeqCst));
    assert!(state.runtime.is_none());
    assert!(state.ipc.worker.is_none());
    fake.stop();
}

#[test]
fn library_session_lost_delivers_deferred_stop_once_busy_mpv_catches_up() {
    let gate_open = Arc::new(AtomicBool::new(false));
    let gate_for_server = Arc::clone(&gate_open);
    let acknowledged_stops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stops_for_server = Arc::clone(&acknowledged_stops);
    let fake = FakeMpv::start(move |command| {
        if !gate_for_server.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(200));
            return vec![json!({
                "request_id": command["request_id"].clone(),
                "error": "success",
            })];
        }
        if command["command"][0] == "stop" {
            stops_for_server.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        vec![json!({
            "request_id": command["request_id"].clone(),
            "error": "success",
        })]
    });
    let (mut state, stopped, handle, _event_rx) = library_controller_with_fake_mpv(&fake);
    let launch = PlaybackRequest::new("https://example.test/video.mkv");
    state.phase = PlaybackPhase::loading(PendingPlayback {
        key: "k".to_string(),
        identity: PlaybackIdentity::from_launch(11, &launch),
        launch,
        reporter: None,
        requested_at: Instant::now(),
    });
    state.mpv_playback_active = true;
    state.ipc.command_timeout_override = Some(std::time::Duration::from_millis(30));

    state.handle_mpv_session_lost("mpv external subtitle transport failed");

    assert!(!stopped.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        state
            .runtime
            .as_ref()
            .and_then(crate::players::mpv::runtime::MpvRuntime::native_window),
        Some(handle)
    );
    assert_eq!(
        acknowledged_stops.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "mpv was still busy during initial reconnect"
    );

    assert!(!state.ensure_configured_mpv_running("poll while still busy"));
    assert_eq!(
        acknowledged_stops.load(std::sync::atomic::Ordering::SeqCst),
        0
    );

    gate_open.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(state.ensure_configured_mpv_running("poll after mpv catches up"));
    assert_eq!(
        acknowledged_stops.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "deferred stop must be delivered and acknowledged once mpv catches up"
    );
    assert!(!state.ipc.pending_stop);
    assert!(state.ensure_configured_mpv_running("subsequent poll stays healthy"));
    assert_eq!(
        acknowledged_stops.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "acknowledged deferred stop must not be resent"
    );

    if let Some(worker) = state.ipc.worker.take() {
        fake.finish(worker);
    } else {
        fake.stop();
    }
}

#[test]
fn library_ipc_reconnect_backs_off_and_caps_when_endpoint_is_gone() {
    let fake = scripted_mpv(false);
    let (mut state, stopped, handle, _event_rx) = library_controller_with_fake_mpv(&fake);
    if let Some(worker) = state.ipc.worker.take() {
        worker.disconnect();
    }
    fake.stop();

    let dead_path = crate::players::mpv::ipc::make_ipc_path();
    state.ipc.path = Some(dead_path);

    assert!(!state.ensure_configured_mpv_running("first failed poll"));
    assert_eq!(state.ipc.reconnect_failures, 1);
    assert!(state.ipc.next_reconnect_at.is_some());

    let immediate = Instant::now();
    assert!(!state.ensure_configured_mpv_running("immediate poll during backoff"));
    assert!(
        immediate.elapsed() < std::time::Duration::from_millis(40),
        "poll during backoff should return immediately, took {:?}",
        immediate.elapsed()
    );
    assert_eq!(state.ipc.reconnect_failures, 1);

    for expected in 2..=super::super::LIBRARY_IPC_MAX_RECONNECT_ATTEMPTS {
        state.ipc.next_reconnect_at = Some(Instant::now() - std::time::Duration::from_secs(1));
        assert!(!state.ensure_configured_mpv_running("backoff elapsed poll"));
        assert_eq!(state.ipc.reconnect_failures, expected);
    }
    assert!(state.ipc.next_reconnect_at.is_none());

    let capped = Instant::now();
    assert!(!state.ensure_configured_mpv_running("poll after reconnect cap"));
    assert!(
        capped.elapsed() < std::time::Duration::from_millis(40),
        "capped poll should return immediately, took {:?}",
        capped.elapsed()
    );
    assert!(!stopped.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        state
            .runtime
            .as_ref()
            .and_then(crate::players::mpv::runtime::MpvRuntime::native_window),
        Some(handle)
    );

    let recovered = scripted_mpv(false);
    state.ipc.path = Some(recovered.path().to_string());
    assert!(state.ensure_mpv_for_load(
        "bundled-libmpv",
        crate::preferences::FullscreenBehavior::Windowed
    ));
    assert_eq!(state.ipc.reconnect_failures, 0);
    assert!(state.ipc.next_reconnect_at.is_none());
    if let Some(worker) = state.ipc.worker.take() {
        recovered.finish(worker);
    } else {
        recovered.stop();
    }
}
