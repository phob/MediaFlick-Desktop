use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Time since the Unix epoch. A wall clock set before 1970 counts as the
/// epoch itself: every caller stamps or compares times, and none can do
/// better with a negative one.
pub fn since_epoch() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
}
