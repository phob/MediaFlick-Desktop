#[cfg(target_os = "windows")]
pub fn set_window_icon(window: &cef::Window) {
    use cef::ImplWindow;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON,
        SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
    };

    // build.rs embeds resources/win/app.ico in the executable with
    // winresource's default numeric ID, so load the same icon from the host
    // executable instead of from CEF/libcef.
    const APPLICATION_ICON_RESOURCE_ID: u16 = 1;

    let hwnd = window.window_handle().0.cast::<core::ffi::c_void>();
    if hwnd.is_null() {
        return;
    }

    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        if module.is_null() {
            tracing::warn!(target: "cef", "failed to get executable module handle for window icon");
            return;
        }

        let icon_resource = APPLICATION_ICON_RESOURCE_ID as usize as windows_sys::core::PCWSTR;
        let big_icon = LoadImageW(
            module,
            icon_resource,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXICON),
            GetSystemMetrics(SM_CYICON),
            LR_SHARED,
        );
        let small_icon = LoadImageW(
            module,
            icon_resource,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_SHARED,
        );

        if !big_icon.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big_icon as isize);
        }
        if !small_icon.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small_icon as isize);
        }
        if big_icon.is_null() && small_icon.is_null() {
            tracing::warn!(target: "cef", "failed to load embedded window icon resource");
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn set_window_icon(_window: &cef::Window) {}
