#![cfg_attr(not(windows), allow(dead_code))]

use crate::preferences::{SegmentSkipConfig, SegmentSkipMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
}
