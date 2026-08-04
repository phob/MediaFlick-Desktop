use std::fmt;
use std::sync::{Mutex, mpsc};

use serde::Deserialize;

use crate::players::mpv::input::MpvInputBindings;

use super::{
    AppSettings, AppearanceAccent, AppearanceDensity, AppearanceTheme, CloseBehavior,
    FileSettingsStore, FullscreenBehavior, PlayerBackend, SegmentSkipMode, SettingsStore,
    StreamingQuality, WebUiWindowSettings,
};

/// Serialized patches accepted by the settings API.  These deliberately name
/// sections instead of exposing `AppSettings` as a generic key/value bag.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerSettingsPatch {
    pub player_backend: Option<String>,
    /// An explicit JSON `null` clears a path; an omitted field preserves it.
    pub mpv_path: Option<Option<String>>,
    pub mpchc_path: Option<Option<String>>,
    pub default_fullscreen: Option<String>,
    /// `None` means leave the binding alone; `Some(None)` is the explicit UI
    /// request to disable it.
    pub mark_watched_next: Option<Option<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaybackSettingsPatch {
    pub streaming_quality: Option<String>,
    pub skip_intro: Option<String>,
    pub skip_credits: Option<String>,
    pub skip_recap: Option<String>,
    pub skip_commercial: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationSettingsPatch {
    pub close_behavior: Option<String>,
    pub show_scrollbars: Option<bool>,
    pub log_level: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppearanceSettingsPatch {
    pub theme: Option<String>,
    pub accent: Option<String>,
    pub density: Option<String>,
    pub artwork_intensity: Option<u8>,
    pub backdrop_intensity: Option<u8>,
    pub reduced_motion: Option<bool>,
    pub rating_sources: Option<Vec<String>>,
}

/// A persisted settings snapshot plus the effects the CEF shell has to apply.
#[derive(Debug, Clone)]
pub struct SettingsChange {
    pub settings: AppSettings,
    pub plan: SettingsApplyPlan,
}

#[derive(Debug)]
pub struct PreferencesError(String);

impl PreferencesError {
    fn invalid(field: &str) -> Self {
        Self(format!("invalid {field}"))
    }
}

impl fmt::Display for PreferencesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PreferencesError {}

/// The sole writer for application preferences.
///
/// A small in-process snapshot prevents a settings PATCH from reloading an
/// older file and accidentally discarding window geometry written by CEF. The
/// caller receives a normalized snapshot, while CEF subscribes to changes to
/// perform UI-thread-only work such as rebuilding the playback backend.
pub struct PreferencesService {
    settings: Mutex<AppSettings>,
    listener: Mutex<Option<mpsc::Sender<SettingsChange>>>,
}

impl PreferencesService {
    pub fn new(mut settings: AppSettings) -> Self {
        settings.sanitize();
        Self {
            settings: Mutex::new(settings),
            listener: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> AppSettings {
        self.settings
            .lock()
            .map(|settings| settings.clone())
            .unwrap_or_default()
    }

    /// The shell has one top-level browser, so a single registered receiver is
    /// sufficient and avoids retaining dead CEF state after shutdown.
    pub fn subscribe(&self) -> mpsc::Receiver<SettingsChange> {
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut listener) = self.listener.lock() {
            *listener = Some(sender);
        }
        receiver
    }

    pub fn patch_player(
        &self,
        patch: PlayerSettingsPatch,
    ) -> Result<SettingsChange, PreferencesError> {
        let binding = patch.mark_watched_next.clone();
        let update_input_bindings = binding.is_some();
        self.update_with_plan(
            move |next| {
                if let Some(value) = patch.player_backend.as_deref() {
                    let backend = PlayerBackend::from_id(value)
                        .ok_or_else(|| PreferencesError::invalid("player backend"))?;
                    if backend == PlayerBackend::Mpchc && !cfg!(target_os = "windows") {
                        return Err(PreferencesError(
                            "MPC-HC is only available on Windows".to_string(),
                        ));
                    }
                    next.player_backend = backend;
                }
                if let Some(value) = patch.mpv_path {
                    next.mpv_path = value.as_deref().and_then(clean_path);
                }
                if let Some(value) = patch.mpchc_path {
                    next.mpchc_path = value.as_deref().and_then(clean_path);
                }
                if let Some(value) = patch.default_fullscreen.as_deref() {
                    next.default_fullscreen = FullscreenBehavior::from_id(value)
                        .ok_or_else(|| PreferencesError::invalid("fullscreen behavior"))?;
                }
                // An unconfigured player is a valid saved state: it lets users reset
                // the section to defaults and finish choosing a backend later. Playback
                // still performs the concrete executable check before it starts.
                if let Some(value) = binding {
                    let bindings = MpvInputBindings {
                        mark_watched_next: value.as_deref().and_then(clean_path),
                    };
                    bindings.save().map_err(|error| {
                        PreferencesError(format!("could not save input bindings: {error}"))
                    })?;
                }
                Ok(())
            },
            move |plan| plan.update_input_bindings = update_input_bindings,
        )
    }

    pub fn patch_playback(
        &self,
        patch: PlaybackSettingsPatch,
    ) -> Result<SettingsChange, PreferencesError> {
        self.update(move |next| {
            if let Some(value) = patch.streaming_quality.as_deref() {
                next.streaming_quality = StreamingQuality::from_id(value)
                    .ok_or_else(|| PreferencesError::invalid("streaming quality"))?;
            }
            set_segment(
                &mut next.skip_intro,
                patch.skip_intro.as_deref(),
                "intro skip mode",
            )?;
            set_segment(
                &mut next.skip_credits,
                patch.skip_credits.as_deref(),
                "credits skip mode",
            )?;
            set_segment(
                &mut next.skip_recap,
                patch.skip_recap.as_deref(),
                "recap skip mode",
            )?;
            set_segment(
                &mut next.skip_commercial,
                patch.skip_commercial.as_deref(),
                "commercial skip mode",
            )?;
            Ok(())
        })
    }

    pub fn patch_application(
        &self,
        patch: ApplicationSettingsPatch,
    ) -> Result<SettingsChange, PreferencesError> {
        self.update(move |next| {
            if let Some(value) = patch.close_behavior.as_deref() {
                next.close_behavior = CloseBehavior::from_id(value)
                    .ok_or_else(|| PreferencesError::invalid("close behavior"))?;
            }
            if let Some(value) = patch.show_scrollbars {
                next.show_scrollbars = value;
            }
            if let Some(value) = patch.log_level {
                let level = value.trim().to_ascii_lowercase();
                if !matches!(
                    level.as_str(),
                    "trace" | "debug" | "info" | "warn" | "error"
                ) {
                    return Err(PreferencesError::invalid("log level"));
                }
                next.log_level = level;
            }
            Ok(())
        })
    }

    pub fn patch_appearance(
        &self,
        patch: AppearanceSettingsPatch,
    ) -> Result<SettingsChange, PreferencesError> {
        self.update(move |next| {
            if let Some(value) = patch.theme.as_deref() {
                next.appearance.theme = AppearanceTheme::from_id(value)
                    .ok_or_else(|| PreferencesError::invalid("theme"))?;
            }
            if let Some(value) = patch.accent.as_deref() {
                next.appearance.accent = AppearanceAccent::from_id(value)
                    .ok_or_else(|| PreferencesError::invalid("accent"))?;
            }
            if let Some(value) = patch.density.as_deref() {
                next.appearance.density = AppearanceDensity::from_id(value)
                    .ok_or_else(|| PreferencesError::invalid("density"))?;
            }
            if let Some(value) = patch.artwork_intensity {
                next.appearance.artwork_intensity = value;
            }
            if let Some(value) = patch.backdrop_intensity {
                next.appearance.backdrop_intensity = value;
            }
            if let Some(value) = patch.reduced_motion {
                next.appearance.reduced_motion = value;
            }
            if let Some(value) = patch.rating_sources {
                next.appearance.rating_sources = value;
            }
            Ok(())
        })
    }

    /// Installation is a shell operation, but writing its discovered path
    /// still follows the same preference pipeline as an ordinary PATCH.
    pub fn set_mpv_path(&self, path: String) -> Result<SettingsChange, PreferencesError> {
        self.update(move |next| {
            next.mpv_path = clean_path(&path);
            // Choosing the one-click installer is an explicit choice of mpv;
            // make its completed installation immediately usable even if the
            // previous backend was MPC-HC.
            next.player_backend = PlayerBackend::Mpv;
            Ok(())
        })
    }

    pub fn record_window(&self, window: WebUiWindowSettings) -> Result<(), PreferencesError> {
        self.update(move |next| {
            next.webui_window = window;
            Ok(())
        })
        .map(|_| ())
    }

    pub fn set_server_url(&self, server_url: String) -> Result<(), PreferencesError> {
        self.update(move |next| {
            next.jellyfin_url = Some(server_url);
            Ok(())
        })
        .map(|_| ())
    }

    fn update(
        &self,
        mutate: impl FnOnce(&mut AppSettings) -> Result<(), PreferencesError>,
    ) -> Result<SettingsChange, PreferencesError> {
        self.update_with_plan(mutate, |_| {})
    }

    fn update_with_plan(
        &self,
        mutate: impl FnOnce(&mut AppSettings) -> Result<(), PreferencesError>,
        augment_plan: impl FnOnce(&mut SettingsApplyPlan),
    ) -> Result<SettingsChange, PreferencesError> {
        let change = {
            let mut current = self
                .settings
                .lock()
                .map_err(|_| PreferencesError("settings service is unavailable".to_string()))?;
            let previous = current.clone();
            let mut next = previous.clone();
            mutate(&mut next)?;
            next.sanitize();
            FileSettingsStore
                .save(&next)
                .map_err(|error| PreferencesError(format!("could not save config: {error}")))?;
            let mut plan = SettingsApplyPlan::between(&previous, &next);
            augment_plan(&mut plan);
            let change = SettingsChange {
                settings: next.clone(),
                plan,
            };
            *current = next;
            change
        };
        if let Ok(listener) = self.listener.lock()
            && let Some(listener) = listener.as_ref()
        {
            let _ = listener.send(change.clone());
        }
        Ok(change)
    }
}

fn clean_path(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn set_segment(
    destination: &mut SegmentSkipMode,
    value: Option<&str>,
    field: &str,
) -> Result<(), PreferencesError> {
    if let Some(value) = value {
        *destination =
            SegmentSkipMode::from_id(value).ok_or_else(|| PreferencesError::invalid(field))?;
    }
    Ok(())
}

/// Runtime effects required after applying a preference change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SettingsApplyPlan {
    pub rebuild_player: bool,
    pub update_input_bindings: bool,
    pub update_segment_policy: bool,
    pub update_shell_css: bool,
    pub restart_required: bool,
}

impl SettingsApplyPlan {
    pub fn between(previous: &AppSettings, next: &AppSettings) -> Self {
        Self {
            rebuild_player: previous.effective_backend() != next.effective_backend()
                || match next.effective_backend() {
                    super::PlayerBackend::Mpv => previous.mpv_path != next.mpv_path,
                    super::PlayerBackend::Mpchc => previous.mpchc_path != next.mpchc_path,
                },
            update_input_bindings: false,
            update_segment_policy: previous.segment_skip_config() != next.segment_skip_config(),
            update_shell_css: previous.show_scrollbars != next.show_scrollbars,
            restart_required: previous.log_level != next.log_level,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::{SegmentSkipMode, StreamingQuality};
    use serde_json::json;

    #[test]
    fn player_patch_contract_accepts_only_writable_fields() {
        let writable = json!({
            "playerBackend": "mpv",
            "mpvPath": null,
            "mpchcPath": null,
            "defaultFullscreen": "fullscreen",
            "markWatchedNext": "w",
        });
        assert!(serde_json::from_value::<PlayerSettingsPatch>(writable).is_ok());

        let response_shape = json!({
            "playerBackend": "mpv",
            "mpvPath": null,
            "mpchcPath": null,
            "defaultFullscreen": "fullscreen",
            "markWatchedNext": "w",
            "playerConfigured": true,
        });
        assert!(serde_json::from_value::<PlayerSettingsPatch>(response_shape).is_err());
    }

    #[test]
    fn ignores_inactive_paths_and_non_destructive_window_defaults() {
        let previous = AppSettings::default();
        let mut next = previous.clone();
        next.mpchc_path = Some("other.exe".to_string());
        next.default_fullscreen = crate::preferences::FullscreenBehavior::Windowed;

        assert!(!SettingsApplyPlan::between(&previous, &next).rebuild_player);
    }

    #[test]
    fn reports_only_the_runtime_effects_required_by_a_change() {
        let previous = AppSettings::default();
        let mut next = previous.clone();
        next.mpv_path = Some("other-mpv".to_string());
        next.streaming_quality = StreamingQuality::Auto;
        next.skip_intro = SegmentSkipMode::Always;
        next.show_scrollbars = !previous.show_scrollbars;
        next.log_level = "trace".to_string();

        assert_eq!(
            SettingsApplyPlan::between(&previous, &next),
            SettingsApplyPlan {
                rebuild_player: true,
                update_input_bindings: false,
                update_segment_policy: true,
                update_shell_css: true,
                restart_required: true,
            }
        );
    }

    #[test]
    fn an_input_binding_request_is_a_live_runtime_effect() {
        let mut plan = SettingsApplyPlan::between(&AppSettings::default(), &AppSettings::default());
        plan.update_input_bindings = true;

        assert!(!plan.rebuild_player);
        assert!(plan.update_input_bindings);
    }

    #[cfg(windows)]
    #[test]
    fn switching_the_effective_backend_rebuilds_the_player() {
        let previous = AppSettings::default();
        let mut next = previous.clone();
        next.player_backend = crate::preferences::PlayerBackend::Mpchc;

        let plan = SettingsApplyPlan::between(&previous, &next);
        assert!(plan.rebuild_player);
        assert!(!plan.update_segment_policy);
        assert!(!plan.update_shell_css);
        assert!(!plan.restart_required);
    }
}
