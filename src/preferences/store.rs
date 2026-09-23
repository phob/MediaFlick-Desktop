use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::AppSettings;
use super::json_file::{RecoveryNotice, load_with_recovery, save_with_backup};

static SETTINGS_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);
static DEVICE_RECOVERY: std::sync::Mutex<Option<RecoveryNotice>> = std::sync::Mutex::new(None);

impl AppSettings {
    pub fn load() -> Self {
        let path = config_file_path();
        match load_with_recovery::<Self>(&path) {
            Ok(Some(loaded)) => {
                if let Ok(mut recovery) = DEVICE_RECOVERY.lock() {
                    *recovery = loaded.recovery;
                }
                let mut settings = loaded.document;
                settings.sanitize();
                settings
            }
            Ok(None) => load_legacy_settings(),
            Err(error) => {
                tracing::warn!("failed to read {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let settings = device_settings(self);

        let path = config_file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        save_with_backup(&path, &settings)
    }
}

fn device_settings(settings: &AppSettings) -> AppSettings {
    let mut settings = settings.clone();
    settings.sanitize();
    // The runtime snapshot carries the active account's appearance so all
    // consumers can keep one concrete settings type. It belongs only in
    // accounts.json and must never leak back into the device file.
    settings.appearance = Default::default();
    settings
}

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("settings.json");
    let mut last_collision = None;

    for _ in 0..100 {
        let counter = SETTINGS_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".{file_name}.tmp-{}-{counter}", std::process::id()));
        let mut file = match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                last_collision = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };

        let result = (|| {
            file.write_all(contents)?;
            file.sync_all()?;
            drop(file);
            replace_file(&temporary, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        return result;
    }

    Err(last_collision.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a temporary settings file",
        )
    }))
}

#[cfg(not(target_os = "windows"))]
fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(temporary, destination)
}

#[cfg(target_os = "windows")]
fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let moved = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn config_file_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn take_device_recovery_notice() -> Option<RecoveryNotice> {
    DEVICE_RECOVERY
        .lock()
        .ok()
        .and_then(|mut recovery| recovery.take())
}

fn load_legacy_settings() -> AppSettings {
    let path = config_dir().join("config.json");
    let parsed = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<AppSettings>(&bytes).ok());
    let mut settings = parsed.unwrap_or_else(|| AppSettings {
        mark_watched_next: legacy_mark_watched_next(),
        ..AppSettings::default()
    });
    settings.sanitize();
    settings
}

/// The mark-watched-next key from `input.json`, where builds before
/// `settings.json` carried the binding kept it. A missing or unreadable file
/// means the default key; a blank value means the binding was disabled. The
/// file is only read, so a newer settings file never disagrees with it.
pub(crate) fn legacy_mark_watched_next() -> Option<String> {
    let path = config_dir().join("input.json");
    let value = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!("failed to read {}: {error}", path.display());
                return Some(super::DEFAULT_MARK_WATCHED_NEXT.to_string());
            }
        },
        Err(_) => return Some(super::DEFAULT_MARK_WATCHED_NEXT.to_string()),
    };
    legacy_binding(&value)
}

fn legacy_binding(value: &serde_json::Value) -> Option<String> {
    let lookup = |key: &str| {
        value
            .get("bindings")
            .and_then(|bindings| bindings.get(key))
            .or_else(|| value.get(key))
    };
    // `kb_watched` is the name the first builds used.
    match lookup("mark_watched_next").or_else(|| lookup("kb_watched")) {
        None => Some(super::DEFAULT_MARK_WATCHED_NEXT.to_string()),
        Some(binding) => super::clean_binding(binding.as_str()),
    }
}

pub fn config_dir() -> PathBuf {
    crate::app::paths::platform_config_dir().join("mediaflick-desktop")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::{AppearanceAccent, AppearanceSettings};

    #[test]
    fn legacy_input_files_keep_their_watched_binding() {
        use serde_json::json;

        for (file, expected) in [
            (
                json!({ "bindings": { "mark_watched_next": " W " } }),
                Some("W"),
            ),
            (
                json!({ "mark_watched_next": "Shift+Meta+w" }),
                Some("Shift+Meta+w"),
            ),
            (json!({ "kb_watched": "x" }), Some("x")),
            (json!({ "bindings": { "mark_watched_next": "" } }), None),
            (json!({ "bindings": { "mark_watched_next": 5 } }), None),
            (json!({}), Some("w")),
        ] {
            assert_eq!(legacy_binding(&file).as_deref(), expected, "{file}");
        }
    }

    #[test]
    fn the_device_snapshot_excludes_account_owned_appearance() {
        let settings = AppSettings {
            appearance: AppearanceSettings {
                accent: AppearanceAccent::Violet,
                ..AppearanceSettings::default()
            },
            log_level: "debug".to_string(),
            ..AppSettings::default()
        };

        let device = device_settings(&settings);

        assert_eq!(device.appearance, AppearanceSettings::default());
        assert_eq!(device.log_level, "debug");
    }
}
