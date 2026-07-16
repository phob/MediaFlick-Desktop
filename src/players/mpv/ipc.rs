//! mpv IPC endpoint allocation and cleanup.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static IPC_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn make_ipc_path() -> String {
    let counter = IPC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    #[cfg(target_os = "windows")]
    {
        format!(
            r"\\.\pipe\mediaflick-desktop-{}-{timestamp}-{counter}",
            std::process::id()
        )
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::temp_dir()
            .join(format!(
                "mediaflick-desktop-{}-{timestamp}-{counter}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(not(target_os = "windows"))]
pub fn cleanup_ipc_path(path: &str) {
    let _ = std::fs::remove_file(path);
}

#[cfg(target_os = "windows")]
pub fn cleanup_ipc_path(_path: &str) {}
