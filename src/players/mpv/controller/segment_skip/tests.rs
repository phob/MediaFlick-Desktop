use serde_json::json;

use crate::playback::segments::{SegmentType, SkipSegment};
use crate::preferences::SegmentSkipMode;

use super::super::ControllerState;
use super::super::test_support::controller_with_pending_load;
use super::{build_segment_chapter_markers, merge_chapter_markers};

fn add_prompt_credits_segment(state: &mut ControllerState) {
    state.skip_segments = vec![SkipSegment {
        segment_type: SegmentType::Outro,
        start_ticks: 100_000_000,
        end_ticks: 200_000_000,
        triggered: false,
    }];
}

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

#[test]
fn native_seek_event_records_prompt_segment_start_before_seeking_property() {
    let mut state = controller_with_pending_load(None);
    add_prompt_credits_segment(&mut state);
    state.last_state.position_ticks = 150_000_000;

    state.handle_seek_event();
    state.last_state.position_ticks = 155_000_000;
    state.handle_seeking_property(true);

    assert_eq!(state.seek_started_at_ticks, Some(150_000_000));
}

#[test]
fn native_seek_waits_for_position_update_when_seeking_false_is_early() {
    let mut state = controller_with_pending_load(None);
    add_prompt_credits_segment(&mut state);
    state.last_state.position_ticks = 150_000_000;

    state.handle_seek_event();
    state.handle_seeking_property(false);
    assert_eq!(state.seek_started_at_ticks, Some(150_000_000));

    state.maybe_accept_pending_native_seek(145_000_000);
    assert_eq!(state.seek_started_at_ticks, None);
}

#[test]
fn always_skip_starts_countdown_without_immediate_trigger() {
    let mut state = controller_with_pending_load(None);
    state.segment_skip_config.credits = SegmentSkipMode::Always;
    add_prompt_credits_segment(&mut state);

    state.update_skip_segment_state(150_000_000);

    assert_eq!(state.segment_skip_state.pending_segment(), Some(0));
    assert!(!state.skip_segments[0].triggered);
}

#[test]
fn always_skip_countdown_cancels_after_leaving_segment() {
    let mut state = controller_with_pending_load(None);
    state.segment_skip_config.credits = SegmentSkipMode::Always;
    add_prompt_credits_segment(&mut state);

    state.update_skip_segment_state(150_000_000);
    state.update_skip_segment_state(250_000_000);

    assert!(state.segment_skip_state.pending_segment().is_none());
    assert!(!state.skip_segments[0].triggered);
}
