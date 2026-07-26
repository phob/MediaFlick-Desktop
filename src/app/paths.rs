//! Per-user data locations shared by the CEF cache, the library database, and
//! the poster cache.

use std::ffi::OsString;
use std::path::PathBuf;

/// A data root handed to us by the environment is only usable when it is
/// absolute. An empty or relative value would put `library.db` under whatever
/// the working directory happens to be, where the next launch — started from
/// somewhere else — would silently find a different, empty database.
fn trusted_root(value: Option<OsString>) -> Option<PathBuf> {
    let path = PathBuf::from(value?);
    path.is_absolute().then_some(path)
}

/// Platform convention for machine-local (non-roaming) application data.
pub fn platform_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(path) = trusted_root(std::env::var_os("LOCALAPPDATA")) {
            return path;
        }
        if let Some(path) = trusted_root(std::env::var_os("APPDATA")) {
            return path;
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = trusted_root(std::env::var_os("HOME")) {
            return home.join("Library").join("Application Support");
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(path) = trusted_root(std::env::var_os("XDG_DATA_HOME")) {
            return path;
        }
        if let Some(home) = trusted_root(std::env::var_os("HOME")) {
            return home.join(".local").join("share");
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
    use super::{app_data_dir, image_cache_dir, library_db_path, platform_data_dir, trusted_root};
    use std::ffi::OsString;

    #[test]
    fn data_locations_live_under_one_app_directory() {
        let base = app_data_dir();
        assert!(base.ends_with("mediaflick-desktop"));
        assert_eq!(library_db_path().parent(), Some(base.as_path()));
        assert_eq!(image_cache_dir().parent(), Some(base.as_path()));
    }

    #[test]
    fn unset_empty_and_relative_roots_are_all_rejected() {
        assert_eq!(trusted_root(None), None);
        assert_eq!(trusted_root(Some(OsString::from(""))), None);
        assert_eq!(trusted_root(Some(OsString::from("data"))), None);
        assert_eq!(trusted_root(Some(OsString::from("../data"))), None);
    }

    #[test]
    fn absolute_roots_are_kept() {
        let absolute = if cfg!(windows) { r"C:\data" } else { "/data" };
        assert_eq!(
            trusted_root(Some(OsString::from(absolute))).as_deref(),
            Some(std::path::Path::new(absolute))
        );
    }

    #[test]
    fn the_data_root_is_never_relative_to_the_working_directory() {
        assert!(platform_data_dir().is_absolute());
    }
}
