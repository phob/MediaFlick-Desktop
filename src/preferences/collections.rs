use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::collections::{CollectionMode, CollectionProfile, ProviderReadiness};

use super::AccountKey;
use super::json_file::{RecoveryNotice, load_with_recovery, save_with_backup};
use super::store::config_dir;

const COLLECTION_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FranchiseSettings {
    #[serde(default)]
    pub include_unreleased: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionAccountSettings {
    #[serde(flatten)]
    key: AccountKey,
    #[serde(default)]
    pub mode_selection: Option<CollectionMode>,
    #[serde(default)]
    pub franchises: FranchiseSettings,
    #[serde(default)]
    pub profiles: Vec<CollectionProfile>,
}

impl CollectionAccountSettings {
    fn new(key: AccountKey) -> Self {
        Self {
            key,
            mode_selection: None,
            franchises: FranchiseSettings::default(),
            profiles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectionConfigurationFile {
    version: u32,
    #[serde(default)]
    accounts: Vec<CollectionAccountSettings>,
}

impl Default for CollectionConfigurationFile {
    fn default() -> Self {
        Self {
            version: COLLECTION_CONFIG_VERSION,
            accounts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct State {
    document: CollectionConfigurationFile,
    recovery: Option<RecoveryNotice>,
}

/// Owns all non-rebuildable collection intent for every Jellyfin account.
/// Mutations serialize and replace the whole document before publishing the
/// new in-memory state.
pub struct CollectionConfigurationService {
    path: PathBuf,
    state: Mutex<State>,
}

impl CollectionConfigurationService {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        reject_unsupported_version(&path)?;
        let loaded = load_with_recovery::<CollectionConfigurationFile>(&path)?;
        let (document, recovery) = match loaded {
            None => (CollectionConfigurationFile::default(), None),
            Some(loaded) if loaded.document.version == COLLECTION_CONFIG_VERSION => {
                (loaded.document, loaded.recovery)
            }
            Some(loaded) => {
                return Err(invalid_data(format!(
                    "unsupported collections configuration version {}",
                    loaded.document.version
                )));
            }
        };
        validate_accounts(&document)?;
        Ok(Self {
            path,
            state: Mutex::new(State { document, recovery }),
        })
    }

    pub fn take_recovery_notice(&self) -> Option<RecoveryNotice> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.recovery.take()
    }

    pub fn account(&self, key: &AccountKey) -> CollectionAccountSettings {
        self.with_state(|state| {
            state
                .document
                .accounts
                .iter()
                .find(|account| account.key == *key)
                .cloned()
                .unwrap_or_else(|| CollectionAccountSettings::new(key.clone()))
        })
    }

    pub fn effective_mode(
        &self,
        key: &AccountKey,
        readiness: &ProviderReadiness,
        has_cached_results: bool,
    ) -> CollectionMode {
        let account = self.account(key);
        account.mode_selection.unwrap_or({
            if readiness.tmdb || !account.profiles.is_empty() || has_cached_results {
                CollectionMode::MediaFlick
            } else {
                CollectionMode::Jellyfin
            }
        })
    }

    pub fn set_mode(
        &self,
        key: &AccountKey,
        mode: CollectionMode,
    ) -> io::Result<CollectionAccountSettings> {
        self.mutate(|document| {
            let account = account_mut(document, key);
            account.mode_selection = Some(mode);
            Ok(account.clone())
        })
    }

    pub fn set_include_unreleased(
        &self,
        key: &AccountKey,
        include_unreleased: bool,
    ) -> io::Result<CollectionAccountSettings> {
        self.mutate(|document| {
            let account = account_mut(document, key);
            account.franchises.include_unreleased = include_unreleased;
            Ok(account.clone())
        })
    }

    pub fn save_profile(
        &self,
        key: &AccountKey,
        profile: CollectionProfile,
    ) -> io::Result<CollectionProfile> {
        profile.validate().map_err(invalid_data)?;
        self.mutate(|document| {
            let account = account_mut(document, key);
            if account.profiles.iter().any(|candidate| {
                candidate.id != profile.id
                    && candidate
                        .title
                        .trim()
                        .eq_ignore_ascii_case(profile.title.trim())
            }) {
                return Err(invalid_data(
                    "collection names must be unique for this account",
                ));
            }
            if let Some(index) = account
                .profiles
                .iter()
                .position(|candidate| candidate.id == profile.id)
            {
                account.profiles[index] = profile.clone();
            } else {
                account.profiles.push(profile.clone());
            }
            Ok(profile)
        })
    }

    pub fn reorder_profiles(&self, key: &AccountKey, ids: &[String]) -> io::Result<()> {
        self.mutate(|document| {
            let account = account_mut(document, key);
            if ids.len() != account.profiles.len() {
                return Err(invalid_data("the collection order is incomplete"));
            }
            let mut profiles: HashMap<String, CollectionProfile> = account
                .profiles
                .drain(..)
                .map(|profile| (profile.id.clone(), profile))
                .collect();
            if profiles.len() != ids.len() {
                return Err(invalid_data("the collection order contains duplicate ids"));
            }
            let mut ordered = Vec::with_capacity(ids.len());
            for id in ids {
                let Some(profile) = profiles.remove(id) else {
                    return Err(invalid_data("the collection order contains an unknown id"));
                };
                ordered.push(profile);
            }
            account.profiles = ordered;
            Ok(())
        })
    }

    pub fn remove_profile(
        &self,
        key: &AccountKey,
        profile_id: &str,
    ) -> io::Result<Option<CollectionProfile>> {
        self.mutate(|document| {
            let account = account_mut(document, key);
            let Some(index) = account
                .profiles
                .iter()
                .position(|profile| profile.id == profile_id)
            else {
                return Ok(None);
            };
            Ok(Some(account.profiles.remove(index)))
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

    pub fn artwork_ids_for_account(&self, key: &AccountKey) -> Vec<String> {
        let mut ids = self
            .account(key)
            .profiles
            .into_iter()
            .filter_map(|profile| profile.custom_poster_id)
            .collect::<HashSet<_>>();
        let backup = super::json_file::backup_path(&self.path);
        if let Ok(bytes) = std::fs::read(backup)
            && let Ok(document) = serde_json::from_slice::<CollectionConfigurationFile>(&bytes)
            && let Some(account) = document
                .accounts
                .into_iter()
                .find(|account| account.key == *key)
        {
            ids.extend(
                account
                    .profiles
                    .into_iter()
                    .filter_map(|profile| profile.custom_poster_id),
            );
        }
        ids.into_iter().collect()
    }

    pub fn profile_errors(&self, key: &AccountKey) -> HashMap<String, String> {
        let account = self.account(key);
        let mut titles = HashSet::new();
        let mut errors = HashMap::new();
        for profile in account.profiles {
            let normalized_title = profile.title.trim().to_lowercase();
            let validation = profile.validate().err().map(str::to_string).or_else(|| {
                (!titles.insert(normalized_title))
                    .then(|| "another collection has the same name".to_string())
            });
            if let Some(error) = validation {
                errors.insert(profile.id, error);
            }
        }
        errors
    }

    fn with_state<T>(&self, read: impl FnOnce(&State) -> T) -> T {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        read(&state)
    }

    fn mutate<T>(
        &self,
        change: impl FnOnce(&mut CollectionConfigurationFile) -> io::Result<T>,
    ) -> io::Result<T> {
        let mut current = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = current.document.clone();
        let result = change(&mut next)?;
        validate_accounts(&next)?;
        save_with_backup(&self.path, &next)?;
        current.document = next;
        drop(current);
        Ok(result)
    }
}

fn reject_unsupported_version(path: &Path) -> io::Result<()> {
    let Some(version) = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.get("version").and_then(serde_json::Value::as_u64))
        .filter(|version| *version != u64::from(COLLECTION_CONFIG_VERSION))
    else {
        return Ok(());
    };
    Err(invalid_data(format!(
        "unsupported collections configuration version {version}"
    )))
}

pub fn collections_file_path() -> PathBuf {
    config_dir().join("collections.json")
}

fn account_mut<'a>(
    document: &'a mut CollectionConfigurationFile,
    key: &AccountKey,
) -> &'a mut CollectionAccountSettings {
    let index = document
        .accounts
        .iter()
        .position(|account| account.key == *key)
        .unwrap_or_else(|| {
            document
                .accounts
                .push(CollectionAccountSettings::new(key.clone()));
            document.accounts.len() - 1
        });
    &mut document.accounts[index]
}

fn validate_accounts(document: &CollectionConfigurationFile) -> io::Result<()> {
    let mut accounts = HashSet::new();
    for account in &document.accounts {
        if !accounts.insert(account.key.clone()) {
            return Err(invalid_data(
                "collections configuration contains a duplicate account",
            ));
        }
        let mut profile_ids = HashSet::new();
        if account
            .profiles
            .iter()
            .any(|profile| !profile_ids.insert(profile.id.clone()))
        {
            return Err(invalid_data(
                "collections configuration contains a duplicate profile id",
            ));
        }
    }
    Ok(())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn scrub_backup(path: &Path) -> io::Result<()> {
    super::json_file::replace_backup_with_primary(path)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::collections::{
        CollectionSource, MediaType, RefreshCadence, ResultLimit, TemplateReference,
    };

    use super::*;

    static COUNTER: AtomicU64 = AtomicU64::new(1);

    fn test_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "mediaflick-collections-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn key(server: &str, user: &str) -> AccountKey {
        AccountKey::new(server, user).expect("account key")
    }

    fn profile(id: &str, title: &str) -> CollectionProfile {
        CollectionProfile {
            id: id.repeat(16 / id.len()),
            revision: "b".repeat(16),
            template: TemplateReference {
                id: "tmdb.discover.popular-movies".to_string(),
            },
            title: title.to_string(),
            description: String::new(),
            custom_poster_id: None,
            source: CollectionSource::TmdbDiscover {
                parameters: BTreeMap::new(),
            },
            media_type: MediaType::Movie,
            limit: ResultLimit::All,
            cadence: RefreshCadence::Daily,
        }
    }

    #[test]
    fn accounts_and_profile_order_are_isolated_and_durable() {
        let path = test_path();
        let service = CollectionConfigurationService::open(path.clone()).expect("open");
        let alice = key("server", "alice");
        let bob = key("server", "bob");
        let first = profile("a", "First");
        let second = profile("c", "Second");
        service.save_profile(&alice, first.clone()).expect("first");
        service
            .save_profile(&alice, second.clone())
            .expect("second");
        service
            .reorder_profiles(&alice, &[second.id.clone(), first.id])
            .expect("reorder");

        assert!(service.account(&bob).profiles.is_empty());
        drop(service);
        let reopened = CollectionConfigurationService::open(path.clone()).expect("reopen");
        assert_eq!(reopened.account(&alice).profiles[0].id, second.id);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(super::super::json_file::backup_path(&path));
    }

    #[test]
    fn profile_names_are_unique_without_case_sensitivity() {
        let path = test_path();
        let service = CollectionConfigurationService::open(path.clone()).expect("open");
        let account = key("server", "alice");
        service
            .save_profile(&account, profile("a", "Popular Movies"))
            .expect("first");
        let error = service
            .save_profile(&account, profile("c", " popular movies "))
            .expect_err("duplicate name");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_newer_file_is_rejected_without_being_modified() {
        let path = test_path();
        let bytes = br#"{"version":2,"futureTopLevel":true,"accounts":[]}"#;
        std::fs::write(&path, bytes).expect("future file");

        assert!(CollectionConfigurationService::open(path.clone()).is_err());
        assert_eq!(std::fs::read(&path).expect("unchanged"), bytes);
        let _ = std::fs::remove_file(&path);
    }
}
