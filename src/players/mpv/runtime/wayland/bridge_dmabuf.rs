use std::os::fd::AsFd;

use wayland_client::Proxy as _;
use wayland_protocols::wp::linux_dmabuf::zv1::server::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_feedback_v1, zwp_linux_dmabuf_v1,
};
use wayland_server::protocol::{wl_shm, wl_shm_pool};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::bridge::{ServerDmaBufParams, ServerShmPool, VideoBufferEntry};
use super::client::{AppState, DmaBufParamsTag, FeedbackEvent, HostBufferTag};

impl GlobalDispatch<wl_shm::WlShm, ()> for AppState {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_shm::WlShm>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let shm = data_init.init(resource, ());
        for fmt in &state.host.shm_formats {
            if let Ok(f) = wl_shm::Format::try_from(*fmt) {
                shm.format(f);
            }
        }
    }
}

impl Dispatch<wl_shm::WlShm, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wl_shm::WlShm,
        request: wl_shm::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm::Request::CreatePool { id, fd, size } = request {
            let pool = data_init.init(id, ());
            if let Some(s) = state.host.surfaces.as_ref()
                && let Some(host_shm) = s.shm.as_ref()
            {
                let host_pool = host_shm.create_pool(fd.as_fd(), size, &state.host.qh, ());
                state
                    .bridge
                    .shm_pools
                    .insert(pool.id(), ServerShmPool { host_pool });
            }
        }
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &wl_shm_pool::WlShmPool,
        request: wl_shm_pool::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let Some(entry) = state.bridge.shm_pools.get(&resource.id()) else {
            return;
        };
        match request {
            wl_shm_pool::Request::CreateBuffer {
                id,
                offset,
                width,
                height,
                stride,
                format,
            } => {
                let client_buf = data_init.init(id, ());
                let fmt_u32 = u32::from(format);
                let fmt = wayland_client::protocol::wl_shm::Format::try_from(fmt_u32)
                    .unwrap_or(wayland_client::protocol::wl_shm::Format::Argb8888);
                let host_buf = entry.host_pool.create_buffer(
                    offset,
                    width,
                    height,
                    stride,
                    fmt,
                    &state.host.qh,
                    HostBufferTag::Video,
                );
                let hid = host_buf.id();
                state
                    .bridge
                    .buffer_id_by_client
                    .insert(client_buf.id(), hid.clone());
                state.bridge.video_buffers.insert(
                    hid,
                    VideoBufferEntry {
                        host_buffer: host_buf,
                        client_buffer: client_buf.downgrade(),
                        _fds: Vec::new(),
                    },
                );
            }
            wl_shm_pool::Request::Resize { size } => entry.host_pool.resize(size),
            wl_shm_pool::Request::Destroy => {
                if let Some(removed) = state.bridge.shm_pools.remove(&resource.id()) {
                    removed.host_pool.destroy();
                }
            }
            _ => {}
        }
    }
}

impl GlobalDispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for AppState {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let dmabuf = data_init.init(resource, ());
        if dmabuf.version() < 4 {
            for &(fmt, modifier) in &state.host.dmabuf_formats {
                dmabuf.format(fmt);
                if dmabuf.version() >= 3 {
                    dmabuf.modifier(fmt, (modifier >> 32) as u32, modifier as u32);
                }
            }
        }
    }
}

impl Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        request: zwp_linux_dmabuf_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwp_linux_dmabuf_v1::Request::CreateParams { params_id } => {
                let params = data_init.init(params_id, ());
                state
                    .bridge
                    .dmabuf_params
                    .insert(params.id(), ServerDmaBufParams { planes: Vec::new() });
            }
            zwp_linux_dmabuf_v1::Request::GetDefaultFeedback { id }
            | zwp_linux_dmabuf_v1::Request::GetSurfaceFeedback { id, .. } => {
                let fb = data_init.init(id, ());
                replay_dmabuf_feedback(&fb, &state.host.dmabuf_feedback);
            }
            _ => {}
        }
    }
}

