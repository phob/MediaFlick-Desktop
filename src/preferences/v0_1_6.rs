use std::io;
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::{AppSettings, clean_binding};

#[derive(Deserialize)]
struct InputFile {
    bindings: InputBindings,
}

#[derive(Deserialize)]
struct InputBindings {
    mark_watched_next: String,
}

pub(super) fn import(dir: &Path) -> AppSettings {
    let mut settings = read::<AppSettings>(&dir.join("config.json")).unwrap_or_default();
    if let Some(input) = read::<InputFile>(&dir.join("input.json")) {
        settings.mark_watched_next = clean_binding(Some(&input.bindings.mark_watched_next));
    }
    settings
}

pub fn carries_0_1_6_data(dir: &Path) -> bool {
    dir.join("config.json").is_file()
}

fn read<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!("failed to read 0.1.6 {}: {error}", path.display());
            return None;
        }
    };
    serde_json::from_slice(&bytes)
        .inspect_err(|error| tracing::warn!("ignoring 0.1.6 {}: {error}", path.display()))
        .ok()
}
