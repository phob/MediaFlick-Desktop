use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::fs::{self, File, OpenOptions};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::PathBuf;
use std::ptr;

use libloading::Library;

use crate::players::mpv::WaylandDmaBufPlane;

pub(super) const GBM_FORMAT_ARGB8888: u32 = 0x3432_5241;
pub(super) const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;
pub(super) const GBM_BO_USE_SCANOUT: u32 = 0x1;
pub(super) const GBM_BO_USE_RENDERING: u32 = 0x4;
pub(super) const GBM_BO_USE_LINEAR: u32 = 0x10;

pub(super) const EGL_NONE: i32 = 0x3038;
pub(super) const EGL_TRUE: c_uint = 1;
pub(super) const EGL_OPENGL_ES_API: c_uint = 0x30a0;
pub(super) const EGL_OPENGL_ES2_BIT: i32 = 0x4;
pub(super) const EGL_RENDERABLE_TYPE: i32 = 0x3040;
pub(super) const EGL_CONTEXT_CLIENT_VERSION: i32 = 0x3098;
pub(super) const EGL_WIDTH: i32 = 0x3057;
pub(super) const EGL_HEIGHT: i32 = 0x3056;
pub(super) const EGL_LINUX_DMA_BUF_EXT: c_uint = 0x3270;
pub(super) const EGL_LINUX_DRM_FOURCC_EXT: i32 = 0x3271;
pub(super) const EGL_PLATFORM_GBM_KHR: c_uint = 0x31d7;

const PLANE_ATTRS: [[i32; 5]; 4] = [
    [0x3272, 0x3273, 0x3274, 0x3443, 0x3444],
    [0x3275, 0x3276, 0x3277, 0x3445, 0x3446],
    [0x3278, 0x3279, 0x327a, 0x3447, 0x3448],
    [0x3440, 0x3441, 0x3442, 0x3449, 0x344a],
];

pub(super) const GL_TEXTURE_2D: c_uint = 0x0de1;
pub(super) const GL_TEXTURE_EXTERNAL_OES: c_uint = 0x8d65;
pub(super) const GL_TEXTURE_MIN_FILTER: c_uint = 0x2801;
pub(super) const GL_TEXTURE_MAG_FILTER: c_uint = 0x2800;
pub(super) const GL_NEAREST: i32 = 0x2600;
pub(super) const GL_TEXTURE_WRAP_S: c_uint = 0x2802;
pub(super) const GL_TEXTURE_WRAP_T: c_uint = 0x2803;
pub(super) const GL_CLAMP_TO_EDGE: i32 = 0x812f;
pub(super) const GL_FRAMEBUFFER: c_uint = 0x8d40;
pub(super) const GL_COLOR_ATTACHMENT0: c_uint = 0x8ce0;
pub(super) const GL_FRAMEBUFFER_COMPLETE: c_uint = 0x8cd5;
pub(super) const GL_VERTEX_SHADER: c_uint = 0x8b31;
pub(super) const GL_FRAGMENT_SHADER: c_uint = 0x8b30;
pub(super) const GL_COMPILE_STATUS: c_uint = 0x8b81;
pub(super) const GL_LINK_STATUS: c_uint = 0x8b82;
pub(super) const GL_FLOAT: c_uint = 0x1406;
pub(super) const GL_TRIANGLE_STRIP: c_uint = 0x5;
pub(super) const GL_RGBA: c_uint = 0x1908;
pub(super) const GL_UNSIGNED_BYTE: c_uint = 0x1401;
pub(super) const GL_COLOR_BUFFER_BIT: c_uint = 0x4000;
pub(super) const GL_BLEND: c_uint = 0x0be2;

const VERTEX_SHADER_SRC: &str = "attribute vec2 a_pos;\nattribute vec2 a_uv;\nvarying vec2 v_uv;\nvoid main() {\n    gl_Position = vec4(a_pos, 0.0, 1.0);\n    v_uv = a_uv;\n}\n";

const FRAGMENT_SHADER_SRC: &str = "#extension GL_OES_EGL_image_external : require\nprecision mediump float;\nvarying vec2 v_uv;\nuniform samplerExternalOES u_tex;\nvoid main() {\n    gl_FragColor = texture2D(u_tex, v_uv);\n}\n";

