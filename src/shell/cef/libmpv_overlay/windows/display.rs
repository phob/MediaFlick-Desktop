use std::mem::size_of;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW, GetMonitorInfoW,
    MONITOR_DEFAULTTONULL, MONITORINFO, MONITORINFOEXW, MonitorFromWindow,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, IsIconic, IsZoomed, SW_MAXIMIZE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOZORDER, SetWindowPos, ShowWindow,
};

use crate::preferences::WebUiWindowSettings;

use super::{DEFAULT_DPI, ViewMetrics};

pub(super) fn apply_saved_window_settings(host: HWND, settings: WebUiWindowSettings) {
    // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
    let dpi = unsafe { GetDpiForWindow(host) }.max(DEFAULT_DPI);
    let (width, height) = settings.size();
    let width = logical_to_physical(width, dpi).max(1);
    let height = logical_to_physical(height, dpi).max(1);
    let (x, y, flags) = settings.position().map_or(
        (0, 0, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE),
        |(x, y)| {
            (
                logical_to_physical(x, dpi),
                logical_to_physical(y, dpi),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        },
    );
    // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
    unsafe {
        SetWindowPos(host, null_mut(), x, y, width, height, flags);
        if settings.maximized {
            ShowWindow(host, SW_MAXIMIZE);
        }
    }
}

pub(super) fn sample_window_settings(
    host: HWND,
    mut settings: WebUiWindowSettings,
) -> Option<WebUiWindowSettings> {
    // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
    if unsafe { IsIconic(host) } != 0 {
        return None;
    }
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live local the call writes.
    if unsafe { GetWindowRect(host, &raw mut rect) } == 0 {
        return None;
    }
    // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
    let maximized = unsafe { IsZoomed(host) } != 0;
    if !maximized && window_fills_monitor(host, &rect) {
        return None;
    }
    // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
    let dpi = unsafe { GetDpiForWindow(host) }.max(DEFAULT_DPI);
    settings.record_bounds(
        physical_to_logical(rect.left, dpi),
        physical_to_logical(rect.top, dpi),
        physical_to_logical(rect.right - rect.left, dpi),
        physical_to_logical(rect.bottom - rect.top, dpi),
        maximized,
    );
    Some(settings)
}

pub(super) fn display_frame_rate(host: HWND) -> i32 {
    // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
    let monitor = unsafe { MonitorFromWindow(host, MONITOR_DEFAULTTONULL) };
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..DEVMODEW::default()
    };
    if monitor.is_null()
        // SAFETY: `info` is a live MONITORINFOEXW with cbSize set, so the call
        // may write the extended layout.
        || unsafe { GetMonitorInfoW(monitor, &raw mut info.monitorInfo) } == 0
        // SAFETY: `mode` is a live DEVMODEW with dmSize set, and `szDevice` is
        // the NUL-terminated device name the previous call filled in.
        || unsafe {
            EnumDisplaySettingsW(info.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &raw mut mode)
        } == 0
    {
        return 60;
    }
    usable_frame_rate(mode.dmDisplayFrequency)
}

fn usable_frame_rate(frequency: u32) -> i32 {
    // Windows uses 0 or 1 for an unspecified/default refresh rate.
    if frequency <= 1 {
        60
    } else {
        frequency.min(240) as i32
    }
}

fn window_fills_monitor(host: HWND, window_rect: &RECT) -> bool {
    // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
    let monitor = unsafe { MonitorFromWindow(host, MONITOR_DEFAULTTONULL) };
    if monitor.is_null() {
        return false;
    }
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..MONITORINFO::default()
    };
    // SAFETY: `info` is a live MONITORINFO with cbSize set.
    if unsafe { GetMonitorInfoW(monitor, &raw mut info) } == 0 {
        return false;
    }
    window_rect.left == info.rcMonitor.left
        && window_rect.top == info.rcMonitor.top
        && window_rect.right == info.rcMonitor.right
        && window_rect.bottom == info.rcMonitor.bottom
}

pub(super) fn sample_metrics(host: HWND) -> Option<ViewMetrics> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live local the call writes.
    if unsafe { GetClientRect(host, &raw mut rect) } == 0 {
        return None;
    }
    let physical_width = rect.right - rect.left;
    let physical_height = rect.bottom - rect.top;
    if physical_width <= 0 || physical_height <= 0 {
        return None;
    }
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY: `origin` is a live local the call converts in place.
    if unsafe { ClientToScreen(host, &raw mut origin) } == 0 {
        return None;
    }
    // SAFETY: Win32 validates the window handle and fails the call for a stale one; no Rust memory is involved.
    let dpi = unsafe { GetDpiForWindow(host) }.max(DEFAULT_DPI);
    Some(ViewMetrics {
        logical_width: physical_to_logical(physical_width, dpi).max(1),
        logical_height: physical_to_logical(physical_height, dpi).max(1),
        physical_width,
        physical_height,
        screen_x: origin.x,
        screen_y: origin.y,
        dpi,
    })
}

pub(super) fn physical_to_logical(value: i32, dpi: u32) -> i32 {
    let dpi = i64::from(dpi.max(1));
    ((i64::from(value) * i64::from(DEFAULT_DPI) + dpi / 2) / dpi) as i32
}

pub(super) fn logical_to_physical(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * i64::from(dpi) + i64::from(DEFAULT_DPI) / 2) / i64::from(DEFAULT_DPI))
        as i32
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_unspecified_display_refresh_falls_back_to_60_hz() {
        for reported in [0, 1] {
            assert_eq!(super::usable_frame_rate(reported), 60);
        }
    }
}
