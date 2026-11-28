use std::collections::HashMap;
use std::io;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use wayland_client::backend::ObjectId;
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_output, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_subcompositor, wl_subsurface, wl_surface,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1;
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_feedback_v1, zwp_linux_dmabuf_v1,
};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1;
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_server::Resource as _;

use super::bridge::{BridgeState, HelperDisconnect};
use super::client_boot::{ExtensionManagers, HostGlobals, apply_output_event, bind_global};
use super::keyboard::WaylandKeymap;
use super::shm::create_black_background_buffer;
use super::{
    WaylandModifiers, WaylandSlotLease, WaylandSurfaceRole, WaylandUiEvent, WaylandUiHandle,
};

const MIN_WINDOW_WIDTH: i32 = 320;
const MIN_WINDOW_HEIGHT: i32 = 240;

pub(super) struct AppState {
    pub host: HostState,
    pub bridge: BridgeState,
    pub running: bool,
    pub shutdown_tx: Option<std::sync::mpsc::Sender<()>>,
    pub disconnected: Arc<HelperDisconnect>,
}

#[derive(Clone, Copy)]
pub(super) enum HostBufferTag {
    Ui(u64),
    Video,
    Static,
}

#[derive(Clone, Copy)]
pub(super) enum HostSurfaceTag {
    Root,
    Video,
    Ui,
    Popup,
}

#[derive(Clone, Copy)]
pub(super) enum DmabufFeedbackTag {
    Default,
}

pub(super) enum DmaBufParamsTag {
    Video(
        wayland_server::Weak<
            wayland_protocols::wp::linux_dmabuf::zv1::server::zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        >,
    ),
    VideoImmed,
    Ui(u64),
}

pub(super) struct PendingUiDmaBuf {
    pub role: WaylandSurfaceRole,
    pub logical_w: u16,
    pub logical_h: u16,
    pub x: i32,
    pub y: i32,
    pub _fds: Vec<OwnedFd>,
    pub _lease: WaylandSlotLease,
}

pub(super) struct UiBufferCleanup {
    pub pool: Option<wl_shm_pool::WlShmPool>,
    pub _fds: Vec<OwnedFd>,
    pub _lease: Option<WaylandSlotLease>,
}

#[derive(Clone, Default)]
pub(super) struct OutputInfo {
    pub global_name: u32,
    pub output: Option<wl_output::WlOutput>,
    pub x: i32,
    pub y: i32,
    pub physical_width: i32,
    pub physical_height: i32,
    pub subpixel: i32,
    pub make: String,
    pub model: String,
    pub transform: i32,
    pub mode_flags: u32,
    pub mode_width: i32,
    pub mode_height: i32,
    pub mode_refresh: i32,
    pub scale: i32,
    pub name: String,
    pub description: String,
    pub done: bool,
}

#[derive(Clone)]
pub(super) enum FeedbackEvent {
    MainDevice(Vec<u8>),
    FormatTable { fd: Arc<OwnedFd>, size: u32 },
    TrancheTargetDevice(Vec<u8>),
    TrancheFlags(u32),
    TrancheFormats(Vec<u8>),
    TrancheDone,
    Done,
}

#[derive(Clone)]
pub(super) struct HeldKey {
    pub evdev_code: u32,
    pub base_sym: u32,
    pub symbol: u32,
    pub next_fire: Instant,
    pub interval: std::time::Duration,
}

pub(super) struct PopupSurface {
    pub surface: wl_surface::WlSurface,
    pub subsurface: wl_subsurface::WlSubsurface,
    pub viewport: Option<wp_viewport::WpViewport>,
    pub x: i32,
    pub y: i32,
}

pub(super) struct HostSurfaces {
    pub compositor: wl_compositor::WlCompositor,
    pub subcompositor: wl_subcompositor::WlSubcompositor,
    pub viewporter: wp_viewporter::WpViewporter,
    pub dmabuf: zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    pub dmabuf_version: u32,
    pub shm: Option<wl_shm::WlShm>,
    pub seat: Option<wl_seat::WlSeat>,
    pub ext: ExtensionManagers,
    pub root_surface: wl_surface::WlSurface,
    pub xdg_surface: xdg_surface::XdgSurface,
    pub xdg_toplevel: xdg_toplevel::XdgToplevel,
    pub root_viewport: wp_viewport::WpViewport,
    pub _decoration: Option<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1>,
    pub _fractional_scale: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    pub black_buffer: Option<wl_buffer::WlBuffer>,
    pub _black_pool: Option<wl_shm_pool::WlShmPool>,
    pub video_surface: wl_surface::WlSurface,
    pub _video_subsurface: wl_subsurface::WlSubsurface,
    pub video_viewport: wp_viewport::WpViewport,
    pub ui_surface: wl_surface::WlSurface,
    pub _ui_subsurface: wl_subsurface::WlSubsurface,
    pub ui_viewport: wp_viewport::WpViewport,
}

