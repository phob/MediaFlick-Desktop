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

use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DispatchMessageW, GWLP_USERDATA, GetMessageW,
    GetWindowLongPtrW, HWND_MESSAGE, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    SMTO_ABORTIFHUNG, SMTO_NORMAL, SendMessageTimeoutW, SetWindowLongPtrW, TranslateMessage,
    WM_CLOSE, WM_COPYDATA, WM_DESTROY, WM_NCCREATE, WM_NCDESTROY, WNDCLASSW,
};

use super::protocol::{self, Inbound};

const SEND_TIMEOUT_MS: u32 = 60_000;
const SHUTDOWN_SEND_TIMEOUT_MS: u32 = 5_000;

struct WindowContext {
    inbound: Sender<Inbound>,
}

struct Outbound {
    command: u32,
    bytes: Vec<u8>,
}

impl Outbound {
    fn command(command: u32, payload: &str) -> Self {
        let wide = protocol::wide_payload(payload);
        let mut bytes = Vec::with_capacity(wide.len() * 2);
        for unit in wide {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        Self { command, bytes }
    }

    fn osd(position: i32, duration_ms: i32, message: &str) -> Self {
        Self {
            command: protocol::CMD_OSDSHOWMESSAGE,
            bytes: protocol::osd_message_bytes(position, duration_ms, message),
        }
    }
}

enum SenderMsg {
    Send(Outbound),
    Stop,
}

pub struct MpcHcTransport {
    our_hwnd: isize,
    target_hwnd: Arc<AtomicIsize>,
    sender_tx: Option<Sender<SenderMsg>>,
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

fn is_coalescable(command: u32) -> bool {
    matches!(
        command,
        protocol::CMD_SETPOSITION | protocol::CMD_GETCURRENTPOSITION
    )
}

fn coalesce(batch: &mut Vec<Outbound>) {
    if batch.len() < 2 {
        return;
    }
    let mut keep = Vec::with_capacity(batch.len());
    for (index, outbound) in batch.iter().enumerate() {
        let superseded = is_coalescable(outbound.command)
            && batch[index + 1..]
                .iter()
                .any(|later| later.command == outbound.command);
        keep.push(!superseded);
    }
    let mut index = 0;
    batch.retain(|_| {
        let retain = keep[index];
        index += 1;
        retain
    });
}

fn send_copydata(
    target: isize,
    our_hwnd: isize,
    command: u32,
    data: *const c_void,
    len: u32,
    timeout_ms: u32,
    flags: u32,
) -> bool {
    if target == 0 {
        tracing::warn!(
            target: "mpchc",
            command = format!("{command:#x}"),
            "no MPC-HC target window; command dropped (not connected)"
        );
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
            our_hwnd as WPARAM,
            &payload as *const COPYDATASTRUCT as LPARAM,
            flags,
            timeout_ms,
            &mut result,
        )
    };
    if sent == 0 {
        let error = unsafe { GetLastError() };
        tracing::warn!(
            target: "mpchc",
            command = format!("{command:#x}"),
            target_hwnd = format!("{:#x}", target as usize),
            last_error = error,
            timeout_ms,
            "WM_COPYDATA send failed; MPC-HC window did not service the message (hung or wrong target)"
        );
    }
    sent != 0
}

fn sender_loop(rx: Receiver<SenderMsg>, our_hwnd: isize, target: Arc<AtomicIsize>) {
    while let Ok(first) = rx.recv() {
        let mut batch = Vec::new();
        let mut stop = false;
        match first {
            SenderMsg::Send(outbound) => batch.push(outbound),
            SenderMsg::Stop => break,
        }
        loop {
            match rx.try_recv() {
                Ok(SenderMsg::Send(outbound)) => batch.push(outbound),
                Ok(SenderMsg::Stop) => {
                    stop = true;
                    break;
                }
                Err(_) => break,
            }
        }
        coalesce(&mut batch);
        for outbound in &batch {
            send_copydata(
                target.load(Ordering::SeqCst),
                our_hwnd,
                outbound.command,
                outbound.bytes.as_ptr() as *const c_void,
                outbound.bytes.len() as u32,
                SEND_TIMEOUT_MS,
                SMTO_NORMAL,
            );
        }
        if stop {
            break;
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

        let target_hwnd = Arc::new(AtomicIsize::new(0));
        let (sender_tx, sender_rx) = mpsc::channel();
        let sender_target = target_hwnd.clone();
        thread::Builder::new()
            .name("mpchc-sender".to_string())
            .spawn(move || sender_loop(sender_rx, our_hwnd, sender_target))?;

        Ok((
            Self {
                our_hwnd,
                target_hwnd,
                sender_tx: Some(sender_tx),
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
        self.enqueue(Outbound::command(command, payload))
    }

    pub fn send_osd(&self, position: i32, duration_ms: i32, message: &str) -> bool {
        self.enqueue(Outbound::osd(position, duration_ms, message))
    }

    fn enqueue(&self, outbound: Outbound) -> bool {
        match &self.sender_tx {
            Some(tx) => tx.send(SenderMsg::Send(outbound)).is_ok(),
            None => false,
        }
    }

    pub fn send_now(&self, command: u32, payload: &str) -> bool {
        let outbound = Outbound::command(command, payload);
        send_copydata(
            self.target_hwnd.load(Ordering::SeqCst),
            self.our_hwnd,
            command,
            outbound.bytes.as_ptr() as *const c_void,
            outbound.bytes.len() as u32,
            SHUTDOWN_SEND_TIMEOUT_MS,
            SMTO_ABORTIFHUNG,
        )
    }

    pub fn shutdown(&mut self) {
        if let Some(tx) = self.sender_tx.take() {
            let _ = tx.send(SenderMsg::Stop);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(command: u32) -> Outbound {
        Outbound {
            command,
            bytes: Vec::new(),
        }
    }

    fn commands(batch: &[Outbound]) -> Vec<u32> {
        batch.iter().map(|outbound| outbound.command).collect()
    }

    #[test]
    fn coalesce_keeps_only_latest_setposition() {
        let mut batch = vec![
            Outbound::command(protocol::CMD_SETPOSITION, "10.000"),
            Outbound::command(protocol::CMD_SETPOSITION, "20.000"),
            Outbound::command(protocol::CMD_SETPOSITION, "30.000"),
        ];
        coalesce(&mut batch);
        assert_eq!(commands(&batch), vec![protocol::CMD_SETPOSITION]);
        assert_eq!(batch[0].bytes, Outbound::command(0, "30.000").bytes);
    }

    #[test]
    fn coalesce_preserves_other_commands_and_order() {
        let mut batch = vec![
            cmd(protocol::CMD_SETPOSITION),
            cmd(protocol::CMD_PAUSE),
            cmd(protocol::CMD_SETPOSITION),
            cmd(protocol::CMD_PLAY),
        ];
        coalesce(&mut batch);
        assert_eq!(
            commands(&batch),
            vec![
                protocol::CMD_PAUSE,
                protocol::CMD_SETPOSITION,
                protocol::CMD_PLAY
            ]
        );
    }

    #[test]
    fn coalesce_collapses_position_polls() {
        let mut batch = vec![
            cmd(protocol::CMD_GETCURRENTPOSITION),
            cmd(protocol::CMD_GETCURRENTPOSITION),
            cmd(protocol::CMD_GETCURRENTPOSITION),
        ];
        coalesce(&mut batch);
        assert_eq!(commands(&batch), vec![protocol::CMD_GETCURRENTPOSITION]);
    }

    #[test]
    fn coalesce_leaves_singletons_untouched() {
        let mut batch = vec![cmd(protocol::CMD_PAUSE), cmd(protocol::CMD_SETPOSITION)];
        coalesce(&mut batch);
        assert_eq!(
            commands(&batch),
            vec![protocol::CMD_PAUSE, protocol::CMD_SETPOSITION]
        );
    }
}
