use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const DEFAULT_WEBUI_WINDOW_WIDTH: i32 = 1280;
const DEFAULT_WEBUI_WINDOW_HEIGHT: i32 = 800;
const MIN_WEBUI_WINDOW_WIDTH: i32 = 640;
const MIN_WEBUI_WINDOW_HEIGHT: i32 = 360;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jellyfin_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mpv_path: Option<String>,
    #[serde(default, skip_serializing_if = "WebUiWindowSettings::is_default")]
    pub webui_window: WebUiWindowSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebUiWindowSettings {
    #[serde(default = "default_webui_window_width")]
    pub width: i32,
    #[serde(default = "default_webui_window_height")]
    pub height: i32,
    #[serde(default)]
    pub maximized: bool,
}

impl Default for WebUiWindowSettings {
    fn default() -> Self {
        Self {
            width: DEFAULT_WEBUI_WINDOW_WIDTH,
            height: DEFAULT_WEBUI_WINDOW_HEIGHT,
            maximized: false,
        }
    }
}

impl WebUiWindowSettings {
    pub fn size(self) -> (i32, i32) {
        (self.width, self.height)
    }

    pub fn record_bounds(&mut self, width: i32, height: i32, maximized: bool) {
        self.maximized = maximized;
        if !maximized {
            self.width = width;
            self.height = height;
            self.sanitize();
        }
    }

    fn sanitize(&mut self) {
        if self.width < MIN_WEBUI_WINDOW_WIDTH || self.height < MIN_WEBUI_WINDOW_HEIGHT {
            self.width = DEFAULT_WEBUI_WINDOW_WIDTH;
            self.height = DEFAULT_WEBUI_WINDOW_HEIGHT;
        }
    }

    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl AppSettings {
    pub fn load() -> Self {
        let path = config_file_path();
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::default();
        };
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(mut settings) => {
                settings.sanitize();
                settings
            }
            Err(error) => {
                tracing::warn!("failed to read {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let mut settings = self.clone();
        settings.sanitize();

        let path = config_file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(&settings).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    pub fn is_complete(&self) -> bool {
        self.jellyfin_url
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            && self
                .mpv_path
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }

    pub fn sanitize(&mut self) {
        self.jellyfin_url = self.jellyfin_url.as_deref().and_then(normalize_server_url);
        self.mpv_path = self
            .mpv_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        self.webui_window.sanitize();
    }
}

fn default_webui_window_width() -> i32 {
    DEFAULT_WEBUI_WINDOW_WIDTH
}

fn default_webui_window_height() -> i32 {
    DEFAULT_WEBUI_WINDOW_HEIGHT
}

pub fn normalize_server_url(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let normalized = if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("file://")
        || lower.starts_with("data:")
        || lower == "about:blank"
    {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    Some(normalized)
}

pub fn config_file_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn config_dir() -> PathBuf {
    roaming_base_dir().join("mediaflick-desktop")
}

fn roaming_base_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(value) = std::env::var_os("APPDATA") {
            return PathBuf::from(value);
        }
        if let Some(home) = std::env::var_os("USERPROFILE") {
            return PathBuf::from(home).join("AppData").join("Roaming");
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
        if let Some(value) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(value);
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".config");
        }
    }

    std::env::temp_dir()
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, WebUiWindowSettings, normalize_server_url};

    #[test]
    fn leaves_absolute_urls_alone() {
        assert_eq!(
            normalize_server_url("https://example.test"),
            Some("https://example.test".to_string())
        );
    }

    #[test]
    fn prefixes_server_hosts() {
        assert_eq!(
            normalize_server_url("localhost:8096"),
            Some("http://localhost:8096".to_string())
        );
    }

    #[test]
    fn blank_url_means_welcome_screen() {
        assert_eq!(normalize_server_url("  "), None);
    }

    #[test]
    fn complete_requires_url_and_mpv() {
        let settings = AppSettings {
            jellyfin_url: Some("http://localhost:8096".to_string()),
            mpv_path: Some("C:/mpv/mpv.exe".to_string()),
            ..Default::default()
        };
        assert!(settings.is_complete());
    }

    #[test]
    fn invalid_webui_window_size_falls_back_to_default() {
        let mut settings = AppSettings {
            webui_window: WebUiWindowSettings {
                width: 100,
                height: 100,
                maximized: true,
            },
            ..Default::default()
        };
        settings.sanitize();
        assert_eq!(settings.webui_window.size(), (1280, 800));
        assert!(settings.webui_window.maximized);
    }

    #[test]
    fn recording_maximized_window_keeps_restored_size() {
        let mut window = WebUiWindowSettings {
            width: 1440,
            height: 900,
            maximized: false,
        };
        window.record_bounds(3840, 2160, true);
        assert_eq!(window.size(), (1440, 900));
        assert!(window.maximized);
    }

    #[test]
    fn recording_restored_window_updates_size() {
        let mut window = WebUiWindowSettings::default();
        window.record_bounds(1600, 900, false);
        assert_eq!(window.size(), (1600, 900));
        assert!(!window.maximized);
    }
}
