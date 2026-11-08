use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::books::{ReaderAppearance, ReadingLocation, SavedPosition};
use crate::jellyfin::session::Live;

use super::AccountKey;
use super::json_file::{
    RecoveryNotice, invalid_data, load_versioned_with_recovery, replace_backup_with_primary,
    save_with_backup,
};
use super::store::config_dir;

const READER_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BookPlace {
    item_id: String,
    location: ReadingLocation,
    synced_ticks: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReaderAccount {
    #[serde(flatten)]
    key: AccountKey,
    #[serde(default)]
    appearance: ReaderAppearance,
    #[serde(default)]
    books: Vec<BookPlace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReaderFile {
    version: u32,
    #[serde(default)]
    accounts: Vec<ReaderAccount>,
}

impl Default for ReaderFile {
    fn default() -> Self {
        Self {
            version: READER_VERSION,
            accounts: Vec::new(),
        }
    }
}

/// `reader.json`: per account, the exact place in each book read in this
/// app and the reader's appearance. Both are user intent the server cannot
/// rebuild (Jellyfin keeps only a coarse position), so they live beside the
/// other account-owned preference files, not in the rebuildable cache.
pub struct ReaderStore {
    path: PathBuf,
    state: Mutex<StoreState>,
}

struct StoreState {
    file: ReaderFile,
    recovery: Option<RecoveryNotice>,
}

impl ReaderStore {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let state = match load_versioned_with_recovery(&path, READER_VERSION, "reader")? {
            Some(loaded) => StoreState {
                file: loaded.document,
                recovery: loaded.recovery,
            },
            None => StoreState {
                file: ReaderFile::default(),
                recovery: None,
            },
        };
        check(&state.file)?;
        Ok(Self {
            path,
            state: Mutex::new(state),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StoreState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn take_recovery_notice(&self) -> Option<RecoveryNotice> {
        self.lock().recovery.take()
    }

    pub fn position(&self, key: &AccountKey, item_id: &str) -> Option<SavedPosition> {
        self.read(key, |account| {
            account
                .books
                .iter()
                .find(|book| book.item_id == item_id)
                .map(|book| SavedPosition {
                    location: book.location.clone(),
                    synced_ticks: book.synced_ticks,
                })
        })
        .flatten()
    }

    pub fn appearance(&self, key: &AccountKey) -> ReaderAppearance {
        self.read(key, |account| account.appearance)
            .unwrap_or_default()
    }

    pub fn save_position(
        &self,
        live: &Live,
        item_id: &str,
        position: SavedPosition,
    ) -> io::Result<()> {
        let place = BookPlace {
            item_id: item_id.to_string(),
            location: position.location,
            synced_ticks: position.synced_ticks,
        };
        self.write(live.account(), |account| {
            match account
                .books
                .iter_mut()
                .find(|book| book.item_id == place.item_id)
            {
                Some(existing) => *existing = place,
                None => account.books.push(place),
            }
        })
    }

    pub fn save_appearance(&self, live: &Live, appearance: ReaderAppearance) -> io::Result<()> {
        self.write(live.account(), |account| account.appearance = appearance)
    }

    pub fn remove_account(&self, key: &AccountKey) -> io::Result<bool> {
        let mut state = self.lock();
        let mut next = state.file.clone();
        next.accounts.retain(|account| account.key != *key);
        let removed = next.accounts.len() != state.file.accounts.len();
        if removed {
            save_with_backup(&self.path, &next)?;
            state.file = next;
        }
        drop(state);
        replace_backup_with_primary(&self.path)?;
        Ok(removed)
    }

    fn read<T>(&self, key: &AccountKey, read: impl FnOnce(&ReaderAccount) -> T) -> Option<T> {
        self.lock()
            .file
            .accounts
            .iter()
            .find(|account| account.key == *key)
            .map(read)
    }

    fn write(&self, key: &AccountKey, change: impl FnOnce(&mut ReaderAccount)) -> io::Result<()> {
        let mut state = self.lock();
        let mut next = state.file.clone();
        let index = match next.accounts.iter().position(|account| account.key == *key) {
            Some(index) => index,
            None => {
                next.accounts.push(ReaderAccount {
                    key: key.clone(),
                    appearance: ReaderAppearance::default(),
                    books: Vec::new(),
                });
                next.accounts.len() - 1
            }
        };
        change(&mut next.accounts[index]);
        check(&next)?;
        save_with_backup(&self.path, &next)?;
        state.file = next;
        drop(state);
        Ok(())
    }
}

pub fn reader_file_path() -> PathBuf {
    config_dir().join("reader.json")
}

fn check(document: &ReaderFile) -> io::Result<()> {
    if document.version != READER_VERSION {
        return Err(invalid_data(format!(
            "unsupported reader file version {}",
            document.version
        )));
    }
    let mut accounts = HashSet::new();
    for account in &document.accounts {
        if !accounts.insert(&account.key) {
            return Err(invalid_data("the reader file lists an account twice"));
        }
        let mut books = HashSet::new();
        for book in &account.books {
            if !crate::app::ids::valid_opaque_id(&book.item_id) || !books.insert(&book.item_id) {
                return Err(invalid_data(
                    "the reader file holds a duplicate or invalid book id",
                ));
            }
            if !book.location.is_valid() || book.synced_ticks < 0 {
                return Err(invalid_data(
                    "the reader file holds an invalid place in a book",
                ));
            }
        }
    }
    Ok(())
}