pub(super) const QUAD_VERTICES: [f32; 16] = [
    -1.0, -1.0, 0.0, 0.0, 1.0, -1.0, 1.0, 0.0, -1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0,
];

pub(super) struct SourcePlane {
    pub fd: RawFd,
    pub offset: u32,
    pub stride: u32,
}

pub(super) struct GbmApi {
    _lib: Library,
    pub create_device: unsafe extern "C" fn(c_int) -> *mut c_void,
    pub device_destroy: unsafe extern "C" fn(*mut c_void),
    pub bo_create: unsafe extern "C" fn(*mut c_void, u32, u32, u32, u32) -> *mut c_void,
    pub bo_destroy: unsafe extern "C" fn(*mut c_void),
    pub bo_get_plane_count: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub bo_get_fd_for_plane: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    pub bo_get_stride_for_plane: unsafe extern "C" fn(*mut c_void, c_int) -> u32,
    pub bo_get_offset: unsafe extern "C" fn(*mut c_void, c_int) -> u32,
    pub bo_get_modifier: unsafe extern "C" fn(*mut c_void) -> u64,
}

pub(super) struct EglApi {
    _lib: Library,
    pub get_platform_display:
        Option<unsafe extern "C" fn(c_uint, *mut c_void, *const isize) -> *mut c_void>,
    pub get_platform_display_ext:
        Option<unsafe extern "C" fn(c_uint, *mut c_void, *const i32) -> *mut c_void>,
    pub initialize: unsafe extern "C" fn(*mut c_void, *mut i32, *mut i32) -> c_uint,
    pub terminate: unsafe extern "C" fn(*mut c_void) -> c_uint,
    pub bind_api: unsafe extern "C" fn(c_uint) -> c_uint,
    pub choose_config:
        unsafe extern "C" fn(*mut c_void, *const i32, *mut *mut c_void, i32, *mut i32) -> c_uint,
    pub create_context:
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const i32) -> *mut c_void,
    pub destroy_context: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_uint,
    pub make_current:
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> c_uint,
    pub get_error: unsafe extern "C" fn() -> i32,
    pub create_image_khr: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        c_uint,
        *mut c_void,
        *const i32,
    ) -> *mut c_void,
    pub destroy_image_khr: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_uint,
}

pub(super) struct GlesApi {
    _lib: Library,
    pub image_target_texture_2d: unsafe extern "C" fn(c_uint, *mut c_void),
    pub gen_textures: unsafe extern "C" fn(i32, *mut c_uint),
    pub delete_textures: unsafe extern "C" fn(i32, *const c_uint),
    pub bind_texture: unsafe extern "C" fn(c_uint, c_uint),
    pub tex_parameteri: unsafe extern "C" fn(c_uint, c_uint, i32),
    pub tex_image_2d:
        unsafe extern "C" fn(c_uint, i32, i32, i32, i32, i32, c_uint, c_uint, *const c_void),
    pub gen_framebuffers: unsafe extern "C" fn(i32, *mut c_uint),
    pub delete_framebuffers: unsafe extern "C" fn(i32, *const c_uint),
    pub bind_framebuffer: unsafe extern "C" fn(c_uint, c_uint),
    pub framebuffer_texture_2d: unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint, i32),
    pub check_framebuffer_status: unsafe extern "C" fn(c_uint) -> c_uint,
    pub create_shader: unsafe extern "C" fn(c_uint) -> c_uint,
    pub delete_shader: unsafe extern "C" fn(c_uint),
    pub shader_source: unsafe extern "C" fn(c_uint, i32, *const *const c_char, *const i32),
    pub compile_shader: unsafe extern "C" fn(c_uint),
    pub get_shaderiv: unsafe extern "C" fn(c_uint, c_uint, *mut i32),
    pub link_program: unsafe extern "C" fn(c_uint),
    pub create_program: unsafe extern "C" fn() -> c_uint,
    pub delete_program: unsafe extern "C" fn(c_uint),
    pub attach_shader: unsafe extern "C" fn(c_uint, c_uint),
    pub get_programiv: unsafe extern "C" fn(c_uint, c_uint, *mut i32),
    pub use_program: unsafe extern "C" fn(c_uint),
    pub get_attrib_location: unsafe extern "C" fn(c_uint, *const c_char) -> i32,
    pub get_uniform_location: unsafe extern "C" fn(c_uint, *const c_char) -> i32,
    pub uniform_1i: unsafe extern "C" fn(i32, i32),
    pub enable_vertex_attrib_array: unsafe extern "C" fn(c_uint),
    pub vertex_attrib_pointer: unsafe extern "C" fn(c_uint, i32, c_uint, u8, i32, *const c_void),
    pub viewport: unsafe extern "C" fn(i32, i32, i32, i32),
    pub disable: unsafe extern "C" fn(c_uint),
    pub clear_color: unsafe extern "C" fn(f32, f32, f32, f32),
    pub clear: unsafe extern "C" fn(c_uint),
    pub draw_arrays: unsafe extern "C" fn(c_uint, i32, i32),
    pub finish: unsafe extern "C" fn(),
    pub read_pixels: unsafe extern "C" fn(i32, i32, i32, i32, c_uint, c_uint, *mut c_void),
}

