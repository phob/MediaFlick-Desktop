use std::time::{Duration, Instant};

use serde::Serialize;

use crate::preferences::{SegmentSkipConfig, SegmentSkipMode};

const AUTO_SKIP_DELAY: Duration = Duration::from_secs(3);
const AUTO_SKIP_COUNTDOWN_INTERVAL: Duration = Duration::from_secs(1);
const PROMPT_DEBOUNCE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum SegmentType {
    Intro,
    Outro,
    Recap,
    Commercial,
}

impl SegmentType {
    pub fn prompt_text(self) -> &'static str {
        match self {
            Self::Intro => "Seek to Skip Intro",
            Self::Outro => "Seek to Skip Credits",
            Self::Recap => "Seek to Skip Recap",
            Self::Commercial => "Seek to Skip Commercial",
        }
    }

    pub fn skipped_text(self) -> &'static str {
        match self {
            Self::Intro => "Skipped Intro",
            Self::Outro => "Skipped Credits",
            Self::Recap => "Skipped Recap",
            Self::Commercial => "Skipped Commercial",
        }
    }

    pub fn countdown_label(self) -> &'static str {
        match self {
            Self::Intro => "Intro",
            Self::Outro => "Credits",
            Self::Recap => "Recap",
            Self::Commercial => "Commercial",
        }
    }

    pub fn marker_start_label(self) -> &'static str {
        self.countdown_label()
    }

    pub fn marker_end_label(self) -> &'static str {
        match self {
            Self::Intro => "Intro End",
            Self::Outro => "Credits End",
            Self::Recap => "Recap End",
            Self::Commercial => "Commercial End",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SkipSegment {
    pub segment_type: SegmentType,
    pub start_ticks: i64,
    pub end_ticks: i64,
    pub triggered: bool,
}

pub fn mode_for_segment(config: &SegmentSkipConfig, segment_type: SegmentType) -> SegmentSkipMode {
    match segment_type {
        SegmentType::Intro => config.intro,
        SegmentType::Outro => config.credits,
        SegmentType::Recap => config.recap,
        SegmentType::Commercial => config.commercial,
    }
}

pub fn is_shown(config: &SegmentSkipConfig, segment_type: SegmentType) -> bool {
    mode_for_segment(config, segment_type) != SegmentSkipMode::Hidden
}

pub fn active_segment_at(
    segments: &[SkipSegment],
    config: &SegmentSkipConfig,
    ticks: i64,
) -> Option<usize> {
    segments.iter().position(|segment| {
        !segment.triggered
            && is_shown(config, segment.segment_type)
            && ticks >= segment.start_ticks
            && ticks < segment.end_ticks
    })
}

pub fn prompt_segment_at(
    segments: &[SkipSegment],
    config: &SegmentSkipConfig,
    ticks: i64,
) -> Option<usize> {
    segments.iter().position(|segment| {
        !segment.triggered
            && mode_for_segment(config, segment.segment_type) == SegmentSkipMode::Prompt
            && ticks >= segment.start_ticks
            && ticks < segment.end_ticks
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentSkipAction {
    Prompt(usize),
    Countdown {
        segment_index: usize,
        remaining_seconds: u64,
    },
    Skip(usize),
}

#[derive(Debug, Default)]
pub struct SegmentSkipState {
    current_segment: Option<usize>,
    pending_auto_skip: Option<PendingAutoSkip>,
    last_prompt_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
struct PendingAutoSkip {
    segment_index: usize,
    due_at: Instant,
    next_countdown_at: Instant,
}

impl SegmentSkipState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn cancel_pending(&mut self) {
        self.pending_auto_skip = None;
    }

    pub fn update(
        &mut self,
        segments: &[SkipSegment],
        config: &SegmentSkipConfig,
        ticks: i64,
        now: Instant,
    ) -> Option<SegmentSkipAction> {
        let Some(index) = active_segment_at(segments, config, ticks) else {
            self.current_segment = None;
            self.pending_auto_skip = None;
            return None;
        };
        let entered = self.current_segment != Some(index);
        self.current_segment = Some(index);
        match mode_for_segment(config, segments[index].segment_type) {
            SegmentSkipMode::Hidden | SegmentSkipMode::Disabled => {
                self.pending_auto_skip = None;
                None
            }
            SegmentSkipMode::Prompt => {
                self.pending_auto_skip = None;
                (entered
                    || self.last_prompt_at.is_none_or(|shown| {
                        now.saturating_duration_since(shown) >= PROMPT_DEBOUNCE
                    }))
                .then_some(SegmentSkipAction::Prompt(index))
            }
            SegmentSkipMode::Always => self.start_auto_skip(segments, index, now),
        }
    }

    pub fn tick(
        &mut self,
        segments: &[SkipSegment],
        config: &SegmentSkipConfig,
        ticks: i64,
        now: Instant,
    ) -> Option<SegmentSkipAction> {
        let pending = self.pending_auto_skip?;
        let valid = segments.get(pending.segment_index).is_some_and(|segment| {
            !segment.triggered
                && mode_for_segment(config, segment.segment_type) == SegmentSkipMode::Always
                && ticks >= segment.start_ticks
                && ticks < segment.end_ticks
        });
        if !valid {
            self.pending_auto_skip = None;
            return None;
        }
        if now >= pending.due_at {
            self.pending_auto_skip = None;
            return Some(SegmentSkipAction::Skip(pending.segment_index));
        }
        if now < pending.next_countdown_at {
            return None;
        }
        let remaining_seconds = pending
            .due_at
            .saturating_duration_since(now)
            .as_millis()
            .div_ceil(1000)
            .max(1) as u64;
        if let Some(current) = &mut self.pending_auto_skip {
            current.next_countdown_at = now + AUTO_SKIP_COUNTDOWN_INTERVAL;
        }
        Some(SegmentSkipAction::Countdown {
            segment_index: pending.segment_index,
            remaining_seconds,
        })
    }

    pub fn mark_prompt_shown(&mut self, now: Instant) {
        self.last_prompt_at = Some(now);
    }

    pub fn finish_skip(&mut self, now: Instant) {
        self.current_segment = None;
        self.pending_auto_skip = None;
        self.last_prompt_at = Some(now);
    }

    fn start_auto_skip(
        &mut self,
        segments: &[SkipSegment],
        index: usize,
        now: Instant,
    ) -> Option<SegmentSkipAction> {
        if self
            .pending_auto_skip
            .is_some_and(|pending| pending.segment_index == index)
            || segments.get(index).is_none_or(|segment| segment.triggered)
        {
            return None;
        }
        self.pending_auto_skip = Some(PendingAutoSkip {
            segment_index: index,
            due_at: now + AUTO_SKIP_DELAY,
            next_countdown_at: now + AUTO_SKIP_COUNTDOWN_INTERVAL,
        });
        Some(SegmentSkipAction::Countdown {
            segment_index: index,
            remaining_seconds: AUTO_SKIP_DELAY.as_secs(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(segment_type: SegmentType, start: i64, end: i64) -> SkipSegment {
        SkipSegment {
            segment_type,
            start_ticks: start,
            end_ticks: end,
            triggered: false,
        }
    }

    #[test]
    fn active_segment_respects_bounds_and_triggered() {
        let mut segments = vec![segment(SegmentType::Intro, 100, 200)];
        let config = SegmentSkipConfig::default();
        assert_eq!(active_segment_at(&segments, &config, 99), None);
        assert_eq!(active_segment_at(&segments, &config, 100), Some(0));
        assert_eq!(active_segment_at(&segments, &config, 199), Some(0));
        assert_eq!(active_segment_at(&segments, &config, 200), None);
        segments[0].triggered = true;
        assert_eq!(active_segment_at(&segments, &config, 150), None);
    }

    #[test]
    fn automatic_skip_counts_down_then_fires() {
        let segments = vec![segment(SegmentType::Intro, 100, 200)];
        let config = SegmentSkipConfig {
            intro: SegmentSkipMode::Always,
            ..SegmentSkipConfig::default()
        };
        let started = Instant::now();
        let mut state = SegmentSkipState::default();

        assert_eq!(
            state.update(&segments, &config, 150, started),
            Some(SegmentSkipAction::Countdown {
                segment_index: 0,
                remaining_seconds: 3,
            })
        );
        assert_eq!(
            state.tick(&segments, &config, 150, started + Duration::from_secs(3)),
            Some(SegmentSkipAction::Skip(0))
        );
    }

    #[test]
    fn leaving_the_segment_cancels_the_automatic_skip_countdown() {
        let segments = vec![segment(SegmentType::Intro, 100, 200)];
        let config = SegmentSkipConfig {
            intro: SegmentSkipMode::Always,
            ..SegmentSkipConfig::default()
        };
        let started = Instant::now();
        let mut state = SegmentSkipState::default();
        let countdown = Some(SegmentSkipAction::Countdown {
            segment_index: 0,
            remaining_seconds: 3,
        });

        assert_eq!(state.update(&segments, &config, 150, started), countdown);
        let left = started + Duration::from_secs(1);
        assert_eq!(state.update(&segments, &config, 250, left), None);
        assert_eq!(state.tick(&segments, &config, 250, left), None);

        let returned = started + Duration::from_secs(2);
        assert_eq!(state.update(&segments, &config, 150, returned), countdown);
        let old_deadline = started + Duration::from_secs(3);
        assert_ne!(
            state.tick(&segments, &config, 150, old_deadline),
            Some(SegmentSkipAction::Skip(0))
        );
    }

    fn always_skip_intro() -> SegmentSkipConfig {
        SegmentSkipConfig {
            intro: SegmentSkipMode::Always,
            ..SegmentSkipConfig::default()
        }
    }

    fn countdown(remaining_seconds: u64) -> Option<SegmentSkipAction> {
        Some(SegmentSkipAction::Countdown {
            segment_index: 0,
            remaining_seconds,
        })
    }

    /// README: per-type auto-skip; a segment's end is exclusive. The skip is
    /// for this segment only while the position is inside it, it is not yet
    /// skipped and its type is still set to Always skip.
    #[test]
    fn a_pending_automatic_skip_never_fires_once_its_reason_is_gone() {
        let ask_me = SegmentSkipConfig::default();
        let cases = [
            ("seeked before the start", 99, false, always_skip_intro()),
            ("reached the end", 200, false, always_skip_intro()),
            ("seeked past the end", 250, false, always_skip_intro()),
            ("skipped by hand", 150, true, always_skip_intro()),
            ("switched to Ask me", 150, false, ask_me),
        ];
        for (case, ticks, skipped, later) in cases {
            let mut segments = vec![segment(SegmentType::Intro, 100, 200)];
            let started = Instant::now();
            let mut state = SegmentSkipState::default();
            assert_eq!(
                state.update(&segments, &always_skip_intro(), 150, started),
                countdown(3),
                "{case}"
            );
            segments[0].triggered = skipped;
            for second in 1..=4 {
                let now = started + Duration::from_secs(second);
                assert_eq!(
                    state.tick(&segments, &later, ticks, now),
                    None,
                    "{case} at {second} s"
                );
            }
        }
    }

    /// README: auto-skip with a countdown, one step per second down to the
    /// skip.
    #[test]
    fn the_automatic_skip_counts_down_once_a_second_then_skips() {
        let segments = vec![segment(SegmentType::Intro, 100, 200)];
        let started = Instant::now();
        let mut state = SegmentSkipState::default();
        assert_eq!(
            state.update(&segments, &always_skip_intro(), 150, started),
            countdown(3)
        );
        for (milliseconds, expected) in [
            (500, None),
            (999, None),
            (1000, countdown(2)),
            (1500, None),
            (2000, countdown(1)),
            (2999, None),
            (3000, Some(SegmentSkipAction::Skip(0))),
        ] {
            let now = started + Duration::from_millis(milliseconds);
            assert_eq!(
                state.tick(&segments, &always_skip_intro(), 150, now),
                expected,
                "{milliseconds} ms"
            );
        }
    }

    /// Playback reports its position several times a second; that must not
    /// restart the countdown, or the skip would never come.
    #[test]
    fn position_updates_inside_the_segment_keep_the_countdown_running() {
        let segments = vec![segment(SegmentType::Intro, 100, 200)];
        let started = Instant::now();
        let mut state = SegmentSkipState::default();
        assert_eq!(
            state.update(&segments, &always_skip_intro(), 150, started),
            countdown(3)
        );
        for step in 1..12_u64 {
            let now = started + Duration::from_millis(step * 250);
            let ticks = 150 + i64::try_from(step).expect("small");
            assert_eq!(
                state.update(&segments, &always_skip_intro(), ticks, now),
                None,
                "update {step}"
            );
        }
        assert_eq!(
            state.tick(
                &segments,
                &always_skip_intro(),
                165,
                started + Duration::from_secs(3)
            ),
            Some(SegmentSkipAction::Skip(0))
        );
    }

    /// features/playback.md: "Only Ask me sends Seek to Skip Intro to the
    /// OSD, every 3 s inside the segment", proven by the `segments` drive.
    /// Entering another segment prompts at once.
    #[test]
    fn ask_me_prompts_every_three_seconds_and_at_once_in_a_new_segment() {
        let segments = vec![
            segment(SegmentType::Intro, 100, 200),
            segment(SegmentType::Outro, 300, 400),
        ];
        let config = SegmentSkipConfig::default();
        let started = Instant::now();
        let at = |milliseconds| started + Duration::from_millis(milliseconds);
        let mut state = SegmentSkipState::default();

        assert_eq!(
            state.update(&segments, &config, 150, at(0)),
            Some(SegmentSkipAction::Prompt(0))
        );
        state.mark_prompt_shown(at(0));
        assert_eq!(state.update(&segments, &config, 151, at(1000)), None);
        assert_eq!(state.update(&segments, &config, 152, at(2999)), None);
        assert_eq!(
            state.update(&segments, &config, 153, at(3000)),
            Some(SegmentSkipAction::Prompt(0))
        );
        state.mark_prompt_shown(at(3000));
        assert_eq!(
            state.update(&segments, &config, 350, at(3500)),
            Some(SegmentSkipAction::Prompt(1))
        );
    }

    /// Same source: the prompt belongs to an Ask me segment that is not yet
    /// skipped, from its start up to, not including, its end.
    #[test]
    fn ask_me_prompts_only_inside_an_unskipped_ask_me_segment() {
        let mut segments = vec![
            segment(SegmentType::Intro, 100, 200),
            segment(SegmentType::Outro, 300, 400),
        ];
        let config = SegmentSkipConfig {
            credits: SegmentSkipMode::Always,
            ..SegmentSkipConfig::default()
        };
        for (ticks, expected) in [
            (99, None),
            (100, Some(0)),
            (199, Some(0)),
            (200, None),
            (350, None),
        ] {
            assert_eq!(
                prompt_segment_at(&segments, &config, ticks),
                expected,
                "{ticks}"
            );
        }
        segments[0].triggered = true;
        assert_eq!(prompt_segment_at(&segments, &config, 150), None);
    }

    /// The mpv controller cancels a pending skip when the skip settings
    /// change, and clears the state when it stores a new segment list, which
    /// then prompts at once.
    #[test]
    fn cancel_stops_a_pending_skip_and_clear_starts_afresh() {
        let segments = vec![segment(SegmentType::Intro, 100, 200)];
        let started = Instant::now();
        let mut state = SegmentSkipState::default();
        assert_eq!(
            state.update(&segments, &always_skip_intro(), 150, started),
            countdown(3)
        );
        state.cancel_pending();
        assert_eq!(
            state.tick(
                &segments,
                &always_skip_intro(),
                150,
                started + Duration::from_secs(3)
            ),
            None
        );

        let ask_me = SegmentSkipConfig::default();
        let mut state = SegmentSkipState::default();
        assert_eq!(
            state.update(&segments, &ask_me, 150, started),
            Some(SegmentSkipAction::Prompt(0))
        );
        state.mark_prompt_shown(started);
        state.clear();
        assert_eq!(
            state.update(&segments, &ask_me, 151, started + Duration::from_secs(1)),
            Some(SegmentSkipAction::Prompt(0))
        );
    }
}
