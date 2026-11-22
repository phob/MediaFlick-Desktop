use std::time::Instant;

use serde_json::{Value, json};

use crate::playback::segments::{SegmentType, SkipSegment};
use crate::playback::{PlaybackRequest, PlayerCommand};
use crate::players::mpv::ipc::MpvEvent;
use crate::players::mpv::ipc::test_server::{COMMAND_CONNECTION, FakeMpv};

use super::super::test_support::controller_with_pending_load;
use super::super::{ControllerState, PendingPlayback, PlaybackIdentity, PlaybackPhase};
use super::{build_segment_chapter_markers, merge_chapter_markers};

#[test]
fn segment_chapter_markers_bound_each_segment_and_drop_out_of_range() {
    let segments = vec![
        SkipSegment {
            segment_type: SegmentType::Intro,
            start_ticks: 100_000_000,
            end_ticks: 300_000_000,
            triggered: false,
        },
        SkipSegment {
            segment_type: SegmentType::Outro,
            start_ticks: 1_400_000_000,
            end_ticks: 1_500_000_000,
            triggered: false,
        },
    ];

    let markers = build_segment_chapter_markers(&segments, 150.0);
    let times: Vec<Value> = markers
        .iter()
        .map(|marker| marker["time"].clone())
        .collect();

    assert_eq!(times, vec![json!(10.0), json!(30.0), json!(140.0)]);
}

#[test]
fn merge_chapter_markers_preserves_embedded_chapters_and_sorts_by_time() {
    let base = vec![
        json!({ "title": "Part 1", "time": 0.0 }),
        json!({ "title": "Part 2", "time": 120.0 }),
    ];
    let markers = vec![
        json!({ "title": "Intro", "time": 10.0 }),
        json!({ "title": "Intro End", "time": 30.0 }),
    ];

    let merged = merge_chapter_markers(base, markers);

    assert_eq!(
        merged,
        vec![
            json!({ "title": "Part 1", "time": 0.0 }),
            json!({ "title": "Intro", "time": 10.0 }),
            json!({ "title": "Intro End", "time": 30.0 }),
            json!({ "title": "Part 2", "time": 120.0 }),
        ]
    );
}

const SESSION: u64 = 1;

fn mpv_event(name: &str, property: Option<&str>, data: Option<Value>) -> MpvEvent {
    MpvEvent {
        name: name.to_string(),
        reason: None,
        property: property.map(str::to_string),
        data,
        args: Vec::new(),
        playlist_entry_id: None,
        raw: json!({ "event": name }),
    }
}

fn time_pos(state: &mut ControllerState, seconds: f64) {
    state.handle_session_event(
        SESSION,
        &mpv_event("property-change", Some("time-pos"), Some(json!(seconds))),
    );
}

fn first_segment_triggered(state: &ControllerState) -> bool {
    state.publish_snapshot().skip_segments[0].triggered
}

fn next_seek_command(fake: &FakeMpv) -> Value {
    loop {
        let command = fake.next_command_on(COMMAND_CONNECTION);
        if command["command"][0] == "seek" {
            return command["command"].clone();
        }
    }
}

fn seeking_event(value: bool) -> MpvEvent {
    mpv_event("property-change", Some("seeking"), Some(json!(value)))
}

fn controller_with_segment(
    start_time_ticks: Option<i64>,
    segment: SkipSegment,
) -> (FakeMpv, ControllerState) {
    let fake = FakeMpv::start(|command| {
        vec![json!({ "request_id": command["request_id"].clone(), "error": "success" })]
    });
    let (worker, _events) = fake.connect();
    let mut state = controller_with_pending_load(start_time_ticks);
    state.ipc.worker = Some(worker);
    state.ipc.active_id = Some(SESSION);
    state.skip_segments = vec![segment];
    (fake, state)
}