impl HostSurfaces {
    pub fn sync_root_geometry(&self, width: u16, height: u16) {
        let (w, h) = (i32::from(width), i32::from(height));
        self.xdg_surface.set_window_geometry(0, 0, w, h);
        self.root_viewport.set_destination(w, h);
        self.ui_viewport.set_destination(w, h);
        if let Some(black) = self.black_buffer.as_ref() {
            self.root_surface.attach(Some(black), 0, 0);
            self.root_surface.damage(0, 0, w, h);
        }
        self.root_surface.commit();
    }
}

pub(super) struct HostState {
    pub conn: Connection,
    pub qh: QueueHandle<AppState>,
    pub ui: WaylandUiHandle,
    pub fullscreen_flag: Arc<AtomicBool>,
    pub ui_frame_committed: Arc<AtomicBool>,
    pub globals: HostGlobals,
    pub surfaces: Option<HostSurfaces>,
    pub _registry: Option<wl_registry::WlRegistry>,
    pub popup: Option<PopupSurface>,
    pub outputs: Vec<OutputInfo>,
    pub shm_formats: Vec<u32>,
    pub dmabuf_formats: Vec<(u32, u64)>,
    pub dmabuf_feedback: Vec<FeedbackEvent>,
    pub dmabuf_feedback_done: bool,
    pub presentation_clock_id: Option<u32>,
    pub cm_intents: Vec<u32>,
    pub cm_features: Vec<u32>,
    pub cm_tfs: Vec<u32>,
    pub cm_primaries: Vec<u32>,
    pub cm_done: bool,
    pub cr_alpha_modes: Vec<u32>,
    pub cr_coeffs_ranges: Vec<(u32, u32)>,
    pub cr_done: bool,
    pub ui_buffers: HashMap<u64, UiBufferCleanup>,
    pub pending_ui_dmabufs: HashMap<u64, PendingUiDmaBuf>,
    pub ui_dmabuf_buffers: HashMap<ObjectId, UiBufferCleanup>,
    pub last_view_commit_seq: u64,
    pub last_popup_commit_seq: u64,
    pub dmabuf_rejected: bool,
    pub next_ui_buffer_id: u64,
    pub logical_w: u16,
    pub logical_h: u16,
    pub scale_120: u32,
    pub fullscreen: bool,
    pub maximized: bool,
    pub pending_configure_w: i32,
    pub pending_configure_h: i32,
    pub pending_configure_fs: bool,
    pub pending_configure_max: bool,
    pub configured_once: bool,
    pub user_configured: bool,
    pub has_server_decoration: bool,
    pub on_output_ids: Vec<u32>,
    pub pointer: Option<wayland_client::protocol::wl_pointer::WlPointer>,
    pub cursor_device: Option<wp_cursor_shape_device_v1::WpCursorShapeDeviceV1>,
    pub pointer_enter_serial: u32,
    pub last_cursor_shape: u32,
    pub pointer_on_popup: bool,
    pub pointer_x: i32,
    pub pointer_y: i32,
    pub modifiers: WaylandModifiers,
    pub wheel_x_acc: f64,
    pub wheel_y_acc: f64,
    pub wheel_x_120: i32,
    pub wheel_y_120: i32,
    pub seat_name: String,
    pub seat_caps: u32,
    pub keyboard: Option<wayland_client::protocol::wl_keyboard::WlKeyboard>,
    pub keymap: Option<WaylandKeymap>,
    pub repeat_rate: i32,
    pub repeat_delay: i32,
    pub held_key: Option<HeldKey>,
    pub default_feedback: Option<zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1>,
}

impl HostState {
    pub fn surfaces(&self) -> io::Result<&HostSurfaces> {
        self.surfaces
            .as_ref()
            .ok_or_else(|| io::Error::other("wayland host surfaces not initialized"))
    }

