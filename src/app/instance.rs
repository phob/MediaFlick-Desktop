//! Single-instance gate: ensures only one interactive MediaFlick session runs
//! at a time, keyed by a stable id persisted in `instance.json` (mirroring
//! Jellyfin Desktop's per-config-directory instance file).

use std::path::{Path, PathBuf};

use crate::app::settings::config_dir;

const FALLBACK_INSTANCE_ID: &str = "default";

pub fn instance_id() -> String {
    let path = instance_file_path();
    if let Some(id) = read_instance_id(&path) {
        return id;
    }

    let id = new_instance_id();
    let value = serde_json::json!({ "instanceId": &id });
    if let Ok(bytes) = serde_json::to_vec_pretty(&value) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&path, &bytes).is_err() {
            return read_instance_id(&path).unwrap_or_else(|| FALLBACK_INSTANCE_ID.to_string());
        }
    }
    id
}

fn instance_file_path() -> PathBuf {
    config_dir().join("instance.json")
}

fn read_instance_id(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let id = value.get("instanceId")?.as_str()?;
    sanitize_instance_id(id)
}

fn sanitize_instance_id(id: &str) -> Option<String> {
    let clean: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    if clean.is_empty() { None } else { Some(clean) }
}

fn new_instance_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{nanos:032x}{pid:08x}")
}

/// Owns the OS handle that marks this process as the live MediaFlick session.
/// Dropping it (or the process exiting) releases the gate for the next launch.
pub struct InstanceGuard {
    #[cfg(target_os = "windows")]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

impl InstanceGuard {
    /// Returns `Some(guard)` when this process now owns the single session, or
    /// `None` when another MediaFlick session already holds it.
    #[cfg(target_os = "windows")]
    pub fn acquire(instance_id: &str) -> Option<Self> {
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let name = to_wide(&format!("Local\\mediaflick-desktop-instance-{instance_id}"));
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        if handle.is_null() {
            tracing::warn!(
                target: "instance",
                "failed to create single-instance mutex; allowing startup"
            );
            return Some(Self {
                handle: std::ptr::null_mut(),
            });
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return None;
        }
        Some(Self { handle })
    }

    #[cfg(not(target_os = "windows"))]
    pub fn acquire(_instance_id: &str) -> Option<Self> {
        Some(Self {})
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        if !self.handle.is_null() {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
        }
    }
}

/// Tells the user a session is already running before the second launch exits.
#[cfg(target_os = "windows")]
pub fn notify_already_running() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};

    let text = to_wide("MediaFlick Desktop is already running.");
    let caption = to_wide("MediaFlick Desktop");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        )
    };
}

#[cfg(not(target_os = "windows"))]
pub fn notify_already_running() {}

#[cfg(target_os = "windows")]
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{new_instance_id, sanitize_instance_id};

    #[test]
    fn sanitize_strips_disallowed_chars() {
        assert_eq!(
            sanitize_instance_id("ab\\cd/.. ef-gh_1").as_deref(),
            Some("abcdef-gh_1")
        );
    }

    #[test]
    fn sanitize_rejects_empty_result() {
        assert_eq!(sanitize_instance_id("\\/. ").as_deref(), None);
    }

    #[test]
    fn new_id_is_sanitizable_and_stable_shape() {
        let id = new_instance_id();
        assert_eq!(sanitize_instance_id(&id).as_deref(), Some(id.as_str()));
    }
}
