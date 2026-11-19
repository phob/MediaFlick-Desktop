//! The startup error screen's routes. They need no services, so they also
//! answer while `library.db` is refused and the services are down.

use super::*;
use crate::app::paths;
use crate::app::services::LibraryRefusal;
use crate::shell::locator::RecreateError;

routes! {
    |request|
    GET ["recovery"] -> StartupRecovery = Ok(startup_recovery()),
    POST ["recovery", "library", "recreate"] -> LibraryRecreated = recreate_library(),
    POST ["recovery", "data-folder", "open"] -> DataFolderOpened = open_data_folder(),
}

/// `/api/recovery`: what the startup error screen can offer.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct StartupRecovery {
    /// Set while the services are down because `library.db` was refused.
    library: Option<RefusedLibrary>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct RefusedLibrary {
    refusal: Refusal,
    /// The folder holding `library.db`, where Recreate library leaves the old one.
    folder: String,
    /// The name Recreate library would give the old file now. The name
    /// carries the second it moves, in UTC.
    backup_name: String,
}

/// How `library.db` was refused. Either way the file was left as it was.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) enum Refusal {
    /// A newer MediaFlick wrote it.
    NewerSchema,
    /// SQLite could not open it, or its schema change failed and rolled back.
    Unusable,
}

/// `/api/recovery/library/recreate`: the services opened on a new `library.db`.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct LibraryRecreated {
    /// Where the old `library.db` went.
    backup: String,
}

/// `/api/recovery/data-folder/open`: the folder was handed to the file manager.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct DataFolderOpened {
    folder: String,
}

fn startup_recovery() -> StartupRecovery {
    let library = locator::library_refusal().map(|refusal| {
        let path = paths::library_db_path();
        let backup = crate::library::db::set_aside_path(&path, crate::library::now_unix());
        RefusedLibrary {
            refusal: match refusal {
                LibraryRefusal::NewerSchema => Refusal::NewerSchema,
                LibraryRefusal::Unusable => Refusal::Unusable,
            },
            folder: paths::app_data_dir().display().to_string(),
            backup_name: backup
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    });
    StartupRecovery { library }
}

fn recreate_library() -> Result<LibraryRecreated, ApiResponse> {
    match locator::recreate_library() {
        Ok(backup) => Ok(LibraryRecreated {
            backup: backup.display().to_string(),
        }),
        Err(RecreateError::Open) => Err(ApiResponse::error(
            409,
            "the library is open, so nothing was moved",
        )),
        Err(RecreateError::NotRefused) => Err(ApiResponse::error(
            409,
            "library.db was not refused, so nothing was moved",
        )),
        Err(RecreateError::SetAside(error)) => Err(ApiResponse::error(
            500,
            format!("could not move library.db aside, so it was left as it was: {error}"),
        )),
        Err(RecreateError::Reopen(reason)) => Err(ApiResponse::startup_failed(&reason)),
    }
}

fn open_data_folder() -> Result<DataFolderOpened, ApiResponse> {
    let folder = paths::app_data_dir();
    match super::super::bridge::open_folder(&folder) {
        Ok(()) => Ok(DataFolderOpened {
            folder: folder.display().to_string(),
        }),
        Err(error) => Err(ApiResponse::error(
            500,
            format!("could not open {}: {error}", folder.display()),
        )),
    }
}
