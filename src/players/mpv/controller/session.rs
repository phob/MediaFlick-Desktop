use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::app::logger;
use crate::jellyfin::playback_reporter::flush_playstate_reports;
use crate::playback::{
    PlaybackRequest, PlayerCommand, PlayerTrack, PlayerTrackKind, seconds_to_ticks,
};
use crate::players::mpv::input::{INPUT_SECTION_NAME, MARK_WATCHED_NEXT_COMMAND, MpvInputBindings};
use crate::players::mpv::ipc::{
    IpcCommandFailure, IpcWorker, MpvEvent, cleanup_ipc_path, make_ipc_path as make_mpv_ipc_path,
    start_ipc_worker,
};
use crate::preferences::FullscreenBehavior;

use super::super::commands::{next_request_id, non_empty};
use super::{
    ConfiguredMpv, ControllerMessage, ControllerState, DUPLICATE_DEBOUNCE, IPC_COMMAND_TIMEOUT,
    IPC_CONNECT_TIMEOUT, IPC_SUBTITLE_COMMAND_TIMEOUT, MPV_RAISE_PULSE_DELAY,
    MPV_SESSION_POLL_INTERVAL, PENDING_FILE_LOADED_TIMEOUT, PLAYSTATE_SHUTDOWN_FLUSH_TIMEOUT,
    PROGRESS_INTERVAL, SHUTDOWN_WAIT, control_command,
};

impl ControllerState {
    pub(super) fn warm(&mut self, mpv_path: &str, fullscreen: FullscreenBehavior) {
        tracing::debug!(
            target: "mpv.ipc",
            mpv_path = %mpv_path,
            fullscreen = %fullscreen.fullscreen_arg(),
            "warming idle mpv process"
        );
        self.remember_configured_mpv(mpv_path, fullscreen);
        if self.ensure_mpv(mpv_path, fullscreen) {
            self.apply_external_default_fullscreen(fullscreen);
        }
    }

    pub(super) fn remember_configured_mpv(
        &mut self,
        mpv_path: &str,
        fullscreen: FullscreenBehavior,
    ) {
        self.configured_mpv = Some(ConfiguredMpv {
            mpv_path: mpv_path.to_string(),
            fullscreen,
        });
    }

    pub(super) fn apply_external_default_fullscreen(&self, fullscreen: FullscreenBehavior) {
        if self.runtime_kind != super::MpvRuntimeKind::External {
            return;
        }
        self.set_fullscreen(fullscreen);
    }

    pub(super) fn apply_library_default_fullscreen(&self) {
        if self.runtime_kind != super::MpvRuntimeKind::Library {
            return;
        }
        let Some(fullscreen) = self
            .configured_mpv
            .as_ref()
            .map(|configured| configured.fullscreen)
        else {
            return;
        };
        self.set_fullscreen(fullscreen);
    }

    pub(super) fn set_fullscreen(&self, fullscreen: FullscreenBehavior) {
        let command = json!({
            "command": ["set_property", "fullscreen", fullscreen == FullscreenBehavior::Fullscreen],
            "request_id": next_request_id(),
        });
        if let Err(error) = self.send_mpv_command(command) {
            tracing::warn!(target: "mpv.ipc", "failed to change fullscreen mode: {error}");
        }
    }

    pub(super) fn send_loadfile_with_reconnect(
        &mut self,
        mpv_path: &str,
        fullscreen: FullscreenBehavior,
        launch: &PlaybackRequest,
    ) -> Result<(), IpcCommandFailure> {
        match self.send_mpv_command(self.loadfile_command(launch)) {
            Ok(()) => Ok(()),
            Err(first_error @ IpcCommandFailure::Rejected(_)) => Err(first_error),
            Err(first_error @ IpcCommandFailure::Transport(_)) => {
                tracing::warn!(target: "mpv.ipc", "failed to send mpv loadfile command; restarting session and retrying once: {first_error}");
                self.reset_mpv();
                if !self.ensure_mpv(mpv_path, fullscreen) {
                    return Err(first_error);
                }
                self.apply_external_default_fullscreen(fullscreen);
                self.send_mpv_command(self.loadfile_command(launch))
                    .map_err(|retry_error| {
                        retry_error
                            .with_context(format!("initial failure: {first_error}; retry failure"))
                    })
            }
        }
    }

