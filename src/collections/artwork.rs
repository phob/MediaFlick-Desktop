use std::collections::HashSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::app::ids::random_hex;
use crate::preferences::config_dir;

const MAX_POSTER_BYTES: usize = 10 * 1024 * 1024;
const ORPHAN_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub struct ArtworkStore {
    directory: PathBuf,
}

impl ArtworkStore {
    pub fn open(directory: PathBuf) -> io::Result<Self> {
        std::fs::create_dir_all(&directory)?;
        Ok(Self { directory })
    }

    /// Writes a poster under a fresh id. The caller must save collections.json
    /// before calling `commit`; `rollback` removes this file if that save fails.
    pub fn stage(&self, bytes: &[u8]) -> io::Result<String> {
        if bytes.is_empty() || bytes.len() > MAX_POSTER_BYTES {
            return Err(invalid_input(
                "poster size must be between 1 byte and 10 MB",
            ));
        }
        let extension = image_extension(bytes)
            .ok_or_else(|| invalid_input("poster must be a PNG, JPEG, or WebP image"))?;
        let id = random_hex(16);
        let path = self.directory.join(format!("{id}.{extension}"));
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        Ok(id)
    }

    pub fn remove(&self, id: &str) -> io::Result<()> {
        if !super::valid_opaque_id(id) {
            return Err(invalid_input("invalid custom poster id"));
        }
        for extension in ["png", "jpg", "webp"] {
            let path = self.directory.join(format!("{id}.{extension}"));
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub fn path(&self, id: &str) -> Option<PathBuf> {
        super::valid_opaque_id(id).then_some(())?;
        ["png", "jpg", "webp"]
            .into_iter()
            .map(|extension| self.directory.join(format!("{id}.{extension}")))
            .find(|path| path.is_file())
    }

    /// References in both the primary and backup document remain live. Files
    /// get a seven-day grace period so a crash between image and JSON commits
    /// cannot turn into immediate data loss.
    pub fn collect_orphans(&self, collections_path: &Path, now: SystemTime) -> io::Result<usize> {
        let live = live_artwork_ids(collections_path);
        let mut removed = 0;
        for entry in std::fs::read_dir(&self.directory)? {
            let entry = entry?;
            let path = entry.path();
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if live.contains(id) {
                continue;
            }
            let modified = entry.metadata()?.modified()?;
            if now.duration_since(modified).unwrap_or_default() < ORPHAN_AGE {
                continue;
            }
            std::fs::remove_file(path)?;
            removed += 1;
        }
        Ok(removed)
    }
}

pub fn custom_art_dir() -> PathBuf {
    config_dir().join("custom-art")
}

fn live_artwork_ids(collections_path: &Path) -> HashSet<String> {
    let mut live = HashSet::new();
    for path in [
        collections_path.to_path_buf(),
        crate::preferences::backup_path(collections_path),
    ] {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        collect_references(&value, &mut live);
    }
    live
}

fn collect_references(value: &Value, live: &mut HashSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(id)) = object.get("customPosterId") {
                live.insert(id.clone());
            }
            for child in object.values() {
                collect_references(child, live);
            }
        }
        Value::Array(array) => {
            for child in array {
                collect_references(child, live);
            }
        }
        _ => {}
    }
}

fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("jpg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poster_type_comes_from_file_bytes() {
        assert_eq!(image_extension(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(image_extension(b"\xff\xd8\xffrest"), Some("jpg"));
        assert_eq!(image_extension(b"not an image"), None);
    }
}
