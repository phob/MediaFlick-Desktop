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
    match read_document(path)? {
        Some(bytes) => parse_or_recover(path, &bytes),
        None => Ok(None),
    }
}

/// Loads a JSON document that records its format in a top-level `version`.
/// The version is read leniently before the strict parse: a document from
/// another app version (typically one that adds fields) is rejected and left
/// byte-for-byte in place, never moved aside or replaced by its backup. A
/// document without a readable numeric version is damaged and goes through
/// the normal recovery path.
pub fn load_versioned_with_recovery<T>(
    path: &Path,
    supported_version: u32,
    document_name: &str,
) -> io::Result<Option<LoadedDocument<T>>>
where
    T: DeserializeOwned + Default,
{
    let Some(bytes) = read_document(path)? else {
        return Ok(None);
    };
    if let Some(version) = declared_version(&bytes)
        && version != u64::from(supported_version)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported {document_name} version {version}"),
        ));
    }
    parse_or_recover(path, &bytes)
}

fn read_document(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn declared_version(bytes: &[u8]) -> Option<u64> {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()?
        .get("version")?
        .as_u64()
}

fn parse_or_recover<T>(path: &Path, bytes: &[u8]) -> io::Result<Option<LoadedDocument<T>>>
where
    T: DeserializeOwned + Default,
{
    match serde_json::from_slice(bytes) {
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
pub(super) mod test_support {
    use std::path::Path;

    /// A document written by a newer app version: an unsupported version plus
    /// a field this reader's strict schema does not know.
    pub const NEWER_DOCUMENT: &[u8] = br#"{"version":2,"futureField":{"added":true}}"#;

    /// Asserts that a rejected document was neither moved aside, replaced by
    /// its backup, nor reset to defaults.
    pub fn assert_left_untouched(path: &Path, original: &[u8]) {
        assert_eq!(std::fs::read(path).expect("read original"), original);
        let name = path
            .file_name()
            .map(|name| format!("{}.broken-", name.to_string_lossy()))
            .unwrap_or_default();
        let parent = path.parent().expect("test file parent");
        let moved_aside = std::fs::read_dir(parent)
            .expect("read test directory")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with(&name));
        assert!(!moved_aside, "the document must not be moved aside");
    }
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

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct VersionedDocument {
        version: u32,
        value: u32,
    }

    impl Default for VersionedDocument {
        fn default() -> Self {
            Self {
                version: 1,
                value: 0,
            }
        }
    }

    fn load_versioned(path: &Path) -> io::Result<Option<LoadedDocument<VersionedDocument>>> {
        load_versioned_with_recovery(path, 1, "test document")
    }

    #[test]
    fn a_newer_version_is_rejected_before_recovery_can_touch_it() {
        let path = test_path();
        save_with_backup(&path, &VersionedDocument::default()).expect("first save");
        save_with_backup(
            &path,
            &VersionedDocument {
                version: 1,
                value: 2,
            },
        )
        .expect("second");
        let backup = std::fs::read(backup_path(&path)).expect("backup");
        std::fs::write(&path, test_support::NEWER_DOCUMENT).expect("newer primary");

        let error = load_versioned(&path).expect_err("newer document");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "unsupported test document version 2");
        test_support::assert_left_untouched(&path, test_support::NEWER_DOCUMENT);
        assert_eq!(std::fs::read(backup_path(&path)).expect("backup"), backup);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(backup_path(&path));
    }

    #[test]
    fn a_document_without_a_readable_supported_version_still_recovers() {
        for damaged in [
            &b"{\"version\":2"[..],
            br#"{"value":5}"#,
            br#"{"version":"2","value":5}"#,
            br#"{"version":1,"value":5,"unknown":true}"#,
        ] {
            let path = test_path();
            save_with_backup(
                &path,
                &VersionedDocument {
                    version: 1,
                    value: 1,
                },
            )
            .expect("first");
            save_with_backup(
                &path,
                &VersionedDocument {
                    version: 1,
                    value: 2,
                },
            )
            .expect("second");
            std::fs::write(&path, damaged).expect("damage primary");

            let loaded = load_versioned(&path).expect("recover").expect("document");

            assert_eq!(
                loaded.document,
                VersionedDocument {
                    version: 1,
                    value: 1
                }
            );
            let notice = loaded.recovery.expect("recovery notice");
            assert!(notice.restored_backup);
            assert_eq!(std::fs::read(&notice.damaged_path).expect("moved"), damaged);
            let _ = std::fs::remove_file(&notice.damaged_path);
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(backup_path(&path));
        }
    }
}
