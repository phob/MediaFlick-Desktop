//! Secrets owned by optional desktop integrations.
//!
//! API keys never enter `AppSettings`, SQLite, logs, or ordinary JSON
//! snapshots. `keyring` selects the operating-system vault: Credential Manager
//! on Windows, Keychain on macOS, and Secret Service on Linux. The small port
//! keeps Public PKCE token storage replaceable without changing callers.

use std::fmt;

const SERVICE: &str = "com.mediaflick.desktop.integrations";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialError(String);

impl CredentialError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CredentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CredentialError {}

/// Secure secret persistence used by rating integrations.
pub trait CredentialStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>, CredentialError>;
    fn set(&self, name: &str, secret: &str) -> Result<(), CredentialError>;
    fn remove(&self, name: &str) -> Result<(), CredentialError>;
}

#[derive(Debug, Default)]
pub struct OsCredentialStore;

impl OsCredentialStore {
    fn entry(name: &str) -> Result<keyring::Entry, CredentialError> {
        keyring::Entry::new(SERVICE, name).map_err(|_| {
            CredentialError::new("the operating-system credential vault is unavailable")
        })
    }
}

impl CredentialStore for OsCredentialStore {
    fn get(&self, name: &str) -> Result<Option<String>, CredentialError> {
        match Self::entry(name)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(CredentialError::new(
                "the operating-system credential vault could not read this credential",
            )),
        }
    }

    fn set(&self, name: &str, secret: &str) -> Result<(), CredentialError> {
        Self::entry(name)?.set_password(secret).map_err(|_| {
            CredentialError::new(
                "the operating-system credential vault could not save this credential",
            )
        })
    }

    fn remove(&self, name: &str) -> Result<(), CredentialError> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(CredentialError::new(
                "the operating-system credential vault could not remove this credential",
            )),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct MemoryCredentialStore {
    values: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

#[cfg(test)]
impl CredentialStore for MemoryCredentialStore {
    fn get(&self, name: &str) -> Result<Option<String>, CredentialError> {
        Ok(self
            .values
            .lock()
            .map_err(|_| CredentialError::new("test credential store is unavailable"))?
            .get(name)
            .cloned())
    }

    fn set(&self, name: &str, secret: &str) -> Result<(), CredentialError> {
        self.values
            .lock()
            .map_err(|_| CredentialError::new("test credential store is unavailable"))?
            .insert(name.to_string(), secret.to_string());
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<(), CredentialError> {
        self.values
            .lock()
            .map_err(|_| CredentialError::new("test credential store is unavailable"))?
            .remove(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CredentialStore, MemoryCredentialStore};

    #[test]
    fn secure_store_contract_round_trips_and_removes_without_preference_state() {
        let store = MemoryCredentialStore::default();
        assert_eq!(store.get("mdblist").expect("read"), None);
        store.set("mdblist", "secret").expect("write");
        assert_eq!(
            store.get("mdblist").expect("read").as_deref(),
            Some("secret")
        );
        store.remove("mdblist").expect("remove");
        assert_eq!(store.get("mdblist").expect("read"), None);
    }
}
