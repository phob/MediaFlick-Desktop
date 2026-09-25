use std::fmt;
use std::sync::{Arc, Mutex, mpsc};

use serde::{Deserialize, Deserializer};

use super::{
    AccountConfigurationService, AccountKey, AppSettings, AppearanceAccent, AppearanceDensity,
    CloseBehavior, FullscreenBehavior, PlayerBackend, SegmentSkipMode, StreamingQuality,
    WebUiWindowSettings, clean_binding,
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
    pub comfort: Option<super::PlayerComfort>,
    pub player_backend: Option<String>,
    /// An explicit JSON `null` clears a path; an omitted field preserves it.
    #[serde(default)]
    pub mpv_path: NullablePatch<String>,
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
        self.update(move |next| {
            if let Some(value) = patch.player_backend.as_deref() {
                next.player_backend = Some(
                    PlayerBackend::from_id(value)
                        .ok_or_else(|| PreferencesError::invalid("player backend"))?,
                );
            }
            match patch.mpv_path {
                NullablePatch::Unchanged => {}
                NullablePatch::Clear => next.mpv_path = None,
                NullablePatch::Set(value) => next.mpv_path = clean_path(&value),
            }
            if let Some(value) = patch.default_fullscreen.as_deref() {
                next.default_fullscreen = FullscreenBehavior::from_id(value)
                    .ok_or_else(|| PreferencesError::invalid("fullscreen behavior"))?;
            }
            if let Some(comfort) = patch.comfort {
                comfort
                    .validate()
                    .map_err(|error| PreferencesError(error.to_string()))?;
                next.comfort = comfort;
            }
            match patch.mark_watched_next {
                NullablePatch::Unchanged => {}
                NullablePatch::Clear => next.mark_watched_next = None,
                NullablePatch::Set(value) => next.mark_watched_next = clean_binding(Some(&value)),
            }
            // An unconfigured player is a valid saved state: it lets users reset
            // the section to defaults and finish choosing a backend later. Playback
            // still performs the concrete executable check before it starts. The
            // built-in player shares its keyboard with the comfort shortcuts, so
            // the watched binding must not collide with them there.
            if next.effective_backend() == PlayerBackend::Libmpv {
                next.comfort
                    .validate_watched_next(next.mark_watched_next.as_deref())
                    .map_err(|error| PreferencesError(error.to_string()))?;
            }
            Ok(())
        })
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
            // previous backend was the built-in player.
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
        let change = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| PreferencesError("settings service is unavailable".to_string()))?;
            let previous = state.settings.clone();
            let mut next = previous.clone();
            mutate(&mut next)?;
            next.sanitize();
            next.save()
                .map_err(|error| PreferencesError(format!("could not save config: {error}")))?;
            let plan = SettingsApplyPlan::between(&previous, &next);
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
    /// Segment skipping, subtitle styling, or the watched binding changed.
    pub update_player_preferences: bool,
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
                    }),
            update_player_preferences: previous.player_preferences() != next.player_preferences(),
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
        AccountConfigurationService, AccountKey, AppearanceAccent, AppearanceSettings,
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
    fn player_patch_contract_groups_comfort_and_watched_next()
    -> Result<(), Box<dyn std::error::Error>> {
        let payload: serde_json::Value =
            serde_json::from_str(include_str!("../../ui/test/fixtures/player-settings.json"))?;
        let patch: PlayerSettingsPatch = serde_json::from_value(payload.clone())?;
        let comfort = patch.comfort.ok_or("missing player comfort")?;
        comfort.validate()?;
        let NullablePatch::Set(watched_next) = patch.mark_watched_next else {
            return Err("missing watched-next shortcut".into());
        };
        comfort.validate_watched_next(Some(&watched_next))?;
        assert!(
            serde_json::from_value::<PlaybackSettingsPatch>(json!({
                "comfort": payload["comfort"]
            }))
            .is_err()
        );
        Ok(())
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
                accent: Some("violet".to_string()),
                ..AppearanceSettingsPatch::default()
            })
            .expect("save Alice appearance");
        assert_eq!(
            service.snapshot().appearance.accent,
            AppearanceAccent::Violet
        );

        service.activate_account(None).expect("log out");
        assert_eq!(service.snapshot().appearance, AppearanceSettings::default());
        service.activate_account(Some(bob)).expect("activate Bob");
        assert_eq!(service.snapshot().appearance, AppearanceSettings::default());
        service
            .activate_account(Some(alice))
            .expect("activate Alice again");
        assert_eq!(
            service.snapshot().appearance.accent,
            AppearanceAccent::Violet
        );

        cleanup_account_test(&path);
    }

    #[test]
    fn appearance_writes_require_an_active_account() {
        let path = account_test_path();
        let accounts = Arc::new(
            AccountConfigurationService::open(path.clone()).expect("open account settings"),
        );
        let service = PreferencesService::new(AppSettings::default(), accounts, None);

        assert!(
            service
                .patch_appearance(AppearanceSettingsPatch {
                    accent: Some("violet".to_string()),
                    ..AppearanceSettingsPatch::default()
                })
                .is_err()
        );
        assert_eq!(service.snapshot().appearance, AppearanceSettings::default());

        cleanup_account_test(&path);
    }

    #[test]
    fn ignores_inactive_paths_and_non_destructive_window_defaults() {
        let previous = AppSettings {
            player_backend: Some(crate::preferences::PlayerBackend::Libmpv),
            ..AppSettings::default()
        };
        let mut next = previous.clone();
        next.mpv_path = Some("other.exe".to_string());
        next.default_fullscreen = crate::preferences::FullscreenBehavior::Windowed;

        assert!(!SettingsApplyPlan::between(&previous, &next).rebuild_player);
    }

    #[test]
    fn mpv_path_and_scrollbar_changes_map_to_their_runtime_effects() {
        let previous = AppSettings::default();
        let mut next = previous.clone();
        next.mpv_path = Some("other-mpv".to_string());
        next.show_scrollbars = !previous.show_scrollbars;

        let plan = SettingsApplyPlan::between(&previous, &next);
        assert_eq!(plan.rebuild_player, cfg!(not(windows)));
        assert!(plan.update_shell_css);
    }

    #[test]
    fn a_watched_binding_or_subtitle_change_is_a_live_player_update() {
        let previous = AppSettings::default();
        let mut binding = previous.clone();
        binding.mark_watched_next = Some("Ctrl+w".to_string());
        let mut subtitles = previous.clone();
        subtitles.comfort.subtitle_size = 140;

        for next in [binding, subtitles] {
            let plan = SettingsApplyPlan::between(&previous, &next);
            assert!(plan.update_player_preferences);
            assert!(!plan.rebuild_player);
        }
    }

    #[cfg(windows)]
    #[test]
    fn switching_the_effective_backend_is_not_hot_swapped() {
        let previous = AppSettings::default();
        let mut next = previous.clone();
        next.player_backend = Some(crate::preferences::PlayerBackend::Mpv);

        assert!(!SettingsApplyPlan::between(&previous, &next).rebuild_player);
    }
}
