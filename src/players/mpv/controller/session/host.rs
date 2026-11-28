#[cfg(target_os = "linux")]
use std::io;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "linux")]
use std::time::Instant;

#[cfg(target_os = "linux")]
use crate::playback::NativeOverlayFrame;
use crate::playback::{NativeWindowHandle, StopReason};
#[cfg(target_os = "linux")]
use crate::players::mpv::runtime::helper::HelperOverlayError;
#[cfg(target_os = "linux")]
use crate::players::mpv::runtime::linux_window::HostWindow;
#[cfg(target_os = "linux")]
use crate::players::mpv::runtime::wayland::{WaylandHost, WaylandUiHandle, use_native_wayland};
use crate::players::mpv::runtime::{MpvRuntime, MpvRuntimeKind};
#[cfg(target_os = "linux")]
use crate::preferences::FullscreenBehavior;

use super::super::{ControllerState, PlaybackIdentity, REJECTED_LOAD};

#[cfg(target_os = "linux")]
pub(in crate::players::mpv::controller) enum LinuxHostWindow {
    Wayland(WaylandHost),
    X11(Box<HostWindow>),
}

#[cfg(target_os = "linux")]
impl LinuxHostWindow {
    pub(super) fn native_window(&self) -> Option<NativeWindowHandle> {
        match self {
            Self::Wayland(_) => None,
            Self::X11(window) => window.handle().ok(),
        }
    }

    pub(super) fn wayland_ui(&self) -> Option<WaylandUiHandle> {
        match self {
            Self::Wayland(host) => Some(host.ui_handle()),
            Self::X11(_) => None,
        }
    }

    pub(super) fn ui_exposes_video(&self) -> bool {
        match self {
            Self::Wayland(host) => host.ui_exposes_video(),
            Self::X11(_) => false,
        }
    }

    pub(super) fn fullscreen(&self) -> io::Result<bool> {
        match self {
            Self::Wayland(host) => Ok(host.fullscreen()),
            Self::X11(window) => window.fullscreen(),
        }
    }

    pub(super) fn set_fullscreen(&self, mode: FullscreenBehavior) -> io::Result<()> {
        match self {
            Self::Wayland(host) => host.set_fullscreen(mode),
            Self::X11(window) => window.set_fullscreen(mode),
        }
    }

    pub(super) fn content_size(&self) -> io::Result<(u16, u16)> {
        match self {
            Self::Wayland(_) => Ok((1280, 720)),
            Self::X11(window) => window.content_size(),
        }
    }

    pub(super) fn poll_events(&self) {
        if let Self::X11(window) = self {
            window.poll_events();
        }
    }
}

impl ControllerState {
    pub(in crate::players::mpv::controller) fn native_window_handle(
        &self,
    ) -> Option<NativeWindowHandle> {
        #[cfg(target_os = "linux")]
        if let Some(handle) = self
            .host_window
            .as_ref()
            .and_then(LinuxHostWindow::native_window)
        {
            return Some(handle);
        }
        self.runtime.as_ref().and_then(MpvRuntime::native_window)
    }

    #[cfg(target_os = "linux")]
    pub(in crate::players::mpv::controller) fn wayland_ui_handle(
        &mut self,
    ) -> Option<WaylandUiHandle> {
        if self.runtime_kind == MpvRuntimeKind::Library
            && self.host_window.is_none()
            && use_native_wayland()
            && let Ok(host) = WaylandHost::new()
        {
            self.host_window = Some(LinuxHostWindow::Wayland(host));
        }
        self.host_window
            .as_ref()
            .and_then(LinuxHostWindow::wayland_ui)
    }

