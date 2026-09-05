//! User preference model, persistence port, and runtime application policy.

mod accounts;
mod collections;
mod deletions;
mod json_file;
pub mod model;
mod playback;
mod viewing;
pub use viewing::{PlayerComfort, SubtitleMode, ViewingSettings};
pub mod service;
pub mod store;

pub use accounts::{
    AccountConfigurationService, AccountKey, HomeBuiltIn, HomeElement, HomeElementId, HomeSettings,
    accounts_file_path,
};
pub use collections::{CollectionConfigurationService, collections_file_path};
pub use deletions::{PendingDeletion, PendingDeletionService, pending_deletions_file_path};
pub use json_file::RecoveryNotice;
pub(crate) use json_file::backup_path;
pub use model::*;
pub use playback::{PlaybackPreferenceService, playback_preferences_file_path};
pub use service::{
    AppearanceSettingsPatch, ApplicationSettingsPatch, PlaybackSettingsPatch, PlayerSettingsPatch,
    PreferencesService, SettingsChange,
};
pub use store::*;