fn replay_dmabuf_feedback(
    fb: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
    events: &[FeedbackEvent],
) {
    for ev in events {
        match ev {
            FeedbackEvent::MainDevice(dev) => fb.main_device(dev.clone()),
            FeedbackEvent::FormatTable { fd, size } => fb.format_table(fd.as_fd(), *size),
            FeedbackEvent::TrancheTargetDevice(dev) => fb.tranche_target_device(dev.clone()),
            FeedbackEvent::TrancheFlags(bits) => {
                let flags = zwp_linux_dmabuf_feedback_v1::TrancheFlags::from_bits_truncate(*bits);
                fb.tranche_flags(flags);
            }
            FeedbackEvent::TrancheFormats(idx) => fb.tranche_formats(idx.clone()),
            FeedbackEvent::TrancheDone => fb.tranche_done(),
            FeedbackEvent::Done => fb.done(),
        }
    }
}

impl Dispatch<zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
        _: zwp_linux_dmabuf_feedback_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        params: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        request: zwp_linux_buffer_params_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let Some(s) = state.host.surfaces.as_ref() else {
            return;
        };
        match request {
            zwp_linux_buffer_params_v1::Request::Add {
                fd,
                plane_idx,
                offset,
                stride,
                modifier_hi,
                modifier_lo,
            } => {
                if let Some(entry) = state.bridge.dmabuf_params.get_mut(&params.id()) {
                    entry
                        .planes
                        .push((fd, plane_idx, offset, stride, modifier_hi, modifier_lo));
                }
            }
            zwp_linux_buffer_params_v1::Request::Create {
                width,
                height,
                format,
                flags,
            } => {
                let Some(entry) = state.bridge.dmabuf_params.remove(&params.id()) else {
                    return;
                };
                let host_params = s
                    .dmabuf
                    .create_params(&state.host.qh, DmaBufParamsTag::Video(params.downgrade()));
                for (fd, plane_idx, offset, stride, mod_hi, mod_lo) in &entry.planes {
                    host_params.add(fd.as_fd(), *plane_idx, *offset, *stride, *mod_hi, *mod_lo);
                }
                let host_flags =
                    wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::Flags::from_bits_truncate(
                        u32::from(flags),
                    );
                host_params.create(width, height, format, host_flags);
            }
            zwp_linux_buffer_params_v1::Request::CreateImmed {
                buffer_id,
                width,
                height,
                format,
                flags,
            } => {
                let Some(entry) = state.bridge.dmabuf_params.remove(&params.id()) else {
                    return;
                };
                let client_buf = data_init.init(buffer_id, ());
                let host_params = s
                    .dmabuf
                    .create_params(&state.host.qh, DmaBufParamsTag::VideoImmed);
                let mut kept_fds = Vec::with_capacity(entry.planes.len());
                for (fd, plane_idx, offset, stride, mod_hi, mod_lo) in entry.planes {
                    host_params.add(fd.as_fd(), plane_idx, offset, stride, mod_hi, mod_lo);
                    kept_fds.push(fd);
                }
                let host_flags =
                    wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::Flags::from_bits_truncate(
                        u32::from(flags),
                    );
                let host_buf = host_params.create_immed(
                    width,
                    height,
                    format,
                    host_flags,
                    &state.host.qh,
                    HostBufferTag::Video,
                );
                host_params.destroy();
                let hid = host_buf.id();
                state
                    .bridge
                    .buffer_id_by_client
                    .insert(client_buf.id(), hid.clone());
                state.bridge.video_buffers.insert(
                    hid,
                    VideoBufferEntry {
                        host_buffer: host_buf,
                        client_buffer: client_buf.downgrade(),
                        _fds: kept_fds,
                    },
                );
            }
            zwp_linux_buffer_params_v1::Request::Destroy => {
                state.bridge.dmabuf_params.remove(&params.id());
            }
            _ => {}
        }
    }
}