unsafe fn load_sym<T: Copy>(lib: &Library, name: &[u8]) -> Result<T, String> {
    // SAFETY: Caller supplies a NUL-terminated C symbol name matching `T`.
    let sym = unsafe { lib.get::<T>(name) }.map_err(|err| {
        format!(
            "failed to load symbol {:?}: {err}",
            CStr::from_bytes_with_nul(name)
        )
    })?;
    Ok(*sym)
}

unsafe fn load_egl_proc<T: Copy>(
    get_proc_address: unsafe extern "C" fn(*const c_char) -> *const c_void,
    name: &'static [u8],
) -> Result<T, String> {
    if std::mem::size_of::<T>() != std::mem::size_of::<*const c_void>() {
        return Err("EGL proc size mismatch".to_string());
    }
    let cname =
        CStr::from_bytes_with_nul(name).map_err(|err| format!("invalid EGL proc name: {err}"))?;
    // SAFETY: `cname` is a valid NUL-terminated static C string.
    let ptr = unsafe { get_proc_address(cname.as_ptr()) };
    if ptr.is_null() {
        return Err(format!("missing EGL extension proc {:?}", cname));
    }
    // SAFETY: Caller matches `T` to the C function pointer type named by `name`.
    Ok(unsafe { std::mem::transmute_copy::<*const c_void, T>(&ptr) })
}

impl GbmApi {
    pub(super) fn load() -> Result<Self, String> {
        // SAFETY: Loading standard Linux system GBM shared library.
        let lib = unsafe { Library::new("libgbm.so.1") }
            .map_err(|err| format!("failed to load libgbm.so.1: {err}"))?;
        // SAFETY: Symbols match documented <gbm.h> C prototypes.
        unsafe {
            Ok(Self {
                create_device: load_sym(&lib, b"gbm_create_device\0")?,
                device_destroy: load_sym(&lib, b"gbm_device_destroy\0")?,
                bo_create: load_sym(&lib, b"gbm_bo_create\0")?,
                bo_destroy: load_sym(&lib, b"gbm_bo_destroy\0")?,
                bo_get_plane_count: load_sym(&lib, b"gbm_bo_get_plane_count\0")?,
                bo_get_fd_for_plane: load_sym(&lib, b"gbm_bo_get_fd_for_plane\0")?,
                bo_get_stride_for_plane: load_sym(&lib, b"gbm_bo_get_stride_for_plane\0")?,
                bo_get_offset: load_sym(&lib, b"gbm_bo_get_offset\0")?,
                bo_get_modifier: load_sym(&lib, b"gbm_bo_get_modifier\0")?,
                _lib: lib,
            })
        }
    }
}

