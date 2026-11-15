//! MediaFlick 0.1.6, the last release before `settings.json`, kept the device
//! settings in `config.json` and the mark-watched-next key in `input.json`,
//! both in the same config directory. A device without `settings.json` starts
//! from them; its next save writes `settings.json`, which is read from then
//! on. These files are only ever read.

use std::io;
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::{AppSettings, clean_binding};

/// `input.json` as 0.1.6 wrote it (`MpvInputBindings::save` in
/// v0.1.6:src/players/mpv/input.rs); a blank key means disabled.
#[derive(Deserialize)]
struct InputFile {
    bindings: InputBindings,
}

#[derive(Deserialize)]
struct InputBindings {
    mark_watched_next: String,
}

/// The settings 0.1.6 left in `dir`, or defaults where a file is missing or
/// unreadable. `config.json` goes through the same schema as `settings.json`,
/// so its MPC-HC backend becomes the built-in player.
pub(super) fn import(dir: &Path) -> AppSettings {
    let mut settings = read::<AppSettings>(&dir.join("config.json")).unwrap_or_default();
    if let Some(input) = read::<InputFile>(&dir.join("input.json")) {
        settings.mark_watched_next = clean_binding(Some(&input.bindings.mark_watched_next));
    }
    settings
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
