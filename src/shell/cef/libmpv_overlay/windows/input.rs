use std::mem::size_of;
use std::ptr::null_mut;

use cef::*;
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_CAPITAL, VK_CONTROL, VK_F4,
    VK_LWIN, VK_MENU, VK_NUMLOCK, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDC_APPSTARTING, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_HELP, IDC_IBEAM, IDC_NO, IDC_SIZEALL,
    IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IDC_WAIT, LoadCursorW, SetCursor, WM_CHAR,
    WM_KEYUP, WM_LBUTTONDBLCLK, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_RBUTTONDBLCLK,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONUP, XBUTTON1,
    XBUTTON2,
};
use windows_sys::core::PCWSTR;

pub(super) fn navigation_mouse_button(message: u32, wparam: WPARAM) -> Option<bool> {
    if message != WM_XBUTTONUP {
        return None;
    }
    match ((wparam >> 16) & 0xffff) as u16 {
        XBUTTON1 => Some(true),
        XBUTTON2 => Some(false),
        _ => None,
    }
}

pub(super) fn track_mouse_leave(window: HWND) {
    let mut event = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: window,
        dwHoverTime: 0,
    };
    // SAFETY: `event` is a live TRACKMOUSEEVENT with cbSize set.
    unsafe { TrackMouseEvent(&raw mut event) };
}

pub(super) fn apply_cursor(cursor: CursorType) {
    let Some(resource) = cursor_resource(cursor) else {
        // SAFETY: a null cursor hides the pointer.
        unsafe { SetCursor(null_mut()) };
        return;
    };
    // SAFETY: `resource` is one of the stock IDC_* cursor identifiers.
    let handle = unsafe { LoadCursorW(null_mut(), resource) };
    // SAFETY: `handle` is a shared stock cursor, or null, which hides it.
    unsafe { SetCursor(handle) };
}

fn cursor_resource(cursor: CursorType) -> Option<PCWSTR> {
    let resource = if cursor == CursorType::NONE {
        return None;
    } else if cursor == CursorType::CROSS {
        IDC_CROSS
    } else if matches!(
        cursor,
        CursorType::HAND | CursorType::GRAB | CursorType::GRABBING
    ) {
        IDC_HAND
    } else if matches!(cursor, CursorType::IBEAM | CursorType::VERTICALTEXT) {
        IDC_IBEAM
    } else if cursor == CursorType::WAIT {
        IDC_WAIT
    } else if cursor == CursorType::HELP {
        IDC_HELP
    } else if matches!(
        cursor,
        CursorType::EASTRESIZE
            | CursorType::WESTRESIZE
            | CursorType::EASTWESTRESIZE
            | CursorType::COLUMNRESIZE
    ) {
        IDC_SIZEWE
    } else if matches!(
        cursor,
        CursorType::NORTHRESIZE
            | CursorType::SOUTHRESIZE
            | CursorType::NORTHSOUTHRESIZE
            | CursorType::ROWRESIZE
    ) {
        IDC_SIZENS
    } else if matches!(
        cursor,
        CursorType::NORTHEASTRESIZE
            | CursorType::SOUTHWESTRESIZE
            | CursorType::NORTHEASTSOUTHWESTRESIZE
    ) {
        IDC_SIZENESW
    } else if matches!(
        cursor,
        CursorType::NORTHWESTRESIZE
            | CursorType::SOUTHEASTRESIZE
            | CursorType::NORTHWESTSOUTHEASTRESIZE
    ) {
        IDC_SIZENWSE
    } else if matches!(
        cursor,
        CursorType::MOVE
            | CursorType::MIDDLEPANNING
            | CursorType::MIDDLE_PANNING_VERTICAL
            | CursorType::MIDDLE_PANNING_HORIZONTAL
    ) {
        IDC_SIZEALL
    } else if cursor == CursorType::PROGRESS {
        IDC_APPSTARTING
    } else if matches!(cursor, CursorType::NODROP | CursorType::NOTALLOWED) {
        IDC_NO
    } else {
        IDC_ARROW
    };
    Some(resource)
}

pub(super) fn is_alt_f4(wparam: WPARAM) -> bool {
    wparam == usize::from(VK_F4)
}

pub(super) fn mouse_modifiers(wparam: WPARAM) -> u32 {
    let flags = wparam as u32;
    let mut modifiers = 0;
    if flags & 0x0001 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_LEFT_MOUSE_BUTTON.0 as u32;
    }
    if flags & 0x0010 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_MIDDLE_MOUSE_BUTTON.0 as u32;
    }
    if flags & 0x0002 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_RIGHT_MOUSE_BUTTON.0 as u32;
    }
    if flags & 0x0004 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_SHIFT_DOWN.0 as u32;
    }
    if flags & 0x0008 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0 as u32;
    }
    if key_down(VK_MENU) {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_ALT_DOWN.0 as u32;
    }
    modifiers
}

