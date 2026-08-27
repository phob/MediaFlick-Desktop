use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::library::ItemPlaybackPreference;

use super::AccountKey;
use super::json_file::{RecoveryNotice, load_with_recovery, save_with_backup};
use super::store::config_dir;

const PLAYBACK_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredPlaybackPreference {
    item_id: String,
    #[serde(flatten)]
    preference: ItemPlaybackPreference,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlaybackAccount {
    #[serde(flatten)]
    key: AccountKey,
    #[serde(default)]
    items: Vec<StoredPlaybackPreference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlaybackPreferenceFile {
    version: u32,
    #[serde(default)]
    accounts: Vec<PlaybackAccount>,
}

impl Default for PlaybackPreferenceFile {
    fn default() -> Self {
        Self {
            version: PLAYBACK_CONFIG_VERSION,
            accounts: Vec::new(),
        }
    }
}

pub struct PlaybackPreferenceService {
    path: PathBuf,
    document: Mutex<PlaybackPreferenceFile>,
    recovery: Mutex<Option<RecoveryNotice>>,
}

impl PlaybackPreferenceService {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let loaded = load_with_recovery(&path)?;
        let recovery = loaded.as_ref().and_then(|loaded| loaded.recovery.clone());
        let document =
            loaded.map_or_else(PlaybackPreferenceFile::default, |loaded| loaded.document);
        validate(&document)?;
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

    pub fn get(&self, key: &AccountKey, item_id: &str) -> Option<ItemPlaybackPreference> {
        let document = self
            .document
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        document
            .accounts
            .iter()
            .find(|account| account.key == *key)
            .and_then(|account| account.items.iter().find(|item| item.item_id == item_id))
            .map(|item| item.preference.clone())
    }

    pub fn save(
        &self,
        key: &AccountKey,
        item_id: &str,
        preference: &ItemPlaybackPreference,
    ) -> io::Result<()> {
        let item_id = item_id.trim();
        if item_id.is_empty() {
            return Err(invalid_data("the Jellyfin item id is empty"));
        }
        self.mutate(|document| {
            let account = account_mut(document, key);
            let stored = StoredPlaybackPreference {
                item_id: item_id.to_string(),
                preference: preference.clone(),
                updated_at: crate::library::now_unix(),
            };
            if let Some(index) = account
                .items
                .iter()
                .position(|candidate| candidate.item_id == item_id)
            {
                account.items[index] = stored;
            } else {
                account.items.push(stored);
            }
            Ok(())
        })
    }

    pub fn remove_account(&self, key: &AccountKey) -> io::Result<bool> {
        let removed = self.mutate(|document| {
            let previous = document.accounts.len();
            document.accounts.retain(|account| account.key != *key);
            Ok(previous != document.accounts.len())
        })?;
        super::json_file::replace_backup_with_primary(&self.path)?;
        Ok(removed)
    }

    fn mutate<T>(
        &self,
        change: impl FnOnce(&mut PlaybackPreferenceFile) -> io::Result<T>,
    ) -> io::Result<T> {
        let mut current = self
            .document
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = current.clone();
        let result = change(&mut next)?;
        validate(&next)?;
        save_with_backup(&self.path, &next)?;
        *current = next;
        drop(current);
        Ok(result)
    }
}

pub fn playback_preferences_file_path() -> PathBuf {
    config_dir().join("playback-preferences.json")
}

fn account_mut<'a>(
    document: &'a mut PlaybackPreferenceFile,
    key: &AccountKey,
) -> &'a mut PlaybackAccount {
    let index = document
        .accounts
        .iter()
        .position(|account| account.key == *key)
        .unwrap_or_else(|| {
            document.accounts.push(PlaybackAccount {
                key: key.clone(),
                items: Vec::new(),
            });
            document.accounts.len() - 1
        });
    &mut document.accounts[index]
}

fn validate(document: &PlaybackPreferenceFile) -> io::Result<()> {
    if document.version != PLAYBACK_CONFIG_VERSION {
        return Err(invalid_data(format!(
            "unsupported playback preference version {}",
            document.version
        )));
    }
    let mut accounts = HashSet::new();
    for account in &document.accounts {
        if !accounts.insert(account.key.clone()) {
            return Err(invalid_data(
                "playback preferences contain a duplicate account",
            ));
        }
        let mut items = HashSet::new();
        if account
            .items
            .iter()
            .any(|item| item.item_id.trim().is_empty() || !items.insert(item.item_id.clone()))
        {
            return Err(invalid_data(
                "playback preferences contain a duplicate or empty item id",
            ));
        }
    }
    Ok(())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static COUNTER: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn preferences_survive_database_recreation_and_item_absence() {
        let path = std::env::temp_dir().join(format!(
            "mediaflick-playback-json-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let key = AccountKey::new("server", "user").expect("key");
        let preference = ItemPlaybackPreference::default();
        let service = PlaybackPreferenceService::open(path.clone()).expect("open");
        service
            .save(&key, "item-that-is-not-in-library", &preference)
            .expect("save");
        drop(service);

        let reopened = PlaybackPreferenceService::open(path.clone()).expect("reopen");
        assert_eq!(
            reopened.get(&key, "item-that-is-not-in-library"),
            Some(preference)
        );
        let _ = std::fs::remove_file(&path);
    }
}