    pub fn init_surfaces(&mut self) -> io::Result<()> {
        let compositor = self
            .globals
            .compositor
            .clone()
            .ok_or_else(|| io::Error::other("missing wl_compositor global"))?;
        let subcompositor = self
            .globals
            .subcompositor
            .clone()
            .ok_or_else(|| io::Error::other("missing wl_subcompositor global"))?;
        let xdg_wm_base = self
            .globals
            .xdg_wm_base
            .clone()
            .ok_or_else(|| io::Error::other("missing xdg_wm_base global"))?;
        let viewporter = self
            .globals
            .viewporter
            .clone()
            .ok_or_else(|| io::Error::other("missing wp_viewporter global"))?;
        let (dmabuf, dmabuf_version) = self
            .globals
            .dmabuf
            .clone()
            .ok_or_else(|| io::Error::other("missing zwp_linux_dmabuf_v1 global"))?;

        let qh = &self.qh;
        let root_surface = compositor.create_surface(qh, HostSurfaceTag::Root);
        let root_viewport = viewporter.get_viewport(&root_surface, qh, ());
        let xdg_surface = xdg_wm_base.get_xdg_surface(&root_surface, qh, ());
        let xdg_toplevel = xdg_surface.get_toplevel(qh, ());
        xdg_toplevel.set_app_id("org.mediaflick.Desktop".to_string());
        xdg_toplevel.set_title("MediaFlick".to_string());
        xdg_toplevel.set_min_size(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT);
        let decoration = self.globals.decoration_mgr.as_ref().map(|mgr| {
            let dec = mgr.get_toplevel_decoration(&xdg_toplevel, qh, ());
            dec.set_mode(zxdg_toplevel_decoration_v1::Mode::ServerSide);
            dec
        });
        let fractional_scale = self
            .globals
            .ext
            .fractional_scale_mgr
            .as_ref()
            .map(|mgr| mgr.get_fractional_scale(&root_surface, qh, ()));

        let video_surface = compositor.create_surface(qh, HostSurfaceTag::Video);
        let empty_region = compositor.create_region(qh, ());
        video_surface.set_input_region(Some(&empty_region));
        empty_region.destroy();
        let video_subsurface = subcompositor.get_subsurface(&video_surface, &root_surface, qh, ());
        video_subsurface.set_desync();
        video_subsurface.set_position(0, 0);
        let video_viewport = viewporter.get_viewport(&video_surface, qh, ());

        let ui_surface = compositor.create_surface(qh, HostSurfaceTag::Ui);
        let ui_subsurface = subcompositor.get_subsurface(&ui_surface, &root_surface, qh, ());
        ui_subsurface.set_desync();
        ui_subsurface.place_above(&video_surface);
        ui_subsurface.set_position(0, 0);
        let ui_viewport = viewporter.get_viewport(&ui_surface, qh, ());
        root_surface.commit();

        let (black_buffer, black_pool) = create_black_background_buffer(
            self.globals.single_pixel_mgr.as_ref(),
            self.globals.shm.as_ref(),
            qh,
        );

        self.surfaces = Some(HostSurfaces {
            compositor,
            subcompositor,
            viewporter,
            dmabuf,
            dmabuf_version,
            shm: self.globals.shm.clone(),
            seat: self.globals.seat.clone(),
            ext: self.globals.ext.clone(),
            root_surface,
            xdg_surface,
            xdg_toplevel,
            root_viewport,
            _decoration: decoration,
            _fractional_scale: fractional_scale,
            black_buffer,
            _black_pool: black_pool,
            video_surface,
            _video_subsurface: video_subsurface,
            video_viewport,
            ui_surface,
            _ui_subsurface: ui_subsurface,
            ui_viewport,
        });
        Ok(())
    }
}

