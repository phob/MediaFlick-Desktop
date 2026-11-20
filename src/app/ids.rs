use std::fmt::Write;

/// A lowercase hex string of `bytes * 2` characters filled from the operating
/// system's CSPRNG.
///
/// These values are not only identifiers: the bridge session token is one, so
/// a hash of the wall clock and a counter — both of which an attacker can
/// guess — is not good enough.
///
/// Without the OS CSPRNG no secure token exists, and the bridge token is a
/// process-lifetime static, so the process exits: stopping is the only
/// fail-closed answer.
pub fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    if let Err(error) = getrandom::fill(&mut buffer) {
        tracing::error!(target: "ids", "the operating system random number generator failed: {error}");
        eprintln!("the operating system random number generator failed: {error}");
        std::process::exit(1);
    }
    let mut out = String::with_capacity(bytes * 2);
    for byte in buffer {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub fn new_device_id() -> String {
    random_hex(16)
}

pub fn valid_opaque_id(value: &str) -> bool {
    matches!(value.len(), 16..=64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