#[test]
fn a_native_forward_seek_accepts_the_skip_prompt_and_a_rewind_does_not() {
    let (fake, mut state) = controller_with_segment(
        None,
        SkipSegment {
            segment_type: SegmentType::Outro,
            start_ticks: 100_000_000,
            end_ticks: 200_000_000,
            triggered: false,
        },
    );
    state.handle_session_event(SESSION, &mpv_event("file-loaded", None, None));
    state.handle_session_event(SESSION, &mpv_event("playback-restart", None, None));

    time_pos(&mut state, 15.0);
    state.handle_session_event(SESSION, &mpv_event("seek", None, None));
    time_pos(&mut state, 12.0);
    assert!(
        !first_segment_triggered(&state),
        "a rewind inside credits is not a skip"
    );

    state.handle_session_event(SESSION, &mpv_event("seek", None, None));
    state.handle_session_event(SESSION, &seeking_event(false));
    assert!(
        !first_segment_triggered(&state),
        "no decision before the new position"
    );
    time_pos(&mut state, 13.0);

    assert!(first_segment_triggered(&state));
    assert_eq!(
        next_seek_command(&fake),
        json!(["seek", 20.0, "absolute+exact"])
    );
    fake.finish(state.ipc.worker.take().expect("worker"));
}

#[test]
fn resuming_inside_a_prompt_segment_does_not_trigger_seek_to_skip() {
    let (fake, mut state) = controller_with_segment(
        Some(300_000_000),
        SkipSegment {
            segment_type: SegmentType::Intro,
            start_ticks: 0,
            end_ticks: 900_000_000,
            triggered: false,
        },
    );
    state.prepare_pending_playback_state();
    state.handle_session_event(SESSION, &mpv_event("file-loaded", None, None));
    state.handle_session_event(SESSION, &mpv_event("playback-restart", None, None));
    time_pos(&mut state, 0.0);
    time_pos(&mut state, 0.2);

    state.startup_seek.as_mut().expect("startup seek").due_at = Instant::now();
    state.maybe_send_startup_seek();
    assert_eq!(
        next_seek_command(&fake),
        json!(["seek", 30.0, "absolute+exact"])
    );

    state.handle_session_event(SESSION, &mpv_event("seek", None, None));
    state.handle_session_event(SESSION, &seeking_event(true));
    time_pos(&mut state, 30.05);
    state.handle_session_event(SESSION, &mpv_event("playback-restart", None, None));
    state.handle_session_event(SESSION, &seeking_event(false));
    time_pos(&mut state, 30.5);

    assert!(
        !first_segment_triggered(&state),
        "startup resume seek inside a prompt intro must not auto-skip it"
    );
    assert_eq!(state.last_state.position_ticks, 305_000_000);

    state.handle_session_event(SESSION, &mpv_event("seek", None, None));
    state.handle_session_event(SESSION, &seeking_event(true));
    time_pos(&mut state, 35.0);
    state.handle_session_event(SESSION, &mpv_event("playback-restart", None, None));
    state.handle_session_event(SESSION, &seeking_event(false));

    assert!(
        first_segment_triggered(&state),
        "a user forward seek after startup resume still skips the intro"
    );
    assert_eq!(
        next_seek_command(&fake),
        json!(["seek", 90.0, "absolute+exact"])
    );
    fake.finish(state.ipc.worker.take().expect("worker"));
}

#[test]
fn starting_at_zero_inside_a_prompt_segment_ignores_seeking_before_first_playback_restart() {
    let (fake, mut state) = controller_with_segment(
        None,
        SkipSegment {
            segment_type: SegmentType::Intro,
            start_ticks: 0,
            end_ticks: 900_000_000,
            triggered: false,
        },
    );
    state.prepare_pending_playback_state();
    state.handle_session_event(SESSION, &seeking_event(true));
    state.handle_session_event(SESSION, &mpv_event("file-loaded", None, None));
    state.handle_session_event(SESSION, &seeking_event(true));
    state.handle_session_event(SESSION, &mpv_event("seek", None, None));
    state.handle_session_event(SESSION, &mpv_event("playback-restart", None, None));
    state.handle_session_event(SESSION, &seeking_event(false));
    time_pos(&mut state, 0.5);

    assert!(
        !first_segment_triggered(&state),
        "seeking before first playback-restart must not skip an intro starting at zero"
    );
    assert_eq!(state.last_state.position_ticks, 5_000_000);

    state.handle_session_event(SESSION, &seeking_event(true));
    time_pos(&mut state, 5.0);
    state.handle_session_event(SESSION, &seeking_event(false));

    assert!(
        first_segment_triggered(&state),
        "a user forward seek after first playback-restart still skips the intro"
    );
    assert_eq!(
        next_seek_command(&fake),
        json!(["seek", 90.0, "absolute+exact"])
    );
    fake.finish(state.ipc.worker.take().expect("worker"));
}