pub(super) fn connect_host(
    connect: impl FnOnce() -> io::Result<Connection>,
    ui: WaylandUiHandle,
    fullscreen_flag: Arc<AtomicBool>,
    ui_frame_committed: Arc<AtomicBool>,
) -> io::Result<(wayland_client::EventQueue<AppState>, AppState)> {
    let conn = connect()?;
    let mut queue = conn.new_event_queue::<AppState>();
    let qh = queue.handle();
    let bridge = BridgeState::new()?;
    let disconnected = Arc::clone(&bridge.disconnected);
    let registry = conn.display().get_registry(&qh, ());
    let host = HostState {
        conn,
        qh,
        ui,
        fullscreen_flag,
        ui_frame_committed,
        globals: HostGlobals::default(),
        surfaces: None,
        _registry: Some(registry),
        popup: None,
        outputs: Vec::new(),
        shm_formats: Vec::new(),
        dmabuf_formats: Vec::new(),
        dmabuf_feedback: Vec::new(),
        dmabuf_feedback_done: false,
        presentation_clock_id: None,
        cm_intents: Vec::new(),
        cm_features: Vec::new(),
        cm_tfs: Vec::new(),
        cm_primaries: Vec::new(),
        cm_done: false,
        cr_alpha_modes: Vec::new(),
        cr_coeffs_ranges: Vec::new(),
        cr_done: false,
        ui_buffers: HashMap::new(),
        pending_ui_dmabufs: HashMap::new(),
        ui_dmabuf_buffers: HashMap::new(),
        last_view_commit_seq: 0,
        last_popup_commit_seq: 0,
        dmabuf_rejected: false,
        next_ui_buffer_id: 1,
        logical_w: 1280,
        logical_h: 720,
        scale_120: 120,
        fullscreen: false,
        maximized: false,
        pending_configure_w: 0,
        pending_configure_h: 0,
        pending_configure_fs: false,
        pending_configure_max: false,
        configured_once: false,
        user_configured: false,
        has_server_decoration: false,
        on_output_ids: Vec::new(),
        pointer: None,
        cursor_device: None,
        pointer_enter_serial: 0,
        last_cursor_shape: 1,
        pointer_on_popup: false,
        pointer_x: 0,
        pointer_y: 0,
        modifiers: WaylandModifiers::default(),
        wheel_x_acc: 0.0,
        wheel_y_acc: 0.0,
        wheel_x_120: 0,
        wheel_y_120: 0,
        seat_name: "seat0".to_string(),
        seat_caps: 0,
        keyboard: None,
        keymap: WaylandKeymap::new(),
        repeat_rate: 25,
        repeat_delay: 400,
        held_key: None,
        default_feedback: None,
    };
    let mut app = AppState {
        host,
        bridge,
        running: true,
        shutdown_tx: None,
        disconnected,
    };
    queue
        .roundtrip(&mut app)
        .map_err(|err| io::Error::other(err.to_string()))?;
    queue
        .roundtrip(&mut app)
        .map_err(|err| io::Error::other(err.to_string()))?;
    app.host.init_surfaces()?;
    queue
        .roundtrip(&mut app)
        .map_err(|err| io::Error::other(err.to_string()))?;
    Ok((queue, app))
}

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            bind_global(&mut state.host, registry, name, &interface, version, qh);
        }
    }
}

