use std::io;

use super::AppSettings;

/// Persistence port for the application's user preferences.
pub trait SettingsStore {
    fn load(&self) -> AppSettings;
    fn save(&self, settings: &AppSettings) -> io::Result<()>;
}

/// JSON file adapter used by the desktop application.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileSettingsStore;

impl SettingsStore for FileSettingsStore {
    fn load(&self) -> AppSettings {
        AppSettings::load()
    }

    fn save(&self, settings: &AppSettings) -> io::Result<()> {
        settings.save()
    }
}
