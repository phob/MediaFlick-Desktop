use std::collections::{HashMap, VecDeque};
use std::io;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use wayland_client::Proxy as _;
use wayland_protocols::wp::fractional_scale::v1::server::{
    wp_fractional_scale_manager_v1, wp_fractional_scale_v1,
};
use wayland_protocols::wp::linux_dmabuf::zv1::server::zwp_linux_dmabuf_v1;
use wayland_protocols::wp::viewporter::server::{wp_viewport, wp_viewporter};
use wayland_protocols::xdg::shell::server::{
    xdg_positioner, xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland_server::WEnum;
use wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use wayland_server::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_region, wl_seat,
    wl_shm, wl_surface, wl_touch,
};
use wayland_server::{
    Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, New, Resource,
};

use super::client::{AppState, HostState, connect_host};
use super::{
    HostControlCommand, PollFd, WaylandUiCommand, WaylandUiHandle, drain_wake, poll_fd,
    poll_has_input, wait_poll,
};

#[derive(Default)]
pub(super) struct HelperDisconnect {
    pub flag: AtomicBool,
}

struct HelperClientState {
    disconnected: Arc<HelperDisconnect>,
}

impl ClientData for HelperClientState {
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {
        self.disconnected.flag.store(true, Ordering::Release);
    }
}

pub(super) struct VideoBufferEntry {
    pub host_buffer: wayland_client::protocol::wl_buffer::WlBuffer,
    pub client_buffer: wayland_server::Weak<wl_buffer::WlBuffer>,
    pub _fds: Vec<OwnedFd>,
}

pub(super) struct ServerShmPool {
    pub host_pool: wayland_client::protocol::wl_shm_pool::WlShmPool,
}

pub(super) struct ServerDmaBufParams {
    pub planes: Vec<(OwnedFd, u32, u32, u32, u32, u32)>,
}

pub(super) enum PendingAttach {
    Attach(wayland_client::protocol::wl_buffer::WlBuffer),
    Detach,
}

