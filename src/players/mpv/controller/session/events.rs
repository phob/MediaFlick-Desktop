use serde_json::Value;

use crate::app::logger;
use crate::playback::{PlayerCommand, PlayerTrack, PlayerTrackKind, StopReason, seconds_to_ticks};
use crate::players::mpv::input::MARK_WATCHED_NEXT_COMMAND;
use crate::players::mpv::ipc::MpvEvent;

use super::super::{ControllerMessage, ControllerState};

impl ControllerState {
    pub(super) fn start_event_relay(
        &self,
        session_id: u64,
        event_rx: std::sync::mpsc::Receiver<MpvEvent>,
    ) {
        let tx = self.tx.clone();
        crate::app::threads::spawn_named("mpv-event-relay", move || {
            while let Ok(event) = event_rx.recv() {
                if tx
                    .send(ControllerMessage::MpvEvent {
                        session_id,
                        event: Box::new(event),
                    })
                    .is_err()
                {
                    return;
                }
            }
            let _ = tx.send(ControllerMessage::MpvEventsDisconnected { session_id });
        });
    }

    pub(in crate::players::mpv::controller) fn handle_session_event(
        &mut self,
        session_id: u64,
        event: &MpvEvent,
    ) {
        if self.ipc.active_id != Some(session_id) {
            tracing::trace!(target: "mpv.ipc", session_id, "ignored event from stale mpv IPC session");
            return;
        }
        self.handle_event(event);
    }

    pub(in crate::players::mpv::controller) fn handle_event_stream_disconnected(
        &mut self,
        session_id: u64,
    ) {
        if self.ipc.active_id != Some(session_id) {
            return;
        }
        tracing::warn!(target: "mpv.ipc", session_id, "mpv IPC event stream disconnected");
        self.handle_mpv_session_lost("mpv IPC event stream disconnected");
    }

    fn handle_event(&mut self, event: &MpvEvent) {
        if event.is_position_property_change() {
            tracing::trace!(target: "mpv.ipc", event = %event.summary(), "received mpv position event");
        } else {
            tracing::debug!(target: "mpv.ipc", event = %event.summary(), "received mpv event");
        }
        tracing::trace!(
            target: "mpv.ipc",
            event = %logger::redacted_json(&event.raw),
            "received raw mpv event"
        );
        match event.name.as_str() {
            "file-loaded" => self.activate_pending(),
            "end-file" => {
                self.finish_active(event.reason.as_deref().and_then(StopReason::parse));
            }
            "shutdown" => {
                self.finish_active(Some(StopReason::Quit));
                self.reset_mpv();
                self.restart_configured_mpv("mpv emitted shutdown");
            }
            "seek" => {
                #[cfg(target_os = "linux")]
                {
                    self.fullscreen_gate.video_ready = false;
                    self.fullscreen_gate.waiting_seek_event = false;
                }
                self.handle_seek_event();
            }
            #[cfg(target_os = "linux")]
            "playback-restart" => self.fullscreen_gate.video_ready = true,
            "property-change" => {
                self.apply_property(event.property.as_deref(), event.data.as_ref());
            }
            #[cfg(target_os = "linux")]
            "client-message"
                if self.runtime_kind == super::super::MpvRuntimeKind::Library
                    && event
                        .args
                        .first()
                        .is_some_and(|arg| arg == "mediaflick-close-window") =>
            {
                crate::players::mpv::runtime::request_window_close();
            }
            "client-message" if is_mark_watched_next_message(&event.args) => {
                self.control(&PlayerCommand::MarkWatchedAndPlayNext);
            }
            _ => tracing::trace!(target: "mpv.ipc", name = %event.name, "ignored mpv event"),
        }
    }

    pub(super) fn apply_property(&mut self, property: Option<&str>, data: Option<&Value>) {
        tracing::trace!(
            target: "mpv.ipc",
            property = property.unwrap_or("unknown"),
            data = %data
                .map(logger::redacted_json)
                .unwrap_or_else(|| "null".to_string()),
            "applying mpv property"
        );
        match property {
            Some("time-pos" | "playback-time" | "pause" | "duration") => {
                self.apply_timeline_property(property, data);
            }
            Some("volume" | "mute") => self.apply_audio_property(property, data),
            Some("track-list") => {
                self.playback_tracks = playback_tracks(data);
                self.publish_snapshot();
            }
            Some(
                "paused-for-cache"
                | "demuxer-cache-time"
                | "vo-drop-frame-count"
                | "estimated-vf-fps",
            ) => self.apply_diagnostics_property(property, data),
            Some("eof-reached" | "seeking" | "chapter-list" | "playback-abort") => {
                self.apply_status_property(property, data);
            }
            Some(other) => {
                tracing::trace!(target: "mpv.ipc", property = other, "ignored mpv property")
            }
            None => tracing::trace!(target: "mpv.ipc", "ignored mpv property with no name"),
        }
    }

