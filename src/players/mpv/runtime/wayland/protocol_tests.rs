use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use wayland_protocols::wp::linux_dmabuf::zv1::server::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_feedback_v1, zwp_linux_dmabuf_v1,
};
use wayland_protocols::wp::viewporter::server::{wp_viewport, wp_viewporter};
use wayland_protocols::xdg::shell::server::{
    xdg_positioner, xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland_server::backend::{ClientData, ClientId, DisconnectReason, ObjectId};
use wayland_server::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_region, wl_shm, wl_shm_pool, wl_subcompositor,
    wl_subsurface, wl_surface,
};
use wayland_server::{
    Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, New, Resource,
};

use super::{
    PollFd, WaylandDmaBufPlane, WaylandFramePayload, WaylandHost, WaylandSlotLease,
    WaylandSurfaceRole, WaylandUiCommand, WaylandUiEvent, poll_fd, poll_has_input, wait_poll,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BufferKind {
    Shm,
    DmaBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DmaBufMode {
    CreateImmediately,
    FailAlways,
    HoldDeferred,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AttachState {
    Unchanged,
    Detached,
    Attached(u64, BufferKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SurfaceCommit {
    surface: ObjectId,
    attached: AttachState,
}

#[derive(Default)]
struct RecordedServer {
    created_surfaces: Vec<ObjectId>,
    created_subsurfaces: Vec<(ObjectId, ObjectId)>,
    subsurface_positions: Vec<(ObjectId, i32, i32)>,
    commits: Vec<SurfaceCommit>,
    destroyed_buffers: Vec<u64>,
    deferred_dmabuf_count: usize,
    created_deferred_buffers: Vec<u64>,
    disconnected: bool,
}

struct ServerState {
    next_buffer_serial: u64,
    dmabuf_mode: DmaBufMode,
    main_device_rdev: Option<u64>,
    pending_attach: HashMap<ObjectId, AttachState>,
    deferred_dmabuf: Vec<(Client, zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1)>,
    dh: Option<DisplayHandle>,
    recorded: Arc<Mutex<RecordedServer>>,
}

impl ServerState {
    fn next_serial(&mut self) -> u64 {
        self.next_buffer_serial = self.next_buffer_serial.saturating_add(1);
        self.next_buffer_serial
    }

    fn release_deferred(&mut self) {
        let Some(dh) = self.dh.clone() else {
            return;
        };
        let queued = std::mem::take(&mut self.deferred_dmabuf);
        for (client, params) in queued {
            let serial = self.next_serial();
            if let Ok(buf) = client.create_resource::<wl_buffer::WlBuffer, (u64, BufferKind), Self>(
                &dh,
                1,
                (serial, BufferKind::DmaBuf),
            ) {
                params.created(&buf);
                if let Ok(mut rec) = self.recorded.lock() {
                    rec.deferred_dmabuf_count = self.deferred_dmabuf.len();
                    rec.created_deferred_buffers.push(serial);
                }
            }
        }
    }
}

struct TestClientData {
    recorded: Arc<Mutex<RecordedServer>>,
}

impl ClientData for TestClientData {
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {
        if let Ok(mut rec) = self.recorded.lock() {
            rec.disconnected = true;
        }
    }
}

enum ServerControl {
    ReleaseDeferred,
    Stop,
}

struct TestServerHarness {
    recorded: Arc<Mutex<RecordedServer>>,
    ctl_tx: mpsc::Sender<ServerControl>,
    wake_tx: UnixStream,
    thread: Option<JoinHandle<()>>,
}

impl TestServerHarness {
    fn start(dmabuf_mode: DmaBufMode, main_device_rdev: Option<u64>) -> (Self, WaylandHost) {
        let (srv_stream, cli_stream) = UnixStream::pair().unwrap();
        let (wake_tx, wake_rx) = UnixStream::pair().unwrap();
        let _ = wake_rx.set_nonblocking(true);
        let (ctl_tx, ctl_rx) = mpsc::channel();
        let recorded = Arc::new(Mutex::new(RecordedServer::default()));
        let rec_srv = Arc::clone(&recorded);
        let thread = thread::spawn(move || {
            run_test_server(
                srv_stream,
                wake_rx,
                &ctl_rx,
                dmabuf_mode,
                main_device_rdev,
                rec_srv,
            );
        });
        let host = WaylandHost::new_on_socket(cli_stream).unwrap();
        (
            Self {
                recorded,
                ctl_tx,
                wake_tx,
                thread: Some(thread),
            },
            host,
        )
    }

    fn release_deferred(&mut self) {
        let _ = self.ctl_tx.send(ServerControl::ReleaseDeferred);
        let _ = self.wake_tx.write_all(&[1]);
    }

    fn wait_for<F>(&self, label: &str, pred: F)
    where
        F: Fn(&RecordedServer) -> bool,
    {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.recorded.lock().is_ok_and(|rec| pred(&rec)) {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for server condition: {label}");
    }
}

impl Drop for TestServerHarness {
    fn drop(&mut self) {
        let _ = self.ctl_tx.send(ServerControl::Stop);
        let _ = self.wake_tx.write_all(&[1]);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn run_test_server(
    srv_stream: UnixStream,
    mut wake_rx: UnixStream,
    ctl_rx: &mpsc::Receiver<ServerControl>,
    dmabuf_mode: DmaBufMode,
    main_device_rdev: Option<u64>,
    recorded: Arc<Mutex<RecordedServer>>,
) {
    let mut display: Display<ServerState> = Display::new().unwrap();
    let mut dh = display.handle();
    register_test_globals(&dh);
    let mut state = ServerState {
        next_buffer_serial: 0,
        dmabuf_mode,
        main_device_rdev,
        pending_attach: HashMap::new(),
        deferred_dmabuf: Vec::new(),
        dh: Some(dh.clone()),
        recorded: Arc::clone(&recorded),
    };
    let _ = dh.insert_client(srv_stream, Arc::new(TestClientData { recorded }));
    let mut running = true;
    while running {
        let mut wake_buf = [0u8; 32];
        while wake_rx.read(&mut wake_buf).is_ok_and(|n| n > 0) {}
        while let Ok(cmd) = ctl_rx.try_recv() {
            match cmd {
                ServerControl::ReleaseDeferred => state.release_deferred(),
                ServerControl::Stop => running = false,
            }
        }
        let _ = display.dispatch_clients(&mut state);
        let _ = display.flush_clients();
        if !running {
            break;
        }
        let mut fds: [PollFd; 2] = [
            poll_fd(display.backend().poll_fd().as_raw_fd()),
            poll_fd(wake_rx.as_raw_fd()),
        ];
        let _ = wait_poll(&mut fds, 25);
        if poll_has_input(fds[0].revents) {
            let _ = display.dispatch_clients(&mut state);
            let _ = display.flush_clients();
        }
    }
}

fn register_test_globals(dh: &DisplayHandle) {
    dh.create_global::<ServerState, wl_compositor::WlCompositor, _>(6, ());
    dh.create_global::<ServerState, wl_subcompositor::WlSubcompositor, _>(1, ());
    dh.create_global::<ServerState, wl_shm::WlShm, _>(1, ());
    dh.create_global::<ServerState, xdg_wm_base::XdgWmBase, _>(6, ());
    dh.create_global::<ServerState, wp_viewporter::WpViewporter, _>(1, ());
    dh.create_global::<ServerState, zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, _>(4, ());
}

macro_rules! trivial_global_bind {
    ($($iface:path),+ $(,)?) => {
        $(
            impl GlobalDispatch<$iface, ()> for ServerState {
                fn bind(
                    _: &mut Self,
                    _: &DisplayHandle,
                    _: &Client,
                    resource: New<$iface>,
                    _: &(),
                    data_init: &mut DataInit<'_, Self>,
                ) {
                    data_init.init(resource, ());
                }
            }
        )+
    };
}

macro_rules! noop_dispatch {
    ($($iface:path => $req:path),+ $(,)?) => {
        $(
            impl Dispatch<$iface, ()> for ServerState {
                fn request(
                    _: &mut Self,
                    _: &Client,
                    _: &$iface,
                    _: $req,
                    _: &(),
                    _: &DisplayHandle,
                    _: &mut DataInit<'_, Self>,
                ) {
                }
            }
        )+
    };
}

macro_rules! dispatch_unit_request {
    ($iface:path, $req:path, |$state:ident, $res:ident, $request:ident, $init:ident| $body:block) => {
        impl Dispatch<$iface, ()> for ServerState {
            fn request(
                $state: &mut Self,
                _: &Client,
                $res: &$iface,
                $request: $req,
                _: &(),
                _: &DisplayHandle,
                $init: &mut DataInit<'_, Self>,
            ) {
                $body
            }
        }
    };
}

trivial_global_bind!(
    wl_compositor::WlCompositor,
    xdg_wm_base::XdgWmBase,
    wp_viewporter::WpViewporter,
    zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
);

noop_dispatch!(
    wl_region::WlRegion => wl_region::Request,
    wl_callback::WlCallback => wl_callback::Request,
    xdg_positioner::XdgPositioner => xdg_positioner::Request,
    xdg_toplevel::XdgToplevel => xdg_toplevel::Request,
    wp_viewport::WpViewport => wp_viewport::Request,
    zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1 => zwp_linux_dmabuf_feedback_v1::Request,
);

impl Dispatch<wl_compositor::WlCompositor, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wl_compositor::WlCompositor,
        request: wl_compositor::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wl_compositor::Request::CreateSurface { id } = request {
            let surface_id = data_init.init(id, ()).id();
            if let Ok(mut rec) = state.recorded.lock() {
                rec.created_surfaces.push(surface_id);
            }
        } else if let wl_compositor::Request::CreateRegion { id } = request {
            data_init.init(id, ());
        }
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        surface: &wl_surface::WlSurface,
        request: wl_surface::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_surface::Request::Attach { buffer, .. } => {
                let attach = buffer
                    .and_then(|b| b.data::<(u64, BufferKind)>().copied())
                    .map_or(AttachState::Detached, |(serial, kind)| {
                        AttachState::Attached(serial, kind)
                    });
                state.pending_attach.insert(surface.id(), attach);
            }
            wl_surface::Request::Frame { callback } => {
                let cb = data_init.init(callback, ());
                cb.done(0);
            }
            wl_surface::Request::Commit => {
                let attached = state
                    .pending_attach
                    .remove(&surface.id())
                    .unwrap_or(AttachState::Unchanged);
                if let Ok(mut rec) = state.recorded.lock() {
                    rec.commits.push(SurfaceCommit {
                        surface: surface.id(),
                        attached,
                    });
                }
            }
            _ => {}
        }
    }
}

impl GlobalDispatch<wl_subcompositor::WlSubcompositor, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_subcompositor::WlSubcompositor>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<wl_subcompositor::WlSubcompositor, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wl_subcompositor::WlSubcompositor,
        request: wl_subcompositor::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wl_subcompositor::Request::GetSubsurface {
            id,
            surface,
            parent,
        } = request
        {
            data_init.init(id, surface.id());
            if let Ok(mut rec) = state.recorded.lock() {
                rec.created_subsurfaces.push((surface.id(), parent.id()));
            }
        }
    }
}

impl Dispatch<wl_subsurface::WlSubsurface, ObjectId> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wl_subsurface::WlSubsurface,
        request: wl_subsurface::Request,
        surface_id: &ObjectId,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let wl_subsurface::Request::SetPosition { x, y } = request
            && let Ok(mut rec) = state.recorded.lock()
        {
            rec.subsurface_positions.push((surface_id.clone(), x, y));
        }
    }
}

impl GlobalDispatch<wl_shm::WlShm, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_shm::WlShm>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let shm = data_init.init(resource, ());
        shm.format(wl_shm::Format::Argb8888);
        shm.format(wl_shm::Format::Xrgb8888);
    }
}