impl EglApi {
    pub(super) fn load()
    -> Result<(Self, unsafe extern "C" fn(*const c_char) -> *const c_void), String> {
        // SAFETY: Loading standard Linux system EGL shared library.
        let lib = unsafe { Library::new("libEGL.so.1") }
            .map_err(|err| format!("failed to load libEGL.so.1: {err}"))?;
        // SAFETY: Symbols match documented <EGL/egl.h> and <EGL/eglext.h> C prototypes.
        unsafe {
            let get_proc_address: unsafe extern "C" fn(*const c_char) -> *const c_void =
                load_sym(&lib, b"eglGetProcAddress\0")?;
            let get_platform_display = load_sym(&lib, b"eglGetPlatformDisplay\0").ok();
            let get_platform_display_ext =
                load_egl_proc(get_proc_address, b"eglGetPlatformDisplayEXT\0").ok();
            let api = Self {
                get_platform_display,
                get_platform_display_ext,
                initialize: load_sym(&lib, b"eglInitialize\0")?,
                terminate: load_sym(&lib, b"eglTerminate\0")?,
                bind_api: load_sym(&lib, b"eglBindAPI\0")?,
                choose_config: load_sym(&lib, b"eglChooseConfig\0")?,
                create_context: load_sym(&lib, b"eglCreateContext\0")?,
                destroy_context: load_sym(&lib, b"eglDestroyContext\0")?,
                make_current: load_sym(&lib, b"eglMakeCurrent\0")?,
                get_error: load_sym(&lib, b"eglGetError\0")?,
                create_image_khr: load_egl_proc(get_proc_address, b"eglCreateImageKHR\0")?,
                destroy_image_khr: load_egl_proc(get_proc_address, b"eglDestroyImageKHR\0")?,
                _lib: lib,
            };
            Ok((api, get_proc_address))
        }
    }
}

impl GlesApi {
    pub(super) fn load(
        get_proc_address: unsafe extern "C" fn(*const c_char) -> *const c_void,
    ) -> Result<Self, String> {
        // SAFETY: Loading standard Linux system OpenGL ES 2 shared library.
        let lib = unsafe { Library::new("libGLESv2.so.2") }
            .map_err(|err| format!("failed to load libGLESv2.so.2: {err}"))?;
        // SAFETY: Symbols match documented <GLES2/gl2.h> and <GLES2/gl2ext.h> C prototypes.
        unsafe {
            Ok(Self {
                image_target_texture_2d: load_egl_proc(
                    get_proc_address,
                    b"glEGLImageTargetTexture2DOES\0",
                )?,
                gen_textures: load_sym(&lib, b"glGenTextures\0")?,
                delete_textures: load_sym(&lib, b"glDeleteTextures\0")?,
                bind_texture: load_sym(&lib, b"glBindTexture\0")?,
                tex_parameteri: load_sym(&lib, b"glTexParameteri\0")?,
                tex_image_2d: load_sym(&lib, b"glTexImage2D\0")?,
                gen_framebuffers: load_sym(&lib, b"glGenFramebuffers\0")?,
                delete_framebuffers: load_sym(&lib, b"glDeleteFramebuffers\0")?,
                bind_framebuffer: load_sym(&lib, b"glBindFramebuffer\0")?,
                framebuffer_texture_2d: load_sym(&lib, b"glFramebufferTexture2D\0")?,
                check_framebuffer_status: load_sym(&lib, b"glCheckFramebufferStatus\0")?,
                create_shader: load_sym(&lib, b"glCreateShader\0")?,
                delete_shader: load_sym(&lib, b"glDeleteShader\0")?,
                shader_source: load_sym(&lib, b"glShaderSource\0")?,
                compile_shader: load_sym(&lib, b"glCompileShader\0")?,
                get_shaderiv: load_sym(&lib, b"glGetShaderiv\0")?,
                link_program: load_sym(&lib, b"glLinkProgram\0")?,
                create_program: load_sym(&lib, b"glCreateProgram\0")?,
                delete_program: load_sym(&lib, b"glDeleteProgram\0")?,
                attach_shader: load_sym(&lib, b"glAttachShader\0")?,
                get_programiv: load_sym(&lib, b"glGetProgramiv\0")?,
                use_program: load_sym(&lib, b"glUseProgram\0")?,
                get_attrib_location: load_sym(&lib, b"glGetAttribLocation\0")?,
                get_uniform_location: load_sym(&lib, b"glGetUniformLocation\0")?,
                uniform_1i: load_sym(&lib, b"glUniform1i\0")?,
                enable_vertex_attrib_array: load_sym(&lib, b"glEnableVertexAttribArray\0")?,
                vertex_attrib_pointer: load_sym(&lib, b"glVertexAttribPointer\0")?,
                viewport: load_sym(&lib, b"glViewport\0")?,
                disable: load_sym(&lib, b"glDisable\0")?,
                clear_color: load_sym(&lib, b"glClearColor\0")?,
                clear: load_sym(&lib, b"glClear\0")?,
                draw_arrays: load_sym(&lib, b"glDrawArrays\0")?,
                finish: load_sym(&lib, b"glFinish\0")?,
                read_pixels: load_sym(&lib, b"glReadPixels\0")?,
                _lib: lib,
            })
        }
    }
}

