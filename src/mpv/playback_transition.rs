//! Playback state transitions for mpv-backed Jellyfin episodes.
//!
//! This module owns the fragile episode handoff decisions: pending-to-active,
//! EOF/mark-watched-next completion, stale stop suppression during next-episode
//! loads, and stopped-event snapshots sent back to Jellyfin Web.

use std::time::Instant;

use crate::app::logger;
use crate::jellyfin::bridge::PlaybackContext;
use crate::jellyfin::playback_reporter::seconds_to_ticks;

use super::{
    ActivePlayback, ControllerState, MpvPlaybackEvent, MpvPlayerSnapshot,
    NEXT_PLAYBACK_HANDOFF_TIMEOUT, PlaybackIdentity, is_completion_reason, normalized_stop_reason,
};

impl ControllerState {
    pub(super) fn should_suppress_stop_during_next_playback_handoff(&mut self) -> bool {
        if self.pending.is_none() {
            return false;
        }
        let Some(deadline) = self.next_playback_handoff_until else {
            return false;
        };
        if Instant::now() <= deadline {
            return true;
        }
        self.next_playback_handoff_until = None;
        false
    }

    pub(super) fn arm_next_playback_handoff(&mut self, reason: &'static str) {
        self.next_playback_handoff_until = Some(Instant::now() + NEXT_PLAYBACK_HANDOFF_TIMEOUT);
        tracing::debug!(
            target: "mpv.ipc",
            reason,
            "keeping mpv alive for next playback handoff"
        );
    }

    pub(super) fn publish_snapshot(&self) -> MpvPlayerSnapshot {
        self.publish_snapshot_with_stop_reason(None)
    }

    pub(super) fn publish_snapshot_with_stop_reason(
        &self,
        stop_reason: Option<&'static str>,
    ) -> MpvPlayerSnapshot {
        self.publish_snapshot_for_identity(stop_reason, self.current_identity())
    }

    fn publish_snapshot_for_identity(
        &self,
        stop_reason: Option<&'static str>,
        identity: Option<&PlaybackIdentity>,
    ) -> MpvPlayerSnapshot {
        let snapshot = MpvPlayerSnapshot {
            active: self.mpv_playback_active || self.active.is_some() || self.pending.is_some(),
            playback_id: identity.map(|identity| identity.playback_id),
            item_id: identity.and_then(|identity| identity.item_id.clone()),
            media_source_id: identity.and_then(|identity| identity.media_source_id.clone()),
            play_session_id: identity.and_then(|identity| identity.play_session_id.clone()),
            position_ms: self.last_state.position_ticks.max(0) as f64 / 10_000.0,
            duration_ms: self
                .last_state
                .duration_ticks
                .filter(|ticks| *ticks > 0)
                .map(|ticks| ticks as f64 / 10_000.0),
            paused: self.last_state.pause,
            volume: self.last_state.volume,
            mute: self.last_state.mute,
            stop_reason,
        };
        if let Ok(mut target) = self.snapshot.lock() {
            *target = snapshot.clone();
        }
        snapshot
    }

    fn current_identity(&self) -> Option<&PlaybackIdentity> {
        self.pending
            .as_ref()
            .map(|pending| &pending.identity)
            .or_else(|| self.active.as_ref().map(|active| &active.identity))
            .or(self.playback_identity.as_ref())
    }

