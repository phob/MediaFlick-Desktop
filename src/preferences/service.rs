use std::fmt;
use std::sync::{Arc, Mutex, mpsc};

use serde::{Deserialize, Deserializer};

use crate::players::mpv::input::MpvInputBindings;

use super::{
    AccountConfigurationService, AccountKey, AppSettings, AppearanceAccent, AppearanceDensity,
    AppearanceTheme, CloseBehavior, FileSettingsStore, FullscreenBehavior, PlayerBackend,
    SegmentSkipMode, SettingsStore, StreamingQuality, WebUiWindowSettings,
};

/// Serialized patches accepted by the settings API.  These deliberately name
/// sections instead of exposing `AppSettings` as a generic key/value bag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NullablePatch<T> {
    #[default]
    Unchanged,
    Clear,
    Set(T),
}

impl<'de, T> Deserialize<'de> for NullablePatch<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer).map(|value| match value {
            Some(value) => Self::Set(value),
            None => Self::Clear,
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerSettingsPatch {
    pub player_backend: Option<String>,
    /// An explicit JSON `null` clears a path; an omitted field preserves it.
    #[serde(default)]
    pub mpv_path: NullablePatch<String>,
    #[serde(default)]
    pub mpchc_path: NullablePatch<String>,
    pub default_fullscreen: Option<String>,
    /// Missing leaves the binding alone; JSON `null` explicitly disables it.
    #[serde(default)]
    pub mark_watched_next: NullablePatch<String>,
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
    pub card_previews: Option<bool>,
    pub show_media_info: Option<bool>,
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
    state: Mutex<PreferencesState>,
    accounts: Arc<AccountConfigurationService>,
    listener: Mutex<Option<mpsc::Sender<SettingsChange>>>,
}

struct PreferencesState {
    settings: AppSettings,
    active_account: Option<AccountKey>,
}

impl PreferencesService {
    pub fn new(
        mut settings: AppSettings,
        accounts: Arc<AccountConfigurationService>,
        active_account: Option<AccountKey>,
    ) -> Self {
        settings.sanitize();
        settings.appearance = active_account
            .as_ref()
            .map(|key| accounts.appearance(key))
            .unwrap_or_default();
        Self {
            state: Mutex::new(PreferencesState {
                settings,
                active_account,
            }),
            accounts,
            listener: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> AppSettings {
        self.state
            .lock()
            .map(|state| state.settings.clone())
            .unwrap_or_default()
    }

    /// Selects the account-owned appearance after sign-in, or the neutral
    /// defaults after sign-out. The account document itself is retained.
    pub fn activate_account(
        &self,
        active_account: Option<AccountKey>,
    ) -> Result<SettingsChange, PreferencesError> {
        if let Some(account) = &active_account {
            self.accounts
                .claim_legacy_appearance(account)
                .map_err(|error| PreferencesError(error.to_string()))?;
        }
        let change = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| PreferencesError("settings service is unavailable".to_string()))?;
            let previous = state.settings.clone();
            let mut next = previous.clone();
            next.appearance = active_account
                .as_ref()
                .map(|key| self.accounts.appearance(key))
                .unwrap_or_default();
            state.active_account = active_account;
            state.settings = next.clone();
            let change = SettingsChange {
                plan: SettingsApplyPlan::between(&previous, &next),
                settings: next,
            };
            drop(state);
            change
        };
        self.notify(&change);
        Ok(change)
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
        let update_input_bindings = !matches!(&binding, NullablePatch::Unchanged);
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
                    next.player_backend = Some(backend);
                }
                match patch.mpv_path {
                    NullablePatch::Unchanged => {}
                    NullablePatch::Clear => next.mpv_path = None,
                    NullablePatch::Set(value) => next.mpv_path = clean_path(&value),
                }
                match patch.mpchc_path {
                    NullablePatch::Unchanged => {}
                    NullablePatch::Clear => next.mpchc_path = None,
                    NullablePatch::Set(value) => next.mpchc_path = clean_path(&value),
                }
                if let Some(value) = patch.default_fullscreen.as_deref() {
                    next.default_fullscreen = FullscreenBehavior::from_id(value)
                        .ok_or_else(|| PreferencesError::invalid("fullscreen behavior"))?;
                }
                // An unconfigured player is a valid saved state: it lets users reset
                // the section to defaults and finish choosing a backend later. Playback
                // still performs the concrete executable check before it starts.
                let save_binding = |mark_watched_next| {
                    MpvInputBindings { mark_watched_next }
                        .save()
                        .map_err(|error| {
                            PreferencesError(format!("could not save input bindings: {error}"))
                        })
                };
                match binding {
                    NullablePatch::Unchanged => {}
                    NullablePatch::Clear => save_binding(None)?,
                    NullablePatch::Set(value) => save_binding(clean_path(&value))?,
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
        let change = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| PreferencesError("settings service is unavailable".to_string()))?;
            let account = state.active_account.clone().ok_or_else(|| {
                PreferencesError("sign in to save appearance settings".to_string())
            })?;
            let previous = state.settings.clone();
            let mut next = previous.clone();
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
            if let Some(value) = patch.card_previews {
                next.appearance.card_previews = value;
            }
            if let Some(value) = patch.show_media_info {
                next.appearance.show_media_info = value;
            }
            if let Some(value) = patch.rating_sources {
                next.appearance.rating_sources = value;
            }
            next.appearance.sanitize();
            self.accounts
                .save_appearance(&account, &next.appearance)
                .map_err(|error| {
                    PreferencesError(format!("could not save account config: {error}"))
                })?;
            state.settings = next.clone();
            let change = SettingsChange {
                plan: SettingsApplyPlan::between(&previous, &next),
                settings: next,
            };
            drop(state);
            change
        };
        self.notify(&change);
        Ok(change)
    }

    /// Installation is a shell operation, but writing its discovered path
    /// still follows the same preference pipeline as an ordinary PATCH.
    pub fn set_mpv_path(&self, path: String) -> Result<SettingsChange, PreferencesError> {
        self.update(move |next| {
            next.mpv_path = clean_path(&path);
            // Choosing the one-click installer is an explicit choice of mpv;
            // make its completed installation immediately usable even if the
            // previous backend was MPC-HC.
            next.player_backend = Some(PlayerBackend::Mpv);
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
            let mut state = self
                .state
                .lock()
                .map_err(|_| PreferencesError("settings service is unavailable".to_string()))?;
            let previous = state.settings.clone();
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
            state.settings = next;
            change
        };
        self.notify(&change);
        Ok(change)
    }

    fn notify(&self, change: &SettingsChange) {
        if let Ok(listener) = self.listener.lock()
            && let Some(listener) = listener.as_ref()
        {
            let _ = listener.send(change.clone());
        }
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
        let backend_changed = previous.effective_backend() != next.effective_backend();
        let window_model_changed = backend_changed
            && (previous.effective_backend() == super::PlayerBackend::Libmpv
                || next.effective_backend() == super::PlayerBackend::Libmpv);
        Self {
            // The selected backend determines whether startup builds a normal
            // CEF window or a DirectComposition surface on mpv's window.
            rebuild_player: !window_model_changed
                && (backend_changed
                    || match next.effective_backend() {
                        super::PlayerBackend::Libmpv => false,
                        super::PlayerBackend::Mpv => previous.mpv_path != next.mpv_path,
                        super::PlayerBackend::Mpchc => previous.mpchc_path != next.mpchc_path,
                    }),
            update_input_bindings: false,
            update_segment_policy: previous.segment_skip_config() != next.segment_skip_config(),
            update_shell_css: previous.show_scrollbars != next.show_scrollbars,
            restart_required: previous.log_level != next.log_level || window_model_changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::preferences::{
        AccountConfigurationService, AccountKey, AppearanceSettings, AppearanceTheme,
        SegmentSkipMode, StreamingQuality,
    };
    use serde_json::json;

    static TEST_PATH_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn account_test_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "mediaflick-preferences-accounts-{}-{}.json",
            std::process::id(),
            TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn cleanup_account_test(path: &Path) {
        let _ = std::fs::remove_file(path);
        let mut backup = path.as_os_str().to_os_string();
        backup.push(".bak");
        let _ = std::fs::remove_file(PathBuf::from(backup));
    }

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
    fn nullable_player_fields_distinguish_omitted_clear_and_set() {
        let omitted = serde_json::from_value::<PlayerSettingsPatch>(json!({})).expect("omitted");
        assert_eq!(omitted.mpv_path, NullablePatch::Unchanged);

        let clear = serde_json::from_value::<PlayerSettingsPatch>(json!({ "mpvPath": null }))
            .expect("clear");
        assert_eq!(clear.mpv_path, NullablePatch::Clear);

        let set = serde_json::from_value::<PlayerSettingsPatch>(json!({
            "mpvPath": "C:/mpv/mpv.exe",
        }))
        .expect("set");
        assert_eq!(
            set.mpv_path,
            NullablePatch::Set("C:/mpv/mpv.exe".to_string())
        );
    }

    #[test]
    fn appearance_patch_accepts_card_preferences() {
        let patch = serde_json::from_value::<AppearanceSettingsPatch>(json!({
            "cardPreviews": false,
            "showMediaInfo": false,
        }))
        .expect("appearance patch");
        assert_eq!(patch.card_previews, Some(false));
        assert_eq!(patch.show_media_info, Some(false));
    }

    #[test]
    fn appearance_follows_the_active_account_and_survives_logout() {
        let path = account_test_path();
        let accounts = Arc::new(
            AccountConfigurationService::open(path.clone()).expect("open account settings"),
        );
        let alice = AccountKey::new("server", "alice").expect("alice account");
        let bob = AccountKey::new("server", "bob").expect("bob account");
        let service =
            PreferencesService::new(AppSettings::default(), accounts, Some(alice.clone()));

        service
            .patch_appearance(AppearanceSettingsPatch {
                theme: Some("dark".to_string()),
                ..AppearanceSettingsPatch::default()
            })
            .expect("save Alice appearance");
        assert_eq!(service.snapshot().appearance.theme, AppearanceTheme::Dark);

        service.activate_account(None).expect("log out");
        assert_eq!(service.snapshot().appearance, AppearanceSettings::default());
        service.activate_account(Some(bob)).expect("activate Bob");
        assert_eq!(service.snapshot().appearance, AppearanceSettings::default());
        service
            .activate_account(Some(alice))
            .expect("activate Alice again");
        assert_eq!(service.snapshot().appearance.theme, AppearanceTheme::Dark);

        cleanup_account_test(&path);
    }

    #[test]
    fn appearance_writes_require_an_active_account() {
        let path = account_test_path();
        let accounts = Arc::new(
            AccountConfigurationService::open(path.clone()).expect("open account settings"),
        );
        let service = PreferencesService::new(AppSettings::default(), accounts, None);

        let error = service
            .patch_appearance(AppearanceSettingsPatch {
                theme: Some("dark".to_string()),
                ..AppearanceSettingsPatch::default()
            })
            .expect_err("anonymous appearance write must fail");
        assert_eq!(error.to_string(), "sign in to save appearance settings");

        cleanup_account_test(&path);
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
        next.player_backend = Some(crate::preferences::PlayerBackend::Mpv);
        next.mpv_path = Some("other-mpv".to_string());
        next.streaming_quality = StreamingQuality::Auto;
        next.skip_intro = SegmentSkipMode::Always;
        next.show_scrollbars = !previous.show_scrollbars;
        next.log_level = "trace".to_string();

        assert_eq!(
            SettingsApplyPlan::between(&previous, &next),
            SettingsApplyPlan {
                rebuild_player: cfg!(not(windows)),
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
    fn switching_the_effective_backend_requires_a_restart() {
        let previous = AppSettings::default();
        let mut next = previous.clone();
        next.player_backend = Some(crate::preferences::PlayerBackend::Mpchc);

        let plan = SettingsApplyPlan::between(&previous, &next);
        assert!(!plan.rebuild_player);
        assert!(!plan.update_segment_policy);
        assert!(!plan.update_shell_css);
        assert!(plan.restart_required);
    }
}
