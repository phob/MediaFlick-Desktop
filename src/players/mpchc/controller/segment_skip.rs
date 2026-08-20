use std::thread;
use std::time::{Duration, Instant};

use crate::jellyfin::media_segments;
use crate::playback::segments::{self, SkipSegment};
use crate::playback::{PlaybackRequest, TICKS_PER_SECOND, seconds_to_ticks};
use crate::preferences::{SegmentSkipConfig, SegmentSkipMode};

use super::{Msg, State};
use crate::players::mpchc::protocol;

const OSD_DURATION_MS: i32 = 3000;
const OSD_DEBOUNCE: Duration = Duration::from_secs(3);
const AUTO_SKIP_DELAY: Duration = Duration::from_secs(3);
const AUTO_SKIP_COUNTDOWN_INTERVAL: Duration = Duration::from_secs(1);
const AUTO_SKIP_COUNTDOWN_OSD_DURATION_MS: i32 = 1200;

#[derive(Clone, Copy)]
pub(super) struct PendingAutoSkip {
    segment_index: usize,
    due_at: Instant,
    next_countdown_at: Instant,
}

impl State {
    pub(super) fn apply_segment_skip_config(&mut self, config: SegmentSkipConfig) {
        self.segment_skip_config = config;
        self.pending_auto_skip = None;
        if config.all_disabled() {
            self.skip_segments.clear();
            self.current_skip_segment = None;
        } else {
            self.update_skip_state(self.last_state.position_ticks);
        }
    }

