//! Opaque identifier generation without pulling in a UUID dependency.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// A lowercase hex string of `bytes * 2` characters seeded from the process
/// `RandomState` (which itself is OS-seeded) mixed with the wall clock.
pub fn random_hex(bytes: usize) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let mut out = String::with_capacity(bytes * 2);
    while out.len() < bytes * 2 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(SEQUENCE.fetch_add(1, Ordering::Relaxed));
        hasher.write_u128(nanos);
        hasher.write_usize(out.len());
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out.truncate(bytes * 2);
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
