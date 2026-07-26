//! Per-user data locations shared by the CEF cache, the library database, and
//! the poster cache.

use std::path::PathBuf;

/// Platform convention for machine-local (non-roaming) application data.
pub fn platform_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(value) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(value);
        }
        if let Some(value) = std::env::var_os("APPDATA") {
            return PathBuf::from(value);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support");
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(value) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(value);
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local").join("share");
        }
    }

    std::env::temp_dir()
}

pub fn app_data_dir() -> PathBuf {
    platform_data_dir().join("mediaflick-desktop")
}

/// SQLite database holding credentials, the metadata cache, and sync state.
pub fn library_db_path() -> PathBuf {
    app_data_dir().join("library.db")
}

/// Disk cache for posters and backdrops proxied from the server.
pub fn image_cache_dir() -> PathBuf {
    app_data_dir().join("image-cache")
}

#[cfg(test)]
mod tests {
    use super::{app_data_dir, image_cache_dir, library_db_path};

    #[test]
    fn data_locations_live_under_one_app_directory() {
        let base = app_data_dir();
        assert!(base.ends_with("mediaflick-desktop"));
        assert_eq!(library_db_path().parent(), Some(base.as_path()));
        assert_eq!(image_cache_dir().parent(), Some(base.as_path()));
    }
}
