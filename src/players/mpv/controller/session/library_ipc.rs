use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::players::mpv::ipc::{IpcCommandFailure, IpcWorker, start_ipc_worker};

use super::super::super::commands::next_request_id;
use super::super::{
    ControllerState, IPC_RECONNECT_TIMEOUT, LIBRARY_IPC_MAX_RECONNECT_ATTEMPTS, MpvRuntimeKind,
};

impl ControllerState {
    pub(super) fn ensure_live_library_ipc(&mut self, allow_backed_off_reconnect: bool) -> bool {
        if self
            .ipc
            .worker
            .as_ref()
            .is_some_and(IpcWorker::is_writer_alive)
        {
            if self.ipc.pending_stop {
                if !self.flush_pending_library_stop() {
                    self.record_library_ipc_reconnect_failure();
                    return false;
                }
                self.install_input_bindings();
            }
            tracing::trace!(target: "mpv.ipc", "reusing existing mpv process");
            return true;
        }
        if !allow_backed_off_reconnect && self.library_ipc_reconnect_blocked() {
            return false;
        }
        tracing::warn!(
            target: "mpv.ipc",
            "reconnecting built-in libmpv IPC because the live runtime has no live IPC worker"
        );
        self.reconnect_library_ipc()
    }

    pub(super) fn library_ipc_reconnect_blocked(&self) -> bool {
        self.ipc.reconnect_failures >= LIBRARY_IPC_MAX_RECONNECT_ATTEMPTS
            || self
                .ipc
                .next_reconnect_at
                .is_some_and(|at| Instant::now() < at)
    }

    fn record_library_ipc_reconnect_failure(&mut self) {
        self.ipc.reconnect_failures = self.ipc.reconnect_failures.saturating_add(1);
        if self.ipc.reconnect_failures >= LIBRARY_IPC_MAX_RECONNECT_ATTEMPTS {
            self.ipc.next_reconnect_at = None;
            tracing::error!(
                target: "mpv.ipc",
                attempts = self.ipc.reconnect_failures,
                "stopped automatic built-in libmpv IPC reconnects after repeated failures"
            );
        } else {
            let backoff = Duration::from_secs(1_u64 << self.ipc.reconnect_failures.min(4));
            self.ipc.next_reconnect_at = Some(Instant::now() + backoff);
            tracing::warn!(
                target: "mpv.ipc",
                attempts = self.ipc.reconnect_failures,
                retry_in_ms = backoff.as_millis(),
                "scheduled next built-in libmpv IPC reconnect attempt"
            );
        }
    }

    pub(super) fn flush_pending_library_stop(&mut self) -> bool {
        let command = json!({
            "command": ["stop"],
            "request_id": next_request_id(),
        });
        match self.send_mpv_command(command) {
            Ok(()) => {
                self.ipc.pending_stop = false;
                tracing::info!(
                    target: "playback",
                    "stopped built-in libmpv after IPC reconnect"
                );
                true
            }
            Err(IpcCommandFailure::Rejected(error)) => {
                self.ipc.pending_stop = false;
                tracing::debug!(
                    target: "mpv.ipc",
                    "mpv rejected deferred stop after IPC reconnect: {error}"
                );
                true
            }
            Err(error) => {
                tracing::warn!(
                    target: "mpv.ipc",
                    "deferred stop after built-in libmpv IPC reconnect failed: {error}"
                );
                false
            }
        }
    }

    pub(super) fn can_recycle_library_ipc(&mut self) -> bool {
        self.runtime_kind == MpvRuntimeKind::Library
            && self.ipc.path.is_some()
            && self.runtime_is_alive()
    }

    pub(super) fn disconnect_ipc_worker(&mut self) {
        self.ipc.active_id = None;
        if let Some(worker) = self.ipc.worker.take() {
            worker.disconnect();
        }
    }

    pub(super) fn reset_library_ipc(&mut self) {
        tracing::debug!(
            target: "mpv.ipc",
            "resetting built-in libmpv IPC without stopping runtime"
        );
        self.clear_playback_session_state();
        self.disconnect_ipc_worker();
        self.publish_snapshot();
    }

    pub(super) fn reconnect_library_ipc(&mut self) -> bool {
        let Some(ipc_path) = self.ipc.path.clone() else {
            return false;
        };
        self.disconnect_ipc_worker();
        let Some(runtime) = self.runtime.as_mut() else {
            return false;
        };
        let (ipc_worker, event_rx) = match start_ipc_worker(
            &ipc_path,
            IPC_RECONNECT_TIMEOUT,
            &self.shutdown_requested,
            || runtime.is_alive(),
        ) {
            Ok(worker) => worker,
            Err(error) => {
                tracing::warn!(
                    target: "mpv.ipc",
                    ipc_path = %ipc_path,
                    "failed to reconnect built-in libmpv IPC: {error}"
                );
                self.record_library_ipc_reconnect_failure();
                return false;
            }
        };
        if self.shutdown_requested.load(Ordering::SeqCst) {
            tracing::debug!(
                target: "mpv.ipc",
                "closing reconnected libmpv IPC because shutdown is requested"
            );
            ipc_worker.disconnect();
            return false;
        }
        let session_id = self.ipc.next_id;
        self.ipc.next_id = self.ipc.next_id.wrapping_add(1).max(1);
        self.start_event_relay(session_id, event_rx);
        self.ipc.worker = Some(ipc_worker);
        self.ipc.active_id = Some(session_id);
        tracing::info!(
            target: "mpv.ipc",
            ipc_path = %ipc_path,
            session_id,
            "reconnected built-in libmpv IPC"
        );
        if self.ipc.pending_stop && !self.flush_pending_library_stop() {
            self.record_library_ipc_reconnect_failure();
            return false;
        }
        self.install_input_bindings();
        if !self
            .ipc
            .worker
            .as_ref()
            .is_some_and(IpcWorker::is_writer_alive)
        {
            self.record_library_ipc_reconnect_failure();
            return false;
        }
        self.ipc.reconnect_failures = 0;
        self.ipc.next_reconnect_at = None;
        true
    }
}
