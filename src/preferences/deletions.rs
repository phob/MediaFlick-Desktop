use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::AccountKey;
use super::json_file::{RecoveryNotice, load_with_recovery, save_with_backup};
use super::store::config_dir;

const DELETION_JOURNAL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingDeletion {
    #[serde(flatten)]
    pub account: AccountKey,
    #[serde(default)]
    pub artwork_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeletionJournal {
    version: u32,
    #[serde(default)]
    deletions: Vec<PendingDeletion>,
}

impl Default for DeletionJournal {
    fn default() -> Self {
        Self {
            version: DELETION_JOURNAL_VERSION,
            deletions: Vec::new(),
        }
    }
}

pub struct PendingDeletionService {
    path: PathBuf,
    journal: Mutex<DeletionJournal>,
    recovery: Mutex<Option<RecoveryNotice>>,
}

impl PendingDeletionService {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let loaded = load_with_recovery(&path)?;
        let recovery = loaded.as_ref().and_then(|loaded| loaded.recovery.clone());
        let journal = loaded.map_or_else(DeletionJournal::default, |loaded| loaded.document);
        if journal.version != DELETION_JOURNAL_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported deletion journal version {}", journal.version),
            ));
        }
        Ok(Self {
            path,
            journal: Mutex::new(journal),
            recovery: Mutex::new(recovery),
        })
    }

    pub fn take_recovery_notice(&self) -> Option<RecoveryNotice> {
        self.recovery
            .lock()
            .ok()
            .and_then(|mut recovery| recovery.take())
    }

    pub fn pending(&self) -> Vec<PendingDeletion> {
        self.journal
            .lock()
            .map(|journal| journal.deletions.clone())
            .unwrap_or_default()
    }

    pub fn begin(&self, deletion: PendingDeletion) -> io::Result<()> {
        self.mutate(|journal| {
            if let Some(existing) = journal
                .deletions
                .iter_mut()
                .find(|existing| existing.account == deletion.account)
            {
                existing.artwork_ids = deletion.artwork_ids;
            } else {
                journal.deletions.push(deletion);
            }
        })
    }

    pub fn finish(&self, account: &AccountKey) -> io::Result<()> {
        self.mutate(|journal| {
            journal
                .deletions
                .retain(|deletion| deletion.account != *account);
        })?;
        super::json_file::replace_backup_with_primary(&self.path)?;
        Ok(())
    }

    fn mutate(&self, change: impl FnOnce(&mut DeletionJournal)) -> io::Result<()> {
        let mut current = self
            .journal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = current.clone();
        change(&mut next);
        save_with_backup(&self.path, &next)?;
        *current = next;
        drop(current);
        Ok(())
    }
}

pub fn pending_deletions_file_path() -> PathBuf {
    config_dir().join("pending-deletions.json")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static COUNTER: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn unfinished_deletions_survive_restart_until_finished() {
        let path = std::env::temp_dir().join(format!(
            "mediaflick-deletions-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let account = AccountKey::new("server", "user").expect("account");
        let service = PendingDeletionService::open(path.clone()).expect("open");
        service
            .begin(PendingDeletion {
                account: account.clone(),
                artwork_ids: vec!["a".repeat(16)],
            })
            .expect("begin");
        drop(service);
        let resumed = PendingDeletionService::open(path.clone()).expect("resume");
        assert_eq!(resumed.pending().len(), 1);
        resumed.finish(&account).expect("finish");
        assert!(resumed.pending().is_empty());
        let _ = std::fs::remove_file(path);
    }
}
