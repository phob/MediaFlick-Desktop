use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Deserializer, Serialize};

use crate::app::ids::valid_opaque_id;
use crate::integrations::letterboxd::{ExternalProfile, MAX_CONNECTED_PROFILES};
use crate::library::libraries::LibraryId;

use super::{AppearanceSettings, ViewingSettings};

const MAX_HOME_ELEMENTS: usize = 512;
const MAX_BROWSING_PAGES: usize = 256;
/// Pages from before libraries had their own route. A file may still hold
/// them; nothing writes them, and the account's next save of a library's
/// filters drops them.
const LEGACY_BROWSING_PAGES: [&str; 2] = ["Movie", "Series"];
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
    RecentlyAddedShows,
    Upcoming,
    LatestMovies,
    LatestShows,
    MyList,
}

impl HomeBuiltIn {
    pub const ORDER: [Self; 8] = [
        Self::Watching,
        Self::BecauseYouWatched,
        Self::RecentlyAdded,
        Self::RecentlyAddedShows,
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

    pub fn insert_missing_built_ins(&mut self) {
        for (order, id) in HomeBuiltIn::ORDER.into_iter().enumerate() {
            let element = HomeElementId::BuiltIn { id };
            if self
                .elements
                .iter()
                .any(|current| current.element == element)
            {
                continue;
            }
            let position = HomeBuiltIn::ORDER[..order]
                .iter()
                .rev()
                .find_map(|previous| {
                    self.elements.iter().position(|current| {
                        current.element == HomeElementId::BuiltIn { id: *previous }
                    })
                })
                .map_or(0, |index| index + 1);
            self.elements.insert(
                position,
                HomeElement {
                    element,
                    enabled: true,
                },
            );
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
    RecoveryNotice, load_versioned_with_recovery, replace_backup_with_primary, save_with_backup,
};
use super::store::config_dir;

const ACCOUNT_CONFIG_VERSION: u32 = 1;

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

pub struct AccountConfigurationService {
    path: PathBuf,
    document: Mutex<AccountConfigurationFile>,
    recovery: Mutex<Option<RecoveryNotice>>,
}

impl AccountConfigurationService {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let loaded =
            load_versioned_with_recovery(&path, ACCOUNT_CONFIG_VERSION, "account configuration")?;
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
        let prunes_legacy = match validate_browsing_route(page, route)? {
            BrowsingPage::Last => false,
            BrowsingPage::Library(_) => true,
            BrowsingPage::Legacy => return Err(invalid_data("invalid browsing destination")),
        };
        let stored = self.browsing(key);
        let prune_due = prunes_legacy
            && LEGACY_BROWSING_PAGES
                .iter()
                .any(|legacy| stored.contains_key(*legacy));
        if !prune_due && stored.get(page).is_some_and(|saved| saved == route) {
            return Ok(());
        }
        self.mutate(|document| {
            let browsing = &mut account_mut(document, key).browsing;
            if prunes_legacy {
                browsing.retain(|page, _| !LEGACY_BROWSING_PAGES.contains(&page.as_str()));
            }
            browsing.insert(page.to_string(), route.to_string());
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
        if account.browsing.len() > MAX_BROWSING_PAGES {
            return Err(invalid_data(
                "account configuration remembers too many browsing pages",
            ));
        }
        for (page, route) in &account.browsing {
            validate_browsing_route(page, route)?;
        }
        if let Some(home) = &mut account.home {
            home.insert_missing_built_ins();
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

enum BrowsingPage {
    Last,
    Library(LibraryId),
    Legacy,
}

impl BrowsingPage {
    fn parse(page: &str) -> Option<Self> {
        if page == "last" {
            return Some(Self::Last);
        }
        if LEGACY_BROWSING_PAGES.contains(&page) {
            return Some(Self::Legacy);
        }
        canonical_library_id(page.strip_prefix("library:")?).map(Self::Library)
    }
}

fn canonical_library_id(raw: &str) -> Option<LibraryId> {
    LibraryId::parse(raw).filter(|id| id.as_str() == raw)
}

/// A route is an app path with or without a query string; a library page's
/// route is its own `/library/<id>`.
fn validate_browsing_route(page: &str, route: &str) -> io::Result<BrowsingPage> {
    let (path, _query) = route.split_once('?').unwrap_or((route, ""));
    let library = path
        .strip_prefix("/library/")
        .and_then(canonical_library_id);
    let app_route = library.is_some()
        || [
            "/",
            "/calendar",
            "/library",
            "/discover",
            "/requests",
            "/collections",
        ]
        .contains(&path)
        || path.starts_with("/collections/");
    BrowsingPage::parse(page)
        .filter(|page| {
            route.len() <= 2048
                && match page {
                    BrowsingPage::Library(id) => library.as_ref() == Some(id),
                    BrowsingPage::Last | BrowsingPage::Legacy => app_route,
                }
        })
        .ok_or_else(|| invalid_data("invalid browsing destination"))
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests;
