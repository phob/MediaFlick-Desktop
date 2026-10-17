use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::app::logger;
use crate::jellyfin::playback_reporter::flush_playstate_reports;
use crate::playback::{PlaybackRequest, PlayerCommand, StopReason, non_empty};
use crate::players::mpv::input::{INPUT_SECTION_NAME, section_contents};
use crate::players::mpv::ipc::{
    IpcCommandFailure, IpcWorker, cleanup_ipc_path, make_ipc_path as make_mpv_ipc_path,
    start_ipc_worker,
};
use crate::preferences::FullscreenBehavior;

use super::super::commands::{loadfile_command, next_request_id};
use super::{
    ConfiguredMpv, ControllerState, DUPLICATE_DEBOUNCE, IPC_COMMAND_TIMEOUT, IPC_CONNECT_TIMEOUT,
    IPC_SUBTITLE_COMMAND_TIMEOUT, MPV_RAISE_PULSE_DELAY, MPV_SESSION_POLL_INTERVAL,
    PENDING_FILE_LOADED_TIMEOUT, PLAYSTATE_SHUTDOWN_FLUSH_TIMEOUT, PROGRESS_INTERVAL,
    SHUTDOWN_WAIT, control_command,
};

mod events;

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

    pub(super) fn apply_library_default_fullscreen(&mut self) {
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
        #[cfg(target_os = "linux")]
        if fullscreen == FullscreenBehavior::Fullscreen {
            // Retire the opaque catalog and finish the delayed resume seek
            // before growing video render targets. Seeking can replace the
            // decoder's buffers; don't overlap that with fullscreen resizing.
            self.fullscreen_gate.pending = true;
            return;
        }
        self.set_fullscreen(fullscreen);
    }

    #[cfg(target_os = "linux")]
    pub(super) fn present_overlay(
        &mut self,
        frame: crate::playback::NativeOverlayFrame,
    ) -> Result<crate::playback::NativeOverlayFrame, String> {
        let background_exposed = frame.exposes_video();
        let frame = self
            .runtime
            .as_mut()
            .ok_or_else(|| "native video renderer is unavailable".to_string())?
            .present_overlay(frame)?;
        self.finish_library_fullscreen(background_exposed);
        Ok(frame)
    }

    #[cfg(target_os = "linux")]
    fn finish_library_fullscreen(&mut self, background_exposed: bool) {
        if !self.mpv_playback_active {
            self.fullscreen_gate.pending = false;
        }
        if background_exposed
            && self.fullscreen_gate.pending
            && self.startup_seek.is_none()
            && self.fullscreen_gate.video_ready
            && !self.fullscreen_gate.waiting_seek_event
        {
            self.fullscreen_gate.pending = false;
            self.set_fullscreen(FullscreenBehavior::Fullscreen);
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn maybe_finish_library_fullscreen(&mut self) {
        let background_exposed = self
            .runtime
            .as_ref()
            .is_some_and(super::MpvRuntime::ui_exposes_video);
        self.finish_library_fullscreen(background_exposed);
    }

    pub(super) fn set_fullscreen(&self, fullscreen: FullscreenBehavior) {
        #[cfg(target_os = "linux")]
        if let Some(runtime) = &self.runtime
            && let Err(error) = runtime.set_fullscreen(fullscreen)
        {
            tracing::warn!(target: "mpv.window", "could not change native fullscreen state: {error}");
            return;
        }
        let command = json!({
            "command": ["set_property", "fullscreen", fullscreen == FullscreenBehavior::Fullscreen],
            "request_id": next_request_id(),
        });
        if let Err(error) = self.send_mpv_command(command) {
            tracing::warn!(target: "mpv.ipc", "failed to change fullscreen mode: {error}");
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn toggle_library_fullscreen(&self) -> bool {
        if self.runtime_kind != super::MpvRuntimeKind::Library {
            return false;
        }
        if let Some(runtime) = &self.runtime {
            match runtime.fullscreen() {
                Ok(fullscreen) => self.set_fullscreen(if fullscreen {
                    FullscreenBehavior::Windowed
                } else {
                    FullscreenBehavior::Fullscreen
                }),
                Err(error) => {
                    tracing::warn!(target: "mpv.window", "could not read native fullscreen state: {error}")
                }
            }
        }
        true
    }

    pub(super) fn send_loadfile_with_reconnect(
        &mut self,
        mpv_path: &str,
        fullscreen: FullscreenBehavior,
        launch: &PlaybackRequest,
    ) -> Result<(), IpcCommandFailure> {
        match self.send_mpv_command(loadfile_command(launch)) {
            Ok(()) => Ok(()),
            Err(first_error @ IpcCommandFailure::Rejected(_)) => Err(first_error),
            Err(first_error @ IpcCommandFailure::Transport(_)) => {
                tracing::warn!(target: "mpv.ipc", "failed to send mpv loadfile command; restarting session and retrying once: {first_error}");
                self.reset_mpv();
                if !self.ensure_mpv(mpv_path, fullscreen) {
                    return Err(first_error);
                }
                self.apply_external_default_fullscreen(fullscreen);
                self.send_mpv_command(loadfile_command(launch))
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
                self.finish_active(Some(StopReason::Quit));
                self.reset_mpv();
            } else if self
                .ipc
                .worker
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
                self.finish_active(Some(StopReason::Quit));
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

        let session_id = self.ipc.next_id;
        self.ipc.next_id = self.ipc.next_id.wrapping_add(1).max(1);
        self.start_event_relay(session_id, event_rx);
        self.runtime = Some(runtime);
        self.current_mpv_path = Some(mpv_path.to_string());
        self.ipc.path = Some(ipc_path.clone());
        self.ipc.worker = Some(ipc_worker);
        self.ipc.active_id = Some(session_id);
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
        let section_contents = section_contents(self.mark_watched_next.as_deref());
        #[cfg(target_os = "linux")]
        let section_contents = if self.runtime_kind == super::MpvRuntimeKind::Library {
            format!("{section_contents}\nCLOSE_WIN script-message mediaflick-close-window")
        } else {
            section_contents
        };

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
            Ok(_) => {
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

    pub(super) fn maybe_report_progress(&mut self) {
        let Some(active) = self.phase.active_mut() else {
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
                self.finish_active(Some(StopReason::Quit));
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
                .ipc
                .worker
                .as_ref()
                .is_some_and(IpcWorker::is_writer_alive)
    }

    pub(super) fn handle_mpv_session_lost(&mut self, reason: &'static str) {
        self.finish_active(Some(StopReason::Quit));
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
        self.phase
            .pending()
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
            .phase
            .pending()
            .is_some_and(|pending| pending.requested_at.elapsed() > PENDING_FILE_LOADED_TIMEOUT)
        {
            tracing::warn!(
                target: "playback",
                timeout_ms = PENDING_FILE_LOADED_TIMEOUT.as_millis(),
                "pending playback timed out waiting for mpv file-loaded"
            );
            self.finish_active(Some(StopReason::Error));
            self.phase.take_pending();
        }
    }

    pub(super) fn shutdown(&mut self) {
        tracing::debug!(target: "playback", state = %self.last_state, "shutting down mpv controller");
        self.finish_active(Some(StopReason::Quit));
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
        #[cfg(target_os = "linux")]
        {
            self.fullscreen_gate.pending = false;
            self.fullscreen_gate.video_ready = false;
            self.fullscreen_gate.waiting_seek_event = false;
        }
        tracing::debug!(target: "mpv.ipc", "resetting mpv process and IPC state");
        self.startup_seek = None;
        self.pending_external_subtitle_url = None;
        self.pending_raise_pulse_reset_at = None;
        self.playback_tracks.clear();
        self.replacement_end_file_pending = false;
        self.clear_skip_segment_state();
        if self.phase.is_idle() {
            self.playback_runtime_ticks = None;
            self.mpv_playback_active = false;
        }
        if let Some(mut runtime) = self.runtime.take() {
            tracing::debug!(target: "mpv.ipc", "stopping live mpv runtime during reset");
            runtime.stop();
        }
        self.current_mpv_path = None;
        if let Some(path) = self.ipc.path.take() {
            tracing::trace!(target: "mpv.ipc", ipc_path = %path, "cleaning mpv IPC path");
            cleanup_ipc_path(&path);
        }
        self.ipc.active_id = None;
        if let Some(worker) = self.ipc.worker.take() {
            worker.shutdown();
        }
        self.last_position_log_bucket = None;
        self.publish_snapshot();
    }

    pub(super) fn send_mpv_command(&self, command: Value) -> Result<(), IpcCommandFailure> {
        self.send_mpv_command_with_timeout(command, IPC_COMMAND_TIMEOUT)
            .map(drop)
    }

    pub(super) fn get_mpv_property(&self, name: &str) -> Result<Value, IpcCommandFailure> {
        let command = json!({
            "command": ["get_property", name],
            "request_id": next_request_id(),
        });
        self.send_mpv_command_with_timeout(command, IPC_COMMAND_TIMEOUT)
    }

    fn send_mpv_command_with_timeout(
        &self,
        command: Value,
        timeout: Duration,
    ) -> Result<Value, IpcCommandFailure> {
        let Some(worker) = &self.ipc.worker else {
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
            Ok(_) => tracing::debug!(target: "mpv.ipc", "sent mpv command"),
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

#[cfg(test)]
mod tests;
