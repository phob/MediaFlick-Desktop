use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

const DEFAULT_WEBUI_WINDOW_WIDTH: i32 = 1280;
const DEFAULT_WEBUI_WINDOW_HEIGHT: i32 = 800;
const MIN_WEBUI_WINDOW_WIDTH: i32 = 640;
const MIN_WEBUI_WINDOW_HEIGHT: i32 = 360;
const DEFAULT_LOG_LEVEL: &str = "debug";
static SETTINGS_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

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
    #[serde(default, skip_serializing_if = "FullscreenBehavior::is_default")]
    pub default_fullscreen: FullscreenBehavior,
    #[serde(default, skip_serializing_if = "StreamingQuality::is_default")]
    pub streaming_quality: StreamingQuality,
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
    /// Visual preferences intentionally live with the otherwise flat legacy
    /// configuration. The API exposes them as a coherent section without
    /// forcing existing installations through a risky file migration.
    #[serde(default, skip_serializing_if = "AppearanceSettings::is_default")]
    pub appearance: AppearanceSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceSettings {
    #[serde(default, skip_serializing_if = "AppearanceTheme::is_default")]
    pub theme: AppearanceTheme,
    #[serde(default, skip_serializing_if = "AppearanceAccent::is_default")]
    pub accent: AppearanceAccent,
    #[serde(default, skip_serializing_if = "AppearanceDensity::is_default")]
    pub density: AppearanceDensity,
    #[serde(
        default = "default_artwork_intensity",
        skip_serializing_if = "is_default_artwork_intensity"
    )]
    pub artwork_intensity: u8,
    #[serde(
        default = "default_backdrop_intensity",
        skip_serializing_if = "is_default_backdrop_intensity"
    )]
    pub backdrop_intensity: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reduced_motion: bool,
    /// Expanded panels shown after resting the pointer on a media card.
    #[serde(default = "default_card_previews", skip_serializing_if = "is_true")]
    pub card_previews: bool,
    /// Technical video/audio facts shown over library card artwork. Enabled by
    /// default to preserve the card presentation from releases that predate
    /// this preference.
    #[serde(default = "default_show_media_info", skip_serializing_if = "is_true")]
    pub show_media_info: bool,
    /// Canonical rating source IDs chosen for card overlays. This is ordinary
    /// presentation state; credentials remain in the operating-system vault.
    /// IDs are limited to the fixed public source catalog so a prior or
    /// tampered server response cannot remain as desktop-visible metadata.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rating_sources: Vec<String>,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: AppearanceTheme::default(),
            accent: AppearanceAccent::default(),
            density: AppearanceDensity::default(),
            artwork_intensity: default_artwork_intensity(),
            backdrop_intensity: default_backdrop_intensity(),
            reduced_motion: false,
            card_previews: default_card_previews(),
            show_media_info: default_show_media_info(),
            rating_sources: Vec::new(),
        }
    }
}

impl AppearanceSettings {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn sanitize(&mut self) {
        self.artwork_intensity = self.artwork_intensity.min(100);
        self.backdrop_intensity = self.backdrop_intensity.min(100);
        let mut seen = std::collections::HashSet::new();
        self.rating_sources = self
            .rating_sources
            .drain(..)
            .filter_map(|source| {
                let source = source.trim().to_ascii_lowercase();
                is_public_rating_source(&source)
                    .then_some(source)
                    .filter(|source| seen.insert(source.clone()))
            })
            .take(64)
            .collect();
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceTheme {
    #[default]
    System,
    Dark,
    Light,
}

impl AppearanceTheme {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "system" => Some(Self::System),
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceAccent {
    #[default]
    Signal,
    Cobalt,
    Amber,
    Violet,
}

impl AppearanceAccent {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Signal => "signal",
            Self::Cobalt => "cobalt",
            Self::Amber => "amber",
            Self::Violet => "violet",
        }
    }
    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "signal" => Some(Self::Signal),
            "cobalt" => Some(Self::Cobalt),
            "amber" => Some(Self::Amber),
            "violet" => Some(Self::Violet),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceDensity {
    Compact,
    #[default]
    Comfortable,
}

impl AppearanceDensity {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Comfortable => "comfortable",
        }
    }
    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" => Some(Self::Compact),
            "comfortable" => Some(Self::Comfortable),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamingQuality {
    #[default]
    #[serde(rename = "original")]
    Original,
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "120_mbps")]
    Mbps120,
    #[serde(rename = "80_mbps")]
    Mbps80,
    #[serde(rename = "60_mbps")]
    Mbps60,
    #[serde(rename = "40_mbps")]
    Mbps40,
    #[serde(rename = "20_mbps")]
    Mbps20,
    #[serde(rename = "10_mbps")]
    Mbps10,
    #[serde(rename = "5_mbps")]
    Mbps5,
    #[serde(rename = "3_mbps")]
    Mbps3,
    #[serde(rename = "1_5_mbps")]
    Mbps1_5,
}