    fn apply_timeline_property(&mut self, property: Option<&str>, data: Option<&Value>) {
        match property {
            Some("time-pos" | "playback-time") => {
                if let Some(ticks) = data.and_then(Value::as_f64).and_then(seconds_to_ticks) {
                    if self.defer_startup_position_update(ticks) {
                        return;
                    }
                    let previous = self.last_state.position_ticks;
                    self.last_state.position_ticks = ticks;
                    self.log_position_change(property.unwrap_or("time"), previous, ticks);
                    if !self.maybe_accept_pending_native_seek(ticks) {
                        self.update_skip_segment_state(ticks);
                    }
                    self.publish_snapshot();
                }
            }
            Some("pause") => {
                if let Some(value) = data.and_then(Value::as_bool) {
                    let previous = self.last_state.pause;
                    self.last_state.pause = value;
                    if previous != value {
                        tracing::debug!(
                            target: "playback",
                            previous,
                            current = value,
                            state = %self.last_state,
                            "mpv pause state changed"
                        );
                    }
                    self.publish_snapshot();
                }
            }
            Some("duration") => {
                self.last_state.duration_ticks =
                    data.and_then(Value::as_f64).and_then(seconds_to_ticks);
                tracing::debug!(
                    target: "playback",
                    state = %self.last_state,
                    "mpv duration changed"
                );
                self.refresh_chapter_markers();
                self.publish_snapshot();
            }
            _ => {}
        }
    }

    fn apply_audio_property(&mut self, property: Option<&str>, data: Option<&Value>) {
        match property {
            Some("volume") => {
                let previous = self.last_state.volume;
                self.last_state.volume = data
                    .and_then(Value::as_f64)
                    .map(|value| value.round() as i64);
                if previous != self.last_state.volume {
                    tracing::debug!(
                        target: "playback",
                        previous = ?previous,
                        current = ?self.last_state.volume,
                        state = %self.last_state,
                        "mpv volume changed"
                    );
                }
                self.publish_snapshot();
            }
            Some("mute") => {
                let previous = self.last_state.mute;
                self.last_state.mute = data.and_then(Value::as_bool);
                if previous != self.last_state.mute {
                    tracing::debug!(
                        target: "playback",
                        previous = ?previous,
                        current = ?self.last_state.mute,
                        state = %self.last_state,
                        "mpv mute state changed"
                    );
                }
                self.publish_snapshot();
            }
            _ => {}
        }
    }

    fn apply_diagnostics_property(&mut self, property: Option<&str>, data: Option<&Value>) {
        match property {
            Some("paused-for-cache") => {
                self.playback_diagnostics.buffering =
                    data.and_then(Value::as_bool).unwrap_or(false);
            }
            Some("demuxer-cache-time") => {
                self.playback_diagnostics.buffered_until_ms = data
                    .and_then(Value::as_f64)
                    .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                    .map(|seconds| seconds * 1000.0);
            }
            Some("vo-drop-frame-count") => {
                self.playback_diagnostics.dropped_frames = data
                    .and_then(|value| {
                        value
                            .as_i64()
                            .or_else(|| value.as_f64().map(|count| count.round() as i64))
                    })
                    .filter(|count| *count >= 0);
            }
            Some("estimated-vf-fps") => {
                self.playback_diagnostics.frame_rate = data
                    .and_then(Value::as_f64)
                    .filter(|rate| rate.is_finite() && *rate > 0.0);
            }
            _ => return,
        }
        self.publish_snapshot();
    }

    fn apply_status_property(&mut self, property: Option<&str>, data: Option<&Value>) {
        match property {
            Some("eof-reached") => {
                let previous = self.last_state.eof_reached;
                self.last_state.eof_reached = data.and_then(Value::as_bool).unwrap_or(false);
                if previous != self.last_state.eof_reached {
                    tracing::debug!(
                        target: "playback",
                        previous,
                        current = self.last_state.eof_reached,
                        state = %self.last_state,
                        "mpv eof state changed"
                    );
                }
            }
            Some("seeking") => {
                if let Some(value) = data.and_then(Value::as_bool) {
                    self.handle_seeking_property(value);
                }
            }
            Some("chapter-list") => {
                if let Some(chapters) = data.and_then(Value::as_array) {
                    self.handle_chapter_list(chapters.clone());
                }
            }
            Some("playback-abort") if data.and_then(Value::as_bool).unwrap_or(false) => {
                tracing::debug!(
                    target: "playback",
                    pending = self.phase.pending().is_some(),
                    active = self.phase.active().is_some(),
                    state = %self.last_state,
                    "mpv playback-abort is true; waiting for end-file before finishing playback"
                );
            }
            Some("playback-abort") => {}
            _ => {}
        }
    }
}

fn playback_tracks(data: Option<&Value>) -> Vec<PlayerTrack> {
    data.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|track| {
            let kind = match track.get("type").and_then(Value::as_str) {
                Some("audio") => PlayerTrackKind::Audio,
                Some("sub") => PlayerTrackKind::Subtitle,
                _ => return None,
            };
            let id = track
                .get("id")
                .and_then(Value::as_i64)
                .filter(|id| *id > 0)?;
            let text = |field| {
                track
                    .get(field)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            };
            Some(PlayerTrack {
                id,
                kind,
                language: text("lang"),
                title: text("title"),
                codec: text("codec"),
                selected: track
                    .get("selected")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                external: track
                    .get("external")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

fn is_mark_watched_next_message(args: &[String]) -> bool {
    match args {
        [command] => command == MARK_WATCHED_NEXT_COMMAND,
        [target, command, ..] => {
            target == "mediaflick-desktop" && command == MARK_WATCHED_NEXT_COMMAND
        }
        _ => false,
    }
}
