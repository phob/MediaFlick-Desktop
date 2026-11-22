use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn since_epoch() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
}
