use std::path::Path;

use crate::app::paths::app_data_dir;

#[cfg(any(target_os = "windows", test))]
pub fn mutex_name_for_dir(dir: &Path) -> String {
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    #[cfg(target_os = "windows")]
    let bytes = canonical
        .to_string_lossy()
        .to_ascii_lowercase()
        .into_bytes();
    #[cfg(not(target_os = "windows"))]
    let bytes = canonical.as_os_str().as_encoded_bytes().to_vec();
    let hash = fnv1a_128(&bytes);
    format!("Local\\mediaflick-desktop-instance-{hash:032x}")
}

#[cfg(any(target_os = "windows", test))]
fn fnv1a_128(bytes: &[u8]) -> u128 {
    const OFFSET_BASIS: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut hash = OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u128::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

pub struct InstanceGuard {
    #[cfg(target_os = "windows")]
    handle: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(not(target_os = "windows"))]
    _lock: Option<std::fs::File>,
}

impl InstanceGuard {
    pub fn acquire() -> Option<Self> {
        Self::acquire_in(&app_data_dir())
    }

    #[cfg(target_os = "windows")]
    pub fn acquire_in(dir: &Path) -> Option<Self> {
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let _ = std::fs::create_dir_all(dir);
        let name = to_wide(&mutex_name_for_dir(dir));
        // SAFETY: `name` is a NUL-terminated wide string that outlives the call;
        // null security attributes select the defaults.
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
        // SAFETY: reads this thread's last error, set by CreateMutexW above.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: `handle` is the mutex handle just opened and is not kept.
            unsafe { CloseHandle(handle) };
            return None;
        }
        Some(Self { handle })
    }

    #[cfg(not(target_os = "windows"))]
    pub fn acquire_in(dir: &Path) -> Option<Self> {
        if let Err(error) = std::fs::create_dir_all(dir) {
            tracing::warn!(
                target: "instance",
                path = %dir.display(),
                "failed to create data directory for single-instance lock; allowing startup: {error}"
            );
            return Some(Self { _lock: None });
        }
        let lock_path = dir.join("instance.lock");
        let file = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
        {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(
                    target: "instance",
                    path = %lock_path.display(),
                    "failed to open single-instance lock file; allowing startup: {error}"
                );
                return Some(Self { _lock: None });
            }
        };
        match file.try_lock() {
            Ok(()) => Some(Self { _lock: Some(file) }),
            Err(std::fs::TryLockError::WouldBlock) => None,
            Err(std::fs::TryLockError::Error(error)) => {
                tracing::warn!(
                    target: "instance",
                    path = %lock_path.display(),
                    "failed to lock single-instance file; allowing startup: {error}"
                );
                Some(Self { _lock: None })
            }
        }
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        if !self.handle.is_null() {
            // SAFETY: the guard owns this mutex handle and closes it only here.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
        }
    }
}

#[cfg(target_os = "windows")]
pub fn notify_already_running() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};

    let text = to_wide("MediaFlick Desktop is already running.");
    let caption = to_wide("MediaFlick Desktop");
    // SAFETY: both strings are NUL-terminated and outlive the modal call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        )
    };
}

#[cfg(target_os = "windows")]
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{InstanceGuard, mutex_name_for_dir};
    use crate::app::ids::random_hex;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mediaflick-inst-{label}-{}", random_hex(8)));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn second_acquire_in_same_directory_is_rejected_until_first_drops() {
        let dir = temp_dir("same");
        let first = InstanceGuard::acquire_in(&dir);
        assert!(first.is_some());
        let second = InstanceGuard::acquire_in(&dir);
        assert!(second.is_none());
        drop(first);
        let third = InstanceGuard::acquire_in(&dir);
        assert!(third.is_some());
        drop(third);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn different_directories_do_not_conflict() {
        let first_dir = temp_dir("profile-a");
        let second_dir = temp_dir("profile-b");
        let first = InstanceGuard::acquire_in(&first_dir);
        let second = InstanceGuard::acquire_in(&second_dir);
        assert!(first.is_some());
        assert!(second.is_some());
        drop(first);
        drop(second);
        let _ = std::fs::remove_dir_all(&first_dir);
        let _ = std::fs::remove_dir_all(&second_dir);
    }

    #[test]
    fn mutex_name_is_deterministic_valid_and_distinct_per_directory() {
        let first_dir = temp_dir("mutex-a");
        let second_dir = temp_dir("mutex-b");
        let first = mutex_name_for_dir(&first_dir);
        let first_via_dot = mutex_name_for_dir(&first_dir.join("."));
        let second = mutex_name_for_dir(&second_dir);

        assert_eq!(first, first_via_dot);
        assert_ne!(first, second);
        let suffix = first
            .strip_prefix("Local\\mediaflick-desktop-instance-")
            .expect("Local namespace prefix");
        assert_eq!(suffix.len(), 32);
        assert!(suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let _ = std::fs::remove_dir_all(&first_dir);
        let _ = std::fs::remove_dir_all(&second_dir);
    }
}