    pub(super) fn ensure_mpv(&mut self, mpv_path: &str, fullscreen: FullscreenBehavior) -> bool {
        if self.shutdown_requested.load(Ordering::SeqCst) {
            tracing::debug!(target: "mpv.ipc", "skipping mpv start because shutdown is requested");
            return false;
        }

        if self.runtime_is_alive() {
            if !self.current_mpv_path_matches(mpv_path) {
                tracing::info!(
                    target: "mpv.ipc",
                    previous = %self.current_mpv_path.as_deref().unwrap_or("unknown"),
                    next = %mpv_path,
                    "restarting mpv because the configured executable changed"
                );
                self.finish_active(Some("quit"));
                self.reset_mpv();
            } else if self
                .ipc_worker
                .as_ref()
                .is_some_and(IpcWorker::is_writer_alive)
            {
                tracing::trace!(
                    target: "mpv.ipc",
                    "reusing existing mpv process"
                );
                return true;
            } else {
                tracing::warn!(
                    target: "mpv.ipc",
                    "restarting mpv because the tracked process has no live IPC worker"
                );
                self.finish_active(Some("quit"));
                self.reset_mpv();
            }
        } else {
            self.reset_mpv();
        }
        let ipc_path = self.next_ipc_path();
        tracing::info!(
            target: "mpv.ipc",
            runtime = ?self.runtime_kind,
            path = %mpv_path,
            ipc_path = %ipc_path,
            "starting idle mpv runtime"
        );
        let runtime_fullscreen = match self.runtime_kind {
            super::MpvRuntimeKind::External => fullscreen,
            super::MpvRuntimeKind::Library => FullscreenBehavior::Windowed,
        };
        let mut runtime = match crate::players::mpv::runtime::MpvRuntime::start(
            self.runtime_kind,
            self.libmpv_profile,
            &PathBuf::from(mpv_path),
            &ipc_path,
            runtime_fullscreen,
        ) {
            Ok(runtime) => runtime,
            Err(error) => {
                tracing::warn!(
                    target: "mpv.ipc",
                    runtime = ?self.runtime_kind,
                    path = %mpv_path,
                    "failed to start mpv for Jellyfin stream: {error}"
                );
                cleanup_ipc_path(&ipc_path);
                return false;
            }
        };

        let (ipc_worker, event_rx) = match start_ipc_worker(
            &ipc_path,
            IPC_CONNECT_TIMEOUT,
            &self.shutdown_requested,
            || runtime.is_alive(),
        ) {
            Ok(worker) => worker,
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", ipc_path = %ipc_path, "failed to connect mpv IPC: {error}");
                runtime.stop();
                cleanup_ipc_path(&ipc_path);
                return false;
            }
        };
        if self.shutdown_requested.load(Ordering::SeqCst) {
            tracing::debug!(target: "mpv.ipc", "closing newly connected mpv because shutdown is requested");
            runtime.stop();
            cleanup_ipc_path(&ipc_path);
            ipc_worker.shutdown();
            return false;
        }

