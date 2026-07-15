#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HWND, LPARAM};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SW_RESTORE, SetForegroundWindow, ShowWindow,
};
#[cfg(target_os = "windows")]
use windows_sys::core::BOOL;

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessWindowActivation {
    Activated,
    WindowNotFound,
    Denied,
}

#[cfg(target_os = "windows")]
struct WindowSearch {
    process_id: u32,
    window: HWND,
}

/// Activates the visible top-level window owned by `process_id` without
/// changing its normal/maximized state. A minimized window is restored first.
#[cfg(target_os = "windows")]
pub fn activate_process_window(process_id: u32) -> ProcessWindowActivation {
    let mut search = WindowSearch {
        process_id,
        window: std::ptr::null_mut(),
    };
    // SAFETY: `search` remains alive and exclusively borrowed until EnumWindows
    // returns; the callback only reads/writes that value on this thread.
    unsafe {
        EnumWindows(
            Some(find_process_window),
            std::ptr::from_mut(&mut search) as LPARAM,
        );
    }

    let window = search.window;
    if window.is_null() {
        return ProcessWindowActivation::WindowNotFound;
    }

    // SAFETY: EnumWindows supplied a live top-level HWND. Win32 APIs tolerate
    // the window disappearing between calls by returning failure.
    unsafe {
        if GetForegroundWindow() == window {
            return ProcessWindowActivation::Activated;
        }
        if IsIconic(window) != 0 {
            ShowWindow(window, SW_RESTORE);
        }
        SetForegroundWindow(window);
        if GetForegroundWindow() == window {
            ProcessWindowActivation::Activated
        } else {
            ProcessWindowActivation::Denied
        }
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn find_process_window(window: HWND, context: LPARAM) -> BOOL {
    // SAFETY: activate_process_window passes a valid WindowSearch pointer for
    // the synchronous lifetime of EnumWindows.
    let search = unsafe { &mut *(context as *mut WindowSearch) };
    if unsafe { IsWindowVisible(window) } == 0 {
        return 1;
    }
    let mut process_id = 0;
    unsafe {
        GetWindowThreadProcessId(window, &mut process_id);
    }
    if process_id != search.process_id {
        return 1;
    }
    search.window = window;
    0
}
