use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::playback::{PlaybackEvent, PlaybackRequest, PlayerCommand, StopReason};

use super::super::StartupSeek;
use super::super::test_support::{controller_with_pending_load, snapshot_active};
use super::super::{
    ControllerState, PendingPlayback, PlaybackIdentity, PlaybackPhase, RuntimeSelection,
};
use crate::players::mpv::ipc::test_server::{COMMAND_CONNECTION, FakeMpv};

fn next_terminal_event(event_rx: &mpsc::Receiver<PlaybackEvent>) -> PlaybackEvent {
    loop {
        match event_rx.try_recv().expect("playback event") {
            PlaybackEvent::StateChanged(_) => {}
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
    state.mpv_playback_active = true;

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
    state.mpv_playback_active = true;
    state.ipc.active_id = Some(1);
    let event = super::MpvEvent {
        name: "client-message".to_string(),
        reason: None,
        property: None,
        data: None,
        args: vec![
            "mediaflick-desktop".to_string(),
            "mark-watched-next".to_string(),
        ],
        raw: json!({"event": "client-message"}),
    };

    state.handle_session_event(2, &event);
    assert!(state.phase.pending().is_some());
    state.handle_session_event(1, &event);

    assert!(state.phase.pending().is_none());
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

#[test]
fn pending_preparation_resets_previous_playback_snapshot_state() {
    let mut state = controller_with_pending_load(None);
    state
        .phase
        .pending_mut()
        .expect("pending")
        .launch
        .runtime_ticks = Some(300_000_000);
    state.last_state.position_ticks = 120_000_000;
    state.last_state.duration_ticks = Some(120_000_000);
    state.last_state.pause = true;
    state.last_state.eof_reached = true;

    state.prepare_pending_playback_state();
    let snapshot = state.publish_snapshot();

    assert!(snapshot.active);
    assert_eq!(snapshot.position_ms, 0.0);
    assert_eq!(snapshot.duration_ms, Some(30_000.0));
    assert!(!snapshot.paused);
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
        PlaybackEvent::Stopped(snapshot)
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

/// A replaced file's stale `end-file` (stop) is ignored while the next file
/// loads, but mpv failing to open that file must still end the load.
#[test]
fn an_end_file_error_ends_a_pending_replacement_load() {
    let mut state = controller_with_pending_load(None);
    let (event_tx, event_rx) = mpsc::channel();
    state.event_tx = Some(event_tx);
    state.ipc.active_id = Some(1);
    state.replacement_end_file_pending = true;
    let end_file = super::MpvEvent {
        name: "end-file".to_string(),
        reason: Some("error".to_string()),
        property: None,
        data: None,
        args: Vec::new(),
        raw: json!({ "event": "end-file", "reason": "error" }),
    };

    state.handle_session_event(1, &end_file);

    assert!(state.phase.pending().is_none());
    assert!(matches!(
        next_terminal_event(&event_rx),
        PlaybackEvent::Stopped(snapshot)
            if !snapshot.active && snapshot.stop_reason == Some(StopReason::Error)
    ));
}

#[test]
fn rejected_replacement_stops_replacement_identity_before_failing() {
    let mut state = controller_with_pending_load(None);
    let (event_tx, event_rx) = mpsc::channel();
    state.event_tx = Some(event_tx);
    state.phase.take_pending();
    state.mpv_playback_active = true;
    state.replacement_end_file_pending = true;
    state.pending_raise_pulse_reset_at = Some(Instant::now());
    let mut replacement = PlaybackRequest::new("https://example.test/replacement.mkv");
    replacement.item_id = Some("replacement-item".to_string());
    replacement.media_source_id = Some("replacement-source".to_string());
    replacement.play_session_id = Some("replacement-session".to_string());
    let replacement_identity = PlaybackIdentity::from_launch(2, &replacement);

    state.handle_rejected_loadfile(true, replacement_identity);

    let stopped = match next_terminal_event(&event_rx) {
        PlaybackEvent::Stopped(stopped) => stopped,
        PlaybackEvent::Failed { .. } => panic!("failure event arrived before stopped event"),
        PlaybackEvent::StateChanged(_) => unreachable!(),
    };
    assert!(!stopped.active);
    assert_eq!(stopped.stop_reason, Some(StopReason::Error));
    assert_eq!(stopped.playback_id, Some(2));
    assert_eq!(stopped.item_id.as_deref(), Some("replacement-item"));
    assert_eq!(
        stopped.media_source_id.as_deref(),
        Some("replacement-source")
    );
    assert_eq!(
        stopped.play_session_id.as_deref(),
        Some("replacement-session")
    );
    assert!(matches!(
        next_terminal_event(&event_rx),
        PlaybackEvent::Failed { .. }
    ));
    assert!(matches!(
        event_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn next_playback_handoff_ignores_old_end_file_while_replacement_is_pending() {
    let mut state = controller_with_pending_load(None);
    state.next_playback_handoff_until = Some(Instant::now() + Duration::from_secs(1));

    state.finish_active(Some(StopReason::Stop));

    assert!(state.phase.pending().is_some());
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
fn eof_arms_next_playback_handoff() {
    let mut state = controller_with_pending_load(None);
    state.activate_pending();
    state.mpv_playback_active = true;
    state.last_state.duration_ticks = Some(120_000_000);

    state.finish_active(Some(StopReason::Eof));

    assert!(state.next_playback_handoff_until.is_some());
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

/// mpv that accepts every command, or rejects every seek.
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

/// 90 s into the item.
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

    // mpv still reports the start of the file: Jellyfin keeps the resume
    // position, and the seek goes out again once the retry is due.
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