fn keyboard_modifiers(lparam: LPARAM) -> u32 {
    let mut modifiers = 0;
    if key_down(VK_SHIFT) {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_SHIFT_DOWN.0 as u32;
    }
    if key_down(VK_CONTROL) {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0 as u32;
    }
    if key_down(VK_MENU) {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_ALT_DOWN.0 as u32;
    }
    if key_down(VK_LWIN) || key_down(VK_RWIN) {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_COMMAND_DOWN.0 as u32;
    }
    // SAFETY: GetKeyState only reads this thread's keyboard state.
    if unsafe { GetKeyState(VK_NUMLOCK as i32) } & 1 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_NUM_LOCK_ON.0 as u32;
    }
    // SAFETY: GetKeyState only reads this thread's keyboard state.
    if unsafe { GetKeyState(VK_CAPITAL as i32) } & 1 != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_CAPS_LOCK_ON.0 as u32;
    }
    if (lparam as usize) & (1 << 30) != 0 {
        modifiers |= sys::cef_event_flags_t::EVENTFLAG_IS_REPEAT.0 as u32;
    }
    modifiers
}

pub(super) fn key_event(message: u32, wparam: WPARAM, lparam: LPARAM) -> KeyEvent {
    let type_ = match message {
        WM_KEYUP | WM_SYSKEYUP => KeyEventType::KEYUP,
        WM_CHAR | WM_SYSCHAR => KeyEventType::CHAR,
        _ => KeyEventType::RAWKEYDOWN,
    };
    let character = if matches!(message, WM_CHAR | WM_SYSCHAR) {
        wparam as u16
    } else {
        0
    };
    KeyEvent {
        type_,
        modifiers: keyboard_modifiers(lparam),
        windows_key_code: wparam as i32,
        native_key_code: lparam as i32,
        is_system_key: i32::from(matches!(message, WM_SYSKEYDOWN | WM_SYSKEYUP | WM_SYSCHAR)),
        character,
        unmodified_character: character,
        ..KeyEvent::default()
    }
}

pub(super) fn right_button_pause(message: u32, paused: bool) -> Option<bool> {
    (message == WM_RBUTTONDOWN).then_some(!paused)
}

pub(super) fn mouse_button(message: u32) -> (MouseButtonType, i32) {
    let button = match message {
        WM_RBUTTONDOWN | WM_RBUTTONUP | WM_RBUTTONDBLCLK => MouseButtonType::RIGHT,
        WM_MBUTTONDOWN | WM_MBUTTONUP | WM_MBUTTONDBLCLK => MouseButtonType::MIDDLE,
        _ => MouseButtonType::LEFT,
    };
    let clicks = i32::from(matches!(
        message,
        WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK
    )) + 1;
    (button, clicks)
}

fn key_down(key: u16) -> bool {
    // SAFETY: GetKeyState only reads this thread's keyboard state.
    (unsafe { GetKeyState(key as i32) }) < 0
}

pub(super) fn signed_low_word(value: isize) -> i32 {
    (value as u16 as i16) as i32
}

pub(super) fn low_word(value: isize) -> u32 {
    u32::from(value as u16)
}

pub(super) fn signed_high_word(value: isize) -> i32 {
    ((value >> 16) as u16 as i16) as i32
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WM_LBUTTONUP, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN,
    };

    use super::*;

    #[test]
    fn navigation_mouse_buttons_act_only_on_release() {
        let back = (XBUTTON1 as WPARAM) << 16;
        let forward = (XBUTTON2 as WPARAM) << 16;
        assert_eq!(navigation_mouse_button(WM_XBUTTONUP, back), Some(true));
        assert_eq!(navigation_mouse_button(WM_XBUTTONUP, forward), Some(false));
        assert_eq!(navigation_mouse_button(WM_XBUTTONDOWN, back), None);
        assert_eq!(navigation_mouse_button(WM_XBUTTONDBLCLK, back), None);
        assert_eq!(navigation_mouse_button(WM_LBUTTONUP, back), None);
        assert_eq!(navigation_mouse_button(WM_XBUTTONUP, 0), None);
    }

    #[test]
    fn right_button_toggles_playback_pause() {
        assert_eq!(right_button_pause(WM_RBUTTONDOWN, false), Some(true));
        assert_eq!(right_button_pause(WM_RBUTTONDOWN, true), Some(false));
        assert_eq!(right_button_pause(WM_RBUTTONUP, false), None);
    }
}
