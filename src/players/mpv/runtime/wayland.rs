use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};

use wayland_client::{Connection, Dispatch, QueueHandle, protocol::wl_registry};

use crate::preferences::FullscreenBehavior;

mod bridge;
mod bridge_dmabuf;
mod bridge_ext;
mod client;
mod client_boot;
mod client_ext;
mod keyboard;

const POLLIN_EVENT: i16 = 0x0001;
const POLLERR_EVENT: i16 = 0x0008;
const POLLHUP_EVENT: i16 = 0x0010;
const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241;
const DRM_FORMAT_ABGR8888: u32 = 0x3432_4241;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
}

pub(super) fn wait_poll(fds: &mut [PollFd], timeout_ms: i32) -> io::Result<()> {
    for pfd in fds.iter_mut() {
        pfd.revents = 0;
    }
    // SAFETY: `fds` points to valid `PollFd` entries for `fds.len()`.
    let rc = unsafe { poll(fds.as_mut_ptr(), fds.len(), timeout_ms) };
    if rc < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::Interrupted {
            return Ok(());
        }
        return Err(err);
    }
    Ok(())
}

pub(super) fn poll_has_input(revents: i16) -> bool {
    revents & (POLLIN_EVENT | POLLERR_EVENT | POLLHUP_EVENT) != 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinuxDisplayBackend {
    Wayland,
    X11,
}

#[derive(Debug, Default)]
struct ProbeGlobals {
    compositor: bool,
    subcompositor: bool,
    xdg_wm_base: bool,
    viewporter: bool,
    dmabuf: bool,
}

impl ProbeGlobals {
    fn ready(&self) -> bool {
        self.compositor && self.subcompositor && self.xdg_wm_base && self.viewporter && self.dmabuf
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for ProbeGlobals {
    fn event(
        state: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { interface, .. } = event {
            match interface.as_str() {
                "wl_compositor" => state.compositor = true,
                "wl_subcompositor" => state.subcompositor = true,
                "xdg_wm_base" => state.xdg_wm_base = true,
                "wp_viewporter" => state.viewporter = true,
                "zwp_linux_dmabuf_v1" => state.dmabuf = true,
                _ => {}
            }
        }
    }
}

pub(crate) fn use_native_wayland() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        if std::env::var_os("MEDIAFLICK_FORCE_X11")
            .is_some_and(|val| val == "1" || val.eq_ignore_ascii_case("true"))
        {
            return false;
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_none_or(|val| val.is_empty()) {
            return false;
        }
        probe_wayland_compositor()
    })
}

fn probe_wayland_compositor() -> bool {
    let Ok(connection) = Connection::connect_to_env() else {
        return false;
    };
    let mut queue = connection.new_event_queue();
    let handle = queue.handle();
    let _registry = connection.display().get_registry(&handle, ());
    let mut probe = ProbeGlobals::default();
    if queue.roundtrip(&mut probe).is_err() {
        return false;
    }
    probe.ready()
}

pub(crate) const fn drm_fourcc_for_cef_color(is_rgba: bool) -> u32 {
    if is_rgba {
        DRM_FORMAT_ABGR8888
    } else {
        DRM_FORMAT_ARGB8888
    }
}

pub(crate) fn physical_extent(logical: i32, scale_120: u32) -> u16 {
    let logical = logical.max(1);
    let scale = if scale_120 == 0 { 120 } else { scale_120 };
    let scaled = (f64::from(logical) * f64::from(scale) / 120.0).round();
    scaled.clamp(1.0, 16_384.0) as u16
}

#[derive(Debug)]
pub struct WaylandDmaBufPlane {
    pub fd: OwnedFd,
    pub offset: u32,
    pub stride: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaylandSurfaceRole {
    View,
    Popup,
}

#[derive(Debug)]
pub enum WaylandFramePayload {
    DmaBuf {
        width: u16,
        height: u16,
        fourcc: u32,
        modifier: u64,
        planes: Vec<WaylandDmaBufPlane>,
    },
    Shm {
        width: u16,
        height: u16,
        pixels: Vec<u8>,
    },
}

#[derive(Debug)]
pub enum WaylandUiCommand {
    ConfigureWindow {
        title: String,
        width: u16,
        height: u16,
        maximized: bool,
    },
    Present {
        role: WaylandSurfaceRole,
        logical_width: u16,
        logical_height: u16,
        x: i32,
        y: i32,
        payload: WaylandFramePayload,
    },
    HidePopup,
    Reveal,
    Minimize,
    SetCursorShape(u32),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WaylandModifiers {
    pub shift: bool,
    pub caps_lock: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
    pub left_button: bool,
    pub middle_button: bool,
    pub right_button: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum WaylandUiEvent {
    Configure {
        width: u16,
        height: u16,
        scale_120: u32,
        fullscreen: bool,
        maximized: bool,
        visible: bool,
    },
    PointerMove {
        x: i32,
        y: i32,
        modifiers: WaylandModifiers,
        leave: bool,
    },
    PointerButton {
        x: i32,
        y: i32,
        button: u32,
        pressed: bool,
        modifiers: WaylandModifiers,
    },
    PointerWheel {
        x: i32,
        y: i32,
        delta_x: i32,
        delta_y: i32,
        modifiers: WaylandModifiers,
    },
    KeyboardFocus(bool),
    Key {
        evdev_code: u32,
        base_sym: u32,
        symbol: u32,
        modifiers: WaylandModifiers,
        pressed: bool,
        repeated: bool,
    },
    CloseRequested,
}

pub(super) enum HostControlCommand {
    CreateHelperSocket(mpsc::Sender<io::Result<OwnedFd>>),
    SetFullscreen(FullscreenBehavior),
    Shutdown(mpsc::Sender<()>),
}

type UiWakerSlot = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;

#[derive(Clone)]
pub struct WaylandUiHandle {
    cmd_tx: mpsc::Sender<WaylandUiCommand>,
    events: Arc<Mutex<VecDeque<WaylandUiEvent>>>,
    wake_tx: Arc<Mutex<UnixStream>>,
    waker: UiWakerSlot,
    ui_frame_committed: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
}

impl WaylandUiHandle {
    pub fn send(&self, command: WaylandUiCommand) {
        if self.closing.load(Ordering::Acquire) {
            return;
        }
        if self.cmd_tx.send(command).is_ok() {
            wake_host(&self.wake_tx);
        }
    }

    pub fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        if let Ok(mut slot) = self.waker.lock() {
            *slot = Some(waker);
        }
    }

    pub fn clear_waker(&self) {
        if let Ok(mut slot) = self.waker.lock() {
            *slot = None;
        }
    }

    pub fn drain_events(&self) -> Vec<WaylandUiEvent> {
        let mut out = Vec::new();
        if let Ok(mut queue) = self.events.lock() {
            while let Some(ev) = queue.pop_front() {
                out.push(ev);
            }
        }
        out
    }

    pub fn ui_frame_committed(&self) -> bool {
        self.ui_frame_committed.load(Ordering::Acquire)
    }

    pub(super) fn push_event(&self, event: WaylandUiEvent) {
        if let Ok(mut queue) = self.events.lock() {
            queue.push_back(event);
        }
        let waker = self.waker.lock().ok().and_then(|slot| slot.clone());
        if let Some(waker) = waker {
            waker();
        }
    }
}

pub(in crate::players::mpv) struct WaylandHost {
    ctl_tx: mpsc::Sender<HostControlCommand>,
    wake_tx: Arc<Mutex<UnixStream>>,
    ui: WaylandUiHandle,
    fullscreen: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WaylandHost {
    pub fn new() -> io::Result<Self> {
        let (wake_rx, wake_tx) = UnixStream::pair()?;
        wake_rx.set_nonblocking(true)?;
        wake_tx.set_nonblocking(true)?;
        let wake_tx = Arc::new(Mutex::new(wake_tx));
        let (ui_cmd_tx, ui_cmd_rx) = mpsc::channel();
        let (ctl_tx, ctl_rx) = mpsc::channel();
        let fullscreen = Arc::new(AtomicBool::new(false));
        let closing = Arc::new(AtomicBool::new(false));
        let ui_frame_committed = Arc::new(AtomicBool::new(false));
        let ui = WaylandUiHandle {
            cmd_tx: ui_cmd_tx,
            events: Arc::new(Mutex::new(VecDeque::new())),
            wake_tx: Arc::clone(&wake_tx),
            waker: Arc::new(Mutex::new(None)),
            ui_frame_committed: Arc::clone(&ui_frame_committed),
            closing: Arc::clone(&closing),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let ui_for_thread = ui.clone();
        let fullscreen_for_thread = Arc::clone(&fullscreen);
        let thread = thread::Builder::new()
            .name("wayland-host".to_string())
            .spawn(move || {
                bridge::run_host_loop(
                    wake_rx,
                    &ui_cmd_rx,
                    &ctl_rx,
                    ui_for_thread,
                    fullscreen_for_thread,
                    ui_frame_committed,
                    &ready_tx,
                );
            })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                ctl_tx,
                wake_tx,
                ui,
                fullscreen,
                closing,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(io::Error::other(
                    "wayland host thread exited during startup",
                ))
            }
        }
    }

    pub fn ui_handle(&self) -> WaylandUiHandle {
        self.ui.clone()
    }

    pub fn ui_exposes_video(&self) -> bool {
        self.ui.ui_frame_committed()
    }

    pub fn create_helper_socket(&self) -> io::Result<OwnedFd> {
        let (tx, rx) = mpsc::channel();
        self.ctl_tx
            .send(HostControlCommand::CreateHelperSocket(tx))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "wayland host stopped"))?;
        wake_host(&self.wake_tx);
        rx.recv().map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "wayland host dropped helper socket request",
            )
        })?
    }

    pub fn fullscreen(&self) -> bool {
        self.fullscreen.load(Ordering::Acquire)
    }

    pub fn set_fullscreen(&self, mode: FullscreenBehavior) -> io::Result<()> {
        self.ctl_tx
            .send(HostControlCommand::SetFullscreen(mode))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "wayland host stopped"))?;
        wake_host(&self.wake_tx);
        Ok(())
    }
}