#[derive(Default)]
pub(super) struct BridgeState {
    pub dhandle: Option<DisplayHandle>,
    pub disconnected: Arc<HelperDisconnect>,
    pub video_client_surface: Option<wayland_server::Weak<wl_surface::WlSurface>>,
    pub client_xdg_surfaces: Vec<wayland_server::Weak<xdg_surface::XdgSurface>>,
    pub client_toplevels: Vec<wayland_server::Weak<xdg_toplevel::XdgToplevel>>,
    pub xdg_initial_configured: bool,
    pub client_fractional_scales: Vec<wayland_server::Weak<wp_fractional_scale_v1::WpFractionalScaleV1>>,
    pub client_outputs: Vec<(u32, wayland_server::Weak<wl_output::WlOutput>)>,
    pub video_buffers: HashMap<wayland_client::backend::ObjectId, VideoBufferEntry>,
    pub buffer_id_by_client: HashMap<wayland_server::backend::ObjectId, wayland_client::backend::ObjectId>,
    pub shm_pools: HashMap<wayland_server::backend::ObjectId, ServerShmPool>,
    pub dmabuf_params: HashMap<wayland_server::backend::ObjectId, ServerDmaBufParams>,
    pub pending_frames: VecDeque<(wayland_client::backend::ObjectId, wl_callback::WlCallback)>,
    pub pending_attach: Option<PendingAttach>,
    pub pending_damage: Vec<(i32, i32, i32, i32, bool)>,
    pub pending_scale: Option<i32>,
    pub pending_transform: Option<i32>,
    pub video_syncobj_surface: Option<
        wayland_protocols::wp::linux_drm_syncobj::v1::client::wp_linux_drm_syncobj_surface_v1::WpLinuxDrmSyncobjSurfaceV1,
    >,
    pub video_fifo: Option<wayland_protocols::wp::fifo::v1::client::wp_fifo_v1::WpFifoV1>,
    pub video_commit_timer: Option<
        wayland_protocols::wp::commit_timing::v1::client::wp_commit_timer_v1::WpCommitTimerV1,
    >,
    pub video_cm_surface: Option<
        wayland_protocols::wp::color_management::v1::client::wp_color_management_surface_v1::WpColorManagementSurfaceV1,
    >,
    pub video_cr_surface: Option<
        wayland_protocols::wp::color_representation::v1::client::wp_color_representation_surface_v1::WpColorRepresentationSurfaceV1,
    >,
    pub host_syncobj_timelines: HashMap<
        wayland_server::backend::ObjectId,
        wayland_protocols::wp::linux_drm_syncobj::v1::client::wp_linux_drm_syncobj_timeline_v1::WpLinuxDrmSyncobjTimelineV1,
    >,
    pub host_pres_feedbacks: HashMap<wayland_client::backend::ObjectId, wayland_protocols::wp::presentation_time::server::wp_presentation_feedback::WpPresentationFeedback>,
    pub host_cm_outputs: HashMap<wayland_client::backend::ObjectId, wayland_protocols::wp::color_management::v1::server::wp_color_management_output_v1::WpColorManagementOutputV1>,
    pub client_cm_outputs: HashMap<wayland_server::backend::ObjectId, wayland_protocols::wp::color_management::v1::client::wp_color_management_output_v1::WpColorManagementOutputV1>,
    pub host_cm_feedbacks: HashMap<wayland_client::backend::ObjectId, wayland_protocols::wp::color_management::v1::server::wp_color_management_surface_feedback_v1::WpColorManagementSurfaceFeedbackV1>,
    pub client_cm_feedbacks: HashMap<wayland_server::backend::ObjectId, wayland_protocols::wp::color_management::v1::client::wp_color_management_surface_feedback_v1::WpColorManagementSurfaceFeedbackV1>,
    pub host_img_descs: HashMap<wayland_client::backend::ObjectId, (wayland_protocols::wp::color_management::v1::client::wp_image_description_v1::WpImageDescriptionV1, wayland_protocols::wp::color_management::v1::server::wp_image_description_v1::WpImageDescriptionV1)>,
    pub host_img_infos: HashMap<wayland_client::backend::ObjectId, wayland_protocols::wp::color_management::v1::server::wp_image_description_info_v1::WpImageDescriptionInfoV1>,
    pub client_img_descs_by_id: HashMap<wayland_server::backend::ObjectId, wayland_protocols::wp::color_management::v1::client::wp_image_description_v1::WpImageDescriptionV1>,
    pub client_img_refs_by_id: HashMap<wayland_server::backend::ObjectId, wayland_protocols::wp::color_management::v1::client::wp_image_description_reference_v1::WpImageDescriptionReferenceV1>,
    pub client_icc_creators: HashMap<wayland_server::backend::ObjectId, wayland_protocols::wp::color_management::v1::client::wp_image_description_creator_icc_v1::WpImageDescriptionCreatorIccV1>,
    pub client_param_creators: HashMap<wayland_server::backend::ObjectId, wayland_protocols::wp::color_management::v1::client::wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1>,
    pub next_serial: u32,
}