impl Dispatch<wl_shm::WlShm, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_shm::WlShm,
        request: wl_shm::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm::Request::CreatePool { id, .. } = request {
            data_init.init(id, ());
        }
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wl_shm_pool::WlShmPool,
        request: wl_shm_pool::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm_pool::Request::CreateBuffer { id, .. } = request {
            let serial = state.next_serial();
            data_init.init(id, (serial, BufferKind::Shm));
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, (u64, BufferKind)> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wl_buffer::WlBuffer,
        request: wl_buffer::Request,
        data: &(u64, BufferKind),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if matches!(request, wl_buffer::Request::Destroy)
            && let Ok(mut rec) = state.recorded.lock()
        {
            rec.destroyed_buffers.push(data.0);
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &xdg_wm_base::XdgWmBase,
        request: xdg_wm_base::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let xdg_wm_base::Request::CreatePositioner { id } = request {
            data_init.init(id, ());
        } else if let xdg_wm_base::Request::GetXdgSurface { id, .. } = request {
            data_init.init(id, ());
        }
    }
}

dispatch_unit_request!(
    xdg_surface::XdgSurface,
    xdg_surface::Request,
    |_state, xdg_surf, request, data_init| {
        if let xdg_surface::Request::GetToplevel { id } = request {
            send_initial_configure(xdg_surf, &data_init.init(id, ()));
        }
    }
);

