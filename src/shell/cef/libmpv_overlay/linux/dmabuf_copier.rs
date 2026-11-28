use std::ffi::{c_uint, c_void};
use std::fs::File;
use std::os::fd::AsRawFd;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::players::mpv::{WaylandDmaBufPlane, WaylandSlotLease, WaylandSurfaceRole};

pub(super) use super::dmabuf_gl::SourcePlane;
use super::dmabuf_gl::{
    CurrentContextGuard, EglApi, EglImageGuard, GBM_BO_USE_LINEAR, GBM_BO_USE_RENDERING,
    GBM_BO_USE_SCANOUT, GBM_FORMAT_ARGB8888, GL_BLEND, GL_COLOR_ATTACHMENT0, GL_COLOR_BUFFER_BIT,
    GL_FLOAT, GL_FRAMEBUFFER, GL_FRAMEBUFFER_COMPLETE, GL_RGBA, GL_TEXTURE_2D,
    GL_TEXTURE_EXTERNAL_OES, GL_TRIANGLE_STRIP, GL_UNSIGNED_BYTE, GbmApi, GlesApi, QUAD_VERTICES,
    build_program, configure_texture_params, create_dmabuf_egl_image, create_gles_context,
    export_gbm_planes, open_gbm_display,
};

const MAX_GBM_SLOTS: usize = 4;

pub(super) struct CopiedDmaBuf {
    pub width: u16,
    pub height: u16,
    pub fourcc: u32,
    pub modifier: u64,
    pub planes: Vec<WaylandDmaBufPlane>,
    pub lease: WaylandSlotLease,
}

struct GbmSlot {
    bo: *mut c_void,
    fourcc: u32,
    modifier: u64,
    planes: Vec<WaylandDmaBufPlane>,
    egl_image: *mut c_void,
    tex: c_uint,
    fbo: c_uint,
    in_use: Arc<AtomicBool>,
}

#[derive(Default)]
struct SlotPool {
    width: u16,
    height: u16,
    slots: Vec<GbmSlot>,
}

#[derive(Clone, Copy)]
struct ShmScratch {
    width: u16,
    height: u16,
    tex: c_uint,
    fbo: c_uint,
}

pub(super) struct DmaBufCopier {
    gbm: GbmApi,
    egl: EglApi,
    gles: GlesApi,
    _drm_file: File,
    gbm_device: *mut c_void,
    egl_display: *mut c_void,
    egl_context: *mut c_void,
    program: c_uint,
    a_pos: c_uint,
    a_uv: c_uint,
    u_tex: i32,
    src_tex: c_uint,
    view_pool: SlotPool,
    popup_pool: SlotPool,
    retired: Vec<GbmSlot>,
    shm_scratch: Option<ShmScratch>,
}

fn init_gl_pipeline(
    egl: &EglApi,
    gles: &GlesApi,
    egl_display: *mut c_void,
    egl_context: *mut c_void,
) -> Result<(c_uint, c_uint, c_uint, i32, c_uint), String> {
    let _ctx = CurrentContextGuard::acquire(egl, egl_display, egl_context)?;
    let (program, a_pos, a_uv, u_tex) = build_program(gles)?;
    let mut src_tex = 0;
    // SAFETY: Current GLES context generates a texture for external OES sampling.
    unsafe {
        (gles.gen_textures)(1, &mut src_tex);
        (gles.bind_texture)(GL_TEXTURE_EXTERNAL_OES, src_tex);
        configure_texture_params(gles, GL_TEXTURE_EXTERNAL_OES);
        (gles.bind_texture)(GL_TEXTURE_EXTERNAL_OES, 0);
    }
    Ok((program, a_pos, a_uv, u_tex, src_tex))
}

fn destroy_slot_raw(
    gles: &GlesApi,
    egl: &EglApi,
    gbm: &GbmApi,
    egl_display: *mut c_void,
    slot: &GbmSlot,
) {
    // SAFETY: Current GLES context owns FBO/tex, EGLDisplay owns `egl_image`, GBM device owns `bo`; destroyed in reverse order.
    unsafe {
        (gles.delete_framebuffers)(1, &slot.fbo);
        (gles.delete_textures)(1, &slot.tex);
        (egl.destroy_image_khr)(egl_display, slot.egl_image);
        (gbm.bo_destroy)(slot.bo);
    }
}

