//! MPC-HC slave-mode wire protocol: command ids and `WM_COPYDATA` payloads.
//!
//! Values mirror `MpcApi.h` from the clsid2/mpc-hc fork. Payloads are
//! pipe-delimited UTF-16 strings, except `CMD_OSDSHOWMESSAGE`, which carries a
//! packed `MPC_OSDDATA` struct.

// Host -> MPC-HC
pub const CMD_OPENFILE: u32 = 0xA000_0000;
pub const CMD_STOP: u32 = 0xA000_0001;
pub const CMD_PLAY: u32 = 0xA000_0004;
pub const CMD_PAUSE: u32 = 0xA000_0005;
pub const CMD_SETPOSITION: u32 = 0xA000_2000;
pub const CMD_SETAUDIOTRACK: u32 = 0xA000_2004;
pub const CMD_SETSUBTITLETRACK: u32 = 0xA000_2005;
pub const CMD_GETNOWPLAYING: u32 = 0xA000_3002;
pub const CMD_GETCURRENTPOSITION: u32 = 0xA000_3004;
pub const CMD_TOGGLEFULLSCREEN: u32 = 0xA000_4000;
pub const CMD_INCREASEVOLUME: u32 = 0xA000_4003;
pub const CMD_DECREASEVOLUME: u32 = 0xA000_4004;
pub const CMD_CLOSEAPP: u32 = 0xA000_4006;
pub const CMD_SETSPEED: u32 = 0xA000_4008;
pub const CMD_OSDSHOWMESSAGE: u32 = 0xA000_5000;

// MPC-HC -> host
pub const CMD_CONNECT: u32 = 0x5000_0000;
pub const CMD_STATE: u32 = 0x5000_0001;
pub const CMD_PLAYMODE: u32 = 0x5000_0002;
pub const CMD_NOWPLAYING: u32 = 0x5000_0003;
pub const CMD_CURRENTPOSITION: u32 = 0x5000_0007;
pub const CMD_NOTIFYSEEK: u32 = 0x5000_0008;
pub const CMD_NOTIFYENDOFSTREAM: u32 = 0x5000_0009;
pub const CMD_DISCONNECT: u32 = 0x5000_000B;

// MPC_LOADSTATE
pub const MLS_LOADED: i64 = 2;
pub const MLS_FAILING: i64 = 4;

// MPC_PLAYSTATE
pub const PS_PLAY: i64 = 0;
pub const PS_PAUSE: i64 = 1;
pub const PS_STOP: i64 = 2;

// MPC_OSD_MESSAGEPOS
pub const OSD_TOPLEFT: i32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    Connect { hwnd: isize },
    State(i64),
    PlayMode(i64),
    NowPlaying { duration_seconds: Option<f64> },
    CurrentPosition(f64),
    NotifySeek(f64),
    EndOfStream,
    Disconnect,
    Ignored(u32),
}

pub fn parse_inbound(command: u32, payload: &str) -> Inbound {
    match command {
        CMD_CONNECT => Inbound::Connect {
            hwnd: payload.trim().parse().unwrap_or(0),
        },
        CMD_STATE => Inbound::State(parse_i64(payload)),
        CMD_PLAYMODE => Inbound::PlayMode(parse_i64(payload)),
        CMD_NOWPLAYING => Inbound::NowPlaying {
            duration_seconds: last_field(payload).and_then(parse_f64),
        },
        CMD_CURRENTPOSITION => Inbound::CurrentPosition(parse_f64(payload).unwrap_or(0.0)),
        CMD_NOTIFYSEEK => Inbound::NotifySeek(parse_f64(payload).unwrap_or(0.0)),
        CMD_NOTIFYENDOFSTREAM => Inbound::EndOfStream,
        CMD_DISCONNECT => Inbound::Disconnect,
        other => Inbound::Ignored(other),
    }
}

/// The last pipe-delimited field. Numeric trailing fields (duration, active
/// track index, position) never contain a pipe, so a plain `rsplit` is robust
/// regardless of how earlier text fields are escaped.
fn last_field(payload: &str) -> Option<&str> {
    payload.rsplit('|').next().map(str::trim)
}

fn parse_i64(payload: &str) -> i64 {
    payload.trim().parse().unwrap_or(0)
}

fn parse_f64(value: &str) -> Option<f64> {
    let value = value.trim();
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// UTF-16 buffer (NUL-terminated) for a string command payload.
pub fn wide_payload(text: &str) -> Vec<u16> {
    let mut buffer: Vec<u16> = text.encode_utf16().collect();
    buffer.push(0);
    buffer
}

/// Packed `MPC_OSDDATA` bytes: `int nMsgPos; int nDurationMS; WCHAR strMsg[128];`.
pub fn osd_message_bytes(position: i32, duration_ms: i32, message: &str) -> Vec<u8> {
    const MESSAGE_CAPACITY: usize = 128;
    let mut bytes = Vec::with_capacity(8 + MESSAGE_CAPACITY * 2);
    bytes.extend_from_slice(&position.to_le_bytes());
    bytes.extend_from_slice(&duration_ms.to_le_bytes());

    let mut message: Vec<u16> = message.encode_utf16().take(MESSAGE_CAPACITY - 1).collect();
    message.push(0);
    message.resize(MESSAGE_CAPACITY, 0);
    for unit in message {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_connect_handle() {
        assert_eq!(
            parse_inbound(CMD_CONNECT, "12345"),
            Inbound::Connect { hwnd: 12345 }
        );
    }

    #[test]
    fn parses_nowplaying_duration_from_last_field() {
        assert_eq!(
            parse_inbound(CMD_NOWPLAYING, "Title|Author|Desc|C:\\a|b.mkv|1325.5"),
            Inbound::NowPlaying {
                duration_seconds: Some(1325.5)
            }
        );
    }

    #[test]
    fn parses_position_and_seek() {
        assert_eq!(
            parse_inbound(CMD_CURRENTPOSITION, "42.250"),
            Inbound::CurrentPosition(42.25)
        );
        assert_eq!(
            parse_inbound(CMD_NOTIFYSEEK, "10"),
            Inbound::NotifySeek(10.0)
        );
    }

    #[test]
    fn parses_state_and_endofstream() {
        assert_eq!(parse_inbound(CMD_STATE, "2"), Inbound::State(MLS_LOADED));
        assert_eq!(
            parse_inbound(CMD_NOTIFYENDOFSTREAM, ""),
            Inbound::EndOfStream
        );
    }

    #[test]
    fn unknown_command_is_ignored() {
        assert_eq!(
            parse_inbound(0xDEAD_BEEF, "x"),
            Inbound::Ignored(0xDEAD_BEEF)
        );
    }

    #[test]
    fn wide_payload_is_nul_terminated() {
        assert_eq!(wide_payload("ab"), vec![0x61, 0x62, 0x00]);
    }

    #[test]
    fn osd_bytes_layout_is_packed_264_bytes() {
        let bytes = osd_message_bytes(OSD_TOPLEFT, 3000, "Hi");
        assert_eq!(bytes.len(), 8 + 128 * 2);
        assert_eq!(&bytes[0..4], &1i32.to_le_bytes());
        assert_eq!(&bytes[4..8], &3000i32.to_le_bytes());
        assert_eq!(&bytes[8..10], &[b'H', 0]);
        assert_eq!(&bytes[10..12], &[b'i', 0]);
        assert_eq!(&bytes[12..14], &[0, 0]);
    }
}