fn send_initial_configure(
    xdg_surf: &xdg_surface::XdgSurface,
    toplevel: &xdg_toplevel::XdgToplevel,
) {
    let activated = (xdg_toplevel::State::Activated as u32)
        .to_ne_bytes()
        .to_vec();
    toplevel.configure(1280, 720, activated);
    xdg_surf.configure(1);
}

dispatch_unit_request!(
    wp_viewporter::WpViewporter,
    wp_viewporter::Request,
    |_state, _vp, request, data_init| {
        if let wp_viewporter::Request::GetViewport { id, .. } = request {
            data_init.init(id, ());
        }
    }
);

impl Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        request: zwp_linux_dmabuf_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let zwp_linux_dmabuf_v1::Request::CreateParams { params_id } = request {
            data_init.init(params_id, ());
        } else if let zwp_linux_dmabuf_v1::Request::GetDefaultFeedback { id }
        | zwp_linux_dmabuf_v1::Request::GetSurfaceFeedback { id, .. } = request
        {
            let feedback = data_init.init(id, ());
            if let Some(rdev) = state.main_device_rdev {
                feedback.main_device(rdev.to_ne_bytes().to_vec());
            }
            feedback.done();
        }
    }
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> for ServerState {
    fn request(
        state: &mut Self,
        client: &Client,
        params: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        request: zwp_linux_buffer_params_v1::Request,
        _: &(),
        dh: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let zwp_linux_buffer_params_v1::Request::Create { .. } = request {
            match state.dmabuf_mode {
                DmaBufMode::CreateImmediately => {
                    let serial = state.next_serial();
                    if let Ok(buf) = client
                        .create_resource::<wl_buffer::WlBuffer, (u64, BufferKind), Self>(
                            dh,
                            1,
                            (serial, BufferKind::DmaBuf),
                        )
                    {
                        params.created(&buf);
                    }
                }
                DmaBufMode::FailAlways => params.failed(),
                DmaBufMode::HoldDeferred => {
                    state.deferred_dmabuf.push((client.clone(), params.clone()));
                    if let Ok(mut rec) = state.recorded.lock() {
                        rec.deferred_dmabuf_count = state.deferred_dmabuf.len();
                    }
                }
            }
        }
    }
}

