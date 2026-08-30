//! Media-player adapters.
//!
//! Player implementations translate backend-specific protocols into the
//! backend-neutral contracts owned by the playback domain.

#[cfg(windows)]
pub mod mpchc;
pub mod mpv;

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::playback::{PlaybackEvent, PlayerBackend};
use crate::players::mpv::MpvController;
use crate::preferences::{AppSettings, PlayerBackend as PlayerBackendKind};
#[cfg(windows)]
use mpchc::MpcHcController;

/// Build the configured player adapter at the application boundary.
pub fn build_backend(
    settings: &AppSettings,
    event_tx: Sender<PlaybackEvent>,
) -> Box<dyn PlayerBackend> {
    match settings.effective_backend() {
        PlayerBackendKind::Libmpv => Box::new(MpvController::new_libmpv(
            Some(event_tx),
            settings.segment_skip_config(),
        )),
        PlayerBackendKind::Mpv => Box::new(MpvController::new(
            Some(event_tx),
            settings.segment_skip_config(),
        )),
        PlayerBackendKind::Mpchc => {
            #[cfg(windows)]
            {
                Box::new(MpcHcController::new(
                    Some(event_tx),
                    settings.segment_skip_config(),
                ))
            }
            #[cfg(not(windows))]
            {
                Box::new(MpvController::new(
                    Some(event_tx),
                    settings.segment_skip_config(),
                ))
            }
        }
    }
}

/// Resolve the selected runtime without making the settings UI provide a path
/// for the bundled backend.
pub fn configured_player_path(settings: &AppSettings) -> Option<String> {
    match settings.effective_backend() {
        PlayerBackendKind::Libmpv => {
            bundled_libmpv_path().map(|path| path.to_string_lossy().into_owned())
        }
        PlayerBackendKind::Mpv | PlayerBackendKind::Mpchc => {
            settings.player_path().map(str::to_string)
        }
    }
}

pub fn bundled_libmpv_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MEDIAFLICK_DESKTOP_LIBMPV_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(std::fs::canonicalize(&path).unwrap_or(path));
        }
    }
    let app_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))?;
    libmpv_candidates(&app_dir)
        .into_iter()
        .find(|path| path.is_file())
}

#[cfg(target_os = "windows")]
fn libmpv_candidates(app_dir: &Path) -> [PathBuf; 3] {
    [
        app_dir.join("libmpv-2.dll"),
        app_dir.join("libmpv").join("libmpv-2.dll"),
        app_dir.join("libmpv-windows-x64").join("libmpv-2.dll"),
    ]
}

#[cfg(target_os = "linux")]
fn libmpv_candidates(app_dir: &Path) -> [PathBuf; 3] {
    [
        app_dir.join("libmpv.so.2"),
        app_dir.join("libmpv.so"),
        app_dir.join("libmpv").join("libmpv.so.2"),
    ]
}

#[cfg(target_os = "macos")]
fn libmpv_candidates(app_dir: &Path) -> [PathBuf; 3] {
    [
        app_dir.join("libmpv.2.dylib"),
        app_dir.join("libmpv.dylib"),
        app_dir.join("../Frameworks/libmpv.2.dylib"),
    ]
}

#[cfg(test)]
mod tests {
    use super::libmpv_candidates;
    use std::path::Path;

    #[test]
    fn bundled_library_search_starts_beside_the_app() {
        let candidates = libmpv_candidates(Path::new("app"));
        assert!(candidates[0].starts_with("app"));
        #[cfg(target_os = "windows")]
        assert_eq!(
            candidates[2],
            Path::new("app/libmpv-windows-x64/libmpv-2.dll")
        );
    }
}