fn linux_major_minor(rdev: u64) -> (u64, u64) {
    let major = ((rdev >> 8) & 0xfff) | ((rdev >> 32) & !0xfff);
    let minor = (rdev & 0xff) | ((rdev >> 12) & 0x000f_ff00);
    (major, minor)
}

fn sysfs_pci_device_for_rdev(rdev: u64) -> Option<PathBuf> {
    let (major, minor) = linux_major_minor(rdev);
    fs::canonicalize(format!("/sys/dev/char/{major}:{minor}/device")).ok()
}

fn order_render_nodes(
    mut candidates: Vec<(PathBuf, u64, Option<PathBuf>)>,
    preferred_rdev: Option<u64>,
    preferred_pci: Option<&std::path::Path>,
) -> Vec<PathBuf> {
    candidates.sort_by(|(path_a, rdev_a, pci_a), (path_b, rdev_b, pci_b)| {
        let rank = |rdev: u64, pci: Option<&std::path::Path>| -> u8 {
            if preferred_rdev == Some(rdev) {
                0
            } else if preferred_pci.zip(pci).is_some_and(|(a, b)| a == b) {
                1
            } else {
                2
            }
        };
        rank(*rdev_a, pci_a.as_deref())
            .cmp(&rank(*rdev_b, pci_b.as_deref()))
            .then_with(|| path_a.cmp(path_b))
    });
    candidates.into_iter().map(|(path, _, _)| path).collect()
}

fn render_node_paths(preferred_rdev: Option<u64>) -> Vec<PathBuf> {
    use std::os::linux::fs::MetadataExt;

    let Ok(entries) = fs::read_dir("/dev/dri") else {
        return Vec::new();
    };
    let candidates: Vec<(PathBuf, u64, Option<PathBuf>)> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("renderD"))
        })
        .map(|entry| {
            let path = entry.path();
            let rdev = fs::metadata(&path).map(|meta| meta.st_rdev()).unwrap_or(0);
            let pci = (rdev != 0)
                .then(|| sysfs_pci_device_for_rdev(rdev))
                .flatten();
            (path, rdev, pci)
        })
        .collect();
    let preferred_pci = preferred_rdev.and_then(sysfs_pci_device_for_rdev);
    order_render_nodes(candidates, preferred_rdev, preferred_pci.as_deref())
}

pub(super) fn open_gbm_display(
    gbm: &GbmApi,
    egl: &EglApi,
    preferred_rdev: Option<u64>,
) -> Result<(File, *mut c_void, *mut c_void), String> {
    for path in render_node_paths(preferred_rdev) {
        let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else {
            continue;
        };
        // SAFETY: `file.as_raw_fd()` is an open DRM render node fd owned by `file`.
        let gbm_device = unsafe { (gbm.create_device)(file.as_raw_fd()) };
        if gbm_device.is_null() {
            continue;
        }
        // SAFETY: `gbm_device` is a valid non-null GBM device pointer.
        let dpy = unsafe { get_gbm_egl_display(egl, gbm_device) };
        if dpy.is_null() {
            // SAFETY: Cleaning up `gbm_device` after display creation failed.
            unsafe { (gbm.device_destroy)(gbm_device) };
            continue;
        }
        // SAFETY: `dpy` is a valid EGLDisplay returned for `gbm_device`.
        if unsafe { (egl.initialize)(dpy, ptr::null_mut(), ptr::null_mut()) } == EGL_TRUE {
            return Ok((file, gbm_device, dpy));
        }
        // SAFETY: Cleaning up failed EGLDisplay and GBM device before trying next render node.
        unsafe {
            (egl.terminate)(dpy);
            (gbm.device_destroy)(gbm_device);
        }
    }
    Err("no working /dev/dri/renderD* GBM+EGL display found".to_string())
}

