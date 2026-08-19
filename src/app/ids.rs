//! Opaque identifier generation without pulling in a UUID dependency.

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

/// A stable per-installation device identifier for the Jellyfin Devices list.
pub fn new_device_id() -> String {
    random_hex(16)
}

#[cfg(test)]
mod tests {
    use super::{new_device_id, random_hex};

    #[test]
    fn random_hex_has_the_requested_length_and_alphabet() {
        let value = random_hex(12);
        assert_eq!(value.len(), 24);
        assert!(value.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn device_ids_are_unique() {
        assert_ne!(new_device_id(), new_device_id());
        assert_eq!(new_device_id().len(), 32);
    }
}
