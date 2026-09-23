use cef::{BrowserHost, ImplBrowserHost, KeyEvent, KeyEventType, sys::cef_event_flags_t};
use x11rb::protocol::xproto::{ConnectionExt, KeyButMask, KeyPressEvent};
use x11rb::rust_connection::RustConnection;

pub(super) fn enable_detectable_repeat(conn: &RustConnection) -> std::io::Result<()> {
    use x11rb::protocol::xkb;
    let extension = xkb::use_extension(conn, 1, 0)
        .map_err(std::io::Error::other)?
        .reply()
        .map_err(std::io::Error::other)?;
    if !extension.supported {
        return Err(std::io::Error::other(
            "X11 keyboard extension is unavailable",
        ));
    }
    xkb::per_client_flags(
        conn,
        xkb::ID::USE_CORE_KBD.into(),
        xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
        xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
        xkb::BoolCtrl::default(),
        xkb::BoolCtrl::default(),
        xkb::BoolCtrl::default(),
    )
    .map_err(std::io::Error::other)?
    .reply()
    .map_err(std::io::Error::other)?;
    Ok(())
}

pub(super) fn modifiers(state: KeyButMask) -> u32 {
    let mut flags = 0;
    for (mask, flag) in [
        (KeyButMask::SHIFT, cef_event_flags_t::EVENTFLAG_SHIFT_DOWN),
        (KeyButMask::LOCK, cef_event_flags_t::EVENTFLAG_CAPS_LOCK_ON),
        (
            KeyButMask::CONTROL,
            cef_event_flags_t::EVENTFLAG_CONTROL_DOWN,
        ),
        (KeyButMask::MOD1, cef_event_flags_t::EVENTFLAG_ALT_DOWN),
        (KeyButMask::MOD4, cef_event_flags_t::EVENTFLAG_COMMAND_DOWN),
        (
            KeyButMask::BUTTON1,
            cef_event_flags_t::EVENTFLAG_LEFT_MOUSE_BUTTON,
        ),
        (
            KeyButMask::BUTTON2,
            cef_event_flags_t::EVENTFLAG_MIDDLE_MOUSE_BUTTON,
        ),
        (
            KeyButMask::BUTTON3,
            cef_event_flags_t::EVENTFLAG_RIGHT_MOUSE_BUTTON,
        ),
    ] {
        if state.contains(mask) {
            flags |= flag.0;
        }
    }
    flags
}

pub(super) fn keysym(conn: &RustConnection, event: &KeyPressEvent) -> Option<(u32, u32)> {
    let map = conn
        .get_keyboard_mapping(event.detail, 1)
        .ok()?
        .reply()
        .ok()?;
    let base = *map.keysyms.first()?;
    let shift = event.state.contains(KeyButMask::SHIFT)
        ^ (event.state.contains(KeyButMask::LOCK) && (0x61..=0x7a).contains(&base));
    let symbol = if shift {
        map.keysyms
            .get(1)
            .copied()
            .filter(|s| *s != 0)
            .unwrap_or(base)
    } else {
        base
    };
    Some((base, symbol))
}

pub(super) fn send_key(
    host: &BrowserHost,
    event: &KeyPressEvent,
    symbols: (u32, u32),
    up: bool,
    repeated: bool,
) {
    let (base, symbol) = symbols;
    let character = unicode(symbol);
    let mut key = KeyEvent {
        type_: if up {
            KeyEventType::KEYUP
        } else {
            KeyEventType::RAWKEYDOWN
        },
        modifiers: modifiers(event.state)
            | if repeated {
                cef_event_flags_t::EVENTFLAG_IS_REPEAT.0
            } else {
                0
            },
        windows_key_code: virtual_key(base),
        native_key_code: i32::from(event.detail),
        character: character as u16,
        unmodified_character: unicode(base) as u16,
        ..KeyEvent::default()
    };
    host.send_key_event(Some(&key));
    if !up
        && character != 0
        && !event
            .state
            .intersects(KeyButMask::CONTROL | KeyButMask::MOD1 | KeyButMask::MOD4)
        && let Some(ch) = char::from_u32(character)
    {
        for unit in ch.encode_utf16(&mut [0; 2]) {
            key.type_ = KeyEventType::CHAR;
            key.character = *unit;
            key.windows_key_code = i32::from(*unit);
            host.send_key_event(Some(&key));
        }
    }
}

fn unicode(symbol: u32) -> u32 {
    match symbol {
        0x20..=0xff => symbol,
        0x01000100..=0x0110ffff => symbol & 0x00ffffff,
        0xff08 => 8,
        0xff09 => 9,
        0xff0d => 13,
        _ => 0,
    }
}

pub(super) fn virtual_key(symbol: u32) -> i32 {
    match symbol {
        0x61..=0x7a => (symbol - 32) as i32,
        0x20 | 0x30..=0x39 | 0x41..=0x5a => symbol as i32,
        0xff08 => 8,
        0xff09 | 0xfe20 => 9,
        0xff0d => 13,
        0xff1b => 27,
        0xff50 => 36,
        0xff51 => 37,
        0xff52 => 38,
        0xff53 => 39,
        0xff54 => 40,
        0xff55 => 33,
        0xff56 => 34,
        0xff57 => 35,
        0xff63 => 45,
        0xffff => 46,
        0xffbe..=0xffd5 => (112 + symbol - 0xffbe) as i32,
        0xffe1 | 0xffe2 => 16,
        0xffe3 | 0xffe4 => 17,
        0xffe9 | 0xffea => 18,
        0xffeb => 91,
        0xffec => 92,
        0x3b => 186,
        0x3d => 187,
        0x2c => 188,
        0x2d => 189,
        0x2e => 190,
        0x2f => 191,
        0x60 => 192,
        0x5b => 219,
        0x5c => 220,
        0x5d => 221,
        0x27 => 222,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_and_text_use_cef_key_codes() {
        assert_eq!(virtual_key(0xffc8), 122); // F11
        assert_eq!(virtual_key(0xff51), 37);
        assert_eq!(virtual_key(u32::from('q')), 81);
        assert_eq!(unicode(0x0101f600), 0x1f600);
        assert_eq!(unicode(0xff51), 0);
        assert_eq!(
            modifiers(KeyButMask::CONTROL | KeyButMask::SHIFT),
            cef_event_flags_t::EVENTFLAG_CONTROL_DOWN.0 | cef_event_flags_t::EVENTFLAG_SHIFT_DOWN.0
        );
    }
}
