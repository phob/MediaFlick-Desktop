//! Render video and controls at native monitor density, then blit once to X11.
//! The dedicated thread exclusively owns EGL and all mpv_render_* calls. It
//! never waits for the controller or calls the ordinary libmpv client API.
use std::ffi::{CStr, c_char, c_int, c_void};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use libloading::Library;

use super::{MpvHandle, load_symbol};
use crate::playback::NativeOverlayFrame;

#[path = "render_gl/window.rs"]
mod window;
use window::GlWindow;

#[repr(C)]
struct Param {
    kind: c_int,
    data: *mut c_void,
}
impl Param {
    fn new<T>(kind: c_int, value: &mut T) -> Self {
        Self {
            kind,
            data: std::ptr::from_mut(value).cast(),
        }
    }
    fn end() -> Self {
        Self {
            kind: 0,
            data: std::ptr::null_mut(),
        }
    }
}
#[repr(C)]
struct Init {
    get_proc: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    context: *mut c_void,
}
#[repr(C)]
struct Fbo {
    fbo: c_int,
    width: c_int,
    height: c_int,
    format: c_int,
}
#[derive(Clone, Copy)]
struct Api {
    create: unsafe extern "C" fn(*mut *mut c_void, *mut MpvHandle, *mut Param) -> c_int,
    free: unsafe extern "C" fn(*mut c_void),
    callback:
        unsafe extern "C" fn(*mut c_void, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    update: unsafe extern "C" fn(*mut c_void) -> u64,
    render: unsafe extern "C" fn(*mut c_void, *mut Param) -> c_int,
    swapped: unsafe extern "C" fn(*mut c_void),
}
impl Api {
    fn load(library: &Library) -> io::Result<Self> {
        Ok(Self {
            create: load_symbol(library, b"mpv_render_context_create\0")?,
            free: load_symbol(library, b"mpv_render_context_free\0")?,
            callback: load_symbol(library, b"mpv_render_context_set_update_callback\0")?,
            update: load_symbol(library, b"mpv_render_context_update\0")?,
            render: load_symbol(library, b"mpv_render_context_render\0")?,
            swapped: load_symbol(library, b"mpv_render_context_report_swap\0")?,
        })
    }
}

struct State {
    stop: AtomicBool,
    failed: AtomicBool,
}

pub(super) struct RenderWorker {
    state: Arc<State>,
    thread: Option<JoinHandle<()>>,
    frames: mpsc::SyncSender<Submission>,
}
impl RenderWorker {
    pub fn start(library: &Library, handle: *mut MpvHandle, window: u32) -> io::Result<Self> {
        let api = Api::load(library)?;
        let state = Arc::new(State {
            stop: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        });
        let shared = Arc::clone(&state);
        let raw = handle as usize;
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (frames, frame_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("libmpv-render".into())
            .spawn(move || {
                let result = render_loop(api, raw, window, &shared, &ready_tx, &frame_rx);
                if let Err(error) = result {
                    shared.failed.store(true, Ordering::Release);
                    let _ = ready_tx.try_send(Err(error.to_string()));
                    tracing::error!(target: "mpv.render", "native rendering failed: {error}");
                }
            })?;
        let worker = Self {
            state,
            thread: Some(thread),
            frames,
        };
        ready_rx
            .recv()
            .map_err(io::Error::other)?
            .map_err(io::Error::other)?;
        Ok(worker)
    }
    pub fn submit(&self, frame: NativeOverlayFrame) -> Result<NativeOverlayFrame, String> {
        let (reply, received) = mpsc::sync_channel(1);
        self.frames
            .try_send(Submission { frame, reply })
            .map_err(|error| error.to_string())?;
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
        received
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())
    }
    pub fn failed(&self) -> bool {
        self.state.failed.load(Ordering::Acquire)
    }
}
impl Drop for RenderWorker {
    fn drop(&mut self) {
        self.state.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            if thread.join().is_err() {
                tracing::error!(target: "mpv.render", "render thread panicked");
            }
        }
    }
}
struct Submission {
    frame: NativeOverlayFrame,
    reply: mpsc::SyncSender<NativeOverlayFrame>,
}