fn dummy_dmabuf_plane() -> WaylandDmaBufPlane {
    let mut file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(std::env::temp_dir().join(format!("mf-test-dmabuf-{}", std::process::id())))
        .unwrap();
    let _ = file.write_all(&[0u8; 64]);
    let fd = OwnedFd::from(file);
    WaylandDmaBufPlane {
        fd,
        offset: 0,
        stride: 16,
    }
}

fn wait_ui_event<F>(host: &WaylandHost, label: &str, pred: F)
where
    F: Fn(&WaylandUiEvent) -> bool,
{
    let ui = host.ui_handle();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        for ev in ui.drain_events() {
            if pred(&ev) {
                return;
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for UI event: {label}");
}

#[test]
fn dmabuf_feedback_main_device_sets_render_node_rdev() {
    let expected_rdev = (226_u64 << 8) | 129_u64;
    let (_srv, host) = TestServerHarness::start(DmaBufMode::CreateImmediately, Some(expected_rdev));
    let ui = host.ui_handle();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if ui.main_device_rdev() == Some(expected_rdev) {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "expected main_device_rdev={expected_rdev:?}, got {:?}",
        ui.main_device_rdev()
    );
}

#[test]
fn compositor_dmabuf_rejection_falls_back_to_shm_without_protocol_error() {
    let (srv, host) = TestServerHarness::start(DmaBufMode::FailAlways, None);
    srv.wait_for("subsurfaces registered", |r| {
        r.created_subsurfaces.len() >= 2
    });
    let ui_surface = srv.recorded.lock().unwrap().created_subsurfaces[1]
        .0
        .clone();

    let in_use = Arc::new(AtomicBool::new(false));
    let lease = WaylandSlotLease::acquire(&in_use);
    let ui = host.ui_handle();
    ui.send(WaylandUiCommand::Present {
        role: WaylandSurfaceRole::View,
        logical_width: 4,
        logical_height: 4,
        x: 0,
        y: 0,
        payload: WaylandFramePayload::DmaBuf {
            width: 4,
            height: 4,
            fourcc: 0x3432_5241,
            modifier: 0,
            planes: vec![dummy_dmabuf_plane()],
            lease,
        },
    });
    wait_ui_event(&host, "DmaBufRejected for View", |ev| {
        matches!(ev, WaylandUiEvent::DmaBufRejected)
    });

    ui.send(WaylandUiCommand::Present {
        role: WaylandSurfaceRole::View,
        logical_width: 4,
        logical_height: 4,
        x: 0,
        y: 0,
        payload: WaylandFramePayload::Shm {
            width: 4,
            height: 4,
            pixels: vec![0xff; 64],
        },
    });
    srv.wait_for("Shm View commit", |r| {
        !r.disconnected
            && r.commits.iter().any(|c| {
                c.surface == ui_surface
                    && matches!(c.attached, AttachState::Attached(_, BufferKind::Shm))
            })
    });
    assert!(!srv.recorded.lock().unwrap().disconnected);
}

#[test]
fn popup_present_sets_subsurface_position_and_commits_parent_ui_surface() {
    let (srv, host) = TestServerHarness::start(DmaBufMode::CreateImmediately, None);
    srv.wait_for("base subsurfaces registered", |r| {
        r.created_subsurfaces.len() >= 2
    });
    let ui_surface = srv.recorded.lock().unwrap().created_subsurfaces[1]
        .0
        .clone();

    host.ui_handle().send(WaylandUiCommand::Present {
        role: WaylandSurfaceRole::Popup,
        logical_width: 100,
        logical_height: 50,
        x: 120,
        y: 240,
        payload: WaylandFramePayload::Shm {
            width: 100,
            height: 50,
            pixels: vec![0xaa; 100 * 50 * 4],
        },
    });

    srv.wait_for(
        "popup subsurface positioned and committed with parent",
        |r| {
            let Some((popup_surface, parent)) = r.created_subsurfaces.get(2) else {
                return false;
            };
            if *parent != ui_surface {
                return false;
            }
            let positioned = r
                .subsurface_positions
                .iter()
                .any(|(s, x, y)| s == popup_surface && *x == 120 && *y == 240);
            let popup_idx = r.commits.iter().position(|c| {
                c.surface == *popup_surface
                    && matches!(c.attached, AttachState::Attached(_, BufferKind::Shm))
            });
            positioned
                && popup_idx
                    .is_some_and(|idx| r.commits[idx + 1..].iter().any(|c| c.surface == ui_surface))
        },
    );
}

#[test]
fn hide_popup_before_late_dmabuf_created_destroys_buffer_without_remapping() {
    let (mut srv, host) = TestServerHarness::start(DmaBufMode::HoldDeferred, None);
    srv.wait_for("base subsurfaces registered", |r| {
        r.created_subsurfaces.len() >= 2
    });
    let ui_surface = srv.recorded.lock().unwrap().created_subsurfaces[1]
        .0
        .clone();
    let ui = host.ui_handle();

    ui.send(WaylandUiCommand::Present {
        role: WaylandSurfaceRole::Popup,
        logical_width: 20,
        logical_height: 20,
        x: 10,
        y: 20,
        payload: WaylandFramePayload::Shm {
            width: 20,
            height: 20,
            pixels: vec![0x11; 20 * 20 * 4],
        },
    });
    srv.wait_for("initial Shm popup mapped", |r| {
        r.created_subsurfaces
            .get(2)
            .is_some_and(|(popup_surface, _)| {
                r.commits.iter().any(|c| {
                    c.surface == *popup_surface
                        && matches!(c.attached, AttachState::Attached(_, BufferKind::Shm))
                })
            })
    });
    let popup_surface = srv.recorded.lock().unwrap().created_subsurfaces[2]
        .0
        .clone();

    let in_use = Arc::new(AtomicBool::new(false));
    let lease = WaylandSlotLease::acquire(&in_use);
    ui.send(WaylandUiCommand::Present {
        role: WaylandSurfaceRole::Popup,
        logical_width: 4,
        logical_height: 4,
        x: 10,
        y: 20,
        payload: WaylandFramePayload::DmaBuf {
            width: 4,
            height: 4,
            fourcc: 0x3432_5241,
            modifier: 0,
            planes: vec![dummy_dmabuf_plane()],
            lease,
        },
    });
    srv.wait_for("popup dmabuf params queued before Created", |r| {
        r.deferred_dmabuf_count == 1
    });

    ui.send(WaylandUiCommand::HidePopup);
    srv.wait_for("popup unmapped on HidePopup", |r| {
        let Some(detach_idx) = r
            .commits
            .iter()
            .rposition(|c| c.surface == popup_surface && c.attached == AttachState::Detached)
        else {
            return false;
        };
        r.commits[detach_idx + 1..]
            .iter()
            .any(|c| c.surface == ui_surface)
    });

    let commits_before_created = srv.recorded.lock().unwrap().commits.len();
    srv.release_deferred();
    srv.wait_for("late dmabuf buffer created and destroyed", |r| {
        r.created_deferred_buffers
            .first()
            .is_some_and(|id| r.destroyed_buffers.contains(id))
    });
    thread::sleep(Duration::from_millis(50));

    let (remapped, disconnected) = {
        let rec = srv.recorded.lock().unwrap();
        let remapped = rec.commits[commits_before_created..].iter().any(|c| {
            c.surface == popup_surface && matches!(c.attached, AttachState::Attached(_, _))
        });
        (remapped, rec.disconnected)
    };
    assert!(
        !remapped,
        "late Created buffer must never re-map a hidden popup"
    );
    assert!(!disconnected);
}