#[test]
fn startup_seek_landing_before_playback_restart_ignores_trailing_seeking_signal() {
    let (fake, mut state) = controller_with_segment(
        Some(300_000_000),
        SkipSegment {
            segment_type: SegmentType::Intro,
            start_ticks: 0,
            end_ticks: 900_000_000,
            triggered: false,
        },
    );
    state.prepare_pending_playback_state();
    state.handle_session_event(SESSION, &mpv_event("file-loaded", None, None));
    state.handle_session_event(SESSION, &mpv_event("playback-restart", None, None));

    state.startup_seek.as_mut().expect("startup seek").due_at = Instant::now();
    state.maybe_send_startup_seek();
    assert_eq!(
        next_seek_command(&fake),
        json!(["seek", 30.0, "absolute+exact"])
    );

    time_pos(&mut state, 29.8);
    state.handle_session_event(SESSION, &seeking_event(true));
    state.handle_session_event(SESSION, &mpv_event("playback-restart", None, None));
    state.handle_session_event(SESSION, &seeking_event(false));
    time_pos(&mut state, 30.2);

    assert!(
        !first_segment_triggered(&state),
        "startup seek signals before its playback-restart must not skip the intro"
    );
    assert_eq!(state.last_state.position_ticks, 302_000_000);
    fake.finish(state.ipc.worker.take().expect("worker"));
}

#[test]
fn replacing_active_playback_ignores_stale_restart_and_pre_restart_seeking() {
    let (fake, mut state) = controller_with_segment(
        None,
        SkipSegment {
            segment_type: SegmentType::Intro,
            start_ticks: 0,
            end_ticks: 900_000_000,
            triggered: false,
        },
    );
    state.handle_session_event(SESSION, &mpv_event("file-loaded", None, None));
    state.handle_session_event(SESSION, &mpv_event("playback-restart", None, None));
    assert!(state.playback_restarted);

    let replacement = PlaybackRequest::new("https://example.test/episode-2.mkv");
    state.phase = PlaybackPhase::loading(PendingPlayback {
        key: "episode-2".to_string(),
        identity: PlaybackIdentity::from_launch(2, &replacement),
        launch: replacement,
        reporter: None,
        requested_at: Instant::now(),
    });
    state.prepare_pending_playback_state();
    state.handle_session_event(SESSION, &mpv_event("playback-restart", None, None));
    assert!(
        !state.playback_restarted,
        "playback-restart before replacement file-loaded must be ignored"
    );

    state.handle_session_event(SESSION, &mpv_event("file-loaded", None, None));
    state.handle_session_event(SESSION, &seeking_event(true));
    state.handle_session_event(SESSION, &mpv_event("playback-restart", None, None));
    state.handle_session_event(SESSION, &seeking_event(false));
    time_pos(&mut state, 0.5);

    assert!(
        !first_segment_triggered(&state),
        "pre-restart seeking on replacement playback must not auto-skip the intro"
    );

    state.control(&PlayerCommand::SeekMilliseconds(10_000.0));
    assert!(
        first_segment_triggered(&state),
        "a user UI forward seek after startup still skips the intro"
    );
    assert_eq!(
        next_seek_command(&fake),
        json!(["seek", 90.0, "absolute+exact"])
    );
    fake.finish(state.ipc.worker.take().expect("worker"));
}
