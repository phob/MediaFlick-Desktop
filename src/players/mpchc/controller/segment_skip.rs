use std::thread;
use std::time::Instant;

use crate::jellyfin::media_segments;
use crate::playback::segments::{self, SegmentSkipAction, SkipSegment};
use crate::playback::{PlaybackRequest, TICKS_PER_SECOND, seconds_to_ticks};
use crate::preferences::SegmentSkipConfig;

use super::{Msg, State};
use crate::players::mpchc::protocol;

const OSD_DURATION_MS: i32 = 3000;
const AUTO_SKIP_COUNTDOWN_OSD_DURATION_MS: i32 = 1200;

impl State {
    pub(super) fn apply_segment_skip_config(&mut self, config: SegmentSkipConfig) {
        self.segment_skip_config = config;
        self.segment_skip_state.cancel_pending();
        if config.all_disabled() {
            self.skip_segments.clear();
            self.segment_skip_state.clear();
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
                self.segment_skip_state.clear();
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
        let action = self.segment_skip_state.update(
            &self.skip_segments,
            &self.segment_skip_config,
            ticks,
            Instant::now(),
        );
        self.handle_segment_skip_action(action);
    }

    pub(super) fn maybe_update_auto_skip(&mut self) {
        let action = self.segment_skip_state.tick(
            &self.skip_segments,
            &self.segment_skip_config,
            self.last_state.position_ticks,
            Instant::now(),
        );
        self.handle_segment_skip_action(action);
    }

    fn handle_segment_skip_action(&mut self, action: Option<SegmentSkipAction>) {
        match action {
            Some(SegmentSkipAction::Prompt(index)) => {
                if let Some(segment) = self.skip_segments.get(index) {
                    self.show_osd(segment.segment_type.prompt_text(), OSD_DURATION_MS);
                    self.segment_skip_state.mark_prompt_shown(Instant::now());
                }
            }
            Some(SegmentSkipAction::Countdown {
                segment_index,
                remaining_seconds,
            }) => self.show_auto_skip_countdown(segment_index, remaining_seconds),
            Some(SegmentSkipAction::Skip(index)) => {
                self.skip_segment(index, "automatic segment skip");
            }
            None => {}
        }
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
            self.segment_skip_state.finish_skip(Instant::now());
            self.mark_triggered(index);
            return false;
        }
        let seconds = end_ticks as f64 / TICKS_PER_SECOND;
        if !self.send_command(protocol::CMD_SETPOSITION, &format!("{seconds:.3}")) {
            tracing::warn!(target: "mpchc", reason, end_ticks, "skip seek failed to send; leaving segment for retry");
            return false;
        }
        self.segment_skip_state.finish_skip(Instant::now());
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
    use crate::preferences::{FullscreenBehavior, SegmentSkipMode};

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
            segment_skip_state: Default::default(),
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

        assert_eq!(state.segment_skip_state.pending_segment(), Some(0));
        assert!(!state.skip_segments[0].triggered);
    }

    #[test]
    fn always_skip_countdown_cancels_after_leaving_segment() {
        let mut state = state_with_transport(None);
        state.segment_skip_config.credits = SegmentSkipMode::Always;
        state.update_skip_state(150_000_000);

        state.update_skip_state(250_000_000);

        assert!(state.segment_skip_state.pending_segment().is_none());
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