unsafe fn get_gbm_egl_display(egl: &EglApi, gbm_device: *mut c_void) -> *mut c_void {
    if let Some(func) = egl.get_platform_display {
        // SAFETY: Caller passes valid `gbm_device` for `EGL_PLATFORM_GBM_KHR`.
        let dpy = unsafe { func(EGL_PLATFORM_GBM_KHR, gbm_device, ptr::null()) };
        if !dpy.is_null() {
            return dpy;
        }
    }
    if let Some(func) = egl.get_platform_display_ext {
        // SAFETY: Caller passes valid `gbm_device` for `EGL_PLATFORM_GBM_KHR`.
        return unsafe { func(EGL_PLATFORM_GBM_KHR, gbm_device, ptr::null()) };
    }
    ptr::null_mut()
}

pub(super) fn create_gles_context(egl: &EglApi, dpy: *mut c_void) -> Result<*mut c_void, String> {
    // SAFETY: `dpy` is an initialized EGLDisplay.
    if unsafe { (egl.bind_api)(EGL_OPENGL_ES_API) } != EGL_TRUE {
        return Err("eglBindAPI(EGL_OPENGL_ES_API) failed".to_string());
    }
    let config_attrs = [EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_NONE];
    let mut config = ptr::null_mut();
    let mut num_configs = 0;
    // SAFETY: `config_attrs` is terminated with `EGL_NONE` and outputs point to valid locals.
    let chose = unsafe {
        (egl.choose_config)(dpy, config_attrs.as_ptr(), &mut config, 1, &mut num_configs)
    };
    let chosen = if chose == EGL_TRUE && num_configs > 0 {
        config
    } else {
        ptr::null_mut()
    };
    let ctx_attrs = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
    // SAFETY: `ctx_attrs` is terminated with `EGL_NONE` and `dpy` is initialized.
    let ctx = unsafe { (egl.create_context)(dpy, chosen, ptr::null_mut(), ctx_attrs.as_ptr()) };
    if ctx.is_null() {
        // SAFETY: `eglGetError` takes no parameters and has no preconditions.
        let err = unsafe { (egl.get_error)() };
        return Err(format!("eglCreateContext failed (0x{err:x})"));
    }
    Ok(ctx)
}

fn compile_shader(gles: &GlesApi, kind: c_uint, src: &str) -> Result<c_uint, String> {
    let csrc = CString::new(src).map_err(|err| format!("invalid shader source: {err}"))?;
    let ptr = csrc.as_ptr();
    let mut compiled = 0;
    // SAFETY: An OpenGL ES context is current; `ptr` points to NUL-terminated GLSL source.
    unsafe {
        let shader = (gles.create_shader)(kind);
        if shader == 0 {
            return Err("glCreateShader returned 0".to_string());
        }
        (gles.shader_source)(shader, 1, &ptr, ptr::null());
        (gles.compile_shader)(shader);
        (gles.get_shaderiv)(shader, GL_COMPILE_STATUS, &mut compiled);
        if compiled == 0 {
            (gles.delete_shader)(shader);
            return Err("GLSL shader compilation failed".to_string());
        }
        Ok(shader)
    }
}

pub(super) fn build_program(gles: &GlesApi) -> Result<(c_uint, c_uint, c_uint, i32), String> {
    let vs = compile_shader(gles, GL_VERTEX_SHADER, VERTEX_SHADER_SRC)?;
    let fs = match compile_shader(gles, GL_FRAGMENT_SHADER, FRAGMENT_SHADER_SRC) {
        Ok(fs) => fs,
        Err(err) => {
            // SAFETY: Current GLES context owns `vs`.
            unsafe { (gles.delete_shader)(vs) };
            return Err(err);
        }
    };
    // SAFETY: `vs` and `fs` are valid compiled shaders in the current GLES context.
    let program = unsafe { (gles.create_program)() };
    if program == 0 {
        // SAFETY: Current GLES context owns `vs` and `fs`.
        unsafe {
            (gles.delete_shader)(vs);
            (gles.delete_shader)(fs);
        }
        return Err("glCreateProgram returned 0".to_string());
    }
    let mut linked = 0;
    // SAFETY: Program and shaders belong to current GLES context; shader objects are deleted after linking.
    unsafe {
        (gles.attach_shader)(program, vs);
        (gles.attach_shader)(program, fs);
        (gles.link_program)(program);
        (gles.delete_shader)(vs);
        (gles.delete_shader)(fs);
        (gles.get_programiv)(program, GL_LINK_STATUS, &mut linked);
    }
    if linked == 0 {
        // SAFETY: Current GLES context owns `program`.
        unsafe { (gles.delete_program)(program) };
        return Err("GLSL program link failed".to_string());
    }
    // SAFETY: `program` linked cleanly and all attribute/uniform names are NUL-terminated.
    let (pos, uv, tex) = unsafe {
        (
            (gles.get_attrib_location)(program, c"a_pos".as_ptr()),
            (gles.get_attrib_location)(program, c"a_uv".as_ptr()),
            (gles.get_uniform_location)(program, c"u_tex".as_ptr()),
        )
    };
    let (Ok(a_pos), Ok(a_uv)) = (c_uint::try_from(pos), c_uint::try_from(uv)) else {
        // SAFETY: Current GLES context owns `program`.
        unsafe { (gles.delete_program)(program) };
        return Err("GLSL shader attributes missing after link".to_string());
    };
    Ok((program, a_pos, a_uv, tex))
}