impl BridgeState {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            disconnected: Arc::new(HelperDisconnect {
                flag: AtomicBool::new(false),
            }),
            next_serial: 1,
            ..Self::default()
        })
    }

    pub fn next_serial(&mut self) -> u32 {
        let s = self.next_serial;
        self.next_serial = self.next_serial.wrapping_add(1).max(1);
        s
    }

    pub fn cleanup_helper(&mut self, host: &HostState) {
        if let Some(obj) = self.video_syncobj_surface.take() {
            obj.destroy();
        }
        if let Some(obj) = self.video_fifo.take() {
            obj.destroy();
        }
        if let Some(obj) = self.video_commit_timer.take() {
            obj.destroy();
        }
        if let Some(obj) = self.video_cm_surface.take() {
            obj.destroy();
        }
        if let Some(obj) = self.video_cr_surface.take() {
            obj.destroy();
        }
        if let Some(s) = host.surfaces.as_ref() {
            s.video_surface.attach(None, 0, 0);
            s.video_viewport.set_source(-1.0, -1.0, -1.0, -1.0);
            s.video_viewport.set_destination(-1, -1);
            s.video_surface.commit();
        }
        for (_, entry) in self.video_buffers.drain() {
            entry.host_buffer.destroy();
        }
        self.buffer_id_by_client.clear();
        for (_, pool) in self.shm_pools.drain() {
            pool.host_pool.destroy();
        }
        self.dmabuf_params.clear();
        self.pending_frames.clear();
        self.pending_attach = None;
        self.pending_damage.clear();
        self.pending_scale = None;
        self.pending_transform = None;
        self.video_client_surface = None;
        self.client_xdg_surfaces.clear();
        self.client_toplevels.clear();
        self.xdg_initial_configured = false;
        self.client_fractional_scales.clear();
        self.client_outputs.clear();
        self.host_syncobj_timelines.clear();
        self.host_pres_feedbacks.clear();
        for (_, h_out) in self.client_cm_outputs.drain() {
            h_out.destroy();
        }
        for (_, h_fb) in self.client_cm_feedbacks.drain() {
            h_fb.destroy();
        }
        self.host_cm_outputs.clear();
        self.host_cm_feedbacks.clear();
        self.host_img_descs.clear();
        self.host_img_infos.clear();
        self.client_img_descs_by_id.clear();
        self.client_img_refs_by_id.clear();
        self.client_icc_creators.clear();
        self.client_param_creators.clear();
    }

    pub fn notify_configure(&mut self, width: u16, height: u16, fullscreen: bool, scale_120: u32) {
        let mut states = Vec::new();
        states.extend_from_slice(&(xdg_toplevel::State::Activated as u32).to_ne_bytes());
        if fullscreen {
            states.extend_from_slice(&(xdg_toplevel::State::Fullscreen as u32).to_ne_bytes());
        }
        self.client_toplevels.retain(|w| {
            if let Ok(tl) = w.upgrade() {
                tl.configure(i32::from(width), i32::from(height), states.clone());
                true
            } else {
                false
            }
        });
        let serial = self.next_serial();
        self.client_xdg_surfaces.retain(|w| {
            if let Ok(xs) = w.upgrade() {
                xs.configure(serial);
                true
            } else {
                false
            }
        });
        self.client_fractional_scales.retain(|w| {
            if let Ok(fs) = w.upgrade() {
                fs.preferred_scale(scale_120);
                true
            } else {
                false
            }
        });
    }
}

pub(super) fn run_host_loop(
    mut wake_rx: UnixStream,
    ui_cmd_rx: &Receiver<WaylandUiCommand>,
    ctl_rx: &Receiver<HostControlCommand>,
    ui: WaylandUiHandle,
    fullscreen_flag: Arc<AtomicBool>,
    ui_frame_committed: Arc<AtomicBool>,
    ready_tx: &Sender<io::Result<()>>,
) {
    let Ok((mut queue, mut app)) = connect_host(ui, fullscreen_flag, ui_frame_committed) else {
        let _ = ready_tx.send(Err(io::Error::other(
            "failed to initialize Wayland host connection",
        )));
        return;
    };
    let mut display = match Display::<AppState>::new() {
        Ok(d) => d,
        Err(err) => {
            let _ = ready_tx.send(Err(io::Error::other(err.to_string())));
            return;
        }
    };
    let dh = display.handle();
    app.bridge.dhandle = Some(dh.clone());
    register_server_globals(&dh, &app.host);
    let _ = ready_tx.send(Ok(()));

    while app.running {
        if app.disconnected.flag.swap(false, Ordering::AcqRel) {
            app.bridge.cleanup_helper(&app.host);
        }
        drain_wake(&mut wake_rx);
        while let Ok(cmd) = ctl_rx.try_recv() {
            handle_control_command(cmd, &mut app, &mut display);
        }
        if !app.running {
            let _ = app.host.conn.flush();
            let _ = display.flush_clients();
            break;
        }
        while let Ok(cmd) = ui_cmd_rx.try_recv() {
            if app.host.apply_ui_command(cmd) {
                app.bridge.notify_configure(
                    app.host.logical_w,
                    app.host.logical_h,
                    app.host.fullscreen,
                    app.host.scale_120,
                );
            }
        }
        let _ = queue.dispatch_pending(&mut app);
        let _ = display.dispatch_clients(&mut app);
        let _ = app.host.conn.flush();
        let _ = display.flush_clients();

        let timeout = app.host.next_repeat_timeout_ms();
        let read_guard = queue.prepare_read();
        let wait_timeout = if read_guard.is_none() { 0 } else { timeout };
        let mut fds: [PollFd; 3] = [
            poll_fd(app.host.conn.as_fd().as_raw_fd()),
            poll_fd(display.backend().poll_fd().as_raw_fd()),
            poll_fd(wake_rx.as_raw_fd()),
        ];
        let _ = wait_poll(&mut fds, wait_timeout);
        if let Some(guard) = read_guard {
            if poll_has_input(fds[0].revents) {
                let _ = guard.read();
            } else {
                drop(guard);
            }
        }
        let _ = queue.dispatch_pending(&mut app);
        if poll_has_input(fds[1].revents) {
            let _ = display.dispatch_clients(&mut app);
        }
    }

    if let Some(tx) = app.shutdown_tx.take() {
        let _ = tx.send(());
    }
}

