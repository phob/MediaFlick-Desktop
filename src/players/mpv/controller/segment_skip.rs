use std::thread;
use std::time::Instant;

use serde_json::{Value, json};

use super::super::commands::next_request_id;
use super::{
    CHAPTER_MARKER_MAX_ATTEMPTS, CHAPTER_MARKER_RETRY_INTERVAL, ControllerMessage, ControllerState,
    SEGMENT_AUTO_SKIP_COUNTDOWN_OSD_DURATION_MS, SEGMENT_SKIP_OSD_DURATION_MS,
    STARTUP_SEEK_POSITION_TOLERANCE,
};
use crate::jellyfin::media_segments;
use crate::playback::segments::{self, SegmentSkipAction, SkipSegment};
use crate::playback::{PlaybackRequest, PlayerCommand, TICKS_PER_SECOND, seconds_to_ticks};

impl ControllerState {
    pub(super) fn handle_media_segments_fetched(
        &mut self,
        playback_id: i64,
        result: Result<Vec<SkipSegment>, String>,
    ) {
        if !self.playback_id_is_current(playback_id) {
            tracing::debug!(
                target: "jellyfin.media_segments",
                playback_id,
                "ignored stale Jellyfin media segments response"
            );
            return;
        }
        if self.segment_skip_config.all_disabled() {
            tracing::debug!(
                target: "jellyfin.media_segments",
                playback_id,
                "ignored Jellyfin media segments because segment skipping is disabled"
            );
            return;
        }
        match result {
            Ok(segments) => {
                tracing::debug!(
                    target: "jellyfin.media_segments",
                    playback_id,
                    count = segments.len(),
                    "stored Jellyfin media segments"
                );
                self.skip_segments = segments;
                self.segment_skip_state.clear();
                self.update_skip_segment_state(self.last_state.position_ticks);
                self.refresh_chapter_markers();
            }
            Err(error) => tracing::warn!(
                target: "jellyfin.media_segments",
                playback_id,
                "failed to fetch Jellyfin media segments: {error}"
            ),
        }
    }