    pub(super) fn fetch_media_segments(&self, playback_id: i64, launch: PlaybackRequest) {
        if self.segment_skip_config.all_disabled() {
            return;
        }
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = media_segments::fetch_for_launch(&launch);
            let _ = tx.send(Msg::MediaSegments {
                playback_id,
                result,
            });
        });
    }

    pub(super) fn handle_media_segments(
        &mut self,
        playback_id: i64,
        result: Result<Vec<SkipSegment>, String>,
    ) {
        if !self.playback_id_is_current(playback_id) || self.segment_skip_config.all_disabled() {
            return;
        }
        match result {
            Ok(segments) => {
                self.skip_segments = segments;
                self.current_skip_segment = None;
                self.last_skip_osd_at = None;
                self.update_skip_state(self.last_state.position_ticks);
            }
            Err(error) => {
                tracing::warn!(target: "mpchc", playback_id, "failed to fetch media segments: {error}")
            }
        }
    }

    fn playback_id_is_current(&self, playback_id: i64) -> bool {
        self.identity
            .as_ref()
            .is_some_and(|identity| identity.playback_id == playback_id)
    }

    pub(super) fn update_skip_state(&mut self, ticks: i64) {
        if self.skip_segments.is_empty() || self.segment_skip_config.all_disabled() {
            self.current_skip_segment = None;
            self.pending_auto_skip = None;
            return;
        }
        let Some(index) = segments::active_segment_at(&self.skip_segments, ticks) else {
            self.current_skip_segment = None;
            self.pending_auto_skip = None;
            return;
        };
        let entered = self.current_skip_segment != Some(index);
        self.current_skip_segment = Some(index);
        match segments::mode_for_segment(
            &self.segment_skip_config,
            self.skip_segments[index].segment_type,
        ) {
            SegmentSkipMode::Disabled => self.pending_auto_skip = None,
            SegmentSkipMode::Prompt => {
                self.pending_auto_skip = None;
                self.maybe_show_prompt(index, entered);
            }
            SegmentSkipMode::Always => self.start_auto_skip(index),
        }
    }

    fn maybe_show_prompt(&mut self, index: usize, entered: bool) {
        let now = Instant::now();
        if !entered
            && self
                .last_skip_osd_at
                .is_some_and(|shown| now.saturating_duration_since(shown) < OSD_DEBOUNCE)
        {
            return;
        }
        let Some(segment) = self.skip_segments.get(index) else {
            return;
        };
        let text = segment.segment_type.prompt_text();
        self.show_osd(text, OSD_DURATION_MS);
        self.last_skip_osd_at = Some(now);
    }

    fn start_auto_skip(&mut self, index: usize) {
        if self
            .pending_auto_skip
            .is_some_and(|pending| pending.segment_index == index)
        {
            return;
        }
        if self
            .skip_segments
            .get(index)
            .is_none_or(|segment| segment.triggered)
        {
            return;
        }
        let now = Instant::now();
        self.pending_auto_skip = Some(PendingAutoSkip {
            segment_index: index,
            due_at: now + AUTO_SKIP_DELAY,
            next_countdown_at: now + AUTO_SKIP_COUNTDOWN_INTERVAL,
        });
        self.show_auto_skip_countdown(index, AUTO_SKIP_DELAY.as_secs().max(1));
    }

    pub(super) fn maybe_update_auto_skip(&mut self) {
        let Some(pending) = self.pending_auto_skip else {
            return;
        };
        if !self.auto_skip_valid(pending.segment_index) {
            self.pending_auto_skip = None;
            return;
        }
        let now = Instant::now();
        if now >= pending.due_at {
            self.pending_auto_skip = None;
            self.skip_segment(pending.segment_index, "automatic segment skip");
            return;
        }
        if now >= pending.next_countdown_at {
            let remaining = pending
                .due_at
                .saturating_duration_since(now)
                .as_millis()
                .div_ceil(1000)
                .max(1) as u64;
            self.show_auto_skip_countdown(pending.segment_index, remaining);
            if let Some(current) = &mut self.pending_auto_skip {
                current.next_countdown_at = now + AUTO_SKIP_COUNTDOWN_INTERVAL;
            }
        }
    }

    fn auto_skip_valid(&self, index: usize) -> bool {
        self.skip_segments.get(index).is_some_and(|segment| {
            !segment.triggered
                && segments::mode_for_segment(&self.segment_skip_config, segment.segment_type)
                    == SegmentSkipMode::Always
                && self.last_state.position_ticks >= segment.start_ticks
                && self.last_state.position_ticks < segment.end_ticks
        })
    }

    fn show_auto_skip_countdown(&self, index: usize, remaining_seconds: u64) {
        let Some(segment) = self.skip_segments.get(index) else {
            return;
        };
        let label = segment.segment_type.countdown_label();
        self.show_osd(
            &format!("Skipping {label} in {remaining_seconds}..."),
            AUTO_SKIP_COUNTDOWN_OSD_DURATION_MS,
        );
    }

    pub(super) fn handle_prompt_skip(&mut self, position_ms: f64) -> bool {
        let Some(requested) = seconds_to_ticks(position_ms / 1000.0) else {
            return false;
        };
        let current = self.last_state.position_ticks;
        if requested <= current {
            return false;
        }
        let Some(index) =
            segments::prompt_segment_at(&self.skip_segments, &self.segment_skip_config, current)
        else {
            return false;
        };
        self.skip_segment(index, "forward seek accepted skip prompt")
    }

    pub(super) fn maybe_accept_seek_skip(&mut self, previous: i64, current: i64) {
        if current <= previous {
            return;
        }
        if let Some(index) =
            segments::prompt_segment_at(&self.skip_segments, &self.segment_skip_config, previous)
        {
            self.skip_segment(index, "native forward seek accepted skip prompt");
        }
    }

    fn skip_segment(&mut self, index: usize, reason: &str) -> bool {
        let Some(segment) = self.skip_segments.get(index) else {
            return false;
        };
        if segment.triggered {
            return false;
        }
        let end_ticks = segment.end_ticks;
        let segment_type = segment.segment_type;
        if end_ticks <= self.last_state.position_ticks {
            self.current_skip_segment = None;
            self.pending_auto_skip = None;
            self.mark_triggered(index);
            return false;
        }
        let seconds = end_ticks as f64 / TICKS_PER_SECOND;
        if !self.send_command(protocol::CMD_SETPOSITION, &format!("{seconds:.3}")) {
            tracing::warn!(target: "mpchc", reason, end_ticks, "skip seek failed to send; leaving segment for retry");
            return false;
        }
        self.current_skip_segment = None;
        self.pending_auto_skip = None;
        self.last_skip_osd_at = Some(Instant::now());
        self.mark_triggered(index);
        tracing::info!(target: "mpchc", reason, end_ticks, "skipped media segment");
        self.show_osd(segment_type.skipped_text(), OSD_DURATION_MS);
        true
    }

    fn mark_triggered(&mut self, index: usize) {
        if let Some(segment) = self.skip_segments.get_mut(index) {
            segment.triggered = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};

    use crate::playback::segments::SegmentType;
    use crate::playback::{PlayerSnapshot, ReportingState};
    use crate::preferences::FullscreenBehavior;

    use super::*;
    use crate::players::mpchc::transport::MpcHcTransport;

    fn credits_segment() -> SkipSegment {
        SkipSegment {
            segment_type: SegmentType::Outro,
            start_ticks: 100_000_000,
            end_ticks: 200_000_000,
            triggered: false,
        }
    }

    fn state_with_transport(transport: Option<MpcHcTransport>) -> State {
        let (tx, rx) = mpsc::channel();
        State {
            tx,
            rx,
            snapshot: Arc::new(Mutex::new(PlayerSnapshot::default())),
            event_tx: None,
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            transport,
            inbound: None,
            child: None,
            connected: false,
            last_state: ReportingState::default(),
            pending: None,
            active: None,
            identity: None,
            playback_active: true,
            awaiting_open: false,
            resume_seconds: None,
            last_position_poll: Instant::now(),
            skip_segments: vec![credits_segment()],
            segment_skip_config: SegmentSkipConfig::default(),
            current_skip_segment: None,
            pending_auto_skip: None,
            last_skip_osd_at: None,
            recent_loads: VecDeque::new(),
            fullscreen_pref: FullscreenBehavior::default(),
            fullscreen_state: false,
            target_volume: 100.0,
            believed_output: 100.0,
            muted: false,
            seeking_osd: false,
        }
    }

    #[test]
    fn always_skip_starts_countdown_without_immediate_trigger() {
        let mut state = state_with_transport(None);
        state.segment_skip_config.credits = SegmentSkipMode::Always;

        state.update_skip_state(150_000_000);

        assert_eq!(
            state.pending_auto_skip.map(|pending| pending.segment_index),
            Some(0)
        );
        assert!(!state.skip_segments[0].triggered);
    }

    #[test]
    fn always_skip_countdown_cancels_after_leaving_segment() {
        let mut state = state_with_transport(None);
        state.segment_skip_config.credits = SegmentSkipMode::Always;
        state.update_skip_state(150_000_000);

        state.update_skip_state(250_000_000);

        assert!(state.pending_auto_skip.is_none());
        assert!(!state.skip_segments[0].triggered);
    }

    #[test]
    fn prompt_skip_accepts_only_forward_seeks() {
        let (transport, _) = MpcHcTransport::spawn().expect("test transport should start");
        let mut state = state_with_transport(Some(transport));
        state.last_state.position_ticks = 150_000_000;

        assert!(!state.handle_prompt_skip(14_000.0));
        assert!(!state.skip_segments[0].triggered);
        assert!(state.handle_prompt_skip(16_000.0));
        assert!(state.skip_segments[0].triggered);
    }

    #[test]
    fn failed_transport_send_leaves_segment_for_retry() {
        let mut state = state_with_transport(None);
        state.last_state.position_ticks = 150_000_000;

        assert!(!state.handle_prompt_skip(16_000.0));
        assert!(!state.skip_segments[0].triggered);
    }
}