fn handle_control_command(
    cmd: HostControlCommand,
    app: &mut AppState,
    display: &mut Display<AppState>,
) {
    match cmd {
        HostControlCommand::CreateHelperSocket(reply) => {
            app.bridge.cleanup_helper(&app.host);
            let res = UnixStream::pair().and_then(|(server_end, client_end)| {
                let client_data = Arc::new(HelperClientState {
                    disconnected: Arc::clone(&app.bridge.disconnected),
                });
                display
                    .handle()
                    .insert_client(server_end, client_data)
                    .map_err(|err| io::Error::other(err.to_string()))?;
                Ok(OwnedFd::from(client_end))
            });
            let _ = reply.send(res);
        }
        HostControlCommand::SetFullscreen(mode) => app.host.set_fullscreen(mode),
        HostControlCommand::Shutdown(tx) => {
            app.bridge.cleanup_helper(&app.host);
            app.shutdown_tx = Some(tx);
            app.running = false;
        }
    }
}

fn register_server_globals(dh: &DisplayHandle, host: &HostState) {
    let Ok(s) = host.surfaces() else {
        return;
    };
    dh.create_global::<AppState, wl_compositor::WlCompositor, _>(
        s.compositor.version().clamp(1, 6),
        (),
    );
    if let Some(shm) = s.shm.as_ref() {
        dh.create_global::<AppState, wl_shm::WlShm, _>(shm.version().clamp(1, 2), ());
    }
    if let Some(seat) = s.seat.as_ref() {
        dh.create_global::<AppState, wl_seat::WlSeat, _>(seat.version().clamp(1, 7), ());
    }
    dh.create_global::<AppState, xdg_wm_base::XdgWmBase, _>(
        s.xdg_surface.version().clamp(1, 6),
        (),
    );
    dh.create_global::<AppState, wp_viewporter::WpViewporter, _>(1, ());
    if s.ext.fractional_scale_mgr.is_some() {
        dh.create_global::<AppState, wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, _>(
            1,
            (),
        );
    }
    dh.create_global::<AppState, zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, _>(
        s.dmabuf_version.clamp(3, 5),
        (),
    );
    for out in &host.outputs {
        let ver = out.output.as_ref().map_or(4, |o| o.version().clamp(1, 4));
        dh.create_global::<AppState, wl_output::WlOutput, u32>(ver, out.global_name);
    }
    super::bridge_ext::register_extension_globals(dh, s);
}

