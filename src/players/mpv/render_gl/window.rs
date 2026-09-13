//! EGL/X11 resources owned and used exclusively by the render thread.
use glow::HasContext;
use khronos_egl as egl;
use libloading::Library;
#[path = "ui.rs"]
mod ui;
use std::collections::VecDeque;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::io;

type Egl = egl::DynamicInstance<egl::EGL1_4>;
struct XDisplay {
    raw: *mut c_void,
    close: unsafe extern "C" fn(*mut c_void) -> c_int,
    _library: Library,
}
impl XDisplay {
    fn new() -> io::Result<Self> {
        let library = unsafe { Library::new("libX11.so.6") }.map_err(io::Error::other)?;
        let open: unsafe extern "C" fn(*const c_char) -> *mut c_void =
            super::super::load_symbol(&library, b"XOpenDisplay\0")?;
        let close = super::super::load_symbol(&library, b"XCloseDisplay\0")?;
        let raw = unsafe { open(std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::other("cannot open the X11 rendering display"));
        }
        Ok(Self {
            raw,
            close,
            _library: library,
        })
    }
}
impl Drop for XDisplay {
    fn drop(&mut self) {
        unsafe {
            (self.close)(self.raw);
        }
    }
}
struct EglContext {
    egl: Egl,
    display: egl::Display,
    context: Option<egl::Context>,
    surface: Option<egl::Surface>,
    x11: XDisplay,
}
impl EglContext {
    fn new(window: u32) -> io::Result<Self> {
        let x11 = XDisplay::new()?;
        let egl = unsafe { Egl::load_required() }.map_err(io::Error::other)?;
        let display = unsafe { egl.get_display(x11.raw) }
            .ok_or_else(|| io::Error::other("EGL cannot use the X11 display"))?;
        egl.initialize(display).map_err(io::Error::other)?;
        let mut result = Self {
            egl,
            display,
            context: None,
            surface: None,
            x11,
        };
        result.initialize(window)?;
        Ok(result)
    }
    fn initialize(&mut self, window: u32) -> io::Result<()> {
        self.egl
            .bind_api(egl::OPENGL_API)
            .map_err(io::Error::other)?;
        let attrs = [
            egl::SURFACE_TYPE,
            egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            0,
            egl::DEPTH_SIZE,
            0,
            egl::NONE,
        ];
        let config = self
            .egl
            .choose_first_config(self.display, &attrs)
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("no OpenGL window configuration"))?;
        self.context = Some(
            self.egl
                .create_context(self.display, config, None, &[egl::NONE])
                .map_err(io::Error::other)?,
        );
        self.surface = Some(
            unsafe {
                self.egl.create_window_surface(
                    self.display,
                    config,
                    window as egl::NativeWindowType,
                    None,
                )
            }
            .map_err(io::Error::other)?,
        );
        self.egl
            .make_current(self.display, self.surface, self.surface, self.context)
            .map_err(io::Error::other)?;
        self.egl
            .swap_interval(self.display, 1)
            .map_err(io::Error::other)
    }
}
impl Drop for EglContext {
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.display, None, None, None);
        if let Some(surface) = self.surface.take() {
            let _ = self.egl.destroy_surface(self.display, surface);
        }
        if let Some(context) = self.context.take() {
            let _ = self.egl.destroy_context(self.display, context);
        }
        let _ = self.egl.terminate(self.display);
    }
}
struct Target {
    texture: glow::NativeTexture,
    fbo: glow::NativeFramebuffer,
    size: (u16, u16),
}
pub(super) struct GlWindow {
    gl: glow::Context,
    target: Option<Target>,
    ui: Option<ui::Ui>,
    pending: VecDeque<glow::NativeFence>,
    egl: EglContext,
}
impl GlWindow {
    pub fn new(window: u32) -> io::Result<Self> {
        let egl = EglContext::new(window)?;
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                egl.egl
                    .get_proc_address(name)
                    .map_or(std::ptr::null(), |p| p as *const _)
            })
        };
        tracing::info!(target: "mpv.render", renderer = unsafe { gl.get_parameter_string(glow::RENDERER) }, "initialized EGL video compositor");
        let ui = Some(ui::Ui::new(&gl)?);
        Ok(Self {
            gl,
            target: None,
            ui,
            pending: VecDeque::new(),
            egl,
        })
    }
    pub fn upload_ui(&mut self, frame: &crate::playback::NativeOverlayFrame) {
        if let Some(ui) = &mut self.ui {
            ui.upload(&self.gl, frame);
        }
    }
    pub fn xdisplay(&self) -> *mut c_void {
        self.egl.x11.raw
    }
    pub fn proc_address(&self, name: &CStr) -> *mut c_void {
        name.to_str()
            .ok()
            .and_then(|name| self.egl.egl.get_proc_address(name))
            .map_or(std::ptr::null_mut(), |p| p as *mut c_void)
    }
    pub fn target(&mut self, size: (u16, u16)) -> io::Result<u32> {
        if let Some(target) = &self.target
            && target.size == size
        {
            return Ok(target.fbo.0.get());
        }
        self.clear_target();
        // SAFETY: the context remains current on this thread for its lifetime.
        unsafe {
            let texture = self.gl.create_texture().map_err(io::Error::other)?;
            let fbo = match self.gl.create_framebuffer() {
                Ok(fbo) => fbo,
                Err(error) => {
                    self.gl.delete_texture(texture);
                    return Err(io::Error::other(error));
                }
            };
            self.target = Some(Target { texture, fbo, size });
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                i32::from(size.0),
                i32::from(size.1),
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            self.gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            if self.gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                return Err(io::Error::other("native video framebuffer is incomplete"));
            }
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.bind_texture(glow::TEXTURE_2D, None);
            tracing::debug!(target: "mpv.render", width=size.0, height=size.1, "resized native video render target");
            Ok(fbo.0.get())
        }
    }
    pub fn present(&mut self, size: (u16, u16)) -> io::Result<()> {
        let surface = self
            .egl
            .surface
            .ok_or_else(|| io::Error::other("EGL window is detached"))?;
        let width = self
            .egl
            .egl
            .query_surface(self.egl.display, surface, egl::WIDTH)
            .map_err(io::Error::other)?;
        let height = self
            .egl
            .egl
            .query_surface(self.egl.display, surface, egl::HEIGHT)
            .map_err(io::Error::other)?;
        let target = self
            .target
            .as_ref()
            .ok_or_else(|| io::Error::other("video target is unavailable"))?;
        unsafe {
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl
                .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.fbo));
            self.gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
            self.gl.blit_framebuffer(
                0,
                0,
                i32::from(size.0),
                i32::from(size.1),
                0,
                0,
                width,
                height,
                glow::COLOR_BUFFER_BIT,
                glow::LINEAR,
            );
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        if let Some(ui) = &self.ui {
            ui.draw(&self.gl, width, height);
        }
        self.egl
            .egl
            .swap_buffers(self.egl.display, surface)
            .map_err(io::Error::other)?;
        let fence = unsafe { self.gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE, 0) }
            .map_err(io::Error::other)?;
        unsafe {
            self.gl.flush();
        }
        self.pending.push_back(fence);
        Ok(())
    }
    pub fn begin_frame(&mut self) -> io::Result<()> {
        if self.pending.len() < 2 {
            return Ok(());
        }
        if let Some(fence) = self.pending.pop_front() {
            let status = unsafe {
                self.gl
                    .client_wait_sync(fence, glow::SYNC_FLUSH_COMMANDS_BIT, 1_000_000_000)
            };
            unsafe {
                self.gl.delete_sync(fence);
            }
            if ![glow::ALREADY_SIGNALED, glow::CONDITION_SATISFIED].contains(&status) {
                return Err(io::Error::other(
                    "GPU presentation did not complete within one second",
                ));
            }
        }
        Ok(())
    }
    fn clear_target(&mut self) {
        if let Some(target) = self.target.take() {
            unsafe {
                self.gl.finish();
                self.gl.delete_framebuffer(target.fbo);
                self.gl.delete_texture(target.texture);
            }
        }
    }
}
impl Drop for GlWindow {
    fn drop(&mut self) {
        self.clear_target();
        for fence in self.pending.drain(..) {
            unsafe {
                self.gl.delete_sync(fence);
            }
        }
        if let Some(ui) = self.ui.take() {
            ui.destroy(&self.gl);
        }
    }
}
