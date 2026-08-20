//! User preference model, persistence port, and runtime application policy.

pub mod model;
pub mod service;
pub mod store;

pub use model::*;
pub use service::{
    AppearanceSettingsPatch, ApplicationSettingsPatch, PlaybackSettingsPatch, PlayerSettingsPatch,
    PreferencesService, SettingsChange,
};
pub use store::*;