impl Drop for WaylandHost {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Release);
        self.ui.clear_waker();
        let (tx, rx) = mpsc::channel();
        if self.ctl_tx.send(HostControlCommand::Shutdown(tx)).is_ok() {
            wake_host(&self.wake_tx);
            let _ = rx.recv_timeout(std::time::Duration::from_millis(500));
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(super) fn wake_host(wake_tx: &Arc<Mutex<UnixStream>>) {
    if let Ok(mut stream) = wake_tx.lock() {
        let _ = stream.write_all(&[1]);
    }
}

pub(super) fn drain_wake(wake_rx: &mut UnixStream) {
    let mut buf = [0_u8; 64];
    loop {
        match wake_rx.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

pub(super) fn poll_fd(raw_fd: i32) -> PollFd {
    PollFd {
        fd: raw_fd,
        events: POLLIN_EVENT,
        revents: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drm_fourcc_maps_cef_color_types_accurately() {
        assert_eq!(drm_fourcc_for_cef_color(true), 0x3432_4241);
        assert_eq!(drm_fourcc_for_cef_color(false), 0x3432_5241);
    }

    #[test]
    fn physical_extent_applies_fractional_scale_and_clamps_bounds() {
        assert_eq!(physical_extent(1920, 120), 1920);
        assert_eq!(physical_extent(1920, 150), 2400);
        assert_eq!(physical_extent(3840, 150), 4800);
        assert_eq!(physical_extent(1, 60), 1);
        assert_eq!(physical_extent(0, 120), 1);
        assert_eq!(physical_extent(-10, 120), 1);
        assert_eq!(physical_extent(1000, 0), 1000);
        assert_eq!(physical_extent(20_000, 240), 16_384);
    }

    #[test]
    fn probe_globals_requires_all_composition_and_dmabuf_protocols() {
        let mut p = ProbeGlobals {
            compositor: true,
            subcompositor: true,
            xdg_wm_base: true,
            viewporter: true,
            dmabuf: false,
        };
        assert!(!p.ready());
        p.dmabuf = true;
        assert!(p.ready());
    }
}
