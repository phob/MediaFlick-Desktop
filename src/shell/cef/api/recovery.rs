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

#[derive(Serialize, ts_rs::TS)]
pub(super) struct StartupRecovery {
    library: Option<RefusedLibrary>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct RefusedLibrary {
    refusal: Refusal,
    folder: String,
    backup_name: String,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) enum Refusal {
    NewerSchema,
    Unusable,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct LibraryRecreated {
    backup: String,
}

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