impl StreamingQuality {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Auto => "auto",
            Self::Mbps120 => "120_mbps",
            Self::Mbps80 => "80_mbps",
            Self::Mbps60 => "60_mbps",
            Self::Mbps40 => "40_mbps",
            Self::Mbps20 => "20_mbps",
            Self::Mbps10 => "10_mbps",
            Self::Mbps5 => "5_mbps",
            Self::Mbps3 => "3_mbps",
            Self::Mbps1_5 => "1_5_mbps",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "original" => Some(Self::Original),
            "auto" => Some(Self::Auto),
            "120_mbps" => Some(Self::Mbps120),
            "80_mbps" => Some(Self::Mbps80),
            "60_mbps" => Some(Self::Mbps60),
            "40_mbps" => Some(Self::Mbps40),
            "20_mbps" => Some(Self::Mbps20),
            "10_mbps" => Some(Self::Mbps10),
            "5_mbps" => Some(Self::Mbps5),
            "3_mbps" => Some(Self::Mbps3),
            "1_5_mbps" => Some(Self::Mbps1_5),
            _ => None,
        }
    }

    pub fn allows_transcoding(self) -> bool {
        self != Self::Original
    }

    pub fn max_streaming_bitrate(self) -> Option<u64> {
        match self {
            Self::Original | Self::Auto => None,
            Self::Mbps120 => Some(120_000_000),
            Self::Mbps80 => Some(80_000_000),
            Self::Mbps60 => Some(60_000_000),
            Self::Mbps40 => Some(40_000_000),
            Self::Mbps20 => Some(20_000_000),
            Self::Mbps10 => Some(10_000_000),
            Self::Mbps5 => Some(5_000_000),
            Self::Mbps3 => Some(3_000_000),
            Self::Mbps1_5 => Some(1_500_000),
        }
    }
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

    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "disabled" => Some(Self::Disabled),
            "prompt" => Some(Self::Prompt),
            "always" => Some(Self::Always),
            _ => None,
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
pub enum FullscreenBehavior {
    #[default]
    Fullscreen,
    Windowed,
}

impl FullscreenBehavior {
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

    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fullscreen" => Some(Self::Fullscreen),
            "windowed" => Some(Self::Windowed),
            _ => None,
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

    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "exit_app" => Some(Self::ExitApp),
            "minimize_window" => Some(Self::MinimizeWindow),
            _ => None,
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
            default_fullscreen: FullscreenBehavior::default(),
            streaming_quality: StreamingQuality::default(),
            close_behavior: CloseBehavior::default(),
            show_scrollbars: false,
            webui_window: WebUiWindowSettings::default(),
            skip_intro: SegmentSkipMode::default(),
            skip_credits: SegmentSkipMode::default(),
            skip_recap: SegmentSkipMode::default(),
            skip_commercial: SegmentSkipMode::default(),
            appearance: AppearanceSettings::default(),
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
        atomic_write(&path, &json)
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
        self.appearance.sanitize();
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("settings.json");
    let mut last_collision = None;

    for _ in 0..100 {
        let counter = SETTINGS_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".{file_name}.tmp-{}-{counter}", std::process::id()));
        let mut file = match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last_collision = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };

        let result = (|| {
            file.write_all(contents)?;
            file.sync_all()?;
            drop(file);
            replace_file(&temporary, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        return result;
    }

    Err(last_collision.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a temporary settings file",
        )
    }))
}

#[cfg(not(target_os = "windows"))]
fn replace_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, destination)
}

#[cfg(target_os = "windows")]
fn replace_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let moved = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
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

fn is_true(value: &bool) -> bool {
    *value
}

fn is_public_rating_source(value: &str) -> bool {
    matches!(
        value,
        "mdblist_score"
            | "mdblist_score_average"
            | "imdb"
            | "trakt"
            | "tmdb"
            | "letterboxd"
            | "tomatoes"
            | "popcorn"
            | "metacritic"
            | "metacriticuser"
            | "rogerebert"
            | "myanimelist"
    )
}

fn default_artwork_intensity() -> u8 {
    100
}

fn default_backdrop_intensity() -> u8 {
    100
}

fn default_show_media_info() -> bool {
    true
}

fn default_card_previews() -> bool {
    true
}

fn is_default_artwork_intensity(value: &u8) -> bool {
    *value == default_artwork_intensity()
}