fn destroy_shm_scratch_raw(gles: &GlesApi, scratch: &ShmScratch) {
    // SAFETY: Current GLES context owns scratch FBO and texture.
    unsafe {
        (gles.delete_framebuffers)(1, &scratch.fbo);
        (gles.delete_textures)(1, &scratch.tex);
    }
}

impl DmaBufCopier {
    pub(super) fn new(preferred_rdev: Option<u64>) -> Result<Self, String> {
        let gbm = GbmApi::load()?;
        let (egl, get_proc) = EglApi::load()?;
        let gles = GlesApi::load(get_proc)?;
        let (drm_file, gbm_device, egl_display) = open_gbm_display(&gbm, &egl, preferred_rdev)?;
        let egl_context = match create_gles_context(&egl, egl_display) {
            Ok(ctx) => ctx,
            Err(err) => {
                // SAFETY: Context creation failed; clean up EGLDisplay and GBM device.
                unsafe {
                    (egl.terminate)(egl_display);
                    (gbm.device_destroy)(gbm_device);
                }
                return Err(err);
            }
        };
        let (program, a_pos, a_uv, u_tex, src_tex) =
            match init_gl_pipeline(&egl, &gles, egl_display, egl_context) {
                Ok(values) => values,
                Err(err) => {
                    // SAFETY: Shader setup failed; destroy context, display, and GBM device.
                    unsafe {
                        (egl.destroy_context)(egl_display, egl_context);
                        (egl.terminate)(egl_display);
                        (gbm.device_destroy)(gbm_device);
                    }
                    return Err(err);
                }
            };
        Ok(Self {
            gbm,
            egl,
            gles,
            _drm_file: drm_file,
            gbm_device,
            egl_display,
            egl_context,
            program,
            a_pos,
            a_uv,
            u_tex,
            src_tex,
            view_pool: SlotPool::default(),
            popup_pool: SlotPool::default(),
            retired: Vec::new(),
            shm_scratch: None,
        })
    }

    fn pool_mut(&mut self, role: WaylandSurfaceRole) -> &mut SlotPool {
        match role {
            WaylandSurfaceRole::View => &mut self.view_pool,
            WaylandSurfaceRole::Popup => &mut self.popup_pool,
        }
    }

    pub(super) fn copy_dmabuf(
        &mut self,
        role: WaylandSurfaceRole,
        width: u16,
        height: u16,
        fourcc: u32,
        modifier: u64,
        planes: &[SourcePlane],
    ) -> Result<CopiedDmaBuf, String> {
        let _ctx = CurrentContextGuard::acquire(&self.egl, self.egl_display, self.egl_context)?;
        let mut stale = {
            let pool = self.pool_mut(role);
            if pool.width == width && pool.height == height {
                Vec::new()
            } else {
                pool.width = width;
                pool.height = height;
                std::mem::take(&mut pool.slots)
            }
        };
        self.retired.append(&mut stale);
        self.sweep_retired();
        let slot_idx = self.acquire_slot_index(role, width, height)?;
        let fbo = self.pool_mut(role).slots[slot_idx].fbo;
        self.render_source_into_fbo(fbo, width, height, fourcc, modifier, planes)?;
        let slot = &self.pool_mut(role).slots[slot_idx];
        let mut cloned_planes = Vec::with_capacity(slot.planes.len());
        for plane in &slot.planes {
            let fd = plane
                .fd
                .try_clone()
                .map_err(|err| format!("failed to duplicate GBM plane fd: {err}"))?;
            cloned_planes.push(WaylandDmaBufPlane {
                fd,
                offset: plane.offset,
                stride: plane.stride,
            });
        }
        let lease = WaylandSlotLease::acquire(&slot.in_use);
        Ok(CopiedDmaBuf {
            width,
            height,
            fourcc: slot.fourcc,
            modifier: slot.modifier,
            planes: cloned_planes,
            lease,
        })
    }