struct Context {
    raw: *mut c_void,
    api: Api,
    _wake: Box<thread::Thread>,
}
impl Context {
    fn new(api: Api, handle: usize, gl: &GlWindow) -> io::Result<Self> {
        let mut init = Init {
            get_proc: get_proc_address,
            context: std::ptr::from_ref(gl).cast_mut().cast(),
        };
        let mut params = [
            Param {
                kind: 1,
                data: c"opengl".as_ptr().cast_mut().cast(),
            },
            Param::new(2, &mut init),
            Param {
                kind: 8,
                data: gl.xdisplay(),
            },
            Param::end(),
        ];
        let mut raw = std::ptr::null_mut();
        // SAFETY: the controller keeps the mpv core and library alive until
        // this thread has freed the render context and joined.
        let status =
            unsafe { (api.create)(&mut raw, handle as *mut MpvHandle, params.as_mut_ptr()) };
        if status < 0 {
            return Err(io::Error::other(format!(
                "creating mpv OpenGL renderer failed ({status})"
            )));
        }
        let mut wake = Box::new(thread::current());
        unsafe {
            (api.callback)(raw, Some(wakeup), std::ptr::from_mut(wake.as_mut()).cast());
        }
        Ok(Self {
            raw,
            api,
            _wake: wake,
        })
    }
    fn draw(&self, gl: &mut GlWindow, size: (u16, u16)) -> io::Result<()> {
        let mut fbo = Fbo {
            fbo: gl.target(size)? as c_int,
            width: i32::from(size.0),
            height: i32::from(size.1),
            format: glow::RGBA8 as c_int,
        };
        let mut flip = 1;
        let mut params = [
            Param::new(3, &mut fbo),
            Param::new(4, &mut flip),
            Param::end(),
        ];
        let status = unsafe { (self.api.render)(self.raw, params.as_mut_ptr()) };
        if status < 0 {
            return Err(io::Error::other(format!(
                "rendering video failed ({status})"
            )));
        }
        Ok(())
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            (self.api.callback)(self.raw, None, std::ptr::null_mut());
            (self.api.free)(self.raw);
        }
    }
}
unsafe extern "C" fn wakeup(context: *mut c_void) {
    // SAFETY: boxed thread handle stays valid until the callback is removed.
    unsafe {
        (&*context.cast::<thread::Thread>()).unpark();
    }
}
unsafe extern "C" fn get_proc_address(context: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: libmpv invokes this during context creation with a valid C name.
    let gl = unsafe { &*context.cast::<GlWindow>() };
    let name = unsafe { CStr::from_ptr(name) };
    gl.proc_address(name)
}
fn render_loop(
    api: Api,
    handle: usize,
    window: u32,
    state: &State,
    ready: &mpsc::SyncSender<Result<(), String>>,
    frames: &mpsc::Receiver<Submission>,
) -> io::Result<()> {
    let mut gl = GlWindow::new(window)?;
    let context = Context::new(api, handle, &gl)?;
    ready.send(Ok(())).map_err(io::Error::other)?;
    let mut previous = (0, 0);
    let mut size = (1280, 720);
    while !state.stop.load(Ordering::Acquire) {
        let submission = frames.try_recv().ok();
        if let Some(submission) = &submission {
            size = (submission.frame.width, submission.frame.height);
        }
        let updated = unsafe { (api.update)(context.raw) } & 1 != 0;
        if updated || submission.is_some() || size != previous {
            gl.begin_frame()?;
            if let Some(submission) = &submission {
                gl.upload_ui(&submission.frame);
            }
        }
        if updated || size != previous {
            context.draw(&mut gl, size)?;
            previous = size;
        }
        if updated || submission.is_some() {
            gl.present(size)?;
            if updated {
                unsafe {
                    (api.swapped)(context.raw);
                }
            }
        }
        if let Some(submission) = submission {
            let _ = submission.reply.send(submission.frame);
        }
        thread::park_timeout(Duration::from_millis(16));
    }
    Ok(())
}