fn is_default_backdrop_intensity(value: &u8) -> bool {
    *value == default_backdrop_intensity()
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
    use super::{
        AppSettings, PlayerBackend, StreamingQuality, WebUiWindowSettings, normalize_server_url,
    };

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
    fn blank_urls_are_rejected() {
        assert_eq!(normalize_server_url("  "), None);
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
    fn streaming_quality_round_trips_and_maps_bitrates() {
        let cases = [
            ("original", StreamingQuality::Original, None),
            ("auto", StreamingQuality::Auto, None),
            ("120_mbps", StreamingQuality::Mbps120, Some(120_000_000)),
            ("80_mbps", StreamingQuality::Mbps80, Some(80_000_000)),
            ("60_mbps", StreamingQuality::Mbps60, Some(60_000_000)),
            ("40_mbps", StreamingQuality::Mbps40, Some(40_000_000)),
            ("20_mbps", StreamingQuality::Mbps20, Some(20_000_000)),
            ("10_mbps", StreamingQuality::Mbps10, Some(10_000_000)),
            ("5_mbps", StreamingQuality::Mbps5, Some(5_000_000)),
            ("3_mbps", StreamingQuality::Mbps3, Some(3_000_000)),
            ("1_5_mbps", StreamingQuality::Mbps1_5, Some(1_500_000)),
        ];

        for (id, quality, bitrate) in cases {
            assert_eq!(StreamingQuality::from_id(id), Some(quality));
            assert_eq!(quality.as_str(), id);
            assert_eq!(
                quality.allows_transcoding(),
                quality != StreamingQuality::Original
            );
            assert_eq!(quality.max_streaming_bitrate(), bitrate);
        }
        assert_eq!(StreamingQuality::from_id("unknown"), None);
    }

    #[test]
    fn streaming_quality_is_backward_compatible_and_omits_default() {
        let defaults: AppSettings = serde_json::from_str("{}").expect("default settings");
        assert_eq!(defaults.streaming_quality, StreamingQuality::Original);
        let serialized = serde_json::to_value(&defaults).expect("serialize settings");
        assert!(serialized.get("streaming_quality").is_none());

        let configured = AppSettings {
            streaming_quality: StreamingQuality::Mbps10,
            ..Default::default()
        };
        let serialized = serde_json::to_value(&configured).expect("serialize configured settings");
        assert_eq!(serialized["streaming_quality"], "10_mbps");
        let restored: AppSettings = serde_json::from_value(serialized).expect("restore settings");
        assert_eq!(restored.streaming_quality, StreamingQuality::Mbps10);
    }

    #[test]
    fn rating_source_preferences_retain_only_the_public_catalog() {
        let defaults: AppSettings = serde_json::from_str("{}").expect("legacy settings");
        assert!(defaults.appearance.rating_sources.is_empty());

        let mut settings: AppSettings = serde_json::from_value(serde_json::json!({
            "appearance": {
                "rating_sources": [" Letterboxd ", "popcorn", "server-mdb-key-must-not-persist", "future_meter", "popcorn", "../bad"]
            }
        }))
        .expect("settings");
        settings.sanitize();
        assert_eq!(
            settings.appearance.rating_sources,
            ["letterboxd", "popcorn"]
        );
        let serialized = serde_json::to_string(&settings).expect("serialize");
        assert!(!serialized.contains("server-mdb-key-must-not-persist"));
        assert!(!serialized.contains("future_meter"));
        assert!(!serialized.contains("api_key"));
        assert!(!serialized.contains("apikey"));
    }

    #[test]
    fn media_info_remains_enabled_for_legacy_settings_until_explicitly_disabled() {
        let defaults: AppSettings = serde_json::from_str("{}").expect("legacy settings");
        assert!(defaults.appearance.show_media_info);
        let serialized = serde_json::to_value(&defaults).expect("serialize defaults");
        assert!(serialized.get("appearance").is_none());

        let disabled: AppSettings = serde_json::from_value(serde_json::json!({
            "appearance": { "show_media_info": false }
        }))
        .expect("settings");
        assert!(!disabled.appearance.show_media_info);
        let serialized = serde_json::to_value(&disabled).expect("serialize disabled setting");
        assert_eq!(serialized["appearance"]["show_media_info"], false);
    }

    #[test]
    fn card_previews_remain_enabled_for_legacy_settings_until_explicitly_disabled() {
        let defaults: AppSettings = serde_json::from_str("{}").expect("legacy settings");
        assert!(defaults.appearance.card_previews);
        let serialized = serde_json::to_value(&defaults).expect("serialize defaults");
        assert!(serialized.get("appearance").is_none());

        let disabled: AppSettings = serde_json::from_value(serde_json::json!({
            "appearance": { "card_previews": false }
        }))
        .expect("settings");
        assert!(!disabled.appearance.card_previews);
        let serialized = serde_json::to_value(&disabled).expect("serialize disabled setting");
        assert_eq!(serialized["appearance"]["card_previews"], false);
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
    fn mpchc_backend_reads_the_mpchc_path() {
        let mut settings = AppSettings {
            player_backend: PlayerBackend::Mpchc,
            ..Default::default()
        };
        assert_eq!(settings.effective_backend(), PlayerBackend::Mpchc);
        assert_eq!(settings.player_path(), None);
        settings.mpchc_path = Some("C:/MPC-HC/mpc-hc64.exe".to_string());
        assert_eq!(settings.player_path(), Some("C:/MPC-HC/mpc-hc64.exe"));
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
