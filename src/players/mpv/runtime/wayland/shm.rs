use std::ffi::CString;
use std::io;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::Ordering;

use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool};
use wayland_client::{Proxy as _, QueueHandle};
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1;
use wayland_protocols::wp::single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1;

use super::client::{
    AppState, DmaBufParamsTag, HostBufferTag, HostState, PendingUiDmaBuf, UiBufferCleanup,
};
use super::{WaylandFramePayload, WaylandSurfaceRole, WaylandUiEvent};

const MFD_CLOEXEC: u32 = 1;
const PROT_READ_WRITE: i32 = 3;
const MAP_SHARED_FLAG: i32 = 1;

unsafe extern "C" {
    fn memfd_create(name: *const i8, flags: u32) -> i32;
    fn ftruncate(fd: i32, length: i64) -> i32;
    fn mmap(
        addr: *mut std::ffi::c_void,
        length: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut std::ffi::c_void;
    fn munmap(addr: *mut std::ffi::c_void, length: usize) -> i32;
}

pub(super) fn create_black_background_buffer(
    spb: Option<&wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1>,
    shm: Option<&wl_shm::WlShm>,
    qh: &QueueHandle<AppState>,
) -> (Option<wl_buffer::WlBuffer>, Option<wl_shm_pool::WlShmPool>) {
    if let Some(mgr) = spb {
        return (
            Some(mgr.create_u32_rgba_buffer(0, 0, 0, u32::MAX, qh, HostBufferTag::Static)),
            None,
        );
    }
    if let Some(shm) = shm
        && let Ok((fd, _)) = create_shm_memfd(&[0, 0, 0, 255])
    {
        let pool = shm.create_pool(fd.as_fd(), 4, qh, ());
        let buf = pool.create_buffer(
            0,
            1,
            1,
            4,
            wl_shm::Format::Xrgb8888,
            qh,
            HostBufferTag::Static,
        );
        return (Some(buf), Some(pool));
    }
    (None, None)
}

pub(super) fn create_shm_memfd(bytes: &[u8]) -> io::Result<(OwnedFd, usize)> {
    let size = bytes.len().max(4);
    let name =
        CString::new("mediaflick-wl-shm").map_err(|err| io::Error::other(err.to_string()))?;
    // SAFETY: `name` is a valid NUL-terminated string.
    let raw = unsafe { memfd_create(name.as_ptr(), MFD_CLOEXEC) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a newly created file descriptor owned by this scope.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    // SAFETY: `fd` is valid and opened for writing.
    if unsafe { ftruncate(fd.as_raw_fd(), i64::try_from(size).unwrap_or(i64::MAX)) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Mapping `size` bytes from the newly sized `memfd`.
    let ptr = unsafe {
        mmap(
            std::ptr::null_mut(),
            size,
            PROT_READ_WRITE,
            MAP_SHARED_FLAG,
            fd.as_raw_fd(),
            0,
        )
    };
    if ptr == std::ptr::dangling_mut::<std::ffi::c_void>()
        || ptr == usize::MAX as *mut std::ffi::c_void
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `ptr` points to a writable region of `size >= bytes.len()` bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
        munmap(ptr, size);
    }
    Ok((fd, size))
}

#[derive(Clone, Copy)]
struct UiTarget {
    role: WaylandSurfaceRole,
    logical_w: u16,
    logical_h: u16,
    x: i32,
    y: i32,
}

impl HostState {
    pub fn present_ui(
        &mut self,
        role: WaylandSurfaceRole,
        logical_w: u16,
        logical_h: u16,
        x: i32,
        y: i32,
        payload: WaylandFramePayload,
    ) {
        let buf_id = self.next_ui_buffer_id;
        self.next_ui_buffer_id = self.next_ui_buffer_id.saturating_add(1);
        let target = UiTarget {
            role,
            logical_w,
            logical_h,
            x,
            y,
        };
        match payload {
            WaylandFramePayload::DmaBuf {
                width,
                height,
                fourcc,
                modifier,
                planes,
                lease,
            } => self.present_ui_dmabuf(
                buf_id, target, width, height, fourcc, modifier, planes, lease,
            ),
            WaylandFramePayload::Shm {
                width,
                height,
                pixels,
            } => self.present_ui_shm(buf_id, target, width, height, &pixels),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "internal helper splitting async DMA-BUF UI presentation from SHM"
    )]
    fn present_ui_dmabuf(
        &mut self,
        buf_id: u64,
        target: UiTarget,
        width: u16,
        height: u16,
        fourcc: u32,
        modifier: u64,
        planes: Vec<super::WaylandDmaBufPlane>,
        lease: super::WaylandSlotLease,
    ) {
        if self.dmabuf_rejected {
            self.ui.push_event(WaylandUiEvent::DmaBufRejected);
            return;
        }
        let Some(s) = self.surfaces.as_ref() else {
            return;
        };
        if planes.is_empty() {
            return;
        }
        let params = s
            .dmabuf
            .create_params(&self.qh, DmaBufParamsTag::Ui(buf_id));
        let mod_hi = (modifier >> 32) as u32;
        let mod_lo = modifier as u32;
        let mut kept_fds = Vec::with_capacity(planes.len());
        for (idx, plane) in planes.into_iter().enumerate() {
            params.add(
                plane.fd.as_fd(),
                u32::try_from(idx).unwrap_or(0),
                plane.offset,
                plane.stride,
                mod_hi,
                mod_lo,
            );
            kept_fds.push(plane.fd);
        }
        self.pending_ui_dmabufs.insert(
            buf_id,
            PendingUiDmaBuf {
                role: target.role,
                logical_w: target.logical_w,
                logical_h: target.logical_h,
                x: target.x,
                y: target.y,
                _fds: kept_fds,
                _lease: lease,
            },
        );
        params.create(
            i32::from(width),
            i32::from(height),
            fourcc,
            zwp_linux_buffer_params_v1::Flags::empty(),
        );
    }

    fn present_ui_shm(
        &mut self,
        buf_id: u64,
        target: UiTarget,
        width: u16,
        height: u16,
        pixels: &[u8],
    ) {
        let Some(s) = self.surfaces.as_ref() else {
            return;
        };
        let Some(shm) = s.shm.as_ref() else {
            return;
        };
        let Ok((fd, size)) = create_shm_memfd(pixels) else {
            return;
        };
        let Some(size_i32) = i32::try_from(size).ok() else {
            return;
        };
        let pool = shm.create_pool(fd.as_fd(), size_i32, &self.qh, ());
        let stride = i32::from(width).saturating_mul(4);
        let buffer = pool.create_buffer(
            0,
            i32::from(width),
            i32::from(height),
            stride,
            wl_shm::Format::Argb8888,
            &self.qh,
            HostBufferTag::Ui(buf_id),
        );
        if self.attach_ui_buffer(
            target.role,
            target.logical_w,
            target.logical_h,
            target.x,
            target.y,
            &buffer,
        ) {
            self.last_ui_commit_seq = buf_id;
            self.ui_buffers.insert(
                buf_id,
                UiBufferCleanup {
                    pool: Some(pool),
                    _fds: vec![fd],
                    _lease: None,
                },
            );
        } else {
            buffer.destroy();
            pool.destroy();
        }
    }

    pub(super) fn handle_ui_dmabuf_params_event(
        &mut self,
        req_id: u64,
        params: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        event: zwp_linux_buffer_params_v1::Event,
    ) {
        let pending = self.pending_ui_dmabufs.remove(&req_id);
        match event {
            zwp_linux_buffer_params_v1::Event::Created { buffer } => {
                if let Some(pending) = pending {
                    let stale = req_id < self.last_ui_commit_seq || self.dmabuf_rejected;
                    if !stale
                        && self.attach_ui_buffer(
                            pending.role,
                            pending.logical_w,
                            pending.logical_h,
                            pending.x,
                            pending.y,
                            &buffer,
                        )
                    {
                        self.last_ui_commit_seq = req_id;
                        self.ui_dmabuf_buffers.insert(
                            buffer.id(),
                            UiBufferCleanup {
                                pool: None,
                                _fds: pending._fds,
                                _lease: Some(pending._lease),
                            },
                        );
                    } else {
                        buffer.destroy();
                    }
                } else {
                    buffer.destroy();
                }
                params.destroy();
            }
            zwp_linux_buffer_params_v1::Event::Failed => {
                drop(pending);
                if !self.dmabuf_rejected {
                    self.dmabuf_rejected = true;
                    tracing::warn!(
                        target: "wayland.host",
                        req_id,
                        "zwp_linux_buffer_params_v1::create failed for UI DMA-BUF; falling back to wl_shm"
                    );
                }
                self.ui.push_event(WaylandUiEvent::DmaBufRejected);
                params.destroy();
            }
            _ => {}
        }
    }

    fn attach_ui_buffer(
        &mut self,
        role: WaylandSurfaceRole,
        logical_w: u16,
        logical_h: u16,
        x: i32,
        y: i32,
        buffer: &wl_buffer::WlBuffer,
    ) -> bool {
        let (target_w, target_h) = (i32::from(logical_w.max(1)), i32::from(logical_h.max(1)));
        match role {
            WaylandSurfaceRole::View => {
                let Some(s) = self.surfaces.as_ref() else {
                    return false;
                };
                s.ui_viewport.set_destination(target_w, target_h);
                s.ui_surface.attach(Some(buffer), 0, 0);
                s.ui_surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
                s.ui_surface.commit();
                self.ui_frame_committed.store(true, Ordering::Release);
                true
            }
            WaylandSurfaceRole::Popup => {
                let Some(popup) = self.ensure_popup_surface() else {
                    return false;
                };
                popup.x = x;
                popup.y = y;
                popup.subsurface.set_position(x, y);
                if let Some(vp) = popup.viewport.as_ref() {
                    vp.set_destination(target_w, target_h);
                }
                popup.surface.attach(Some(buffer), 0, 0);
                popup.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
                popup.surface.commit();
                true
            }
        }
    }
}