    fn notify_playback_stopped(&self, snapshot: MpvPlayerSnapshot) {
        tracing::debug!(
            target: "playback",
            playback_id = ?snapshot.playback_id,
            item_id = %snapshot.item_id.as_deref().unwrap_or("unknown"),
            media_source_id = %snapshot.media_source_id.as_deref().unwrap_or("unknown"),
            play_session_id = %snapshot.play_session_id.as_deref().unwrap_or("unknown"),
            stop_reason = %snapshot.stop_reason.unwrap_or("unknown"),
            "notifying WebUI that mpv playback stopped"
        );
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(MpvPlaybackEvent::Stopped(snapshot));
        }
    }

    pub(super) fn update_active_playback_context(&mut self, context: PlaybackContext) {
        let mut updated = false;

        if let Some(pending) = &mut self.pending
            && identity_matches_context(&pending.identity, &context)
        {
            context.merge_into_launch(&mut pending.launch);
            update_identity_from_context(&mut pending.identity, &context);
            if let Some(reporter) = &mut pending.reporter {
                reporter.merge_context(&context);
            }
            updated = true;
        }

        if let Some(active) = &mut self.active
            && identity_matches_context(&active.identity, &context)
        {
            update_identity_from_context(&mut active.identity, &context);
            active.reporter.merge_context(&context);
            if active.runtime_ticks.is_none() {
                active.runtime_ticks = context.runtime_ticks.filter(|ticks| *ticks > 0);
            }
            if self.playback_runtime_ticks.is_none() {
                self.playback_runtime_ticks = active.runtime_ticks;
            }
            updated = true;
        }

        if let Some(identity) = &mut self.playback_identity
            && identity_matches_context(identity, &context)
        {
            update_identity_from_context(identity, &context);
            updated = true;
        }

        if updated {
            tracing::debug!(
                target: "playback",
                context = %playback_context_update_summary(&context),
                "merged playback context into active mpv playback"
            );
            self.publish_snapshot();
        }
    }

    pub(super) fn prepare_pending_playback_state(&mut self) {
        let Some(pending) = self.pending.as_ref() else {
            tracing::debug!(target: "playback", "no pending playback state to prepare");
            return;
        };
        self.last_state.position_ticks = pending
            .launch
            .start_seconds()
            .and_then(seconds_to_ticks)
            .unwrap_or_default();
        self.last_state.duration_ticks = pending.launch.runtime_ticks.filter(|ticks| *ticks > 0);
        self.last_state.pause = false;
        self.last_state.eof_reached = false;
        self.last_position_log_bucket = None;
    }

    pub(super) fn activate_pending(&mut self) {
        let Some(pending) = self.pending.take() else {
            tracing::debug!(target: "playback", "mpv reported file-loaded without pending playback");
            return;
        };
        tracing::info!(
            target: "playback",
            playback_id = pending.identity.playback_id,
            dedupe_key = %pending.key,
            launch = %logger::launch_summary(&pending.launch),
            state = %self.last_state,
            "activating pending playback"
        );
        self.last_state.position_ticks = pending
            .launch
            .start_seconds()
            .and_then(seconds_to_ticks)
            .unwrap_or_default();
        self.last_state.duration_ticks = None;
        self.last_state.pause = false;
        self.last_state.eof_reached = false;
        self.last_position_log_bucket = None;
        let launch = pending.launch.clone();
        let identity = pending.identity.clone();
        self.playback_identity = Some(identity.clone());
        self.playback_runtime_ticks = launch.runtime_ticks.filter(|ticks| *ticks > 0);
        if let Some(reporter) = pending.reporter {
            reporter.report_start(&self.last_state);
            self.active = Some(ActivePlayback {
                identity,
                reporter,
                runtime_ticks: launch.runtime_ticks.filter(|ticks| *ticks > 0),
                last_progress_sent: Instant::now(),
                last_pause: self.last_state.pause,
            });
        } else {
            tracing::debug!(
                target: "jellyfin.playstate",
                "activated playback without Jellyfin reporter"
            );
        }
        self.mpv_playback_active = true;
        self.next_playback_handoff_until = None;
        self.schedule_mpv_raise("file-loaded");
        self.load_external_subtitle(&launch);
        self.kick_start_playback(&launch);
        self.publish_snapshot();
    }

    pub(super) fn mark_watched_and_play_next(&mut self) {
        if self.active.is_none() && self.pending.is_none() && !self.mpv_playback_active {
            tracing::debug!(target: "playback", "ignored mark-watched-next because playback is idle");
            return;
        }

        let duration_ticks = self
            .last_state
            .duration_ticks
            .filter(|ticks| *ticks > 0)
            .or(self.playback_runtime_ticks)
            .or_else(|| self.active.as_ref().and_then(|active| active.runtime_ticks))
            .or_else(|| {
                self.pending
                    .as_ref()
                    .and_then(|pending| pending.launch.runtime_ticks.filter(|ticks| *ticks > 0))
            });
        if let Some(duration_ticks) = duration_ticks {
            self.last_state.duration_ticks = Some(duration_ticks);
            self.last_state.position_ticks = duration_ticks;
        } else {
            tracing::warn!(
                target: "playback",
                "mark-watched-next requested before a duration was known; reporting current position"
            );
        }

        tracing::info!(
            target: "playback",
            state = %self.last_state,
            "marking current item watched and requesting next item"
        );
        self.finish_active(Some("watched-next"));
        self.arm_next_playback_handoff("watched-next");
    }

    pub(super) fn finish_active(&mut self, reason: Option<&str>) {
        tracing::debug!(
            target: "playback",
            reason = reason.unwrap_or("unknown"),
            state = %self.last_state,
            "finishing playback"
        );
        let had_mpv_playback =
            self.mpv_playback_active || self.pending.is_some() || self.active.is_some();
        self.startup_seek = None;
        let failed = matches!(reason, Some("error"));
        let stop_reason = normalized_stop_reason(reason);
        if self.should_ignore_pending_end_file_during_next_playback_handoff(reason) {
            tracing::debug!(
                target: "playback",
                reason = reason.unwrap_or("unknown"),
                "ignored old mpv end-file while next playback handoff is pending"
            );
            return;
        }
        if let Some(pending) = self.pending.take() {
            self.next_playback_handoff_until = None;
            if let Some(reporter) = pending.reporter {
                if failed {
                    tracing::warn!(
                        target: "playback",
                        reason = reason.unwrap_or("unknown"),
                        "pending playback failed before activation"
                    );
                    reporter.report_stopped(&self.last_state, true);
                } else if is_completion_reason(reason) {
                    tracing::info!(
                        target: "playback",
                        reason = reason.unwrap_or("unknown"),
                        state = %self.last_state,
                        "reporting pending playback completed before activation"
                    );
                    reporter.report_stopped(&self.last_state, false);
                }
            }
        }

        if is_completion_reason(reason)
            && let Some(duration) = self.completion_duration_ticks()
        {
            self.last_state.duration_ticks = Some(duration);
            self.last_state.position_ticks = duration;
        }

        let Some(active) = self.active.take() else {
            self.mpv_playback_active = false;
            self.playback_runtime_ticks = None;
            if had_mpv_playback {
                let snapshot = self.publish_snapshot_with_stop_reason(stop_reason);
                self.notify_playback_stopped(snapshot);
                if reason.is_some_and(|reason| reason.eq_ignore_ascii_case("eof")) {
                    self.arm_next_playback_handoff("eof");
                }
            }
            tracing::trace!(target: "playback", "no active playback to finish");
            return;
        };
        self.mpv_playback_active = false;
        tracing::info!(
            target: "playback",
            failed,
            reason = reason.unwrap_or("unknown"),
            state = %self.last_state,
            "reporting active playback stopped"
        );
        active.reporter.report_stopped(&self.last_state, failed);
        self.playback_runtime_ticks = None;
        let snapshot = self.publish_snapshot_with_stop_reason(stop_reason);
        self.notify_playback_stopped(snapshot);
        if reason.is_some_and(|reason| reason.eq_ignore_ascii_case("eof")) {
            self.arm_next_playback_handoff("eof");
        }
    }

    fn completion_duration_ticks(&self) -> Option<i64> {
        self.last_state
            .duration_ticks
            .filter(|ticks| *ticks > 0)
            .or(self.playback_runtime_ticks)
            .or_else(|| self.active.as_ref().and_then(|active| active.runtime_ticks))
            .or_else(|| {
                self.pending
                    .as_ref()
                    .and_then(|pending| pending.launch.runtime_ticks.filter(|ticks| *ticks > 0))
            })
    }

    fn should_ignore_pending_end_file_during_next_playback_handoff(
        &mut self,
        reason: Option<&str>,
    ) -> bool {
        if self.pending.is_none() {
            return false;
        }
        if !matches!(reason, Some("stop" | "redirect")) {
            return false;
        }
        self.should_suppress_stop_during_next_playback_handoff()
    }
}

