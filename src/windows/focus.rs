#[cfg(target_os = "windows")]
pub fn focus_process_window(process_id: u32) -> bool {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, IsWindowVisible,
    };
    use windows_sys::core::BOOL;

    struct SearchState {
        process_id: u32,
        hwnd: HWND,
    }

    unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &mut *(lparam as *mut SearchState) };
        let mut window_process_id = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut window_process_id);
        }
        if window_process_id == state.process_id
            && unsafe { IsWindowVisible(hwnd) } != 0
            && unsafe { GetWindow(hwnd, GW_OWNER) }.is_null()
        {
            state.hwnd = hwnd;
            return 0;
        }
        1
    }

    let mut state = SearchState {
        process_id,
        hwnd: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(Some(enum_window), &mut state as *mut SearchState as LPARAM);
    }
    if state.hwnd.is_null() {
        return false;
    }

    unsafe { focus_window(state.hwnd) }
}

#[cfg(target_os = "windows")]
unsafe fn focus_window(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetActiveWindow, SetFocus};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, SW_RESTORE,
        SW_SHOW, SetForegroundWindow, ShowWindow,
    };

    let current_thread = unsafe { GetCurrentThreadId() };
    let mut target_process_id = 0;
    let target_thread = unsafe { GetWindowThreadProcessId(hwnd, &mut target_process_id) };
    let foreground = unsafe { GetForegroundWindow() };
    let foreground_thread = if foreground.is_null() {
        0
    } else {
        unsafe { GetWindowThreadProcessId(foreground, std::ptr::null_mut()) }
    };

    let attached_foreground = foreground_thread != 0
        && foreground_thread != current_thread
        && unsafe { AttachThreadInput(current_thread, foreground_thread, 1) } != 0;
    let attached_target = target_thread != 0
        && target_thread != current_thread
        && unsafe { AttachThreadInput(current_thread, target_thread, 1) } != 0;

    if unsafe { IsIconic(hwnd) } != 0 {
        unsafe { ShowWindow(hwnd, SW_RESTORE) };
    } else {
        unsafe { ShowWindow(hwnd, SW_SHOW) };
    }
    unsafe {
        BringWindowToTop(hwnd);
        SetActiveWindow(hwnd);
        SetFocus(hwnd);
    }
    let foreground_set = unsafe { SetForegroundWindow(hwnd) } != 0;

    if attached_target {
        unsafe { AttachThreadInput(current_thread, target_thread, 0) };
    }
    if attached_foreground {
        unsafe { AttachThreadInput(current_thread, foreground_thread, 0) };
    }

    foreground_set || unsafe { GetForegroundWindow() } == hwnd
}

#[cfg(not(target_os = "windows"))]
pub fn focus_process_window(_process_id: u32) -> bool {
    true
}