pub(super) fn configure_texture_params(gles: &GlesApi, target: c_uint) {
    // SAFETY: Texture object is bound to `target` in the current GLES context.
    unsafe {
        (gles.tex_parameteri)(target, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        (gles.tex_parameteri)(target, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        (gles.tex_parameteri)(target, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
        (gles.tex_parameteri)(target, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    }
}

pub(super) fn create_dmabuf_egl_image(
    egl: &EglApi,
    dpy: *mut c_void,
    width: u16,
    height: u16,
    fourcc: u32,
    modifier: u64,
    planes: &[SourcePlane],
) -> Result<*mut c_void, String> {
    if planes.is_empty() || planes.len() > PLANE_ATTRS.len() {
        return Err(format!("unsupported DMA-BUF plane count {}", planes.len()));
    }
    let fourcc_i32 = i32::from_ne_bytes(fourcc.to_ne_bytes());
    let mut attrs = Vec::with_capacity(7 + planes.len() * 10);
    attrs.extend_from_slice(&[
        EGL_WIDTH,
        i32::from(width),
        EGL_HEIGHT,
        i32::from(height),
        EGL_LINUX_DRM_FOURCC_EXT,
        fourcc_i32,
    ]);
    let has_modifier = modifier != DRM_FORMAT_MOD_INVALID;
    let mod_lo = i32::from_ne_bytes((modifier as u32).to_ne_bytes());
    let mod_hi = i32::from_ne_bytes(((modifier >> 32) as u32).to_ne_bytes());
    for (idx, plane) in planes.iter().enumerate() {
        let keys = PLANE_ATTRS[idx];
        let offset_i32 = i32::try_from(plane.offset).map_err(|_| "DMA-BUF offset overflow")?;
        let stride_i32 = i32::try_from(plane.stride).map_err(|_| "DMA-BUF stride overflow")?;
        attrs.extend_from_slice(&[keys[0], plane.fd, keys[1], offset_i32, keys[2], stride_i32]);
        if has_modifier {
            attrs.extend_from_slice(&[keys[3], mod_lo, keys[4], mod_hi]);
        }
    }
    attrs.push(EGL_NONE);
    // SAFETY: `dpy` is initialized, `attrs` is terminated with `EGL_NONE`, and fds remain valid during the call.
    let img = unsafe {
        (egl.create_image_khr)(
            dpy,
            ptr::null_mut(),
            EGL_LINUX_DMA_BUF_EXT,
            ptr::null_mut(),
            attrs.as_ptr(),
        )
    };
    if img.is_null() {
        // SAFETY: `eglGetError` requires no arguments.
        let err = unsafe { (egl.get_error)() };
        return Err(format!("eglCreateImageKHR(DMA_BUF) failed (0x{err:x})"));
    }
    Ok(img)
}

pub(super) fn export_gbm_planes(
    gbm: &GbmApi,
    bo: *mut c_void,
) -> Result<Vec<WaylandDmaBufPlane>, String> {
    // SAFETY: `bo` is a valid allocated `gbm_bo`.
    let raw_count = unsafe { (gbm.bo_get_plane_count)(bo) };
    let count = usize::try_from(raw_count)
        .ok()
        .filter(|&n| (1..=PLANE_ATTRS.len()).contains(&n))
        .ok_or_else(|| format!("unexpected GBM plane count {raw_count}"))?;
    let mut planes = Vec::with_capacity(count);
    for idx in 0..count {
        let plane = idx as c_int;
        // SAFETY: `plane` is in `0..count` on valid `bo`.
        let raw_fd = unsafe { (gbm.bo_get_fd_for_plane)(bo, plane) };
        if raw_fd < 0 {
            return Err(format!("gbm_bo_get_fd_for_plane({plane}) failed"));
        }
        // SAFETY: `raw_fd` is a newly created valid fd returned by `gbm_bo_get_fd_for_plane`.
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        // SAFETY: `plane` is valid on `bo`.
        let stride = unsafe { (gbm.bo_get_stride_for_plane)(bo, plane) };
        // SAFETY: `plane` is valid on `bo`.
        let offset = unsafe { (gbm.bo_get_offset)(bo, plane) };
        planes.push(WaylandDmaBufPlane { fd, offset, stride });
    }
    Ok(planes)
}

pub(super) struct EglImageGuard {
    pub destroy_image_khr: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_uint,
    pub dpy: *mut c_void,
    pub img: *mut c_void,
}

impl Drop for EglImageGuard {
    fn drop(&mut self) {
        if !self.img.is_null() {
            // SAFETY: `self.img` was created on `self.dpy` and is no longer referenced.
            unsafe {
                (self.destroy_image_khr)(self.dpy, self.img);
            }
        }
    }
}

pub(super) struct CurrentContextGuard {
    make_current:
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> c_uint,
    dpy: *mut c_void,
}

impl CurrentContextGuard {
    pub(super) fn acquire(
        egl: &EglApi,
        dpy: *mut c_void,
        ctx: *mut c_void,
    ) -> Result<Self, String> {
        // SAFETY: `dpy` is initialized and `ctx` belongs to `dpy`.
        if unsafe { (egl.make_current)(dpy, ptr::null_mut(), ptr::null_mut(), ctx) } != EGL_TRUE {
            // SAFETY: `eglGetError` takes no parameters.
            let err = unsafe { (egl.get_error)() };
            return Err(format!("eglMakeCurrent failed (0x{err:x})"));
        }
        Ok(Self {
            make_current: egl.make_current,
            dpy,
        })
    }
}

impl Drop for CurrentContextGuard {
    fn drop(&mut self) {
        // SAFETY: Releasing the current EGL context back to null on the calling thread.
        unsafe {
            (self.make_current)(self.dpy, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_render_nodes_prefers_exact_rdev_then_matching_pci_then_path_order() {
        let pci_igpu = PathBuf::from("/sys/devices/pci0000:00/0000:00:02.0");
        let pci_nvidia = PathBuf::from("/sys/devices/pci0000:00/0000:01:00.0");
        let nodes = vec![
            (
                PathBuf::from("/dev/dri/renderD128"),
                (226 << 8) | 128,
                Some(pci_igpu.clone()),
            ),
            (
                PathBuf::from("/dev/dri/renderD129"),
                (226 << 8) | 129,
                Some(pci_nvidia.clone()),
            ),
        ];

        let exact = order_render_nodes(nodes.clone(), Some((226 << 8) | 129), Some(&pci_igpu));
        assert_eq!(
            exact,
            vec![
                PathBuf::from("/dev/dri/renderD129"),
                PathBuf::from("/dev/dri/renderD128"),
            ]
        );

        let via_pci = order_render_nodes(nodes.clone(), Some((226 << 8) | 1), Some(&pci_nvidia));
        assert_eq!(
            via_pci,
            vec![
                PathBuf::from("/dev/dri/renderD129"),
                PathBuf::from("/dev/dri/renderD128"),
            ]
        );

        let fallback = order_render_nodes(nodes, None, None);
        assert_eq!(
            fallback,
            vec![
                PathBuf::from("/dev/dri/renderD128"),
                PathBuf::from("/dev/dri/renderD129"),
            ]
        );
    }

    #[test]
    fn linux_major_minor_decodes_standard_and_extended_rdev() {
        assert_eq!(linux_major_minor((226 << 8) | 129), (226, 129));
        let ext = (226 << 8) | 5 | (1 << 20);
        assert_eq!(linux_major_minor(ext), (226, 261));
    }
}