    #[cfg(target_os = "linux")]
    pub(in crate::players::mpv::controller) fn poll_host_window_events(&self) {
        if let Some(window) = &self.host_window {
            window.poll_events();
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn start_linux_helper_runtime(
        &mut self,
        path: &Path,
        ipc_path: &str,
        fullscreen: FullscreenBehavior,
    ) -> io::Result<MpvRuntime> {
        if self.host_window.is_none() {
            self.host_window = Some(if use_native_wayland() {
                LinuxHostWindow::Wayland(WaylandHost::new()?)
            } else {
                LinuxHostWindow::X11(Box::new(HostWindow::new()?))
            });
        }
        match self
            .host_window
            .as_ref()
            .ok_or_else(|| io::Error::other("native host window missing"))?
        {
            LinuxHostWindow::Wayland(host) => {
                let fd = host.create_helper_socket()?;
                MpvRuntime::start_linux_wayland_helper(
                    path,
                    ipc_path,
                    fd,
                    fullscreen,
                    self.libmpv_profile,
                )
            }
            LinuxHostWindow::X11(window) => {
                let wid = window.handle()?.content();
                MpvRuntime::start_linux_helper(path, ipc_path, wid, fullscreen, self.libmpv_profile)
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn recover_crashed_library_helper(&mut self, reason: &'static str) -> bool {
        if self.shutdown_requested.load(Ordering::SeqCst)
            || self.runtime_kind != MpvRuntimeKind::Library
        {
            return false;
        }
        let had_active = self.mpv_playback_active || !self.phase.is_idle();
        let active_load_id = self.current_identity().map(|id| id.playback_id);
        self.finish_active(Some(StopReason::Error));
        if had_active && let Some(load_id) = active_load_id {
            self.report_playback_failure(load_id, "Built-in player crashed and restarted cleanly.");
        }
        self.reset_mpv();
        if !self.helper_restarts.record_and_allow(Instant::now()) {
            tracing::error!(
                target: "mpv.ipc",
                reason,
                "built-in player helper exceeded restart budget; leaving player idle until next user load"
            );
            return false;
        }
        self.restart_configured_mpv(reason);
        self.runtime.is_some()
    }

    #[cfg(target_os = "linux")]
    pub(in crate::players::mpv::controller) fn present_overlay(
        &mut self,
        frame: NativeOverlayFrame,
    ) -> Result<NativeOverlayFrame, String> {
        if self.shutdown_requested.load(Ordering::SeqCst) {
            return Ok(frame);
        }
        let background_exposed = frame.exposes_video();
        let display = match &self.host_window {
            Some(window) => window.content_size().map_err(|error| error.to_string())?,
            None => (1280, 720),
        };
        if self.host_window.is_some()
            && !self.runtime_is_alive()
            && !self.recover_crashed_library_helper(
                "built-in player helper exited before overlay present",
            )
        {
            return Err("built-in player helper unavailable or restart limit reached".to_string());
        }
        let outcome = self
            .runtime
            .as_mut()
            .ok_or_else(|| "native video renderer is unavailable".to_string())?
            .present_helper_overlay(frame, display);
        match outcome {
            Ok(returned) => {
                self.finish_library_fullscreen(background_exposed);
                Ok(returned)
            }
            Err(HelperOverlayError::TimedOut(returned)) => Ok(returned),
            Err(HelperOverlayError::HelperExited(returned, reason)) => {
                self.retry_overlay_after_helper_exit(returned, display, background_exposed, reason)
            }
            Err(HelperOverlayError::Composition(error)) => Err(error),
        }
    }

    #[cfg(target_os = "linux")]
    fn retry_overlay_after_helper_exit(
        &mut self,
        frame: NativeOverlayFrame,
        display: (u16, u16),
        background_exposed: bool,
        reason: String,
    ) -> Result<NativeOverlayFrame, String> {
        if !self
            .recover_crashed_library_helper("built-in player helper exited during overlay present")
        {
            return Err(reason);
        }
        let Some(runtime) = self.runtime.as_mut() else {
            return Ok(frame);
        };
        match runtime.present_helper_overlay(frame, display) {
            Ok(returned) => {
                self.finish_library_fullscreen(background_exposed);
                Ok(returned)
            }
            Err(
                HelperOverlayError::TimedOut(returned)
                | HelperOverlayError::HelperExited(returned, _),
            ) => Ok(returned),
            Err(HelperOverlayError::Composition(error)) => Err(error),
        }
    }

    pub(in crate::players::mpv::controller) fn handle_rejected_loadfile(
        &mut self,
        replacing_active_file: bool,
        identity: PlaybackIdentity,
    ) {
        self.mpv_playback_active = false;
        let load_id = identity.playback_id;
        if replacing_active_file {
            self.playback_identity = Some(identity);
            if self.runtime_kind == MpvRuntimeKind::Library && self.runtime_is_alive() {
                tracing::warn!(
                    target: "mpv.ipc",
                    "stopping built-in libmpv playback after rejected replacement loadfile command"
                );
                self.clear_playback_session_state();
                self.stop_mpv("rejected replacement loadfile");
            } else {
                tracing::warn!(
                    target: "mpv.ipc",
                    "resetting mpv after rejected replacement loadfile command"
                );
                self.reset_mpv();
            }
            let snapshot = self.publish_snapshot_with_stop_reason(Some(StopReason::Error));
            self.notify_playback_stopped(snapshot);
        }
        self.report_playback_failure(load_id, REJECTED_LOAD);
    }

    pub(in crate::players::mpv::controller) fn report_playback_failure(
        &self,
        load_id: i64,
        message: impl Into<String>,
    ) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(crate::playback::BackendEvent::Failed {
                load_id,
                message: message.into(),
            });
        }
    }
}