    pub(super) fn copy_to_shm(
        &mut self,
        width: u16,
        height: u16,
        fourcc: u32,
        modifier: u64,
        planes: &[SourcePlane],
    ) -> Result<Vec<u8>, String> {
        let _ctx = CurrentContextGuard::acquire(&self.egl, self.egl_display, self.egl_context)?;
        let fbo = self.ensure_shm_fbo(width, height)?;
        self.render_source_into_fbo(fbo, width, height, fourcc, modifier, planes)?;
        let len = usize::from(width)
            .checked_mul(usize::from(height))
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| "SHM copy buffer size overflow".to_string())?;
        let mut pixels = vec![0u8; len];
        // SAFETY: `fbo` is complete for `(width, height)` and `pixels` has `4 * width * height` bytes.
        unsafe {
            (self.gles.bind_framebuffer)(GL_FRAMEBUFFER, fbo);
            (self.gles.read_pixels)(
                0,
                0,
                i32::from(width),
                i32::from(height),
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                pixels.as_mut_ptr().cast(),
            );
            (self.gles.bind_framebuffer)(GL_FRAMEBUFFER, 0);
        }
        for px in pixels.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        Ok(pixels)
    }

    fn acquire_slot_index(
        &mut self,
        role: WaylandSurfaceRole,
        width: u16,
        height: u16,
    ) -> Result<usize, String> {
        {
            let pool = self.pool_mut(role);
            if let Some(idx) = pool
                .slots
                .iter()
                .position(|slot| !slot.in_use.load(Ordering::Acquire))
            {
                return Ok(idx);
            }
            if pool.slots.len() >= MAX_GBM_SLOTS {
                return Err("all GBM UI buffer slots are in use".to_string());
            }
        }
        let slot = self.create_gbm_slot(width, height)?;
        let pool = self.pool_mut(role);
        pool.slots.push(slot);
        Ok(pool.slots.len() - 1)
    }

    fn create_gbm_slot(&self, width: u16, height: u16) -> Result<GbmSlot, String> {
        let flag_candidates = [
            GBM_BO_USE_SCANOUT | GBM_BO_USE_RENDERING,
            GBM_BO_USE_RENDERING,
            GBM_BO_USE_RENDERING | GBM_BO_USE_LINEAR,
        ];
        let mut last_err = String::from("gbm_bo_create failed");
        for flags in flag_candidates {
            match self.try_create_gbm_slot(width, height, flags) {
                Ok(slot) => return Ok(slot),
                Err(err) => last_err = err,
            }
        }
        Err(last_err)
    }

    fn try_create_gbm_slot(&self, width: u16, height: u16, flags: u32) -> Result<GbmSlot, String> {
        // SAFETY: `self.gbm_device` is valid and non-null.
        let bo = unsafe {
            (self.gbm.bo_create)(
                self.gbm_device,
                u32::from(width),
                u32::from(height),
                GBM_FORMAT_ARGB8888,
                flags,
            )
        };
        if bo.is_null() {
            return Err(format!(
                "gbm_bo_create({width}x{height}, 0x{flags:x}) failed"
            ));
        }
        let planes = match export_gbm_planes(&self.gbm, bo) {
            Ok(planes) => planes,
            Err(err) => {
                // SAFETY: `bo` was just allocated cleanly and failed plane export.
                unsafe { (self.gbm.bo_destroy)(bo) };
                return Err(err);
            }
        };
        // SAFETY: `bo` is valid.
        let modifier = unsafe { (self.gbm.bo_get_modifier)(bo) };
        let src_planes: Vec<SourcePlane> = planes
            .iter()
            .map(|p| SourcePlane {
                fd: p.fd.as_raw_fd(),
                offset: p.offset,
                stride: p.stride,
            })
            .collect();
        let egl_image = match create_dmabuf_egl_image(
            &self.egl,
            self.egl_display,
            width,
            height,
            GBM_FORMAT_ARGB8888,
            modifier,
            &src_planes,
        ) {
            Ok(img) => img,
            Err(err) => {
                // SAFETY: `bo` has no associated EGLImage yet.
                unsafe { (self.gbm.bo_destroy)(bo) };
                return Err(err);
            }
        };
        let (tex, fbo) = match self.create_fbo_for_egl_image(egl_image) {
            Ok(pair) => pair,
            Err(err) => {
                // SAFETY: Destroy `egl_image` before `bo` in reverse creation order.
                unsafe {
                    (self.egl.destroy_image_khr)(self.egl_display, egl_image);
                    (self.gbm.bo_destroy)(bo);
                }
                return Err(err);
            }
        };
        Ok(GbmSlot {
            bo,
            fourcc: GBM_FORMAT_ARGB8888,
            modifier,
            planes,
            egl_image,
            tex,
            fbo,
            in_use: Arc::new(AtomicBool::new(false)),
        })
    }

    fn create_fbo_for_egl_image(&self, egl_image: *mut c_void) -> Result<(c_uint, c_uint), String> {
        let mut tex = 0;
        let mut fbo = 0;
        // SAFETY: Current GLES context allocates texture + FBO bound to `egl_image`.
        let status = unsafe {
            (self.gles.gen_textures)(1, &mut tex);
            (self.gles.bind_texture)(GL_TEXTURE_2D, tex);
            configure_texture_params(&self.gles, GL_TEXTURE_2D);
            (self.gles.image_target_texture_2d)(GL_TEXTURE_2D, egl_image);
            (self.gles.bind_texture)(GL_TEXTURE_2D, 0);
            (self.gles.gen_framebuffers)(1, &mut fbo);
            (self.gles.bind_framebuffer)(GL_FRAMEBUFFER, fbo);
            (self.gles.framebuffer_texture_2d)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                tex,
                0,
            );
            let status = (self.gles.check_framebuffer_status)(GL_FRAMEBUFFER);
            (self.gles.bind_framebuffer)(GL_FRAMEBUFFER, 0);
            status
        };
        if status != GL_FRAMEBUFFER_COMPLETE {
            // SAFETY: Cleaning up incomplete FBO and texture in current GLES context.
            unsafe {
                (self.gles.delete_framebuffers)(1, &fbo);
                (self.gles.delete_textures)(1, &tex);
            }
            return Err(format!("GBM destination FBO incomplete (0x{status:x})"));
        }
        Ok((tex, fbo))
    }

    fn ensure_shm_fbo(&mut self, width: u16, height: u16) -> Result<c_uint, String> {
        if let Some(scratch) = &self.shm_scratch
            && scratch.width == width
            && scratch.height == height
        {
            return Ok(scratch.fbo);
        }
        if let Some(old) = self.shm_scratch.take() {
            destroy_shm_scratch_raw(&self.gles, &old);
        }
        let mut tex = 0;
        let mut fbo = 0;
        // SAFETY: Current GLES context allocates an RGBA texture and FBO for SHM readback.
        let status = unsafe {
            (self.gles.gen_textures)(1, &mut tex);
            (self.gles.bind_texture)(GL_TEXTURE_2D, tex);
            configure_texture_params(&self.gles, GL_TEXTURE_2D);
            (self.gles.tex_image_2d)(
                GL_TEXTURE_2D,
                0,
                i32::from_ne_bytes(GL_RGBA.to_ne_bytes()),
                i32::from(width),
                i32::from(height),
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                ptr::null(),
            );
            (self.gles.bind_texture)(GL_TEXTURE_2D, 0);
            (self.gles.gen_framebuffers)(1, &mut fbo);
            (self.gles.bind_framebuffer)(GL_FRAMEBUFFER, fbo);
            (self.gles.framebuffer_texture_2d)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                tex,
                0,
            );
            let status = (self.gles.check_framebuffer_status)(GL_FRAMEBUFFER);
            (self.gles.bind_framebuffer)(GL_FRAMEBUFFER, 0);
            status
        };
        if status != GL_FRAMEBUFFER_COMPLETE {
            // SAFETY: Cleaning up incomplete SHM scratch FBO and texture.
            unsafe {
                (self.gles.delete_framebuffers)(1, &fbo);
                (self.gles.delete_textures)(1, &tex);
            }
            return Err(format!("SHM scratch FBO incomplete (0x{status:x})"));
        }
        self.shm_scratch = Some(ShmScratch {
            width,
            height,
            tex,
            fbo,
        });
        Ok(fbo)
    }

    fn render_source_into_fbo(
        &self,
        fbo: c_uint,
        width: u16,
        height: u16,
        fourcc: u32,
        modifier: u64,
        planes: &[SourcePlane],
    ) -> Result<(), String> {
        let src_img = create_dmabuf_egl_image(
            &self.egl,
            self.egl_display,
            width,
            height,
            fourcc,
            modifier,
            planes,
        )?;
        let _img_guard = EglImageGuard {
            destroy_image_khr: self.egl.destroy_image_khr,
            dpy: self.egl_display,
            img: src_img,
        };
        let stride = 4 * i32::try_from(std::mem::size_of::<f32>()).unwrap_or(16);
        let uv_offset = 2 * std::mem::size_of::<f32>();
        // SAFETY: Current GLES context binds destination `fbo`, samples `src_img`, draws `QUAD_VERTICES`, and calls `glFinish` so GPU copy finishes before CEF reuses source buffer.
        unsafe {
            (self.gles.bind_framebuffer)(GL_FRAMEBUFFER, fbo);
            (self.gles.viewport)(0, 0, i32::from(width), i32::from(height));
            (self.gles.disable)(GL_BLEND);
            (self.gles.clear_color)(0.0, 0.0, 0.0, 0.0);
            (self.gles.clear)(GL_COLOR_BUFFER_BIT);
            (self.gles.use_program)(self.program);
            (self.gles.bind_texture)(GL_TEXTURE_EXTERNAL_OES, self.src_tex);
            (self.gles.image_target_texture_2d)(GL_TEXTURE_EXTERNAL_OES, src_img);
            (self.gles.uniform_1i)(self.u_tex, 0);
            (self.gles.enable_vertex_attrib_array)(self.a_pos);
            (self.gles.enable_vertex_attrib_array)(self.a_uv);
            (self.gles.vertex_attrib_pointer)(
                self.a_pos,
                2,
                GL_FLOAT,
                0,
                stride,
                QUAD_VERTICES.as_ptr().cast(),
            );
            (self.gles.vertex_attrib_pointer)(
                self.a_uv,
                2,
                GL_FLOAT,
                0,
                stride,
                QUAD_VERTICES.as_ptr().cast::<u8>().add(uv_offset).cast(),
            );
            (self.gles.draw_arrays)(GL_TRIANGLE_STRIP, 0, 4);
            (self.gles.bind_texture)(GL_TEXTURE_EXTERNAL_OES, 0);
            (self.gles.bind_framebuffer)(GL_FRAMEBUFFER, 0);
            (self.gles.finish)();
        }
        Ok(())
    }

    fn sweep_retired(&mut self) {
        let mut keep = Vec::new();
        for slot in self.retired.drain(..) {
            if slot.in_use.load(Ordering::Acquire) {
                keep.push(slot);
            } else {
                destroy_slot_raw(&self.gles, &self.egl, &self.gbm, self.egl_display, &slot);
            }
        }
        self.retired = keep;
    }
}

