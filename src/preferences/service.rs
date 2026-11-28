use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, mpsc};

use serde::{Deserialize, Deserializer, Serialize};

use super::{
    AccountConfigurationService, AccountKey, AppSettings, AppearanceAccent, AppearanceDensity,
    CloseBehavior, FullscreenBehavior, PlayerBackend, SegmentSkipMode, StreamingQuality,
    WebUiWindowSettings, clean_binding,
};

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

#[derive(Debug, Clone, Default, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerSettingsPatch {
    pub comfort: Option<super::PlayerComfort>,
    #[ts(as = "Option<PlayerBackend>")]
    pub player_backend: Option<String>,
    #[serde(default)]
    #[ts(as = "Option<String>")]
    pub mpv_path: NullablePatch<String>,
    #[ts(as = "Option<FullscreenBehavior>")]
    pub default_fullscreen: Option<String>,
    #[serde(default)]
    #[ts(as = "Option<String>")]
    pub mark_watched_next: NullablePatch<String>,
    pub audio_passthrough: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaybackSettingsPatch {
    #[ts(as = "Option<StreamingQuality>")]
    pub streaming_quality: Option<String>,
    #[ts(as = "Option<SegmentSkipMode>")]
    pub skip_intro: Option<String>,
    #[ts(as = "Option<SegmentSkipMode>")]
    pub skip_credits: Option<String>,
    #[ts(as = "Option<SegmentSkipMode>")]
    pub skip_recap: Option<String>,
    #[ts(as = "Option<SegmentSkipMode>")]
    pub skip_commercial: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationSettingsPatch {
    #[ts(as = "Option<CloseBehavior>")]
    pub close_behavior: Option<String>,
    pub show_scrollbars: Option<bool>,
    pub log_level: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppearanceSettingsPatch {
    #[ts(as = "Option<AppearanceAccent>")]
    pub accent: Option<String>,
    #[ts(as = "Option<AppearanceDensity>")]
    pub density: Option<String>,
    pub artwork_intensity: Option<u8>,
    pub backdrop_intensity: Option<u8>,
    pub reduced_motion: Option<bool>,
    pub card_previews: Option<bool>,
    pub show_media_info: Option<bool>,
    pub rating_sources: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct SettingsChange {
    pub revision: u64,
    pub settings: AppSettings,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RestartRequired {
    pub log_level: bool,
    pub player_backend: bool,
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

pub struct PreferencesService {
    state: Mutex<PreferencesState>,
    accounts: Arc<AccountConfigurationService>,
    upgraded_from_0_1_6: bool,
    config_path: Option<PathBuf>,
    startup_log_level: String,
    startup_backend: PlayerBackend,
}

struct PreferencesState {
    settings: AppSettings,
    active_account: Option<AccountKey>,
    revision: u64,
    listener: Option<mpsc::Sender<SettingsChange>>,
}

impl PreferencesState {
    fn emit(&mut self, next: AppSettings) -> SettingsChange {
        self.revision = self.revision.saturating_add(1);
        let change = SettingsChange {
            revision: self.revision,
            settings: next.clone(),
        };
        self.settings = next;
        if let Some(listener) = self.listener.as_ref() {
            let _ = listener.send(change.clone());
        }
        change
    }
}

impl PreferencesService {
    pub fn new(
        settings: AppSettings,
        accounts: Arc<AccountConfigurationService>,
        active_account: Option<AccountKey>,
        upgraded_from_0_1_6: bool,
    ) -> Self {
        Self::with_path(
            settings,
            accounts,
            active_account,
            upgraded_from_0_1_6,
            None,
        )
    }

    #[cfg(test)]
    pub fn new_in(
        config_path: PathBuf,
        settings: AppSettings,
        accounts: Arc<AccountConfigurationService>,
        active_account: Option<AccountKey>,
        upgraded_from_0_1_6: bool,
    ) -> Self {
        Self::with_path(
            settings,
            accounts,
            active_account,
            upgraded_from_0_1_6,
            Some(config_path),
        )
    }

    fn with_path(
        mut settings: AppSettings,
        accounts: Arc<AccountConfigurationService>,
        active_account: Option<AccountKey>,
        upgraded_from_0_1_6: bool,
        config_path: Option<PathBuf>,
    ) -> Self {
        settings.sanitize();
        settings.appearance = active_account
            .as_ref()
            .map(|key| accounts.appearance(key))
            .unwrap_or_default();
        let startup_log_level = settings.log_level.clone();
        let startup_backend = settings.effective_backend();
        Self {
            state: Mutex::new(PreferencesState {
                settings,
                active_account,
                revision: 0,
                listener: None,
            }),
            accounts,
            upgraded_from_0_1_6,
            config_path,
            startup_log_level,
            startup_backend,
        }
    }

    pub fn restart_required(&self, settings: &AppSettings) -> RestartRequired {
        let backend_changed = settings.effective_backend() != self.startup_backend;
        let window_model_changed = backend_changed
            && (settings.effective_backend() == PlayerBackend::Libmpv
                || self.startup_backend == PlayerBackend::Libmpv);
        RestartRequired {
            log_level: settings.log_level != self.startup_log_level,
            player_backend: window_model_changed,
        }
    }

    pub fn snapshot(&self) -> AppSettings {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .settings
            .clone()
    }

    pub fn activate_account(
        &self,
        active_account: Option<AccountKey>,
    ) -> Result<SettingsChange, PreferencesError> {
        if let Some(account) = &active_account {
            self.accounts
                .claim_legacy_appearance(account)
                .map_err(|error| PreferencesError(error.to_string()))?;
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| PreferencesError("settings service is unavailable".to_string()))?;
        let mut next = state.settings.clone();
        next.appearance = active_account
            .as_ref()
            .map(|key| self.accounts.appearance(key))
            .unwrap_or_default();
        state.active_account = active_account;
        let change = state.emit(next);
        drop(state);
        Ok(change)
    }

    pub fn subscribe(&self) -> mpsc::Receiver<SettingsChange> {
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut state) = self.state.lock() {
            state.listener = Some(sender);
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
                if next.effective_backend() == PlayerBackend::Libmpv {
                    comfort
                        .validate()
                        .map_err(|error| PreferencesError(error.to_string()))?;
                    next.comfort = comfort;
                } else if comfort.validate().is_ok() {
                    next.comfort = comfort;
                }
            }
            match patch.mark_watched_next {
                NullablePatch::Unchanged => {}
                NullablePatch::Clear => next.mark_watched_next = None,
                NullablePatch::Set(value) => next.mark_watched_next = clean_binding(Some(&value)),
            }
            if let Some(enabled) = patch.audio_passthrough {
                next.audio_passthrough = enabled;
            }
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
        account: &AccountKey,
        patch: AppearanceSettingsPatch,
    ) -> Result<SettingsChange, PreferencesError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PreferencesError("settings service is unavailable".to_string()))?;
        let mut appearance = self.accounts.appearance(account);
        if let Some(value) = patch.accent.as_deref() {
            appearance.accent = AppearanceAccent::from_id(value)
                .ok_or_else(|| PreferencesError::invalid("accent"))?;
        }
        if let Some(value) = patch.density.as_deref() {
            appearance.density = AppearanceDensity::from_id(value)
                .ok_or_else(|| PreferencesError::invalid("density"))?;
        }
        if let Some(value) = patch.artwork_intensity {
            appearance.artwork_intensity = value;
        }
        if let Some(value) = patch.backdrop_intensity {
            appearance.backdrop_intensity = value;
        }
        if let Some(value) = patch.reduced_motion {
            appearance.reduced_motion = value;
        }
        if let Some(value) = patch.card_previews {
            appearance.card_previews = value;
        }
        if let Some(value) = patch.show_media_info {
            appearance.show_media_info = value;
        }
        if let Some(value) = patch.rating_sources {
            appearance.rating_sources = value;
        }
        appearance.sanitize();
        self.accounts
            .save_appearance(account, &appearance)
            .map_err(|error| PreferencesError(format!("could not save account config: {error}")))?;
        let mut next = state.settings.clone();
        next.appearance = appearance;
        let change = if state.active_account.as_ref() == Some(account) {
            state.emit(next)
        } else {
            SettingsChange {
                revision: state.revision,
                settings: next,
            }
        };
        drop(state);
        Ok(change)
    }

    pub fn set_mpv_path(&self, path: String) -> Result<SettingsChange, PreferencesError> {
        self.update(move |next| {
            next.mpv_path = clean_path(&path);
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

    pub fn whats_new_pending(&self) -> bool {
        self.upgraded_from_0_1_6 && !self.snapshot().whats_new_seen
    }

    pub fn close_whats_new(&self) -> Result<(), PreferencesError> {
        self.update(|next| {
            next.whats_new_seen = true;
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
        let mut state = self
            .state
            .lock()
            .map_err(|_| PreferencesError("settings service is unavailable".to_string()))?;
        let mut next = state.settings.clone();
        mutate(&mut next)?;
        next.sanitize();
        match &self.config_path {
            Some(path) => next.save_to(path),
            None => next.save(),
        }
        .map_err(|error| PreferencesError(format!("could not save config: {error}")))?;
        let change = state.emit(next);
        drop(state);
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SettingsApplyPlan {
    pub rebuild_player: bool,
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
            PreferencesService::new(AppSettings::default(), accounts, Some(alice.clone()), false);

        service
            .patch_appearance(
                &alice,
                AppearanceSettingsPatch {
                    accent: Some("violet".to_string()),
                    ..AppearanceSettingsPatch::default()
                },
            )
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
    fn appearance_writes_target_the_supplied_account_without_clobbering_the_active_one() {
        let path = account_test_path();
        let accounts = Arc::new(
            AccountConfigurationService::open(path.clone()).expect("open account settings"),
        );
        let alice = AccountKey::new("server", "alice").expect("alice account");
        let bob = AccountKey::new("server", "bob").expect("bob account");
        let service = PreferencesService::new(
            AppSettings::default(),
            accounts.clone(),
            Some(alice.clone()),
            false,
        );

        service
            .patch_appearance(
                &alice,
                AppearanceSettingsPatch {
                    accent: Some("amber".to_string()),
                    ..AppearanceSettingsPatch::default()
                },
            )
            .expect("save Alice appearance");
        service
            .patch_appearance(
                &bob,
                AppearanceSettingsPatch {
                    accent: Some("violet".to_string()),
                    ..AppearanceSettingsPatch::default()
                },
            )
            .expect("save Bob appearance");

        assert_eq!(
            service.snapshot().appearance.accent,
            AppearanceAccent::Amber
        );
        assert_eq!(accounts.appearance(&alice).accent, AppearanceAccent::Amber);
        assert_eq!(accounts.appearance(&bob).accent, AppearanceAccent::Violet);

        cleanup_account_test(&path);
    }

    #[test]
    fn switching_to_external_mpv_ignores_hidden_builtin_shortcut_conflicts_without_corrupting_comfort()
     {
        let accounts_path = account_test_path();
        let settings_path = account_test_path();
        let accounts =
            Arc::new(AccountConfigurationService::open(accounts_path.clone()).expect("accounts"));
        let initial = AppSettings {
            player_backend: Some(PlayerBackend::Libmpv),
            ..AppSettings::default()
        };
        let original_comfort = initial.comfort.clone();
        let service =
            PreferencesService::new_in(settings_path.clone(), initial, accounts, None, false);

        let conflicting = super::super::PlayerComfort {
            stop_key: "k".into(),
            pause_key: "k".into(),
            ..Default::default()
        };
        let libmpv_err = service.patch_player(PlayerSettingsPatch {
            player_backend: Some("libmpv".to_string()),
            comfort: Some(conflicting.clone()),
            ..Default::default()
        });
        assert!(libmpv_err.is_err());

        let mpv_saved = service.patch_player(PlayerSettingsPatch {
            player_backend: Some("mpv".to_string()),
            comfort: Some(conflicting),
            ..Default::default()
        });
        assert!(mpv_saved.is_ok(), "{mpv_saved:?}");
        let snapshot = service.snapshot();
        assert_eq!(snapshot.effective_backend(), PlayerBackend::Mpv);
        assert_eq!(snapshot.comfort, original_comfort);

        cleanup_account_test(&accounts_path);
        cleanup_account_test(&settings_path);
    }

    #[test]
    fn concurrent_updates_emit_monotonically_ordered_revisions_matching_final_snapshot() {
        let accounts_path = account_test_path();
        let settings_path = account_test_path();
        let accounts =
            Arc::new(AccountConfigurationService::open(accounts_path.clone()).expect("accounts"));
        let service = Arc::new(PreferencesService::new_in(
            settings_path.clone(),
            AppSettings::default(),
            accounts,
            None,
            false,
        ));
        let receiver = service.subscribe();

        let workers: Vec<_> = (0..4)
            .map(|worker| {
                let service = Arc::clone(&service);
                std::thread::spawn(move || {
                    for step in 0..8 {
                        let show = (worker + step) % 2 == 0;
                        service
                            .patch_application(ApplicationSettingsPatch {
                                show_scrollbars: Some(show),
                                ..Default::default()
                            })
                            .expect("patch application");
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("worker");
        }

        let emitted: Vec<SettingsChange> = receiver.try_iter().collect();
        assert_eq!(emitted.len(), 32);
        for (index, change) in emitted.iter().enumerate() {
            assert_eq!(change.revision, (index + 1) as u64);
        }
        assert_eq!(
            emitted.last().expect("last").settings.show_scrollbars,
            service.snapshot().show_scrollbars
        );

        cleanup_account_test(&accounts_path);
        cleanup_account_test(&settings_path);
    }

    #[test]
    fn restart_required_persists_across_unrelated_saves_and_clears_when_reverted() {
        let accounts_path = account_test_path();
        let settings_path = account_test_path();
        let accounts =
            Arc::new(AccountConfigurationService::open(accounts_path.clone()).expect("accounts"));
        let initial = AppSettings {
            player_backend: Some(PlayerBackend::Libmpv),
            log_level: "debug".to_string(),
            ..AppSettings::default()
        };
        let service =
            PreferencesService::new_in(settings_path.clone(), initial, accounts, None, false);

        assert_eq!(
            service.restart_required(&service.snapshot()),
            RestartRequired {
                log_level: false,
                player_backend: false,
            }
        );

        let change = service
            .patch_application(ApplicationSettingsPatch {
                log_level: Some("warn".to_string()),
                ..Default::default()
            })
            .expect("warn");
        assert!(service.restart_required(&change.settings).log_level);

        let unrelated = service
            .patch_application(ApplicationSettingsPatch {
                show_scrollbars: Some(true),
                ..Default::default()
            })
            .expect("scrollbars");
        assert!(service.restart_required(&unrelated.settings).log_level);

        let backend = service
            .patch_player(PlayerSettingsPatch {
                player_backend: Some("mpv".to_string()),
                ..Default::default()
            })
            .expect("mpv");
        assert_eq!(
            service.restart_required(&backend.settings),
            RestartRequired {
                log_level: true,
                player_backend: true,
            }
        );

        let reverted_log = service
            .patch_application(ApplicationSettingsPatch {
                log_level: Some("debug".to_string()),
                ..Default::default()
            })
            .expect("debug");
        assert_eq!(
            service.restart_required(&reverted_log.settings),
            RestartRequired {
                log_level: false,
                player_backend: true,
            }
        );

        let reverted_backend = service
            .patch_player(PlayerSettingsPatch {
                player_backend: Some("libmpv".to_string()),
                ..Default::default()
            })
            .expect("libmpv");
        assert_eq!(
            service.restart_required(&reverted_backend.settings),
            RestartRequired {
                log_level: false,
                player_backend: false,
            }
        );

        cleanup_account_test(&accounts_path);
        cleanup_account_test(&settings_path);
    }
}
