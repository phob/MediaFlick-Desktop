use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::integrations::letterboxd::{ExternalProfile, MAX_CONNECTED_PROFILES};

use super::AppearanceSettings;
use super::json_file::{
    RecoveryNotice, load_with_recovery, replace_backup_with_primary, save_with_backup,
};
use super::store::config_dir;

const ACCOUNT_CONFIG_VERSION: u32 = 1;

/// Stable identity for one Jellyfin account. The same user id on another
/// server is a different account and must not inherit its preferences.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountKey {
    server_id: String,
    user_id: String,
}

impl AccountKey {
    pub fn new(server_id: impl Into<String>, user_id: impl Into<String>) -> Option<Self> {
        let server_id = server_id.into();
        let user_id = user_id.into();
        if server_id.trim().is_empty() || user_id.trim().is_empty() {
            return None;
        }
        Some(Self { server_id, user_id })
    }

    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountConfiguration {
    #[serde(flatten)]
    key: AccountKey,
    #[serde(default, skip_serializing_if = "AppearanceSettings::is_default")]
    appearance: AppearanceSettings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    letterboxd_profiles: Vec<ExternalProfile>,
}

impl AccountConfiguration {
    fn new(key: AccountKey) -> Self {
        Self {
            key,
            appearance: AppearanceSettings::default(),
            letterboxd_profiles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountConfigurationFile {
    version: u32,
    #[serde(default)]
    accounts: Vec<AccountConfiguration>,
}

impl Default for AccountConfigurationFile {
    fn default() -> Self {
        Self {
            version: ACCOUNT_CONFIG_VERSION,
            accounts: Vec::new(),
        }
    }
}

/// The sole owner of small, durable settings associated with Jellyfin users.
/// Catalog rebuilds never open this file. Mutations update the in-memory copy
/// only after the atomic replacement succeeds.
pub struct AccountConfigurationService {
    path: PathBuf,
    document: Mutex<AccountConfigurationFile>,
    recovery: Mutex<Option<RecoveryNotice>>,
}

impl AccountConfigurationService {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let loaded = load_with_recovery(&path)?;
        let recovery = loaded.as_ref().and_then(|loaded| loaded.recovery.clone());
        let mut document =
            loaded.map_or_else(AccountConfigurationFile::default, |loaded| loaded.document);
        validate_document(&mut document)?;
        Ok(Self {
            path,
            document: Mutex::new(document),
            recovery: Mutex::new(recovery),
        })
    }

    pub fn take_recovery_notice(&self) -> Option<RecoveryNotice> {
        self.recovery
            .lock()
            .ok()
            .and_then(|mut recovery| recovery.take())
    }

    pub fn appearance(&self, key: &AccountKey) -> AppearanceSettings {
        self.with_document(|document| {
            document
                .accounts
                .iter()
                .find(|account| account.key == *key)
                .map(|account| account.appearance.clone())
                .unwrap_or_default()
        })
    }

    pub fn save_appearance(
        &self,
        key: &AccountKey,
        appearance: &AppearanceSettings,
    ) -> io::Result<()> {
        self.mutate(|document| {
            account_mut(document, key).appearance = appearance.clone();
            Ok(())
        })
    }

    pub fn letterboxd_profiles(&self, key: &AccountKey) -> Vec<ExternalProfile> {
        self.with_document(|document| {
            let mut profiles = document
                .accounts
                .iter()
                .find(|account| account.key == *key)
                .map(|account| account.letterboxd_profiles.clone())
                .unwrap_or_default();
            profiles.sort_by_key(|profile| std::cmp::Reverse(profile.created_at));
            for profile in &mut profiles {
                apply_scope(profile, key);
            }
            profiles
        })
    }

    pub fn save_letterboxd_profile(
        &self,
        key: &AccountKey,
        profile: &ExternalProfile,
    ) -> io::Result<ExternalProfile> {
        let mut profile = profile.clone();
        apply_scope(&mut profile, key);
        self.mutate(|document| {
            let profiles = &mut account_mut(document, key).letterboxd_profiles;
            if let Some(index) = profiles.iter().position(|stored| stored.id == profile.id) {
                profiles[index] = profile.clone();
            } else if let Some(index) = profiles
                .iter()
                .position(|stored| stored.profile_key == profile.profile_key)
            {
                profile.id.clone_from(&profiles[index].id);
                profile.created_at = profiles[index].created_at;
                profiles[index] = profile.clone();
            } else {
                profiles.push(profile.clone());
            }
            Ok(profile)
        })
    }

    pub fn letterboxd_profile(&self, key: &AccountKey, id: &str) -> Option<ExternalProfile> {
        self.with_document(|document| {
            document
                .accounts
                .iter()
                .find(|account| account.key == *key)
                .and_then(|account| {
                    account
                        .letterboxd_profiles
                        .iter()
                        .find(|profile| profile.id == id)
                })
                .cloned()
                .map(|mut profile| {
                    apply_scope(&mut profile, key);
                    profile
                })
        })
    }

    pub fn set_letterboxd_profile_enabled(
        &self,
        key: &AccountKey,
        id: &str,
        enabled: bool,
    ) -> io::Result<Option<ExternalProfile>> {
        self.mutate(|document| {
            let Some(account) = document
                .accounts
                .iter_mut()
                .find(|account| account.key == *key)
            else {
                return Ok(None);
            };
            let Some(profile) = account
                .letterboxd_profiles
                .iter_mut()
                .find(|profile| profile.id == id)
            else {
                return Ok(None);
            };
            profile.enabled = enabled;
            let mut saved = profile.clone();
            apply_scope(&mut saved, key);
            Ok(Some(saved))
        })
    }

    pub fn remove_letterboxd_profile(&self, key: &AccountKey, id: &str) -> io::Result<bool> {
        self.mutate(|document| {
            let Some(account) = document
                .accounts
                .iter_mut()
                .find(|account| account.key == *key)
            else {
                return Ok(false);
            };
            let previous = account.letterboxd_profiles.len();
            account
                .letterboxd_profiles
                .retain(|profile| profile.id != id);
            Ok(account.letterboxd_profiles.len() != previous)
        })
    }

    pub fn remove_account(&self, key: &AccountKey) -> io::Result<bool> {
        let removed = self.mutate(|document| {
            let previous = document.accounts.len();
            document.accounts.retain(|account| account.key != *key);
            Ok(document.accounts.len() != previous)
        })?;
        scrub_backup(&self.path)?;
        Ok(removed)
    }

    fn with_document<T>(&self, read: impl FnOnce(&AccountConfigurationFile) -> T) -> T {
        let document = self
            .document
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        read(&document)
    }

    fn mutate<T>(
        &self,
        change: impl FnOnce(&mut AccountConfigurationFile) -> io::Result<T>,
    ) -> io::Result<T> {
        // File replacement is part of this critical section. Releasing the
        // guard before it completes would let two API requests both save an
        // older clone and silently discard whichever mutation committed first.
        let mut current = self
            .document
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = current.clone();
        let result = change(&mut next)?;
        validate_document(&mut next)?;
        save_document(&self.path, &next)?;
        *current = next;
        drop(current);
        Ok(result)
    }
}

pub fn accounts_file_path() -> PathBuf {
    config_dir().join("accounts.json")
}

fn account_mut<'a>(
    document: &'a mut AccountConfigurationFile,
    key: &AccountKey,
) -> &'a mut AccountConfiguration {
    let index = document
        .accounts
        .iter()
        .position(|account| account.key == *key)
        .unwrap_or_else(|| {
            document
                .accounts
                .push(AccountConfiguration::new(key.clone()));
            document.accounts.len() - 1
        });
    &mut document.accounts[index]
}

fn apply_scope(profile: &mut ExternalProfile, key: &AccountKey) {
    profile.jellyfin_server_id.clone_from(&key.server_id);
    profile.jellyfin_user_id.clone_from(&key.user_id);
}

fn validate_document(document: &mut AccountConfigurationFile) -> io::Result<()> {
    if document.version != ACCOUNT_CONFIG_VERSION {
        return Err(invalid_data(format!(
            "unsupported account configuration version {}",
            document.version
        )));
    }
    let mut accounts = HashSet::new();
    for account in &mut document.accounts {
        if !accounts.insert(account.key.clone()) {
            return Err(invalid_data(
                "account configuration contains a duplicate account",
            ));
        }
        account.appearance.sanitize();
        if account.letterboxd_profiles.len() > MAX_CONNECTED_PROFILES {
            return Err(invalid_data(
                "account configuration contains too many Letterboxd profiles",
            ));
        }
        let mut ids = HashSet::new();
        let mut usernames = HashSet::new();
        for profile in &account.letterboxd_profiles {
            if !profile.valid_for_storage()
                || !ids.insert(profile.id.clone())
                || !usernames.insert(profile.profile_key.clone())
            {
                return Err(invalid_data(
                    "account configuration contains an invalid Letterboxd profile",
                ));
            }
        }
    }
    Ok(())
}

fn save_document(path: &Path, document: &AccountConfigurationFile) -> io::Result<()> {
    save_with_backup(path, document)
}

fn scrub_backup(path: &Path) -> io::Result<()> {
    replace_backup_with_primary(path)
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::preferences::{AppearanceAccent, AppearanceTheme};

    static TEST_PATH_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn test_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mediaflick-accounts-{label}-{}-{}.json",
            std::process::id(),
            TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn key(server: &str, user: &str) -> AccountKey {
        AccountKey::new(server, user).expect("valid account")
    }

    fn profile() -> ExternalProfile {
        ExternalProfile {
            id: "0123456789abcdef".to_string(),
            provider: "letterboxd".to_string(),
            profile_key: "alice".to_string(),
            display_name: "Alice".to_string(),
            canonical_url: "https://letterboxd.com/alice/".to_string(),
            enabled: true,
            verification_status: "verified".to_string(),
            created_at: 1,
            last_checked_at: Some(2),
            jellyfin_server_id: String::new(),
            jellyfin_user_id: String::new(),
        }
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(super::super::json_file::backup_path(path));
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
            && let Ok(entries) = std::fs::read_dir(parent)
        {
            let prefix = format!("{}.broken-", name.to_string_lossy());
            for entry in entries.filter_map(Result::ok) {
                if entry.file_name().to_string_lossy().starts_with(&prefix) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    #[test]
    fn account_settings_are_isolated_and_survive_reopen() {
        let path = test_path("round-trip");
        let alice = key("server", "alice");
        let bob = key("server", "bob");
        let service = AccountConfigurationService::open(path.clone()).expect("open");
        let appearance = AppearanceSettings {
            theme: AppearanceTheme::Dark,
            accent: AppearanceAccent::Violet,
            ..AppearanceSettings::default()
        };
        service
            .save_appearance(&alice, &appearance)
            .expect("save appearance");
        service
            .save_letterboxd_profile(&alice, &profile())
            .expect("save profile");
        let saved_json = std::fs::read_to_string(&path).expect("read saved account settings");
        assert!(!saved_json.contains("jellyfinServerId"));
        assert!(!saved_json.contains("jellyfinUserId"));

        assert_eq!(service.appearance(&bob), AppearanceSettings::default());
        assert!(service.letterboxd_profiles(&bob).is_empty());
        drop(service);

        let reopened = AccountConfigurationService::open(path.clone()).expect("reopen");
        assert_eq!(reopened.appearance(&alice), appearance);
        let profiles = reopened.letterboxd_profiles(&alice);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].jellyfin_server_id, "server");
        assert_eq!(profiles[0].jellyfin_user_id, "alice");
        cleanup(&path);
    }

    #[test]
    fn adding_the_same_letterboxd_username_preserves_its_identity() {
        let path = test_path("profile-upsert");
        let alice = key("server", "alice");
        let service = AccountConfigurationService::open(path.clone()).expect("open");
        let original = service
            .save_letterboxd_profile(&alice, &profile())
            .expect("save original profile");
        let mut replacement = profile();
        replacement.id = "fedcba9876543210".to_string();
        replacement.display_name = "Alice Updated".to_string();
        replacement.created_at = 99;

        let updated = service
            .save_letterboxd_profile(&alice, &replacement)
            .expect("update profile");

        assert_eq!(updated.id, original.id);
        assert_eq!(updated.created_at, original.created_at);
        assert_eq!(updated.display_name, "Alice Updated");
        assert_eq!(service.letterboxd_profiles(&alice), vec![updated]);
        cleanup(&path);
    }

    #[test]
    fn malformed_configuration_is_moved_aside_and_isolated_to_defaults() {
        let path = test_path("malformed");
        let contents = b"{ definitely not json";
        std::fs::write(&path, contents).expect("write malformed file");

        let service = AccountConfigurationService::open(path.clone()).expect("open defaults");
        assert_eq!(
            service.appearance(&key("server", "user")),
            AppearanceSettings::default()
        );
        assert!(!path.exists());
        assert!(path.parent().is_some_and(|parent| {
            std::fs::read_dir(parent).is_ok_and(|entries| {
                entries.filter_map(Result::ok).any(|entry| {
                    entry.file_name().to_string_lossy().starts_with(&format!(
                        "{}.broken-",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ))
                })
            })
        }));
        cleanup(&path);
    }

    #[test]
    fn a_newer_configuration_version_is_left_untouched() {
        let path = test_path("future");
        let contents = br#"{"version":2,"accounts":[]}"#;
        std::fs::write(&path, contents).expect("write future file");

        let error = AccountConfigurationService::open(path.clone())
            .err()
            .expect("future file must fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(std::fs::read(&path).expect("read original"), contents);
        cleanup(&path);
    }
}
