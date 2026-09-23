//! gpu-next owns the video surface and composites CEF's bitmap on its GPU.
//! These synchronous client calls run on the existing controller thread. mpv
//! copies the validated bitmap before returning; no borrowed pixels escape.
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io;
use std::thread;
use std::time::{Duration, Instant};

use libloading::Library;

use super::{MpvErrorString, MpvHandle, load_symbol, mpv_error};
use crate::playback::NativeOverlayFrame;

const FORMAT_STRING: c_int = 1;
const FORMAT_NODE_MAP: c_int = 8;

// The public mpv_node ABI (client.h). All union alternatives have pointer or
// 64-bit scalar size/alignment on supported Linux targets.
#[repr(C)]
union NodeData {
    string: *mut c_char,
    list: *mut NodeList,
    integer: i64,
}

#[cfg(test)]
#[path = "gpu_next/tests.rs"]
mod tests;

#[repr(C)]
struct Node {
    data: NodeData,
    format: c_int,
}

#[repr(C)]
struct NodeList {
    count: c_int,
    values: *mut Node,
    keys: *mut *mut c_char,
}

pub(super) struct Compositor {
    command: unsafe extern "C" fn(*mut MpvHandle, *mut Node, *mut Node) -> c_int,
    property: unsafe extern "C" fn(*mut MpvHandle, *const c_char) -> *mut c_char,
    free: unsafe extern "C" fn(*mut c_void),
    error: MpvErrorString,
}

impl Compositor {
    pub fn new(library: &Library, handle: *mut MpvHandle) -> io::Result<Self> {
        let compositor = Self {
            command: load_symbol(library, b"mpv_command_node\0")?,
            property: load_symbol(library, b"mpv_get_property_string\0")?,
            free: load_symbol(library, b"mpv_free\0")?,
            error: load_symbol(library, b"mpv_error_string\0")?,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while compositor.property(handle, c"current-vo") != "gpu-next" {
            if Instant::now() >= deadline {
                return Err(io::Error::other(
                    "libmpv could not initialize gpu-next on X11",
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
        tracing::info!(target: "mpv.render", context = compositor.property(handle, c"current-gpu-context"), "initialized gpu-next video and browser compositor");
        Ok(compositor)
    }

    fn property(&self, handle: *mut MpvHandle, name: &CStr) -> String {
        // SAFETY: the runtime owns the handle/library throughout this call.
        // mpv allocates the returned string, which is released using mpv_free.
        let raw = unsafe { (self.property)(handle, name.as_ptr()) };
        if raw.is_null() {
            return String::new();
        }
        // SAFETY: a non-null result is a NUL-terminated string owned by mpv.
        let result = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: `raw` came from mpv_get_property_string, is freed once, and
        // `result` owns a copy of its contents.
        unsafe { (self.free)(raw.cast()) };
        result
    }

    pub fn submit(
        &self,
        handle: *mut MpvHandle,
        frame: &NativeOverlayFrame,
        display: (u16, u16),
    ) -> Result<(), String> {
        frame.validate()?;
        if display.0 == 0 || display.1 == 0 {
            return Err("native overlay display dimensions are empty".into());
        }
        // Use named arguments: overlay-add's positional argument order is not
        // part of its compatibility contract. The address stays private to
        // libmpv, and the synchronous command copies all pixels before return.
        let args = [
            ("name", "overlay-add".to_string()),
            ("id", "0".to_string()),
            ("x", "0".to_string()),
            ("y", "0".to_string()),
            ("file", format!("&{}", frame.pixels.as_ptr() as usize)),
            ("offset", "0".to_string()),
            ("fmt", "bgra".to_string()),
            ("w", frame.width.to_string()),
            ("h", frame.height.to_string()),
            ("stride", (u32::from(frame.width) * 4).to_string()),
            ("dw", display.0.to_string()),
            ("dh", display.1.to_string()),
        ];
        self.command(handle, &args)
            .map_err(|error| error.to_string())
    }

    fn command(&self, handle: *mut MpvHandle, args: &[(&str, String)]) -> io::Result<()> {
        let strings = args
            .iter()
            .map(|(key, value)| Ok((CString::new(*key)?, CString::new(value.as_str())?)))
            .collect::<Result<Vec<_>, std::ffi::NulError>>()
            .map_err(io::Error::other)?;
        let mut keys: Vec<_> = strings.iter().map(|(k, _)| k.as_ptr().cast_mut()).collect();
        let mut values: Vec<_> = strings
            .iter()
            .map(|(_, v)| Node {
                data: NodeData {
                    string: v.as_ptr().cast_mut(),
                },
                format: FORMAT_STRING,
            })
            .collect();
        let mut list = NodeList {
            count: c_int::try_from(values.len()).map_err(io::Error::other)?,
            values: values.as_mut_ptr(),
            keys: keys.as_mut_ptr(),
        };
        let mut node = Node {
            data: NodeData {
                list: &raw mut list,
            },
            format: FORMAT_NODE_MAP,
        };
        // SAFETY: nodes, strings, and the bitmap borrowed by submit remain
        // valid until this synchronous call finishes. mpv does not retain them.
        let status = unsafe { (self.command)(handle, &raw mut node, std::ptr::null_mut()) };
        if status < 0 {
            return Err(io::Error::other(format!(
                "libmpv UI composition failed: {}",
                mpv_error(self.error, status)
            )));
        }
        Ok(())
    }
}
