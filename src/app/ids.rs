use std::fmt::Write;

/// A lowercase hex string of `bytes * 2` characters filled from the operating
/// system's CSPRNG.
///
/// These values are not only identifiers: the bridge session token is one, so
/// a hash of the wall clock and a counter — both of which an attacker can
/// guess — is not good enough.
pub fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).unwrap_or_else(|error| {
        panic!("the operating system random number generator failed: {error}");
    });
    let mut out = String::with_capacity(bytes * 2);
    for byte in buffer {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub fn new_device_id() -> String {
    random_hex(16)
}

/// A Jellyfin item id, or one of MediaFlick's own random ids: 16 to 64 hex
/// digits, so it is safe as a route segment, a SQL argument and a file name.
pub fn valid_opaque_id(value: &str) -> bool {
    matches!(value.len(), 16..=64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
