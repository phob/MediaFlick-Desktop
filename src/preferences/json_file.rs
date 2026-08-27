use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::store::atomic_write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryNotice {
    pub damaged_path: PathBuf,
    pub restored_backup: bool,
}

#[derive(Debug)]
pub struct LoadedDocument<T> {
    pub document: T,
    pub recovery: Option<RecoveryNotice>,
}

/// Loads one app-owned JSON document. A malformed primary is preserved under
/// a timestamped name. A valid backup is restored before the caller sees the
/// document, so the next save still starts from known-good bytes.
pub fn load_with_recovery<T>(path: &Path) -> io::Result<Option<LoadedDocument<T>>>
where
    T: DeserializeOwned + Default,
{
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    match serde_json::from_slice(&bytes) {
        Ok(document) => Ok(Some(LoadedDocument {
            document,
            recovery: None,
        })),
        Err(primary_error) => recover_from_backup(path, &primary_error),
    }
}

fn recover_from_backup<T>(
    path: &Path,
    primary_error: &serde_json::Error,
) -> io::Result<Option<LoadedDocument<T>>>
where
    T: DeserializeOwned + Default,
{
    let damaged_path = damaged_path(path);
    std::fs::rename(path, &damaged_path)?;
    let backup = backup_path(path);
    let restored = std::fs::read(&backup).ok().and_then(|bytes| {
        serde_json::from_slice::<T>(&bytes)
            .ok()
            .map(|document| (bytes, document))
    });
    if let Some((bytes, document)) = restored {
        atomic_write(path, &bytes)?;
        return Ok(Some(LoadedDocument {
            document,
            recovery: Some(RecoveryNotice {
                damaged_path,
                restored_backup: true,
            }),
        }));
    }
    tracing::warn!(
        target: "config",
        path = %path.display(),
        damaged_path = %damaged_path.display(),
        "could not parse settings and no valid backup exists: {primary_error}"
    );
    Ok(Some(LoadedDocument {
        document: T::default(),
        recovery: Some(RecoveryNotice {
            damaged_path,
            restored_backup: false,
        }),
    }))
}

/// Writes a complete document, retaining the last valid primary as `.bak`.
/// The caller must update its in-memory copy only after this returns success.
pub fn save_with_backup<T>(path: &Path, document: &T) -> io::Result<()>
where
    T: Serialize + DeserializeOwned,
{
    let bytes = serde_json::to_vec_pretty(document).map_err(io::Error::other)?;
    // Reparse before touching either destination. This catches a custom
    // serializer that emitted a value the corresponding reader cannot load.
    serde_json::from_slice::<T>(&bytes).map_err(io::Error::other)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.is_file() {
        let previous = std::fs::read(path)?;
        // A file changed behind the app's back is not a valid recovery point.
        // Keep the last known-good backup instead of replacing it with bytes
        // this reader cannot load.
        if serde_json::from_slice::<T>(&previous).is_ok() {
            atomic_write(&backup_path(path), &previous)?;
        }
    }
    atomic_write(path, &bytes)
}

/// Account deletion must scrub the old account from both the primary and its
/// recovery copy. Replace the backup atomically after the primary commits.
pub fn replace_backup_with_primary(path: &Path) -> io::Result<()> {
    if path.is_file() {
        atomic_write(&backup_path(path), &std::fs::read(path)?)?;
    }
    Ok(())
}

pub fn backup_path(path: &Path) -> PathBuf {
    appended_path(path, ".bak")
}

fn damaged_path(path: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    appended_path(path, &format!(".broken-{timestamp}"))
}

fn appended_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde::{Deserialize, Serialize};

    use super::*;

    static COUNTER: AtomicU64 = AtomicU64::new(1);

    #[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct Document {
        value: u32,
    }

    fn test_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "mediaflick-json-file-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn a_valid_backup_recovers_a_damaged_primary() {
        let path = test_path();
        save_with_backup(&path, &Document { value: 1 }).expect("first save");
        save_with_backup(&path, &Document { value: 2 }).expect("second save");
        std::fs::write(&path, b"not json").expect("damage primary");

        let loaded = load_with_recovery::<Document>(&path)
            .expect("recover")
            .expect("document");

        assert_eq!(loaded.document, Document { value: 1 });
        assert!(loaded.recovery.is_some_and(|notice| notice.restored_backup));
        assert_eq!(
            serde_json::from_slice::<Document>(&std::fs::read(&path).expect("primary"))
                .expect("valid primary"),
            Document { value: 1 }
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(backup_path(&path));
    }

    #[test]
    fn defaults_are_isolated_when_no_valid_backup_exists() {
        let path = test_path();
        std::fs::write(&path, b"not json").expect("damaged primary");

        let loaded = load_with_recovery::<Document>(&path)
            .expect("recover with defaults")
            .expect("document");

        assert_eq!(loaded.document, Document::default());
        assert!(
            loaded
                .recovery
                .is_some_and(|notice| !notice.restored_backup)
        );
        assert!(!path.exists());
    }

    #[test]
    fn an_invalid_primary_never_replaces_the_last_valid_backup() {
        let path = test_path();
        save_with_backup(&path, &Document { value: 1 }).expect("first save");
        save_with_backup(&path, &Document { value: 2 }).expect("second save");
        std::fs::write(&path, b"not json").expect("external damage");

        save_with_backup(&path, &Document { value: 3 }).expect("repair with next state");

        let backup: Document =
            serde_json::from_slice(&std::fs::read(backup_path(&path)).expect("valid backup"))
                .expect("backup document");
        assert_eq!(backup, Document { value: 1 });
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(backup_path(&path));
    }
}
