use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const DEFAULT_WEBUI_WINDOW_WIDTH: i32 = 1280;
const DEFAULT_WEBUI_WINDOW_HEIGHT: i32 = 800;
const MIN_WEBUI_WINDOW_WIDTH: i32 = 640;
const MIN_WEBUI_WINDOW_HEIGHT: i32 = 360;
const DEFAULT_LOG_LEVEL: &str = "debug";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jellyfin_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mpv_path: Option<String>,
    #[serde(default, skip_serializing_if = "PlayerBackend::is_default")]
    pub player_backend: PlayerBackend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mpchc_path: Option<String>,
    #[serde(
        default = "default_log_level_string",
        skip_serializing_if = "is_default_log_level"
    )]
    pub log_level: String,
    #[serde(default, skip_serializing_if = "MpvFullscreenBehavior::is_default")]
    pub default_fullscreen: MpvFullscreenBehavior,
    #[serde(default, skip_serializing_if = "CloseBehavior::is_default")]
    pub close_behavior: CloseBehavior,
    #[serde(default, skip_serializing_if = "is_false")]
    pub show_scrollbars: bool,
    #[serde(default, skip_serializing_if = "WebUiWindowSettings::is_default")]
    pub webui_window: WebUiWindowSettings,
    #[serde(default, skip_serializing_if = "SegmentSkipMode::is_default")]
    pub skip_intro: SegmentSkipMode,
    #[serde(default, skip_serializing_if = "SegmentSkipMode::is_default")]
    pub skip_credits: SegmentSkipMode,
    #[serde(default, skip_serializing_if = "SegmentSkipMode::is_default")]
    pub skip_recap: SegmentSkipMode,
    #[serde(default, skip_serializing_if = "SegmentSkipMode::is_default")]
    pub skip_commercial: SegmentSkipMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentSkipMode {
    Disabled,
    #[default]
    Prompt,
    Always,
}

impl SegmentSkipMode {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Prompt => "prompt",
            Self::Always => "always",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentSkipConfig {
    pub intro: SegmentSkipMode,
    pub credits: SegmentSkipMode,
    pub recap: SegmentSkipMode,
    pub commercial: SegmentSkipMode,
}

impl Default for SegmentSkipConfig {
    fn default() -> Self {
        Self {
            intro: SegmentSkipMode::Prompt,
            credits: SegmentSkipMode::Prompt,
            recap: SegmentSkipMode::Prompt,
            commercial: SegmentSkipMode::Prompt,
        }
    }
}

impl SegmentSkipConfig {
    pub fn all_disabled(self) -> bool {
        self.intro == SegmentSkipMode::Disabled
            && self.credits == SegmentSkipMode::Disabled
            && self.recap == SegmentSkipMode::Disabled
            && self.commercial == SegmentSkipMode::Disabled
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerBackend {
    #[default]
    Mpv,
    Mpchc,
}

impl PlayerBackend {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mpv => "mpv",
            Self::Mpchc => "mpchc",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mpv" => Some(Self::Mpv),
            "mpchc" | "mpc-hc" | "mpc_hc" => Some(Self::Mpchc),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MpvFullscreenBehavior {
    #[default]
    Fullscreen,
    Windowed,
}

impl MpvFullscreenBehavior {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn fullscreen_arg(self) -> &'static str {
        match self {
            Self::Fullscreen => "yes",
            Self::Windowed => "no",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fullscreen => "fullscreen",
            Self::Windowed => "windowed",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    #[default]
    ExitApp,
    MinimizeWindow,
}

impl CloseBehavior {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExitApp => "exit_app",
            Self::MinimizeWindow => "minimize_window",
        }
    }
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

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            jellyfin_url: None,
            mpv_path: None,
            player_backend: PlayerBackend::default(),
            mpchc_path: None,
            log_level: DEFAULT_LOG_LEVEL.to_string(),
            default_fullscreen: MpvFullscreenBehavior::default(),
            close_behavior: CloseBehavior::default(),
            show_scrollbars: false,
            webui_window: WebUiWindowSettings::default(),
            skip_intro: SegmentSkipMode::default(),
            skip_credits: SegmentSkipMode::default(),
            skip_recap: SegmentSkipMode::default(),
            skip_commercial: SegmentSkipMode::default(),
        }
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
            && self.player_path().is_some()
    }

