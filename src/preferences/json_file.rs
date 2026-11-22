use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

pub fn load_with_recovery<T>(path: &Path) -> io::Result<Option<LoadedDocument<T>>>
where
    T: DeserializeOwned + Default,
{
    match read_document(path)? {
        Some(bytes) => parse_or_recover(path, &bytes, is_json_object(&bytes)),
        None => Ok(None),
    }
}

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
    let declared = declared_version(&bytes);
    if let Some(version) = declared
        && version != u64::from(supported_version)
    {
        return Err(invalid_data(format!(
            "unsupported {document_name} version {version}"
        )));
    }
    parse_or_recover(path, &bytes, declared.is_some())
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

fn is_json_object(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes).is_ok_and(|value| value.is_object())
}

fn parse_or_recover<T>(
    path: &Path,
    bytes: &[u8],
    intact: bool,
) -> io::Result<Option<LoadedDocument<T>>>
where
    T: DeserializeOwned + Default,
{
    match serde_json::from_slice(bytes) {
        Ok(document) => Ok(Some(LoadedDocument {
            document,
            recovery: None,
        })),
        Err(primary_error) if intact => Err(invalid_data(format!(
            "{} was written by a different MediaFlick version and was left unchanged: \
                 {primary_error}",
            path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned()
            )
        ))),
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

pub fn save_with_backup<T>(path: &Path, document: &T) -> io::Result<()>
where
    T: Serialize + DeserializeOwned,
{
    let bytes = serde_json::to_vec_pretty(document).map_err(io::Error::other)?;
    serde_json::from_slice::<T>(&bytes).map_err(io::Error::other)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.is_file() {
        let previous = std::fs::read(path)?;
        if serde_json::from_slice::<T>(&previous).is_ok() {
            atomic_write(&backup_path(path), &previous)?;
        }
    }
    atomic_write(path, &bytes)
}

pub fn replace_backup_with_primary(path: &Path) -> io::Result<()> {
    if path.is_file() {
        atomic_write(&backup_path(path), &std::fs::read(path)?)?;
    }
    Ok(())
}

pub fn mutate_locked<D, T>(
    document: &Mutex<D>,
    change: impl FnOnce(&mut D) -> io::Result<T>,
    validate: impl FnOnce(&mut D) -> io::Result<()>,
    save: impl FnOnce(&D) -> io::Result<()>,
) -> io::Result<T>
where
    D: Clone,
{
    let mut current = document
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut next = current.clone();
    let result = change(&mut next)?;
    validate(&mut next)?;
    save(&next)?;
    *current = next;
    drop(current);
    Ok(result)
}

pub fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

pub fn backup_path(path: &Path) -> PathBuf {
    appended_path(path, ".bak")
}

const DAMAGED_SUFFIX: &str = ".broken-";

fn damaged_path(path: &Path) -> PathBuf {
    let timestamp = crate::app::clock::since_epoch().as_secs();
    appended_path(path, &format!("{DAMAGED_SUFFIX}{timestamp}"))
}

pub fn damaged_copy_exists(path: &Path) -> io::Result<bool> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(false);
    };
    let prefix = format!("{}{DAMAGED_SUFFIX}", name.to_string_lossy());
    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    for entry in entries {
        if entry?.file_name().to_string_lossy().starts_with(&prefix) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn appended_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(test)]
pub(super) mod test_support {
    use std::path::Path;

    pub const NEWER_DOCUMENT: &[u8] = br#"{"version":2,"futureField":{"added":true}}"#;

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
        atomic_write(&path, b"not json").expect("damage primary");

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
    fn an_invalid_primary_never_replaces_the_last_valid_backup() {
        let path = test_path();
        save_with_backup(&path, &Document { value: 1 }).expect("first save");
        save_with_backup(&path, &Document { value: 2 }).expect("second save");
        atomic_write(&path, b"not json").expect("external damage");

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
    fn another_versions_document_is_rejected_before_recovery_can_touch_it() {
        for foreign in [
            test_support::NEWER_DOCUMENT,
            br#"{"version":1,"value":5,"unknown":true}"#,
        ] {
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
            atomic_write(&path, foreign).expect("foreign primary");

            let error = load_versioned(&path).expect_err("foreign document");

            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            test_support::assert_left_untouched(&path, foreign);
            assert_eq!(std::fs::read(backup_path(&path)).expect("backup"), backup);
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(backup_path(&path));
        }
    }

    #[test]
    fn a_document_without_a_readable_supported_version_still_recovers() {
        for damaged in [
            &b"{\"version\":2"[..],
            br#"{"value":5}"#,
            br#"{"version":"2","value":5}"#,
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
            atomic_write(&path, damaged).expect("damage primary");

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
