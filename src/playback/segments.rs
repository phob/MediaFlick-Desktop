#![cfg_attr(not(windows), allow(dead_code))]

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::preferences::{SegmentSkipConfig, SegmentSkipMode};

const AUTO_SKIP_DELAY: Duration = Duration::from_secs(3);
const AUTO_SKIP_COUNTDOWN_INTERVAL: Duration = Duration::from_secs(1);
const PROMPT_DEBOUNCE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

pub fn active_segment_at(segments: &[SkipSegment], ticks: i64) -> Option<usize> {
    segments.iter().position(|segment| {
        !segment.triggered && ticks >= segment.start_ticks && ticks < segment.end_ticks
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

    #[cfg(test)]
    pub fn pending_segment(&self) -> Option<usize> {
        self.pending_auto_skip.map(|pending| pending.segment_index)
    }

    pub fn update(
        &mut self,
        segments: &[SkipSegment],
        config: &SegmentSkipConfig,
        ticks: i64,
        now: Instant,
    ) -> Option<SegmentSkipAction> {
        let Some(index) = active_segment_at(segments, ticks).filter(|_| !config.all_disabled())
        else {
            self.current_segment = None;
            self.pending_auto_skip = None;
            return None;
        };
        let entered = self.current_segment != Some(index);
        self.current_segment = Some(index);
        match mode_for_segment(config, segments[index].segment_type) {
            SegmentSkipMode::Disabled => {
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
        assert_eq!(active_segment_at(&segments, 99), None);
        assert_eq!(active_segment_at(&segments, 100), Some(0));
        assert_eq!(active_segment_at(&segments, 199), Some(0));
        assert_eq!(active_segment_at(&segments, 200), None);
        segments[0].triggered = true;
        assert_eq!(active_segment_at(&segments, 150), None);
    }

    #[test]
    fn prompt_segment_only_matches_prompt_mode() {
        let segments = vec![segment(SegmentType::Intro, 100, 200)];
        let prompt = SegmentSkipConfig {
            intro: SegmentSkipMode::Prompt,
            ..SegmentSkipConfig::default()
        };
        let always = SegmentSkipConfig {
            intro: SegmentSkipMode::Always,
            ..SegmentSkipConfig::default()
        };
        assert_eq!(prompt_segment_at(&segments, &prompt, 150), Some(0));
        assert_eq!(prompt_segment_at(&segments, &always, 150), None);
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
}