        let session_id = self.next_ipc_session_id;
        self.next_ipc_session_id = self.next_ipc_session_id.wrapping_add(1).max(1);
        self.start_event_relay(session_id, event_rx);
        self.runtime = Some(runtime);
        self.current_mpv_path = Some(mpv_path.to_string());
        self.ipc_path = Some(ipc_path.clone());
        self.ipc_worker = Some(ipc_worker);
        self.active_ipc_session_id = Some(session_id);
        tracing::info!(target: "mpv.ipc", ipc_path = %ipc_path, session_id, "mpv IPC connected");
        self.install_input_bindings();
        true
    }

    fn next_ipc_path(&self) -> String {
        #[cfg(target_os = "windows")]
        if self.runtime_kind == super::MpvRuntimeKind::Library
            && self.libmpv_profile == crate::players::mpv::runtime::LibmpvProfile::Svp
        {
            return r"\\.\pipe\mpvpipe".to_string();
        }
        make_mpv_ipc_path()
    }

    fn current_mpv_path_matches(&self, mpv_path: &str) -> bool {
        self.current_mpv_path
            .as_deref()
            .is_some_and(|current| equivalent_mpv_path(current, mpv_path))
    }

    pub(super) fn install_input_bindings(&self) {
        let bindings = MpvInputBindings::load();
        let section_contents = bindings.section_contents();

        let define = json!({
            "command": ["define-section", INPUT_SECTION_NAME, section_contents, "force"],
            "request_id": next_request_id(),
        });
        if let Err(error) = self.send_mpv_command(define) {
            tracing::warn!(target: "mpv.ipc", "failed to define mpv input bindings: {error}");
            return;
        }

        let enable = json!({
            "command": ["enable-section", INPUT_SECTION_NAME, "allow-hide-cursor+allow-vo-dragging"],
            "request_id": next_request_id(),
        });
        if let Err(error) = self.send_mpv_command(enable) {
            tracing::warn!(target: "mpv.ipc", "failed to enable mpv input bindings: {error}");
        }
    }

    fn start_event_relay(&self, session_id: u64, event_rx: std::sync::mpsc::Receiver<MpvEvent>) {
        let tx = self.tx.clone();
        thread::spawn(move || {
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

    pub(super) fn handle_session_event(&mut self, session_id: u64, event: &MpvEvent) {
        if self.active_ipc_session_id != Some(session_id) {
            tracing::trace!(target: "mpv.ipc", session_id, "ignored event from stale mpv IPC session");
            return;
        }
        self.handle_event(event);
    }

    pub(super) fn handle_event_stream_disconnected(&mut self, session_id: u64) {
        if self.active_ipc_session_id != Some(session_id) {
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
            "end-file" => self.finish_active(event.reason.as_deref()),
            "shutdown" => {
                self.finish_active(Some("quit"));
                self.reset_mpv();
                self.restart_configured_mpv("mpv emitted shutdown");
            }
            "seek" => self.handle_seek_event(),
            "property-change" => {
                self.apply_property(event.property.as_deref(), event.data.as_ref());
            }
            "client-message" if is_mark_watched_next_message(&event.args) => {
                self.mark_watched_and_play_next();
            }
            _ => tracing::trace!(target: "mpv.ipc", name = %event.name, "ignored mpv event"),
        }
    }

    pub(super) fn stage_external_subtitle(&mut self, launch: &PlaybackRequest) {
        self.pending_external_subtitle_url =
            non_empty(launch.subtitle_url.as_deref()).map(ToOwned::to_owned);
    }

    pub(super) fn load_pending_external_subtitle(&mut self) {
        let Some(subtitle_url) = self.pending_external_subtitle_url.take() else {
            return;
        };
        let Some(command) = control_command(&PlayerCommand::AddSubtitle(subtitle_url.clone()))
        else {
            return;
        };
        tracing::debug!(
            target: "mpv.ipc",
            subtitle_url = %logger::redact_url_secrets(&subtitle_url),
            timeout_ms = IPC_SUBTITLE_COMMAND_TIMEOUT.as_millis(),
            "loading selected external Jellyfin subtitle from its remote URL"
        );
        match self.send_mpv_command_with_timeout(command, IPC_SUBTITLE_COMMAND_TIMEOUT) {
            Ok(()) => {
                tracing::debug!(target: "mpv.ipc", "loaded selected external Jellyfin subtitle")
            }
            Err(IpcCommandFailure::Rejected(error)) => {
                tracing::warn!(target: "mpv.ipc", "mpv rejected external subtitle: {error}");
            }
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", "failed to load external subtitle in mpv: {error}");
                self.handle_mpv_session_lost("mpv external subtitle transport failed");
            }
        }
    }

    pub(super) fn complete_library_startup(&mut self) {
        if self.runtime_kind != super::MpvRuntimeKind::Library {
            return;
        }
        let pause = self.pending_library_pause.take().unwrap_or(false);
        self.last_state.pause = pause;
        if pause {
            return;
        }
        let Some(command) = control_command(&PlayerCommand::SetPause(false)) else {
            return;
        };
        if let Err(error) = self.send_mpv_command(command) {
            tracing::warn!(target: "mpv.ipc", "failed to resume libmpv after file setup: {error}");
            if error.is_transport() {
                self.handle_mpv_session_lost("libmpv startup resume transport failed");
            }
        }
    }

    fn apply_property(&mut self, property: Option<&str>, data: Option<&Value>) {
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
                        self.update_skip_segment_state(previous, ticks);
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
                    self.handle_chapter_list_event(chapters.clone());
                }
            }
            Some("playback-abort") if data.and_then(Value::as_bool).unwrap_or(false) => {
                tracing::debug!(
                    target: "playback",
                    pending = self.pending.is_some(),
                    active = self.active.is_some(),
                    state = %self.last_state,
                    "mpv playback-abort is true; waiting for end-file before finishing playback"
                );
            }
            Some("playback-abort") => {}
            _ => {}
        }
    }

    pub(super) fn maybe_report_progress(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };
        let now = Instant::now();
        let due = now.saturating_duration_since(active.last_progress_sent) >= PROGRESS_INTERVAL;
        if due || active.last_pause != self.last_state.pause {
            tracing::trace!(
                target: "jellyfin.playstate",
                due,
                pause_changed = active.last_pause != self.last_state.pause,
                state = %self.last_state,
                "Jellyfin playback progress report due"
            );
            active.reporter.report_progress(&self.last_state);
            active.last_progress_sent = now;
            active.last_pause = self.last_state.pause;
        }
    }

    pub(super) fn poll_runtime(&mut self) {
        let Some(runtime) = &mut self.runtime else {
            return;
        };
        match runtime.is_alive() {
            Ok(false) => {
                tracing::info!(target: "mpv.ipc", "mpv runtime stopped");
                self.finish_active(Some("quit"));
                self.reset_mpv();
                self.restart_configured_mpv("mpv runtime stopped");
            }
            Ok(true) => {}
            Err(error) => {
                tracing::warn!(target: "mpv.ipc", "failed to poll mpv runtime: {error}");
            }
        }
    }

    pub(super) fn maybe_poll_mpv_session(&mut self) {
        let now = Instant::now();
        if now.saturating_duration_since(self.last_session_poll) < MPV_SESSION_POLL_INTERVAL {
            return;
        }
        self.last_session_poll = now;
        self.ensure_configured_mpv_running("scheduled mpv session poll");
    }

    pub(super) fn ensure_configured_mpv_running(&mut self, reason: &'static str) -> bool {
        if self.shutdown_requested.load(Ordering::SeqCst) {
            tracing::trace!(target: "mpv.ipc", reason, "not ensuring mpv session because shutdown is requested");
            return false;
        }
        let Some(config) = self.configured_mpv.clone() else {
            tracing::trace!(target: "mpv.ipc", reason, "no configured mpv executable to supervise");
            return false;
        };
        if self.configured_mpv_session_ready(&config) {
            return true;
        }
        tracing::info!(
            target: "mpv.ipc",
            reason,
            mpv_path = %config.mpv_path,
            "ensuring configured mpv session is running"
        );
        if self.ensure_mpv(&config.mpv_path, config.fullscreen) {
            self.apply_external_default_fullscreen(config.fullscreen);
            true
        } else {
            false
        }
    }

    fn configured_mpv_session_ready(&mut self, config: &ConfiguredMpv) -> bool {
        self.runtime_is_alive()
            && self.current_mpv_path_matches(&config.mpv_path)
            && self
                .ipc_worker
                .as_ref()
                .is_some_and(IpcWorker::is_writer_alive)
    }

    pub(super) fn handle_mpv_session_lost(&mut self, reason: &'static str) {
        self.finish_active(Some("quit"));
        self.reset_mpv();
        self.restart_configured_mpv(reason);
    }

    fn restart_configured_mpv(&mut self, reason: &'static str) {
        if self.shutdown_requested.load(Ordering::SeqCst) {
            tracing::debug!(target: "mpv.ipc", reason, "not restarting mpv because shutdown is requested");
            return;
        }
        let Some(config) = self.configured_mpv.clone() else {
            tracing::debug!(target: "mpv.ipc", reason, "not restarting mpv because no executable is configured");
            return;
        };
        tracing::info!(
            target: "mpv.ipc",
            reason,
            mpv_path = %config.mpv_path,
            "restarting idle mpv process"
        );
        if self.ensure_mpv(&config.mpv_path, config.fullscreen) {
            self.apply_external_default_fullscreen(config.fullscreen);
        }
    }

    pub(super) fn schedule_mpv_raise(&mut self, reason: &'static str) {
        if self.runtime_kind == super::MpvRuntimeKind::Library {
            tracing::trace!(target: "mpv.focus", reason, "skipped external-player raise for libmpv window");
            return;
        }
        if self.begin_mpv_raise(reason) {
            self.pending_raise_pulse_reset_at = Some(Instant::now() + MPV_RAISE_PULSE_DELAY);
        }
    }

    pub(super) fn maybe_finish_mpv_raise(&mut self) {
        let Some(due_at) = self.pending_raise_pulse_reset_at else {
            return;
        };
        if Instant::now() < due_at {
            return;
        }
        self.pending_raise_pulse_reset_at = None;
        self.finish_mpv_raise();
    }

    #[cfg(windows)]
    fn begin_mpv_raise(&self, reason: &'static str) -> bool {
        let Some(runtime) = self.runtime.as_ref() else {
            tracing::trace!(target: "mpv.focus", reason, "cannot activate mpv because its runtime is unavailable");
            return false;
        };
        let Some(process_id) = runtime.process_id() else {
            tracing::debug!(target: "mpv.focus", reason, "raising the same-process libmpv window with a minimize/restore pulse");
            return self.set_mpv_bool_property("window-minimized", true, reason);
        };
        match crate::windows::activate_process_window(process_id) {
            crate::windows::ProcessWindowActivation::Activated => {
                tracing::debug!(target: "mpv.focus", reason, process_id, "activated mpv with the Windows foreground API");
                false
            }
            crate::windows::ProcessWindowActivation::WindowNotFound => {
                tracing::debug!(target: "mpv.focus", reason, process_id, "mpv window was not available for native activation; using minimize/restore fallback");
                self.set_mpv_bool_property("window-minimized", true, reason)
            }
            crate::windows::ProcessWindowActivation::Denied => {
                tracing::debug!(target: "mpv.focus", reason, process_id, "Windows denied native mpv activation; using minimize/restore fallback");
                self.set_mpv_bool_property("window-minimized", true, reason)
            }
        }
    }

    #[cfg(windows)]
    fn finish_mpv_raise(&self) {
        self.set_mpv_bool_property("window-minimized", false, "raise pulse restore");
    }

    #[cfg(not(windows))]
    fn begin_mpv_raise(&self, reason: &'static str) -> bool {
        self.set_mpv_bool_property("ontop", true, reason)
    }

    #[cfg(not(windows))]
    fn finish_mpv_raise(&self) {
        self.set_mpv_bool_property("ontop", false, "raise pulse reset");
    }

    fn set_mpv_bool_property(&self, property: &str, value: bool, reason: &'static str) -> bool {
        let command = json!({
            "command": ["set_property", property, value],
            "request_id": next_request_id(),
        });
        match self.send_mpv_command(command) {
            Ok(()) => {
                tracing::debug!(target: "mpv.focus", reason, property, value, "set mpv property for window raise");
                true
            }
            Err(error) => {
                tracing::trace!(target: "mpv.focus", reason, property, value, "failed to set mpv property for window raise: {error}");
                false
            }
        }
    }

    fn runtime_is_alive(&mut self) -> bool {
        self.runtime
            .as_mut()
            .is_some_and(|runtime| runtime.is_alive().unwrap_or(false))
    }

    pub(super) fn is_duplicate(&mut self, key: &str) -> bool {
        self.prune_recent_loads();
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.key == key)
            || self
                .recent_loads
                .iter()
                .any(|load| load.key == key && load.seen_at.elapsed() <= DUPLICATE_DEBOUNCE)
    }

    pub(super) fn prune_recent_loads(&mut self) {
        while self
            .recent_loads
            .front()
            .is_some_and(|load| load.seen_at.elapsed() > DUPLICATE_DEBOUNCE)
        {
            self.recent_loads.pop_front();
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.requested_at.elapsed() > PENDING_FILE_LOADED_TIMEOUT)
        {
            tracing::warn!(
                target: "playback",
                timeout_ms = PENDING_FILE_LOADED_TIMEOUT.as_millis(),
                "pending playback timed out waiting for mpv file-loaded"
            );
            self.finish_active(Some("error"));
            self.pending = None;
        }
    }

    pub(super) fn shutdown(&mut self) {
        tracing::debug!(target: "playback", state = %self.last_state, "shutting down mpv controller");
        self.finish_active(Some("quit"));
        if let Err(error) = self.send_mpv_command(json!({ "command": ["quit"] })) {
            tracing::debug!(target: "mpv.ipc", "failed to send mpv quit during shutdown: {error}");
        }
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        while Instant::now() < deadline {
            if !self.runtime_is_alive() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        if self.runtime_is_alive()
            && let Some(runtime) = &mut self.runtime
        {
            tracing::warn!(target: "mpv.ipc", "mpv did not exit before shutdown deadline; terminating runtime");
            runtime.stop();
        }
        self.reset_mpv();
        if !flush_playstate_reports(PLAYSTATE_SHUTDOWN_FLUSH_TIMEOUT) {
            tracing::warn!(target: "jellyfin.playstate", "timed out flushing queued Jellyfin playback state during shutdown");
        }
    }

    pub(super) fn reset_mpv(&mut self) {
        tracing::debug!(target: "mpv.ipc", "resetting mpv process and IPC state");
        self.startup_seek = None;
        self.pending_library_pause = None;
        self.pending_external_subtitle_url = None;
        self.pending_raise_pulse_reset_at = None;
        self.playback_tracks.clear();
        self.replacement_end_file_pending = false;
        self.clear_skip_segment_state();
        if self.active.is_none() && self.pending.is_none() {
            self.playback_runtime_ticks = None;
            self.mpv_playback_active = false;
        }
        if let Some(mut runtime) = self.runtime.take() {
            tracing::debug!(target: "mpv.ipc", "stopping live mpv runtime during reset");
            runtime.stop();
        }
        self.current_mpv_path = None;
        if let Some(path) = self.ipc_path.take() {
            tracing::trace!(target: "mpv.ipc", ipc_path = %path, "cleaning mpv IPC path");
            cleanup_ipc_path(&path);
        }
        self.active_ipc_session_id = None;
        if let Some(worker) = self.ipc_worker.take() {
            worker.shutdown();
        }
        self.last_position_log_bucket = None;
        self.publish_snapshot();
    }

    pub(super) fn send_mpv_command(&self, command: Value) -> Result<(), IpcCommandFailure> {
        self.send_mpv_command_with_timeout(command, IPC_COMMAND_TIMEOUT)
    }

    fn send_mpv_command_with_timeout(
        &self,
        command: Value,
        timeout: Duration,
    ) -> Result<(), IpcCommandFailure> {
        let Some(worker) = &self.ipc_worker else {
            tracing::warn!(
                target: "mpv.ipc",
                command = %logger::mpv_command_summary(&command),
                "cannot send mpv command because IPC worker is not connected"
            );
            return Err(IpcCommandFailure::Transport(io::Error::new(
                io::ErrorKind::NotConnected,
                "mpv IPC worker is not connected",
            )));
        };
        tracing::debug!(
            target: "mpv.ipc",
            command = %logger::mpv_command_summary(&command),
            "sending mpv command"
        );
        tracing::trace!(
            target: "mpv.ipc",
            command = %logger::redacted_json(&command),
            "sending raw mpv command"
        );
        let result = worker.send_with_timeout(command, timeout);
        match &result {
            Ok(()) => tracing::debug!(target: "mpv.ipc", "sent mpv command"),
            Err(error) => tracing::warn!(target: "mpv.ipc", "mpv command send failed: {error}"),
        }
        result
    }
}