    pub(super) fn fetch_media_segments(&self, playback_id: i64, launch: PlaybackRequest) {
        if self.segment_skip_config.all_disabled() {
            return;
        }
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = media_segments::fetch_for_launch(&launch);
            let _ = tx.send(ControllerMessage::MediaSegmentsFetched {
                playback_id,
                result,
            });
        });
    }

    fn playback_id_is_current(&self, playback_id: i64) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.identity.playback_id == playback_id)
            || self
                .active
                .as_ref()
                .is_some_and(|active| active.identity.playback_id == playback_id)
            || (self.mpv_playback_active
                && self
                    .playback_identity
                    .as_ref()
                    .is_some_and(|identity| identity.playback_id == playback_id))
    }

    pub(super) fn clear_skip_segment_state(&mut self) {
        self.skip_segments.clear();
        self.segment_skip_state.clear();
        self.seek_started_at_ticks = None;
    }

    pub(super) fn reset_chapter_markers(&mut self) {
        self.original_chapters = None;
        self.injected_chapter_markers.clear();
        self.last_sent_chapter_list = None;
        self.pending_chapter_markers = None;
        self.chapter_marker_attempts = 0;
        self.chapter_marker_next_attempt_at = None;
    }

    pub(super) fn handle_chapter_list_event(&mut self, chapters: Vec<Value>) {
        if !self.injected_chapter_markers.is_empty()
            && self
                .injected_chapter_markers
                .iter()
                .all(|marker| chapters.contains(marker))
        {
            self.pending_chapter_markers = None;
            self.chapter_marker_next_attempt_at = None;
            let base = chapters
                .iter()
                .filter(|chapter| !self.injected_chapter_markers.contains(chapter))
                .cloned()
                .collect::<Vec<_>>();
            self.original_chapters = Some(base);
            self.last_sent_chapter_list = Some(chapters);
            return;
        }
        self.capture_original_chapters(chapters);
    }

    pub(super) fn capture_original_chapters(&mut self, chapters: Vec<Value>) {
        let base = chapters
            .into_iter()
            .filter(|chapter| !self.injected_chapter_markers.contains(chapter))
            .collect::<Vec<_>>();
        if self.original_chapters.as_deref() == Some(base.as_slice()) {
            return;
        }
        self.original_chapters = Some(base);
        self.refresh_chapter_markers();
    }

    pub(super) fn refresh_chapter_markers(&mut self) {
        if !self.mpv_playback_active {
            return;
        }

        let markers = if self.skip_segments.is_empty() {
            Vec::new()
        } else {
            if self.original_chapters.is_none() {
                return;
            }
            let Some(duration_ticks) = self.last_state.duration_ticks.filter(|ticks| *ticks > 0)
            else {
                return;
            };
            let duration_seconds = duration_ticks as f64 / TICKS_PER_SECOND;
            build_segment_chapter_markers(&self.skip_segments, duration_seconds)
        };

        let base = self.original_chapters.clone().unwrap_or_default();

        if markers.is_empty() {
            self.injected_chapter_markers.clear();
            self.pending_chapter_markers = None;
            self.chapter_marker_next_attempt_at = None;
            if self.last_sent_chapter_list.take().is_some() {
                let _ = self.send_chapter_list(&base);
            }
            return;
        }

        self.injected_chapter_markers.clone_from(&markers);
        self.queue_chapter_list(merge_chapter_markers(base, markers));
    }

    fn queue_chapter_list(&mut self, list: Vec<Value>) {
        if self.pending_chapter_markers.is_none()
            && self.last_sent_chapter_list.as_ref() == Some(&list)
        {
            return;
        }
        if self.pending_chapter_markers.as_ref() == Some(&list) {
            return;
        }
        self.pending_chapter_markers = Some(list);
        self.chapter_marker_attempts = 0;
        self.chapter_marker_next_attempt_at = Some(Instant::now());
    }

    pub(super) fn maybe_apply_chapter_markers(&mut self) {
        let Some(list) = self.pending_chapter_markers.clone() else {
            return;
        };
        if !self.mpv_playback_active {
            return;
        }
        let now = Instant::now();
        if self
            .chapter_marker_next_attempt_at
            .is_some_and(|at| now < at)
        {
            return;
        }
        if self.chapter_marker_attempts >= CHAPTER_MARKER_MAX_ATTEMPTS {
            tracing::debug!(
                target: "mpv.ipc",
                "gave up applying segment chapter markers after {CHAPTER_MARKER_MAX_ATTEMPTS} attempts"
            );
            self.pending_chapter_markers = None;
            self.chapter_marker_next_attempt_at = None;
            return;
        }
        self.chapter_marker_attempts += 1;
        self.last_sent_chapter_list = Some(list.clone());
        let _ = self.send_chapter_list(&list);
        self.chapter_marker_next_attempt_at = Some(now + CHAPTER_MARKER_RETRY_INTERVAL);
    }

    fn send_chapter_list(&self, chapters: &[Value]) -> bool {
        let command = json!({
            "command": ["set_property", "chapter-list", chapters],
            "request_id": next_request_id(),
        });
        match self.send_mpv_command(command) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", "failed to set mpv segment chapter markers: {error}");
                false
            }
        }
    }

    pub(super) fn handle_prompt_skip_control(&mut self, command: &PlayerCommand) -> bool {
        let PlayerCommand::SeekMilliseconds(position_ms) = command else {
            return false;
        };
        let Some(requested_ticks) = seconds_to_ticks(position_ms / 1000.0) else {
            return false;
        };
        let current_ticks = self.last_state.position_ticks;
        if requested_ticks <= current_ticks {
            return false;
        }
        let Some(index) = segments::prompt_segment_at(
            &self.skip_segments,
            &self.segment_skip_config,
            current_ticks,
        ) else {
            return false;
        };
        tracing::debug!(
            target: "playback",
            current_ticks,
            requested_ticks,
            "treating forward seek as segment skip"
        );
        self.skip_segment(index, "web forward seek accepted segment skip prompt")
    }

    pub(super) fn handle_seek_event(&mut self) {
        let current_ticks = self.last_state.position_ticks;
        if segments::prompt_segment_at(
            &self.skip_segments,
            &self.segment_skip_config,
            current_ticks,
        )
        .is_some()
        {
            self.seek_started_at_ticks = Some(current_ticks);
            tracing::debug!(
                target: "playback",
                current_ticks,
                "recorded native seek start inside prompt segment"
            );
        }
    }

    pub(super) fn handle_seeking_property(&mut self, seeking: bool) {
        if seeking {
            if self.seek_started_at_ticks.is_none() {
                self.seek_started_at_ticks = Some(self.last_state.position_ticks);
            }
            return;
        }
        self.maybe_accept_pending_native_seek(self.last_state.position_ticks);
    }

    pub(super) fn maybe_accept_pending_native_seek(&mut self, current_ticks: i64) -> bool {
        let Some(start_ticks) = self.seek_started_at_ticks else {
            return false;
        };
        if current_ticks == start_ticks {
            return false;
        }
        self.seek_started_at_ticks = None;
        if current_ticks < start_ticks {
            tracing::debug!(
                target: "playback",
                start_ticks,
                current_ticks,
                "ignored native backward seek during segment skip prompt"
            );
            return false;
        }
        let Some(index) = segments::prompt_segment_at(
            &self.skip_segments,
            &self.segment_skip_config,
            start_ticks,
        ) else {
            return false;
        };
        tracing::debug!(
            target: "playback",
            start_ticks,
            current_ticks,
            "treating native forward seek as segment skip"
        );
        self.skip_segment(index, "native forward seek accepted segment skip prompt")
    }

    pub(super) fn update_skip_segment_state(&mut self, current_ticks: i64) {
        let action = self.segment_skip_state.update(
            &self.skip_segments,
            &self.segment_skip_config,
            current_ticks,
            Instant::now(),
        );
        self.handle_segment_skip_action(action);
    }

    pub(super) fn maybe_update_auto_skip_countdown(&mut self) {
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
                if self.show_skip_prompt(index) {
                    self.segment_skip_state.mark_prompt_shown(Instant::now());
                }
            }
            Some(SegmentSkipAction::Countdown {
                segment_index,
                remaining_seconds,
            }) => self.show_auto_skip_countdown(segment_index, remaining_seconds),
            Some(SegmentSkipAction::Skip(index)) => {
                self.skip_segment(index, "automatic segment skip after countdown");
            }
            None => {}
        }
    }

    fn show_auto_skip_countdown(&self, index: usize, remaining_seconds: u64) {
        let Some(segment) = self.skip_segments.get(index) else {
            return;
        };
        let label = segment.segment_type.countdown_label();
        let command = json!({
            "command": [
                "show-text",
                format!("Skipping {label} in {remaining_seconds}..."),
                SEGMENT_AUTO_SKIP_COUNTDOWN_OSD_DURATION_MS,
                1
            ],
            "request_id": next_request_id(),
        });
        if let Err(error) = self.send_mpv_command(command) {
            tracing::warn!(target: "mpv.ipc", "failed to show automatic segment skip countdown: {error}");
        }
    }

    fn show_skip_prompt(&self, index: usize) -> bool {
        let Some(segment) = self.skip_segments.get(index) else {
            return false;
        };
        let command = json!({
            "command": ["show-text", segment.segment_type.prompt_text(), SEGMENT_SKIP_OSD_DURATION_MS, 1],
            "request_id": next_request_id(),
        });
        if let Err(error) = self.send_mpv_command(command) {
            tracing::warn!(target: "mpv.ipc", "failed to show segment skip prompt: {error}");
            return false;
        }
        true
    }

    fn skip_segment(&mut self, index: usize, reason: &'static str) -> bool {
        let current_ticks = self.last_state.position_ticks;
        let Some(segment) = self.skip_segments.get(index) else {
            return false;
        };
        if segment.triggered {
            return false;
        }
        let end_ticks = segment.end_ticks;
        let segment_type = segment.segment_type;

        self.segment_skip_state.finish_skip(Instant::now());
        if end_ticks <= current_ticks {
            self.mark_segment_triggered(index);
            tracing::debug!(
                target: "playback",
                reason,
                current_ticks,
                end_ticks,
                "segment skip target is behind current position"
            );
            return false;
        }

        let seconds = end_ticks as f64 / TICKS_PER_SECOND;
        let command = json!({
            "command": ["seek", seconds, "absolute+exact"],
            "request_id": next_request_id(),
        });
        match self.send_mpv_command(command) {
            Ok(()) => {
                self.mark_segment_triggered(index);
                tracing::info!(
                    target: "playback",
                    reason,
                    current_ticks,
                    end_ticks,
                    "skipped Jellyfin media segment"
                );
                let text = segment_type.skipped_text();
                let command = json!({
                    "command": ["show-text", text, SEGMENT_SKIP_OSD_DURATION_MS, 1],
                    "request_id": next_request_id(),
                });
                if let Err(error) = self.send_mpv_command(command) {
                    tracing::warn!(target: "mpv.ipc", "failed to show segment skipped OSD: {error}");
                }
                true
            }
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", reason, "failed to skip Jellyfin media segment: {error}");
                if error.is_transport() {
                    self.handle_mpv_session_lost("segment skip command transport failed");
                }
                false
            }
        }
    }

    fn mark_segment_triggered(&mut self, index: usize) {
        if let Some(segment) = self.skip_segments.get_mut(index) {
            segment.triggered = true;
        }
    }

    pub(super) fn defer_startup_position_update(&mut self, ticks: i64) -> bool {
        let Some(startup_seek) = self.startup_seek else {
            return false;
        };
        // mpv reports 0.0 immediately after file-loaded even for resumed media.
        // Do not let that transient sample overwrite Jellyfin/Web's resume
        // position before the delayed startup seek has landed.
        let target_ticks = seconds_to_ticks(startup_seek.position_ms / 1000.0).unwrap_or_default();
        let minimum_resume_tick = target_ticks.saturating_sub(STARTUP_SEEK_POSITION_TOLERANCE);
        if target_ticks > 0 && ticks < minimum_resume_tick {
            tracing::trace!(
                target: "playback",
                current_ticks = ticks,
                target_ticks,
                "holding Jellyfin position while mpv startup seek is pending"
            );
            return true;
        }

        tracing::debug!(
            target: "playback",
            current_ticks = ticks,
            target_ticks,
            seek_sent = startup_seek.sent_at.is_some(),
            "mpv startup seek reached resume range"
        );
        self.startup_seek = None;
        self.load_pending_external_subtitle();
        false
    }

    pub(super) fn log_position_change(&mut self, property: &str, previous: i64, current: i64) {
        tracing::trace!(
            target: "playback",
            property,
            previous_ticks = previous,
            current_ticks = current,
            state = %self.last_state,
            "mpv playback position changed"
        );

        let bucket = current / 100_000_000;
        if self.last_position_log_bucket != Some(bucket) {
            self.last_position_log_bucket = Some(bucket);
            tracing::trace!(
                target: "playback",
                property,
                previous_ticks = previous,
                current_ticks = current,
                state = %self.last_state,
                "mpv playback position sample"
            );
        }
    }
}

