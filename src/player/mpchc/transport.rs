//! Hidden message-pump window that drives MPC-HC over `WM_COPYDATA` slave mode.
//!
//! MPC-HC is launched once with `/slave <hwnd>` pointing at this window. It
//! replies `CMD_CONNECT` with its own HWND, after which every command is sent
//! as `WM_COPYDATA` and every notification is received the same way.

use std::ffi::c_void;
use std::io;
use std::ptr;
use std::sync::Arc;
use std::sync::Once;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DispatchMessageW, GWLP_USERDATA, GetMessageW,
    GetWindowLongPtrW, HWND_MESSAGE, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    SMTO_ABORTIFHUNG, SendMessageTimeoutW, SetWindowLongPtrW, TranslateMessage, WM_CLOSE,
    WM_COPYDATA, WM_DESTROY, WM_NCCREATE, WM_NCDESTROY, WNDCLASSW,
};

use super::protocol::{self, Inbound};

const SEND_TIMEOUT_MS: u32 = 5000;

struct WindowContext {
    inbound: Sender<Inbound>,
}

pub struct MpcHcTransport {
    our_hwnd: isize,
    target_hwnd: Arc<AtomicIsize>,
    pump: Option<JoinHandle<()>>,
}

fn class_name() -> Vec<u16> {
    "MediaFlickDesktopMpcHcSlave\0".encode_utf16().collect()
}

static REGISTER_CLASS: Once = Once::new();

unsafe fn register_class() {
    REGISTER_CLASS.call_once(|| unsafe {
        let mut class: WNDCLASSW = std::mem::zeroed();
        class.lpfnWndProc = Some(wndproc);
        class.hInstance = GetModuleHandleW(ptr::null());
        let name = class_name();
        class.lpszClassName = name.as_ptr();
        RegisterClassW(&class);
    });
}

unsafe fn wide_to_string(ptr: *const u16, byte_len: usize) -> String {
    if ptr.is_null() || byte_len < 2 {
        return String::new();
    }
    let len = byte_len / 2;
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    let end = slice.iter().position(|&unit| unit == 0).unwrap_or(len);
    String::from_utf16_lossy(&slice[..end])
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCREATE => {
                let create = lparam as *const CREATESTRUCTW;
                if !create.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_COPYDATA => {
                let context = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowContext;
                let data = lparam as *const COPYDATASTRUCT;
                if !context.is_null() && !data.is_null() {
                    let command = (*data).dwData as u32;
                    let text =
                        wide_to_string((*data).lpData as *const u16, (*data).cbData as usize);
                    let _ = (*context)
                        .inbound
                        .send(protocol::parse_inbound(command, &text));
                }
                1
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            WM_NCDESTROY => {
                let context = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
                if context != 0 {
                    drop(Box::from_raw(context as *mut WindowContext));
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

impl MpcHcTransport {
    pub fn spawn() -> io::Result<(Self, Receiver<Inbound>)> {
        let (inbound_tx, inbound_rx) = mpsc::channel();
        let (hwnd_tx, hwnd_rx) = mpsc::channel();

        let pump = thread::Builder::new()
            .name("mpchc-pump".to_string())
            .spawn(move || unsafe {
                register_class();
                let context = Box::into_raw(Box::new(WindowContext {
                    inbound: inbound_tx,
                }));
                let name = class_name();
                let hwnd = CreateWindowExW(
                    0,
                    name.as_ptr(),
                    name.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    ptr::null_mut(),
                    GetModuleHandleW(ptr::null()),
                    context as *const c_void,
                );
                if hwnd.is_null() {
                    drop(Box::from_raw(context));
                    let _ = hwnd_tx.send(0isize);
                    return;
                }
                let _ = hwnd_tx.send(hwnd as isize);

                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            })?;

        let our_hwnd = hwnd_rx.recv().unwrap_or(0);
        if our_hwnd == 0 {
            let _ = pump.join();
            return Err(io::Error::other(
                "failed to create MPC-HC slave message window",
            ));
        }

        Ok((
            Self {
                our_hwnd,
                target_hwnd: Arc::new(AtomicIsize::new(0)),
                pump: Some(pump),
            },
            inbound_rx,
        ))
    }

    pub fn our_hwnd_arg(&self) -> String {
        (self.our_hwnd as usize).to_string()
    }

    pub fn set_target(&self, hwnd: isize) {
        self.target_hwnd.store(hwnd, Ordering::SeqCst);
    }

    pub fn clear_target(&self) {
        self.target_hwnd.store(0, Ordering::SeqCst);
    }

    pub fn send_command(&self, command: u32, payload: &str) -> bool {
        let wide = protocol::wide_payload(payload);
        self.send_copydata(
            command,
            wide.as_ptr() as *const c_void,
            (wide.len() * 2) as u32,
        )
    }

    pub fn send_osd(&self, position: i32, duration_ms: i32, message: &str) -> bool {
        let bytes = protocol::osd_message_bytes(position, duration_ms, message);
        self.send_copydata(
            protocol::CMD_OSDSHOWMESSAGE,
            bytes.as_ptr() as *const c_void,
            bytes.len() as u32,
        )
    }

    fn send_copydata(&self, command: u32, data: *const c_void, len: u32) -> bool {
        let target = self.target_hwnd.load(Ordering::SeqCst);
        if target == 0 {
            return false;
        }
        let payload = COPYDATASTRUCT {
            dwData: command as usize,
            cbData: len,
            lpData: data as *mut c_void,
        };
        let mut result: usize = 0;
        let sent = unsafe {
            SendMessageTimeoutW(
                target as HWND,
                WM_COPYDATA,
                self.our_hwnd as WPARAM,
                &payload as *const COPYDATASTRUCT as LPARAM,
                SMTO_ABORTIFHUNG,
                SEND_TIMEOUT_MS,
                &mut result,
            )
        };
        sent != 0
    }

    pub fn shutdown(&mut self) {
        if self.our_hwnd != 0 {
            unsafe {
                PostMessageW(self.our_hwnd as HWND, WM_CLOSE, 0, 0);
            }
        }
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
        self.our_hwnd = 0;
    }
}

impl Drop for MpcHcTransport {
    fn drop(&mut self) {
        self.shutdown();
    }
}