    pub fn effective_backend(&self) -> PlayerBackend {
        #[cfg(target_os = "windows")]
        {
            self.player_backend
        }
        #[cfg(not(target_os = "windows"))]
        {
            PlayerBackend::Mpv
        }
    }

    pub fn player_path(&self) -> Option<&str> {
        let path = match self.effective_backend() {
            PlayerBackend::Mpv => self.mpv_path.as_deref(),
            PlayerBackend::Mpchc => self.mpchc_path.as_deref(),
        };
        path.map(str::trim).filter(|value| !value.is_empty())
    }

    pub fn segment_skip_config(&self) -> SegmentSkipConfig {
        SegmentSkipConfig {
            intro: self.skip_intro,
            credits: self.skip_credits,
            recap: self.skip_recap,
            commercial: self.skip_commercial,
        }
    }

    pub fn sanitize(&mut self) {
        self.jellyfin_url = self.jellyfin_url.as_deref().and_then(normalize_server_url);
        self.mpv_path = self
            .mpv_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        self.mpchc_path = self
            .mpchc_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        self.log_level = self.log_level.trim().to_string();
        if self.log_level.is_empty() {
            self.log_level = DEFAULT_LOG_LEVEL.to_string();
        }
        self.webui_window.sanitize();
    }
}

fn default_log_level_string() -> String {
    DEFAULT_LOG_LEVEL.to_string()
}

fn is_default_log_level(value: &str) -> bool {
    value == DEFAULT_LOG_LEVEL
}

fn is_false(value: &bool) -> bool {
    !*value
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
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Some(trimmed.to_string())
    } else if has_explicit_scheme(trimmed) {
        None
    } else {
        Some(format!("http://{trimmed}"))
    }
}

fn has_explicit_scheme(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    let valid_scheme = chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'));
    if !valid_scheme {
        return false;
    }
    !rest.chars().next().is_some_and(|ch| ch.is_ascii_digit())
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
    use super::{AppSettings, PlayerBackend, WebUiWindowSettings, normalize_server_url};

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
        assert_eq!(
            normalize_server_url("media.example.com:8920"),
            Some("http://media.example.com:8920".to_string())
        );
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert_eq!(normalize_server_url("file:///etc/passwd"), None);
        assert_eq!(normalize_server_url("data:text/html,<h1>x</h1>"), None);
        assert_eq!(normalize_server_url("about:blank"), None);
        assert_eq!(normalize_server_url("javascript:alert(1)"), None);
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
    fn player_backend_round_trips_by_id() {
        assert_eq!(PlayerBackend::Mpv.as_str(), "mpv");
        assert_eq!(PlayerBackend::Mpchc.as_str(), "mpchc");
        assert_eq!(PlayerBackend::from_id("mpv"), Some(PlayerBackend::Mpv));
        assert_eq!(PlayerBackend::from_id("MPC-HC"), Some(PlayerBackend::Mpchc));
        assert_eq!(PlayerBackend::from_id("mpchc"), Some(PlayerBackend::Mpchc));
        assert_eq!(PlayerBackend::from_id("vlc"), None);
    }

    #[test]
    fn sanitize_trims_mpchc_path() {
        let mut settings = AppSettings {
            mpchc_path: Some("  C:/MPC-HC/mpc-hc64.exe  ".to_string()),
            ..Default::default()
        };
        settings.sanitize();
        assert_eq!(
            settings.mpchc_path.as_deref(),
            Some("C:/MPC-HC/mpc-hc64.exe")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn complete_with_mpchc_backend_requires_mpchc_path() {
        let mut settings = AppSettings {
            jellyfin_url: Some("http://localhost:8096".to_string()),
            player_backend: PlayerBackend::Mpchc,
            ..Default::default()
        };
        assert_eq!(settings.effective_backend(), PlayerBackend::Mpchc);
        assert!(!settings.is_complete());
        settings.mpchc_path = Some("C:/MPC-HC/mpc-hc64.exe".to_string());
        assert_eq!(settings.player_path(), Some("C:/MPC-HC/mpc-hc64.exe"));
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
