use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Deserializer, Serialize};

use crate::collections::valid_opaque_id;
use crate::integrations::letterboxd::{ExternalProfile, MAX_CONNECTED_PROFILES};

use super::{AppearanceSettings, ViewingSettings};

const MAX_HOME_ELEMENTS: usize = 512;
const PREFERRED_HOME_GENRES: [&str; 12] = [
    "Action",
    "Comedy",
    "Drama",
    "Science Fiction",
    "Thriller",
    "Documentary",
    "Animation",
    "Horror",
    "Adventure",
    "Crime",
    "Fantasy",
    "Romance",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HomeBuiltIn {
    Watching,
    BecauseYouWatched,
    RecentlyAdded,
    Upcoming,
    LatestMovies,
    LatestShows,
    MyList,
}

impl HomeBuiltIn {
    pub const ORDER: [Self; 7] = [
        Self::Watching,
        Self::BecauseYouWatched,
        Self::RecentlyAdded,
        Self::Upcoming,
        Self::LatestMovies,
        Self::LatestShows,
        Self::MyList,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum HomeElementId {
    BuiltIn { id: HomeBuiltIn },
    Genre { id: String },
    Collection { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeElement {
    #[serde(flatten)]
    pub element: HomeElementId,
    pub enabled: bool,
}

impl<'de> Deserialize<'de> for HomeElement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
        enum Wire {
            BuiltIn { id: HomeBuiltIn, enabled: bool },
            Genre { id: String, enabled: bool },
            Collection { id: String, enabled: bool },
        }

        let (element, enabled) = match Wire::deserialize(deserializer)? {
            Wire::BuiltIn { id, enabled } => (HomeElementId::BuiltIn { id }, enabled),
            Wire::Genre { id, enabled } => (HomeElementId::Genre { id }, enabled),
            Wire::Collection { id, enabled } => (HomeElementId::Collection { id }, enabled),
        };
        Ok(Self { element, enabled })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HomeWatchingSettings {
    pub continue_watching: bool,
    pub next_up: bool,
    pub combine: bool,
}

impl Default for HomeWatchingSettings {
    fn default() -> Self {
        Self {
            continue_watching: true,
            next_up: true,
            combine: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HomeSettings {
    pub billboard: bool,
    pub watching: HomeWatchingSettings,
    pub elements: Vec<HomeElement>,
}

impl HomeSettings {
    pub fn fresh(genres: &[String]) -> Self {
        let mut ordered_genres = PREFERRED_HOME_GENRES
            .iter()
            .filter(|preferred| genres.iter().any(|genre| genre == **preferred))
            .map(|genre| (*genre).to_string())
            .collect::<Vec<_>>();
        ordered_genres.extend(
            genres
                .iter()
                .filter(|genre| !PREFERRED_HOME_GENRES.contains(&genre.as_str()))
                .cloned(),
        );
        let mut elements = HomeBuiltIn::ORDER
            .into_iter()
            .map(|id| HomeElement {
                element: HomeElementId::BuiltIn { id },
                enabled: true,
            })
            .collect::<Vec<_>>();
        elements.extend(
            ordered_genres
                .into_iter()
                .enumerate()
                .map(|(index, id)| HomeElement {
                    element: HomeElementId::Genre { id },
                    enabled: index < 6,
                }),
        );
        Self {
            billboard: true,
            watching: HomeWatchingSettings::default(),
            elements,
        }
    }

    pub fn validate(&self) -> io::Result<()> {
        if self.elements.len() > MAX_HOME_ELEMENTS {
            return Err(invalid_data(
                "home configuration contains too many elements",
            ));
        }
        let mut ids = HashSet::new();
        for element in &self.elements {
            if !ids.insert(element.element.clone()) {
                return Err(invalid_data(
                    "home configuration contains a duplicate element",
                ));
            }
            match &element.element {
                HomeElementId::BuiltIn { .. } => {}
                HomeElementId::Genre { id }
                    if id.trim().is_empty()
                        || id.chars().count() > 100
                        || id.chars().any(char::is_control) =>
                {
                    return Err(invalid_data("home configuration contains an invalid genre"));
                }
                HomeElementId::Collection { id } if !valid_opaque_id(id) => {
                    return Err(invalid_data(
                        "home configuration contains an invalid collection id",
                    ));
                }
                HomeElementId::Genre { .. } | HomeElementId::Collection { .. } => {}
            }
        }
        if HomeBuiltIn::ORDER
            .into_iter()
            .any(|id| !ids.contains(&HomeElementId::BuiltIn { id }))
        {
            return Err(invalid_data(
                "home configuration is missing a built-in element",
            ));
        }
        Ok(())
    }

    pub fn remove_collection(&mut self, profile_id: &str) {
        self.elements.retain(|element| {
            !matches!(&element.element, HomeElementId::Collection { id } if id == profile_id)
        });
    }
}

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
    #[serde(default)]
    viewing: ViewingSettings,
    #[serde(default)]
    browsing: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    home: Option<HomeSettings>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    letterboxd_profiles: Vec<ExternalProfile>,
}

impl AccountConfiguration {
    fn new(key: AccountKey) -> Self {
        Self {
            key,
            appearance: AppearanceSettings::default(),
            viewing: ViewingSettings::default(),
            browsing: std::collections::BTreeMap::new(),
            home: None,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_appearance: Option<AppearanceSettings>,
}

impl Default for AccountConfigurationFile {
    fn default() -> Self {
        Self {
            version: ACCOUNT_CONFIG_VERSION,
            accounts: Vec::new(),
            legacy_appearance: None,
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

    pub fn viewing(&self, key: &AccountKey) -> ViewingSettings {
        self.with_document(|document| {
            document
                .accounts
                .iter()
                .find(|account| account.key == *key)
                .map(|account| account.viewing.clone())
                .unwrap_or_default()
        })
    }

    pub fn save_viewing(&self, key: &AccountKey, viewing: &ViewingSettings) -> io::Result<()> {
        viewing.validate()?;
        self.mutate(|document| {
            account_mut(document, key).viewing = viewing.clone();
            Ok(())
        })
    }

    pub fn browsing(&self, key: &AccountKey) -> std::collections::BTreeMap<String, String> {
        self.with_document(|document| {
            document
                .accounts
                .iter()
                .find(|account| account.key == *key)
                .map(|account| account.browsing.clone())
                .unwrap_or_default()
        })
    }

    pub fn save_browsing(&self, key: &AccountKey, page: &str, route: &str) -> io::Result<()> {
        validate_browsing_route(page, route)?;
        self.mutate(|document| {
            account_mut(document, key)
                .browsing
                .insert(page.to_string(), route.to_string());
            Ok(())
        })
    }

    pub fn home(&self, key: &AccountKey) -> Option<HomeSettings> {
        self.with_document(|document| {
            document
                .accounts
                .iter()
                .find(|account| account.key == *key)
                .and_then(|account| account.home.clone())
        })
    }

    pub fn save_home(&self, key: &AccountKey, home: &HomeSettings) -> io::Result<()> {
        home.validate()?;
        self.mutate(|document| {
            account_mut(document, key).home = Some(home.clone());
            Ok(())
        })
    }

    pub fn forget_home_collection(&self, key: &AccountKey, profile_id: &str) -> io::Result<()> {
        self.mutate(|document| {
            if let Some(home) = &mut account_mut(document, key).home {
                home.remove_collection(profile_id);
            }
            Ok(())
        })
    }

    /// Moves the pre-account-scoping appearance into durable account storage.
    /// When startup is signed out, keep it pending until the next account can
    /// claim it instead of stripping the only copy from settings.json.
    pub fn import_legacy_appearance(
        &self,
        key: Option<&AccountKey>,
        appearance: &AppearanceSettings,
    ) -> io::Result<()> {
        if appearance.is_default() {
            return Ok(());
        }
        self.mutate(|document| {
            if let Some(key) = key {
                let account = account_mut(document, key);
                if account.appearance.is_default() {
                    account.appearance = appearance.clone();
                }
            } else if document.legacy_appearance.is_none() {
                document.legacy_appearance = Some(appearance.clone());
            }
            Ok(())
        })
    }

    pub fn claim_legacy_appearance(&self, key: &AccountKey) -> io::Result<()> {
        if self.with_document(|document| document.legacy_appearance.is_none()) {
            return Ok(());
        }
        self.mutate(|document| {
            let Some(appearance) = document.legacy_appearance.take() else {
                return Ok(());
            };
            let account = account_mut(document, key);
            if account.appearance.is_default() {
                account.appearance = appearance;
            }
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
    if let Some(appearance) = &mut document.legacy_appearance {
        appearance.sanitize();
        if appearance.is_default() {
            document.legacy_appearance = None;
        }
    }
    let mut accounts = HashSet::new();
    for account in &mut document.accounts {
        if !accounts.insert(account.key.clone()) {
            return Err(invalid_data(
                "account configuration contains a duplicate account",
            ));
        }
        account.appearance.sanitize();
        account.viewing.validate()?;
        for (page, route) in &account.browsing {
            validate_browsing_route(page, route)?;
        }
        if let Some(home) = &account.home {
            home.validate()?;
        }
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

fn validate_browsing_route(page: &str, route: &str) -> io::Result<()> {
    let path = route.split('?').next().unwrap_or(route);
    if !["last", "Movie", "Series"].contains(&page)
        || route.len() > 2048
        || !([
            "/",
            "/calendar",
            "/library",
            "/discover",
            "/requests",
            "/collections",
        ]
        .contains(&path)
            || path.starts_with("/collections/"))
    {
        return Err(invalid_data("invalid browsing destination"));
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
    fn home_elements_reject_unknown_fields() {
        let value = serde_json::json!({
            "kind": "genre",
            "id": "Drama",
            "enabled": true,
            "label": "not persisted"
        });
        assert!(serde_json::from_value::<HomeElement>(value).is_err());
    }

    #[test]
    fn viewing_and_browsing_survive_reopen_and_reject_invalid_writes() {
        let path = test_path("viewing-round-trip");
        let alice = key("server", "alice");
        let bob = key("server", "bob");
        let service = AccountConfigurationService::open(path.clone()).expect("open");
        let settings = ViewingSettings {
            spoiler_protection: true,
            audio_languages: vec!["en".into()],
            ..Default::default()
        };
        service.save_viewing(&alice, &settings).expect("save");
        service
            .save_browsing(&alice, "Movie", "/library?kind=Movie&sort=year")
            .expect("save browsing");
        assert!(
            service
                .save_browsing(&alice, "last", "https://example.com")
                .is_err()
        );
        let invalid = ViewingSettings {
            text_scale: 0,
            ..settings.clone()
        };
        assert!(service.save_viewing(&alice, &invalid).is_err());
        drop(service);
        let reopened = AccountConfigurationService::open(path).expect("reopen");
        assert_eq!(reopened.viewing(&alice), settings);
        assert_eq!(reopened.viewing(&bob), ViewingSettings::default());
        assert!(reopened.browsing(&bob).is_empty());
        assert_eq!(
            reopened.browsing(&alice)["Movie"],
            "/library?kind=Movie&sort=year"
        );
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
        let home = HomeSettings::fresh(&["Action".to_owned(), "Comedy".to_owned()]);
        service.save_home(&alice, &home).expect("save home");
        let saved_json = std::fs::read_to_string(&path).expect("read saved account settings");
        assert!(!saved_json.contains("jellyfinServerId"));
        assert!(!saved_json.contains("jellyfinUserId"));

        assert_eq!(service.appearance(&bob), AppearanceSettings::default());
        assert!(service.letterboxd_profiles(&bob).is_empty());
        assert_eq!(service.home(&bob), None);
        drop(service);

        let reopened = AccountConfigurationService::open(path.clone()).expect("reopen");
        assert_eq!(reopened.appearance(&alice), appearance);
        assert_eq!(reopened.home(&alice), Some(home));
        let profiles = reopened.letterboxd_profiles(&alice);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].jellyfin_server_id, "server");
        assert_eq!(profiles[0].jellyfin_user_id, "alice");
        cleanup(&path);
    }

    #[test]
    fn signed_out_legacy_appearance_waits_for_the_next_account() {
        let path = test_path("pending-appearance");
        let appearance = AppearanceSettings {
            theme: AppearanceTheme::Dark,
            accent: AppearanceAccent::Violet,
            ..AppearanceSettings::default()
        };
        let alice = key("server", "alice");
        let service = AccountConfigurationService::open(path.clone()).expect("open");

        service
            .import_legacy_appearance(None, &appearance)
            .expect("stage signed-out appearance");
        drop(service);

        let reopened = AccountConfigurationService::open(path.clone()).expect("reopen");
        assert_eq!(reopened.appearance(&alice), AppearanceSettings::default());
        reopened
            .claim_legacy_appearance(&alice)
            .expect("claim appearance");
        assert_eq!(reopened.appearance(&alice), appearance);
        // A startup retry cannot move or overwrite the value a second time.
        reopened
            .claim_legacy_appearance(&key("server", "bob"))
            .expect("idempotent retry");
        assert_eq!(reopened.appearance(&alice), appearance);
        cleanup(&path);
    }

    #[test]
    fn authenticated_legacy_appearance_does_not_overwrite_account_settings() {
        let path = test_path("authenticated-appearance");
        let alice = key("server", "alice");
        let service = AccountConfigurationService::open(path.clone()).expect("open");
        let existing = AppearanceSettings {
            theme: AppearanceTheme::Dark,
            ..AppearanceSettings::default()
        };
        let legacy = AppearanceSettings {
            accent: AppearanceAccent::Violet,
            ..AppearanceSettings::default()
        };
        service
            .save_appearance(&alice, &existing)
            .expect("seed account appearance");
        service
            .import_legacy_appearance(Some(&alice), &legacy)
            .expect("import legacy appearance");
        assert_eq!(service.appearance(&alice), existing);
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