impl GlobalDispatch<wl_compositor::WlCompositor, ()> for AppState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_compositor::WlCompositor>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_compositor::WlCompositor,
        request: wl_compositor::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_compositor::Request::CreateSurface { id } => {
                data_init.init(id, ());
            }
            wl_compositor::Request::CreateRegion { id } => {
                data_init.init(id, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_region::WlRegion, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_region::WlRegion,
        _: wl_region::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        surface: &wl_surface::WlSurface,
        request: wl_surface::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let is_video = state
            .bridge
            .video_client_surface
            .as_ref()
            .is_some_and(|w| w.id() == surface.id());
        if let wl_surface::Request::Frame { callback } = request {
            let cb = data_init.init(callback, ());
            if is_video && let Some(s) = state.host.surfaces.as_ref() {
                let hcb = s.video_surface.frame(&state.host.qh, ());
                state.bridge.pending_frames.push_back((hcb.id(), cb));
            } else {
                cb.done(0);
            }
            return;
        }
        if !is_video {
            return;
        }
        match request {
            wl_surface::Request::Attach { buffer, .. } => {
                let pending = match buffer {
                    Some(b) => state
                        .bridge
                        .buffer_id_by_client
                        .get(&b.id())
                        .and_then(|hid| state.bridge.video_buffers.get(hid))
                        .map_or(PendingAttach::Detach, |entry| {
                            PendingAttach::Attach(entry.host_buffer.clone())
                        }),
                    None => PendingAttach::Detach,
                };
                state.bridge.pending_attach = Some(pending);
            }
            wl_surface::Request::Damage {
                x,
                y,
                width,
                height,
            } => state
                .bridge
                .pending_damage
                .push((x, y, width, height, false)),
            wl_surface::Request::DamageBuffer {
                x,
                y,
                width,
                height,
            } => state
                .bridge
                .pending_damage
                .push((x, y, width, height, true)),
            wl_surface::Request::SetBufferScale { scale } => {
                state.bridge.pending_scale = Some(scale);
            }
            wl_surface::Request::SetBufferTransform {
                transform: WEnum::Value(tr),
            } => {
                state.bridge.pending_transform = Some(tr as i32);
            }
            wl_surface::Request::Commit => {
                if !state.bridge.xdg_initial_configured && !state.bridge.client_toplevels.is_empty()
                {
                    state.bridge.xdg_initial_configured = true;
                    state.bridge.notify_configure(
                        state.host.logical_w,
                        state.host.logical_h,
                        state.host.fullscreen,
                        state.host.scale_120,
                    );
                }
                let Some(s) = state.host.surfaces.as_ref() else {
                    return;
                };
                match state.bridge.pending_attach.take() {
                    Some(PendingAttach::Attach(buf)) => s.video_surface.attach(Some(&buf), 0, 0),
                    Some(PendingAttach::Detach) => s.video_surface.attach(None, 0, 0),
                    None => {}
                }
                for (x, y, w, h, is_buf) in state.bridge.pending_damage.drain(..) {
                    if is_buf {
                        s.video_surface.damage_buffer(x, y, w, h);
                    } else {
                        s.video_surface.damage(x, y, w, h);
                    }
                }
                if let Some(scale) = state.bridge.pending_scale.take() {
                    s.video_surface.set_buffer_scale(scale);
                }
                if let Some(tr) = state.bridge.pending_transform.take()
                    && let Ok(t) =
                        wayland_client::protocol::wl_output::Transform::try_from(tr as u32)
                {
                    s.video_surface.set_buffer_transform(t);
                }
                s.video_surface.commit();
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_callback::WlCallback,
        _: wl_callback::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &wl_buffer::WlBuffer,
        request: wl_buffer::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let wl_buffer::Request::Destroy = request
            && let Some(hid) = state.bridge.buffer_id_by_client.remove(&resource.id())
            && let Some(entry) = state.bridge.video_buffers.remove(&hid)
        {
            entry.host_buffer.destroy();
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &wl_buffer::WlBuffer, _: &()) {
        if let Some(hid) = state.bridge.buffer_id_by_client.remove(&resource.id())
            && let Some(entry) = state.bridge.video_buffers.remove(&hid)
        {
            entry.host_buffer.destroy();
        }
    }
}

impl GlobalDispatch<wl_output::WlOutput, u32> for AppState {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_output::WlOutput>,
        global_name: &u32,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let out = data_init.init(resource, *global_name);
        if let Some(info) = state
            .host
            .outputs
            .iter()
            .find(|o| o.global_name == *global_name)
        {
            let sp = wl_output::Subpixel::try_from(u32::try_from(info.subpixel).unwrap_or(0))
                .unwrap_or(wl_output::Subpixel::Unknown);
            let tr = wl_output::Transform::try_from(u32::try_from(info.transform).unwrap_or(0))
                .unwrap_or(wl_output::Transform::Normal);
            out.geometry(
                info.x,
                info.y,
                info.physical_width,
                info.physical_height,
                sp,
                info.make.clone(),
                info.model.clone(),
                tr,
            );
            if info.mode_width > 0 && info.mode_height > 0 {
                let flags = wl_output::Mode::from_bits_truncate(info.mode_flags);
                out.mode(flags, info.mode_width, info.mode_height, info.mode_refresh);
            }
            if out.version() >= 2 && info.scale > 0 {
                out.scale(info.scale);
            }
            if out.version() >= 4 {
                if !info.name.is_empty() {
                    out.name(info.name.clone());
                }
                if !info.description.is_empty() {
                    out.description(info.description.clone());
                }
            }
            if out.version() >= 2 {
                out.done();
            }
        }
        state
            .bridge
            .client_outputs
            .push((*global_name, out.downgrade()));
    }
}

impl Dispatch<wl_output::WlOutput, u32> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_output::WlOutput,
        _: wl_output::Request,
        _: &u32,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<wl_seat::WlSeat, ()> for AppState {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_seat::WlSeat>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let seat = data_init.init(resource, ());
        seat.capabilities(wl_seat::Capability::Pointer | wl_seat::Capability::Keyboard);
        if seat.version() >= 2 {
            seat.name(state.host.seat_name.clone());
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_seat::WlSeat,
        request: wl_seat::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_seat::Request::GetPointer { id } => {
                data_init.init(id, ());
            }
            wl_seat::Request::GetKeyboard { id } => {
                data_init.init(id, ());
            }
            wl_seat::Request::GetTouch { id } => {
                data_init.init(id, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_pointer::WlPointer,
        _: wl_pointer::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_keyboard::WlKeyboard,
        _: wl_keyboard::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<wl_touch::WlTouch, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_touch::WlTouch,
        _: wl_touch::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<xdg_wm_base::XdgWmBase, ()> for AppState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<xdg_wm_base::XdgWmBase>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &xdg_wm_base::XdgWmBase,
        request: xdg_wm_base::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            xdg_wm_base::Request::CreatePositioner { id } => {
                data_init.init(id, ());
            }
            xdg_wm_base::Request::GetXdgSurface { id, surface } => {
                state.bridge.video_client_surface = Some(surface.downgrade());
                let xs = data_init.init(id, ());
                state.bridge.client_xdg_surfaces.push(xs.downgrade());
                for global_name in &state.host.on_output_ids {
                    for (gn, weak_out) in &state.bridge.client_outputs {
                        if gn == global_name
                            && let Ok(out) = weak_out.upgrade()
                        {
                            surface.enter(&out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<xdg_positioner::XdgPositioner, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &xdg_positioner::XdgPositioner,
        _: xdg_positioner::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        xdg_surf: &xdg_surface::XdgSurface,
        request: xdg_surface::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let xdg_surface::Request::GetToplevel { id } = request {
            let tl = data_init.init(id, ());
            state.bridge.client_toplevels.push(tl.downgrade());
            state.bridge.xdg_initial_configured = true;
            let mut states = Vec::new();
            states.extend_from_slice(&(xdg_toplevel::State::Activated as u32).to_ne_bytes());
            if state.host.fullscreen {
                states.extend_from_slice(&(xdg_toplevel::State::Fullscreen as u32).to_ne_bytes());
            }
            tl.configure(
                i32::from(state.host.logical_w),
                i32::from(state.host.logical_h),
                states,
            );
            let serial = state.bridge.next_serial();
            xdg_surf.configure(serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &xdg_toplevel::XdgToplevel,
        _: xdg_toplevel::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<wp_viewporter::WpViewporter, ()> for AppState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wp_viewporter::WpViewporter>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<wp_viewporter::WpViewporter, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wp_viewporter::WpViewporter,
        request: wp_viewporter::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wp_viewporter::Request::GetViewport { id, .. } = request {
            data_init.init(id, ());
        }
    }
}

impl Dispatch<wp_viewport::WpViewport, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_viewport::WpViewport,
        request: wp_viewport::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let Some(s) = state.host.surfaces.as_ref() else {
            return;
        };
        match request {
            wp_viewport::Request::SetSource {
                x,
                y,
                width,
                height,
            } => s.video_viewport.set_source(x, y, width, height),
            wp_viewport::Request::SetDestination { width, height } => {
                s.video_viewport.set_destination(width, height);
            }
            wp_viewport::Request::Destroy => {
                s.video_viewport.set_source(-1.0, -1.0, -1.0, -1.0);
                s.video_viewport.set_destination(-1, -1);
            }
            _ => {}
        }
    }
}

impl GlobalDispatch<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, ()> for AppState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        request: wp_fractional_scale_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wp_fractional_scale_manager_v1::Request::GetFractionalScale { id, .. } = request {
            let fs = data_init.init(id, ());
            fs.preferred_scale(state.host.scale_120);
            state.bridge.client_fractional_scales.push(fs.downgrade());
        }
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        _: wp_fractional_scale_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}
