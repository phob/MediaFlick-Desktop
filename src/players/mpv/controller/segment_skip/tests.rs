use serde_json::{Value, json};

use crate::playback::segments::{SegmentType, SkipSegment};
use crate::players::mpv::ipc::MpvEvent;
use crate::players::mpv::ipc::test_server::{COMMAND_CONNECTION, FakeMpv};

use super::super::ControllerState;
use super::super::test_support::controller_with_pending_load;
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

    assert_eq!(
        markers,
        vec![
            json!({ "title": "Intro", "time": 10.0 }),
            json!({ "title": "Intro End", "time": 30.0 }),
            json!({ "title": "Credits", "time": 140.0 }),
        ]
    );
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

#[test]
fn merge_chapter_markers_drops_marker_coinciding_with_embedded_chapter() {
    let base = vec![json!({ "title": "Chapter", "time": 10.0 })];
    let markers = vec![json!({ "title": "Intro", "time": 10.0 })];

    let merged = merge_chapter_markers(base, markers);

    assert_eq!(merged, vec![json!({ "title": "Chapter", "time": 10.0 })]);
}

const SESSION: u64 = 1;

fn mpv_event(name: &str, property: Option<&str>, data: Option<Value>) -> MpvEvent {
    MpvEvent {
        name: name.to_string(),
        reason: None,
        property: property.map(str::to_string),
        data,
        args: Vec::new(),
        raw: json!({ "event": name }),
    }
}

fn time_pos(state: &mut ControllerState, seconds: f64) {
    state.handle_session_event(
        SESSION,
        &mpv_event("property-change", Some("time-pos"), Some(json!(seconds))),
    );
}

fn credits_triggered(state: &ControllerState) -> bool {
    state.publish_snapshot().skip_segments[0].triggered
}

/// The prompt reads "Seek to Skip Credits": a native forward seek inside the
/// segment skips to its end, while rewinding inside it does not. mpv can report
/// `seeking=false` before the new position, so the decision waits for it.
#[test]
fn a_native_forward_seek_accepts_the_skip_prompt_and_a_rewind_does_not() {
    let fake = FakeMpv::start(|command| {
        vec![json!({ "request_id": command["request_id"].clone(), "error": "success" })]
    });
    let (worker, _events) = fake.connect();
    let mut state = controller_with_pending_load(None);
    state.ipc.worker = Some(worker);
    state.ipc.active_id = Some(SESSION);
    state.skip_segments = vec![SkipSegment {
        segment_type: SegmentType::Outro,
        start_ticks: 100_000_000,
        end_ticks: 200_000_000,
        triggered: false,
    }];
    let seeking = |value: bool| mpv_event("property-change", Some("seeking"), Some(json!(value)));

    time_pos(&mut state, 15.0);
    state.handle_session_event(SESSION, &mpv_event("seek", None, None));
    time_pos(&mut state, 12.0);
    assert!(
        !credits_triggered(&state),
        "a rewind inside credits is not a skip"
    );

    state.handle_session_event(SESSION, &mpv_event("seek", None, None));
    state.handle_session_event(SESSION, &seeking(false));
    assert!(
        !credits_triggered(&state),
        "no decision before the new position"
    );
    time_pos(&mut state, 13.0);

    assert!(credits_triggered(&state));
    let seek = loop {
        let command = fake.next_command_on(COMMAND_CONNECTION);
        if command["command"][0] == "seek" {
            break command["command"].clone();
        }
    };
    assert_eq!(seek, json!(["seek", 20.0, "absolute+exact"]));
    fake.finish(state.ipc.worker.take().expect("worker"));
}