fn identity_matches_context(identity: &PlaybackIdentity, context: &PlaybackContext) -> bool {
    let mut matched = false;
    for (expected, actual) in [
        (identity.item_id.as_deref(), context.item_id.as_deref()),
        (
            identity.media_source_id.as_deref(),
            context.media_source_id.as_deref(),
        ),
        (
            identity.play_session_id.as_deref(),
            context.play_session_id.as_deref(),
        ),
    ] {
        let expected = non_empty(expected);
        let actual = non_empty(actual);
        let (Some(expected), Some(actual)) = (expected, actual) else {
            continue;
        };
        if expected != actual {
            return false;
        }
        matched = true;
    }
    matched
}

fn update_identity_from_context(identity: &mut PlaybackIdentity, context: &PlaybackContext) {
    fill_string(&mut identity.item_id, context.item_id.as_deref());
    fill_string(
        &mut identity.media_source_id,
        context.media_source_id.as_deref(),
    );
    fill_string(
        &mut identity.play_session_id,
        context.play_session_id.as_deref(),
    );
}

fn fill_string(target: &mut Option<String>, value: Option<&str>) {
    if non_empty(target.as_deref()).is_none()
        && let Some(value) = non_empty(value)
    {
        *target = Some(value.to_string());
    }
}

fn playback_context_update_summary(context: &PlaybackContext) -> String {
    format!(
        "item={} media_source={} play_session={} runtime={}",
        non_empty(context.item_id.as_deref()).unwrap_or("unknown"),
        non_empty(context.media_source_id.as_deref()).unwrap_or("unknown"),
        non_empty(context.play_session_id.as_deref()).unwrap_or("unknown"),
        context
            .runtime_ticks
            .map(|ticks| ticks.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    )
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