impl Drop for DmaBufCopier {
    fn drop(&mut self) {
        let current =
            CurrentContextGuard::acquire(&self.egl, self.egl_display, self.egl_context).ok();
        if current.is_some() {
            for slot in self
                .view_pool
                .slots
                .drain(..)
                .chain(self.popup_pool.slots.drain(..))
                .chain(self.retired.drain(..))
            {
                destroy_slot_raw(&self.gles, &self.egl, &self.gbm, self.egl_display, &slot);
            }
            if let Some(scratch) = self.shm_scratch.take() {
                destroy_shm_scratch_raw(&self.gles, &scratch);
            }
            // SAFETY: GLES resources destroyed with context current before terminating EGLDisplay and GBM device.
            unsafe {
                (self.gles.delete_textures)(1, &self.src_tex);
                (self.gles.delete_program)(self.program);
            }
        } else {
            for slot in self
                .view_pool
                .slots
                .drain(..)
                .chain(self.popup_pool.slots.drain(..))
                .chain(self.retired.drain(..))
            {
                // SAFETY: Without a current GLES context, free EGLImage and GBM BO cleanly.
                unsafe {
                    (self.egl.destroy_image_khr)(self.egl_display, slot.egl_image);
                    (self.gbm.bo_destroy)(slot.bo);
                }
            }
        }
        drop(current);
        // SAFETY: Destroying EGLContext, EGLDisplay, and GBM device after all child objects are freed.
        unsafe {
            (self.egl.destroy_context)(self.egl_display, self.egl_context);
            (self.egl.terminate)(self.egl_display);
            (self.gbm.device_destroy)(self.gbm_device);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GBM_BO_TRANSFER_WRITE: u32 = 1 << 1;

    fn create_test_top_red_bottom_blue_bo(
        gbm: &GbmApi,
        gbm_device: *mut c_void,
        width: u16,
        height: u16,
    ) -> (*mut c_void, u64, Vec<WaylandDmaBufPlane>) {
        // SAFETY: Test loads `libgbm.so.1` solely to resolve `gbm_bo_map`/`gbm_bo_unmap` for synthetic DMA-BUF pixel fill.
        let lib = unsafe { libloading::Library::new("libgbm.so.1") }.expect("load libgbm");
        type BoMapFn = unsafe extern "C" fn(
            *mut c_void,
            u32,
            u32,
            u32,
            u32,
            u32,
            *mut u32,
            *mut *mut c_void,
        ) -> *mut c_void;
        type BoUnmapFn = unsafe extern "C" fn(*mut c_void, *mut c_void);
        // SAFETY: `gbm_bo_map` and `gbm_bo_unmap` match documented <gbm.h> prototypes.
        let bo_map: BoMapFn = unsafe { *lib.get(b"gbm_bo_map\0").expect("gbm_bo_map") };
        // SAFETY: `gbm_bo_unmap` matches documented <gbm.h> prototype.
        let bo_unmap: BoUnmapFn = unsafe { *lib.get(b"gbm_bo_unmap\0").expect("gbm_bo_unmap") };
        // SAFETY: `gbm_device` is open and initialized.
        let bo = unsafe {
            (gbm.bo_create)(
                gbm_device,
                u32::from(width),
                u32::from(height),
                GBM_FORMAT_ARGB8888,
                GBM_BO_USE_LINEAR | GBM_BO_USE_RENDERING,
            )
        };
        assert!(!bo.is_null(), "gbm_bo_create for test fixture failed");
        let mut map_stride = 0u32;
        let mut map_data = ptr::null_mut();
        // SAFETY: `bo` is valid linear GBM buffer; mapping full `(width, height)` for CPU write.
        let mapped = unsafe {
            bo_map(
                bo,
                0,
                0,
                u32::from(width),
                u32::from(height),
                GBM_BO_TRANSFER_WRITE,
                &mut map_stride,
                &mut map_data,
            )
        };
        assert!(!mapped.is_null(), "gbm_bo_map failed");
        let stride = usize::try_from(map_stride).expect("stride");
        let total = stride * usize::from(height);
        // SAFETY: `mapped` covers `stride * height` bytes for `height` linear rows until `gbm_bo_unmap`.
        let buf = unsafe { std::slice::from_raw_parts_mut(mapped.cast::<u8>(), total) };
        for y in 0..usize::from(height) {
            let color: [u8; 4] = if y < usize::from(height) / 2 {
                [0, 0, 255, 255]
            } else {
                [255, 0, 0, 255]
            };
            let row = &mut buf[y * stride..y * stride + usize::from(width) * 4];
            for px in row.as_chunks_mut::<4>().0 {
                *px = color;
            }
        }
        // SAFETY: Matching unmap call for `bo` and `map_data`.
        unsafe { bo_unmap(bo, map_data) };
        let planes = export_gbm_planes(gbm, bo).expect("export test planes");
        // SAFETY: `bo` is valid.
        let modifier = unsafe { (gbm.bo_get_modifier)(bo) };
        (bo, modifier, planes)
    }

    fn assert_popup_pool_isolation(
        copier: &mut DmaBufCopier,
        modifier: u64,
        src_refs: &[SourcePlane],
    ) {
        let popup_copy = copier
            .copy_dmabuf(
                WaylandSurfaceRole::Popup,
                2,
                2,
                GBM_FORMAT_ARGB8888,
                modifier,
                src_refs,
            )
            .expect("popup copy_dmabuf");
        assert_eq!(copier.popup_pool.slots.len(), 1);
        assert_eq!(
            copier.view_pool.slots.len(),
            2,
            "popup frames must not retire view slots"
        );
        assert!(copier.retired.is_empty());
        drop(popup_copy);
    }

    #[test]
    fn dmabuf_copier_preserves_orientation_channels_and_slot_leases_on_gpu() {
        let Ok(mut copier) = DmaBufCopier::new(None) else {
            return;
        };
        let (w, h) = (4u16, 4u16);
        let (src_bo, modifier, planes) =
            create_test_top_red_bottom_blue_bo(&copier.gbm, copier.gbm_device, w, h);
        let src_refs: Vec<SourcePlane> = planes
            .iter()
            .map(|p| SourcePlane {
                fd: p.fd.as_raw_fd(),
                offset: p.offset,
                stride: p.stride,
            })
            .collect();

        let shm = copier
            .copy_to_shm(w, h, GBM_FORMAT_ARGB8888, modifier, &src_refs)
            .expect("copy_to_shm");
        assert_eq!(&shm[0..4], &[0, 0, 255, 255], "top row must remain red");
        let bottom_start = (usize::from(h) - 1) * usize::from(w) * 4;
        assert_eq!(
            &shm[bottom_start..bottom_start + 4],
            &[255, 0, 0, 255],
            "bottom row must remain blue"
        );

        let copied1 = copier
            .copy_dmabuf(
                WaylandSurfaceRole::View,
                w,
                h,
                GBM_FORMAT_ARGB8888,
                modifier,
                &src_refs,
            )
            .expect("copy_dmabuf 1");
        assert_eq!(copier.view_pool.slots.len(), 1);
        assert!(copier.view_pool.slots[0].in_use.load(Ordering::Acquire));

        let copied2 = copier
            .copy_dmabuf(
                WaylandSurfaceRole::View,
                w,
                h,
                GBM_FORMAT_ARGB8888,
                modifier,
                &src_refs,
            )
            .expect("copy_dmabuf 2");
        assert_eq!(copier.view_pool.slots.len(), 2);
        assert!(copier.view_pool.slots[1].in_use.load(Ordering::Acquire));

        drop(copied1);
        assert!(!copier.view_pool.slots[0].in_use.load(Ordering::Acquire));

        let copied3 = copier
            .copy_dmabuf(
                WaylandSurfaceRole::View,
                w,
                h,
                GBM_FORMAT_ARGB8888,
                modifier,
                &src_refs,
            )
            .expect("copy_dmabuf 3");
        assert_eq!(
            copier.view_pool.slots.len(),
            2,
            "released slot 0 should be reused"
        );

        assert_popup_pool_isolation(&mut copier, modifier, &src_refs);

        let roundtrip_refs: Vec<SourcePlane> = copied3
            .planes
            .iter()
            .map(|p| SourcePlane {
                fd: p.fd.as_raw_fd(),
                offset: p.offset,
                stride: p.stride,
            })
            .collect();
        let roundtrip = copier
            .copy_to_shm(
                copied3.width,
                copied3.height,
                copied3.fourcc,
                copied3.modifier,
                &roundtrip_refs,
            )
            .expect("roundtrip dmabuf to shm");
        assert_eq!(
            &roundtrip[0..4],
            &[0, 0, 255, 255],
            "copied DMA-BUF top row must remain red"
        );
        assert_eq!(
            &roundtrip[bottom_start..bottom_start + 4],
            &[255, 0, 0, 255],
            "copied DMA-BUF bottom row must remain blue"
        );

        drop(copied2);
        drop(copied3);
        drop(planes);
        // SAFETY: `src_bo` was allocated cleanly in this test and planes/images referencing it are gone.
        unsafe { (copier.gbm.bo_destroy)(src_bo) };
    }
}