impl Dispatch<wl_shm::WlShm, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_shm::WlShm,
        event: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_shm::Event::Format { format } = event {
            state.host.shm_formats.push(u32::from(format));
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        global_name: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(info) = state
            .host
            .outputs
            .iter_mut()
            .find(|o| o.global_name == *global_name)
        {
            apply_output_event(info, event);
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for AppState {
    fn event(
        state: &mut Self,
        callback: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { callback_data } = event
            && let Some(pos) = state
                .bridge
                .pending_frames
                .iter()
                .position(|(hid, _)| *hid == callback.id())
            && let Some((_, client_cb)) = state.bridge.pending_frames.remove(pos)
        {
            client_cb.done(callback_data);
        }
    }
}

impl Dispatch<wl_surface::WlSurface, HostSurfaceTag> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_surface::WlSurface,
        event: wl_surface::Event,
        tag: &HostSurfaceTag,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if !matches!(tag, HostSurfaceTag::Root | HostSurfaceTag::Video) {
            return;
        }
        match event {
            wl_surface::Event::Enter { output } => {
                if let Some(gn) = state
                    .host
                    .outputs
                    .iter()
                    .find(|o| o.output.as_ref().is_some_and(|ho| ho.id() == output.id()))
                    .map(|o| o.global_name)
                {
                    if !state.host.on_output_ids.contains(&gn) {
                        state.host.on_output_ids.push(gn);
                    }
                    if let Some(cs_weak) = state.bridge.video_client_surface.as_ref()
                        && let Ok(cs) = cs_weak.upgrade()
                    {
                        for (cgn, weak_co) in &state.bridge.client_outputs {
                            if *cgn == gn
                                && let Ok(co) = weak_co.upgrade()
                            {
                                cs.enter(&co);
                            }
                        }
                    }
                }
            }
            wl_surface::Event::Leave { output } => {
                if let Some(gn) = state
                    .host
                    .outputs
                    .iter()
                    .find(|o| o.output.as_ref().is_some_and(|ho| ho.id() == output.id()))
                    .map(|o| o.global_name)
                {
                    state.host.on_output_ids.retain(|id| *id != gn);
                    if let Some(cs_weak) = state.bridge.video_client_surface.as_ref()
                        && let Ok(cs) = cs_weak.upgrade()
                    {
                        for (cgn, weak_co) in &state.bridge.client_outputs {
                            if *cgn == gn
                                && let Ok(co) = weak_co.upgrade()
                            {
                                cs.leave(&co);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, HostBufferTag> for AppState {
    fn event(
        state: &mut Self,
        buf: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        tag: &HostBufferTag,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if !matches!(event, wl_buffer::Event::Release) {
            return;
        }
        match tag {
            HostBufferTag::Ui(id) => {
                buf.destroy();
                if let Some(cleanup) = state.host.ui_buffers.remove(id)
                    && let Some(pool) = cleanup.pool
                {
                    pool.destroy();
                }
            }
            HostBufferTag::Video => {
                if let Some(cleanup) = state.host.ui_dmabuf_buffers.remove(&buf.id()) {
                    buf.destroy();
                    drop(cleanup);
                } else if let Some(entry) = state.bridge.video_buffers.get(&buf.id())
                    && let Ok(cbuf) = entry.client_buffer.upgrade()
                {
                    cbuf.release();
                } else {
                    buf.destroy();
                }
            }
            HostBufferTag::Static => {}
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for AppState {
    fn event(
        _: &mut Self,
        wm_base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for AppState {
    fn event(
        state: &mut Self,
        xdg_surf: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let xdg_surface::Event::Configure { serial } = event else {
            return;
        };
        xdg_surf.ack_configure(serial);
        if state.host.pending_configure_w > 0 && state.host.pending_configure_h > 0 {
            state.host.logical_w =
                u16::try_from(state.host.pending_configure_w.clamp(320, 16384)).unwrap_or(1280);
            state.host.logical_h =
                u16::try_from(state.host.pending_configure_h.clamp(240, 16384)).unwrap_or(720);
        }
        state.host.fullscreen = state.host.pending_configure_fs;
        state.host.maximized = state.host.pending_configure_max;
        state
            .host
            .fullscreen_flag
            .store(state.host.fullscreen, Ordering::Release);
        state.host.configured_once = true;
        if let Some(s) = state.host.surfaces.as_ref() {
            s.sync_root_geometry(state.host.logical_w, state.host.logical_h);
        }
        state.host.emit_configure_event();
        state.bridge.notify_configure(
            state.host.logical_w,
            state.host.logical_h,
            state.host.fullscreen,
            state.host.scale_120,
        );
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                state.host.pending_configure_w = width;
                state.host.pending_configure_h = height;
                let mut fs = false;
                let mut max = false;
                for &chunk in states.as_chunks::<4>().0 {
                    let val = u32::from_ne_bytes(chunk);
                    if val == xdg_toplevel::State::Fullscreen as u32 {
                        fs = true;
                    } else if val == xdg_toplevel::State::Maximized as u32 {
                        max = true;
                    }
                }
                state.host.pending_configure_fs = fs;
                state.host.pending_configure_max = max;
            }
            xdg_toplevel::Event::Close => {
                state.host.ui.push_event(WaylandUiEvent::CloseRequested);
            }
            _ => {}
        }
    }
}

impl Dispatch<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1,
        event: zxdg_toplevel_decoration_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_toplevel_decoration_v1::Event::Configure { mode } = event {
            state.host.has_server_decoration = matches!(
                mode,
                WEnum::Value(zxdg_toplevel_decoration_v1::Mode::ServerSide)
            );
        }
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            let clamped = if scale == 0 { 120 } else { scale };
            if clamped != state.host.scale_120 {
                state.host.scale_120 = clamped;
                state.host.emit_configure_event();
                state.bridge.notify_configure(
                    state.host.logical_w,
                    state.host.logical_h,
                    state.host.fullscreen,
                    state.host.scale_120,
                );
            }
        }
    }
}

impl Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        event: zwp_linux_dmabuf_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_linux_dmabuf_v1::Event::Format { format } => {
                state.host.dmabuf_formats.push((format, 0));
            }
            zwp_linux_dmabuf_v1::Event::Modifier {
                format,
                modifier_hi,
                modifier_lo,
            } => {
                let modifier = (u64::from(modifier_hi) << 32) | u64::from(modifier_lo);
                state.host.dmabuf_formats.push((format, modifier));
            }
            _ => {}
        }
    }
}

impl Dispatch<zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1, DmabufFeedbackTag>
    for AppState
{
    fn event(
        state: &mut Self,
        _: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
        event: zwp_linux_dmabuf_feedback_v1::Event,
        _: &DmabufFeedbackTag,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state.host.dmabuf_feedback_done
            && matches!(
                event,
                zwp_linux_dmabuf_feedback_v1::Event::MainDevice { .. }
                    | zwp_linux_dmabuf_feedback_v1::Event::FormatTable { .. }
            )
        {
            state.host.dmabuf_feedback.clear();
            state.host.dmabuf_feedback_done = false;
        }
        match event {
            zwp_linux_dmabuf_feedback_v1::Event::MainDevice { device } => {
                let rdev = <[u8; 8]>::try_from(device.as_slice())
                    .ok()
                    .map(u64::from_ne_bytes)
                    .or_else(|| {
                        <[u8; 4]>::try_from(device.as_slice())
                            .ok()
                            .map(|bytes| u64::from(u32::from_ne_bytes(bytes)))
                    });
                if let Some(rdev) = rdev {
                    state.host.ui.set_main_device_rdev(rdev);
                }
                state
                    .host
                    .dmabuf_feedback
                    .push(FeedbackEvent::MainDevice(device));
            }
            zwp_linux_dmabuf_feedback_v1::Event::FormatTable { fd, size } => {
                state.host.dmabuf_feedback.push(FeedbackEvent::FormatTable {
                    fd: Arc::new(fd),
                    size,
                });
            }
            zwp_linux_dmabuf_feedback_v1::Event::TrancheTargetDevice { device } => {
                state
                    .host
                    .dmabuf_feedback
                    .push(FeedbackEvent::TrancheTargetDevice(device));
            }
            zwp_linux_dmabuf_feedback_v1::Event::TrancheFlags { flags } => {
                state
                    .host
                    .dmabuf_feedback
                    .push(FeedbackEvent::TrancheFlags(u32::from(flags)));
            }
            zwp_linux_dmabuf_feedback_v1::Event::TrancheFormats { indices } => {
                state
                    .host
                    .dmabuf_feedback
                    .push(FeedbackEvent::TrancheFormats(indices));
            }
            zwp_linux_dmabuf_feedback_v1::Event::TrancheDone => {
                state.host.dmabuf_feedback.push(FeedbackEvent::TrancheDone);
            }
            zwp_linux_dmabuf_feedback_v1::Event::Done => {
                state.host.dmabuf_feedback.push(FeedbackEvent::Done);
                state.host.dmabuf_feedback_done = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, DmaBufParamsTag> for AppState {
    wayland_client::event_created_child!(AppState, zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, [
        zwp_linux_buffer_params_v1::EVT_CREATED_OPCODE => (wl_buffer::WlBuffer, HostBufferTag::Video),
    ]);

    fn event(
        state: &mut Self,
        params: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        event: zwp_linux_buffer_params_v1::Event,
        tag: &DmaBufParamsTag,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match tag {
            DmaBufParamsTag::VideoImmed => {
                params.destroy();
            }
            DmaBufParamsTag::Ui(req_id) => {
                state
                    .host
                    .handle_ui_dmabuf_params_event(*req_id, params, event);
            }
            DmaBufParamsTag::Video(weak_cparams) => {
                let Ok(cparams) = weak_cparams.upgrade() else {
                    params.destroy();
                    return;
                };
                match event {
                    zwp_linux_buffer_params_v1::Event::Created { buffer } => {
                        if let Some(dh) = state.bridge.dhandle.as_ref()
                            && let Ok(client) = dh.get_client(cparams.id())
                            && let Ok(cbuf) = client.create_resource::<
                                wayland_server::protocol::wl_buffer::WlBuffer,
                                (),
                                AppState,
                            >(dh, 1, ())
                        {
                            let hid = buffer.id();
                            state
                                .bridge
                                .buffer_id_by_client
                                .insert(cbuf.id(), hid.clone());
                            state.bridge.video_buffers.insert(
                                hid,
                                super::bridge::VideoBufferEntry {
                                    host_buffer: buffer,
                                    client_buffer: cbuf.downgrade(),
                                    _fds: Vec::new(),
                                },
                            );
                            cparams.created(&cbuf);
                        } else {
                            buffer.destroy();
                            cparams.failed();
                        }
                        params.destroy();
                    }
                    zwp_linux_buffer_params_v1::Event::Failed => {
                        cparams.failed();
                        params.destroy();
                    }
                    _ => {}
                }
            }
        }
    }
}