fn equivalent_mpv_path(left: &str, right: &str) -> bool {
    let left = normalize_mpv_path_for_compare(left);
    let right = normalize_mpv_path_for_compare(right);
    left == right
}

fn normalize_mpv_path_for_compare(path: &str) -> String {
    let path = path.trim().trim_matches('"');
    let display = Path::new(path).to_string_lossy();

    #[cfg(windows)]
    {
        display.replace('/', "\\").to_ascii_lowercase()
    }

    #[cfg(not(windows))]
    {
        display.into_owned()
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

pub(super) fn normalized_stop_reason(reason: Option<&str>) -> Option<&'static str> {
    match reason.map(str::trim).filter(|reason| !reason.is_empty()) {
        Some(reason) if reason.eq_ignore_ascii_case("eof") => Some("eof"),
        Some(reason) if reason.eq_ignore_ascii_case("watched-next") => Some("watched-next"),
        Some(reason) if reason.eq_ignore_ascii_case("stop") => Some("stop"),
        Some(reason) if reason.eq_ignore_ascii_case("quit") => Some("quit"),
        Some(reason) if reason.eq_ignore_ascii_case("error") => Some("error"),
        Some(reason) if reason.eq_ignore_ascii_case("redirect") => Some("redirect"),
        Some(reason) if reason.eq_ignore_ascii_case("shutdown") => Some("shutdown"),
        Some(_) => Some("unknown"),
        None => None,
    }
}

pub(super) fn is_completion_reason(reason: Option<&str>) -> bool {
    reason.is_some_and(|reason| {
        reason.eq_ignore_ascii_case("eof") || reason.eq_ignore_ascii_case("watched-next")
    })
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

#[cfg(test)]
mod tests;
