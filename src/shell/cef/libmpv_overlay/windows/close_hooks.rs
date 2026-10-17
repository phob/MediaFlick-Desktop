use std::io;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CWPRETSTRUCT, CallNextHookEx, GetWindowThreadProcessId, HHOOK, MSG, PostMessageW,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_CALLWNDPROCRET, WH_GETMESSAGE, WM_CLOSE,
};

use super::WM_HOST_CLOSE_REQUESTED;

pub(super) static HOOKED_HOST: AtomicUsize = AtomicUsize::new(0);
pub(super) static CLOSE_TARGET: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
pub(super) struct HostCloseHooks {
    dispatched: HHOOK,
    queued: HHOOK,
}

impl HostCloseHooks {
    pub(super) fn uninstall(self) {
        for hook in [self.dispatched, self.queued] {
            if !hook.is_null() {
                // SAFETY: `hook` came from SetWindowsHookExW and is unhooked once,
                // because `uninstall` consumes the pair that owns it.
                unsafe { UnhookWindowsHookEx(hook) };
            }
        }
    }
}

impl Default for HostCloseHooks {
    fn default() -> Self {
        Self {
            dispatched: null_mut(),
            queued: null_mut(),
        }
    }
}

pub(super) fn install_host_close_hooks(host: HWND, input: HWND) -> io::Result<HostCloseHooks> {
    // SAFETY: a null process-id pointer is allowed; only the thread id is read.
    let thread_id = unsafe { GetWindowThreadProcessId(host, null_mut()) };
    if thread_id == 0 {
        return Err(io::Error::last_os_error());
    }
    HOOKED_HOST.store(host as usize, Ordering::Release);
    CLOSE_TARGET.store(input as usize, Ordering::Release);
    // SAFETY: the hook procedure is a static function with the hook signature,
    // installed for one thread of this process, and removed by `uninstall`.
    let dispatched = unsafe {
        SetWindowsHookExW(
            WH_CALLWNDPROCRET,
            Some(dispatched_host_message_hook),
            null_mut(),
            thread_id,
        )
    };
    if dispatched.is_null() {
        CLOSE_TARGET.store(0, Ordering::Release);
        HOOKED_HOST.store(0, Ordering::Release);
        return Err(io::Error::last_os_error());
    }
    // SAFETY: as for the dispatched-message hook above.
    let queued = unsafe {
        SetWindowsHookExW(
            WH_GETMESSAGE,
            Some(queued_host_message_hook),
            null_mut(),
            thread_id,
        )
    };
    if queued.is_null() {
        HostCloseHooks { dispatched, queued }.uninstall();
        CLOSE_TARGET.store(0, Ordering::Release);
        HOOKED_HOST.store(0, Ordering::Release);
        return Err(io::Error::last_os_error());
    }
    Ok(HostCloseHooks { dispatched, queued })
}

unsafe extern "system" fn dispatched_host_message_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && lparam != 0 {
        // SAFETY: for a non-negative code, a WH_CALLWNDPROCRET hook receives a
        // valid CWPRETSTRUCT for the duration of the call.
        let message = unsafe { &*(lparam as *const CWPRETSTRUCT) };
        forward_host_close(message.hwnd, message.message);
    }
    // SAFETY: passes the hook arguments through unchanged, as required.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn queued_host_message_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && lparam != 0 {
        // SAFETY: for a non-negative code, a WH_GETMESSAGE hook receives a
        // valid MSG for the duration of the call.
        let message = unsafe { &*(lparam as *const MSG) };
        forward_host_close(message.hwnd, message.message);
    }
    // SAFETY: passes the hook arguments through unchanged, as required.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

fn forward_host_close(window: HWND, message: u32) {
    let host = HOOKED_HOST.load(Ordering::Acquire) as HWND;
    if host.is_null() || window != host || message != WM_CLOSE {
        return;
    }
    let input = CLOSE_TARGET.load(Ordering::Acquire) as HWND;
    if !input.is_null() {
        // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
        unsafe { PostMessageW(input, WM_HOST_CLOSE_REQUESTED, 0, 0) };
    }
}