fn build_segment_chapter_markers(segments: &[SkipSegment], duration_seconds: f64) -> Vec<Value> {
    let mut markers: Vec<(f64, &'static str)> = Vec::with_capacity(segments.len() * 2);
    for segment in segments {
        let start = segment.start_ticks as f64 / TICKS_PER_SECOND;
        let end = segment.end_ticks as f64 / TICKS_PER_SECOND;
        markers.push((start, segment.segment_type.marker_start_label()));
        markers.push((end, segment.segment_type.marker_end_label()));
    }
    markers.retain(|(time, _)| *time > 0.0 && *time < duration_seconds);
    markers.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    markers.dedup_by(|a, b| (a.0 - b.0).abs() < 0.001);
    markers
        .into_iter()
        .map(|(time, title)| json!({ "title": title, "time": (time * 1000.0).round() / 1000.0 }))
        .collect()
}

fn merge_chapter_markers(base: Vec<Value>, markers: Vec<Value>) -> Vec<Value> {
    let mut chapters = base;
    chapters.extend(markers);
    chapters.sort_by(|a, b| {
        chapter_time(a)
            .partial_cmp(&chapter_time(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    chapters.dedup_by(|a, b| (chapter_time(a) - chapter_time(b)).abs() < 0.001);
    chapters
}

fn chapter_time(chapter: &Value) -> f64 {
    chapter
        .get("time")
        .and_then(Value::as_f64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
