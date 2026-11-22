use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, mpsc};

use crate::app::paths;
use crate::books::OpenBooks;
use crate::collections::artwork::{ArtworkStore, custom_art_dir};
use crate::companion::CompanionSession;
use crate::integrations::letterboxd::ReviewService;
use crate::integrations::mdblist::RatingsService;
use crate::jellyfin::session::Session;
use crate::jellyfin::socket::{self, SocketHandle};
use crate::library::Library;
use crate::library::LibraryChangeBatch;
use crate::library::browse::ChildrenSources;
use crate::library::db::OpenError;
use crate::library::sync::{self, SyncHandle};
use crate::playback::PlaybackCoordinator;
use crate::preferences::{
    AccountConfigurationService, AccountKey, AppSettings, CollectionConfigurationService,
    PendingDeletion, PendingDeletionService, PlaybackPreferenceService, PreferencesService,
    ReaderStore, accounts_file_path, carries_0_1_6_data, collections_file_path, config_dir,
    pending_deletions_file_path, playback_preferences_file_path, reader_file_path,
};
use crate::shell::InitPermit;

pub struct Services {
    pub library: Arc<Library>,
    pub session: Arc<Session>,
    pub companion: Arc<CompanionSession>,
    pub ratings: Arc<RatingsService>,
    pub letterboxd: Arc<ReviewService>,
    pub sync: SyncHandle,
    pub socket: SocketHandle,
    pub accounts: Arc<AccountConfigurationService>,
    pub collections: Arc<CollectionConfigurationService>,
    pub playback_preferences: Arc<PlaybackPreferenceService>,
    pub reader: Arc<ReaderStore>,
    pub artwork: Arc<ArtworkStore>,
    pub pending_deletions: Arc<PendingDeletionService>,
    pub preferences: Arc<PreferencesService>,
    pub shell: Arc<ShellBridge>,
    pub home_watched: Mutex<HashMap<AccountKey, Option<crate::library::ItemDetail>>>,
    pub children_sources: ChildrenSources,
    pub open_books: OpenBooks,
    playback: RwLock<Option<Arc<PlaybackCoordinator>>>,
}

#[derive(Debug, Clone)]
pub enum ShellRequest {
    MainWindowReady,
    FilePicker {
        request_id: String,
    },
    InstallMpv {
        request_id: String,
    },
    LibraryChanged {
        item_ids: Vec<String>,
        context_ids: Vec<String>,
    },
    LiveChanged {
        item_ids: Vec<String>,
        context_ids: Vec<String>,
    },
    LiveResynced,
    CatalogChanged {
        item_ids: Vec<String>,
        context_ids: Vec<String>,
        library_ids: Vec<String>,
    },
    CollectionsChanged,
    LibrariesChanged,
    LibraryMembershipsSettled,
    SessionExpired,
}

#[derive(Default)]
pub struct ShellBridge {
    sender: Mutex<Option<mpsc::Sender<ShellRequest>>>,
}

impl ShellBridge {
    pub fn subscribe(&self) -> mpsc::Receiver<ShellRequest> {
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut slot) = self.sender.lock() {
            *slot = Some(sender);
        }
        receiver
    }

    pub fn request(&self, request: ShellRequest) -> Result<(), &'static str> {
        let sender = self
            .sender
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .ok_or("the desktop shell is not ready")?;
        sender
            .send(request)
            .map_err(|_| "the desktop shell is unavailable")
    }

    pub fn library_changed(&self, changes: LibraryChangeBatch) {
        if changes.item_ids.is_empty() {
            return;
        }
        let _ = self.request(ShellRequest::LibraryChanged {
            item_ids: changes.item_ids,
            context_ids: changes.context_ids,
        });
    }

    pub fn live_changed(&self, item_ids: Vec<String>, context_ids: Vec<String>) {
        if item_ids.is_empty() && context_ids.is_empty() {
            return;
        }
        let _ = self.request(ShellRequest::LiveChanged {
            item_ids,
            context_ids,
        });
    }

    pub fn live_resynced(&self) {
        let _ = self.request(ShellRequest::LiveResynced);
    }

    pub fn catalog_changed(&self, changes: LibraryChangeBatch, library_ids: Vec<String>) {
        let _ = self.request(ShellRequest::CatalogChanged {
            item_ids: changes.item_ids,
            context_ids: changes.context_ids,
            library_ids,
        });
    }

    pub fn session_expired(&self) {
        let _ = self.request(ShellRequest::SessionExpired);
    }

    pub fn collections_changed(&self) {
        let _ = self.request(ShellRequest::CollectionsChanged);
    }

    pub fn libraries_changed(&self) {
        let _ = self.request(ShellRequest::LibrariesChanged);
    }

    pub fn library_memberships_settled(&self) {
        let _ = self.request(ShellRequest::LibraryMembershipsSettled);
    }

    pub fn library_sync_completed(&self) {
        let _ = self.request(ShellRequest::LibraryChanged {
            item_ids: Vec::new(),
            context_ids: Vec::new(),
        });
    }
}

impl Services {
    pub fn attach_playback(&self, playback: Arc<PlaybackCoordinator>) {
        if let Ok(mut slot) = self.playback.write() {
            *slot = Some(playback);
        }
    }

    pub fn playback(&self) -> Option<Arc<PlaybackCoordinator>> {
        self.playback.read().ok().and_then(|slot| slot.clone())
    }
}

pub struct StartupFailure {
    pub reason: String,
    pub library: Option<LibraryRefusal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryRefusal {
    NewerSchema,
    Unusable,
}

impl From<String> for StartupFailure {
    fn from(reason: String) -> Self {
        Self {
            reason,
            library: None,
        }
    }
}

pub fn open(
    _: &InitPermit,
    initial_settings: AppSettings,
) -> Result<Arc<Services>, StartupFailure> {
    let library = open_library()?;
    let restored = library.credentials().is_authenticated();
    let shell = Arc::new(ShellBridge::default());
    let session = Arc::new(Session::restore(library.clone(), shell.clone()));
    let account_path = accounts_file_path();
    let accounts = open_configuration(&account_path, AccountConfigurationService::open)?;
    let active_account = session.account_key();
    if let Err(error) =
        accounts.import_legacy_appearance(active_account.as_ref(), &initial_settings.appearance)
    {
        return Err(initialization_failure(format!(
            "could not import legacy appearance settings: {error}"
        ))
        .into());
    }
    let collections_path = collections_file_path();
    let collections = open_configuration(&collections_path, CollectionConfigurationService::open)?;
    let playback_path = playback_preferences_file_path();
    let playback_preferences = open_configuration(&playback_path, PlaybackPreferenceService::open)?;
    if let Err(error) = import_legacy_account_data(&library, &accounts, &playback_preferences) {
        return Err(initialization_failure(error).into());
    }
    let reader = open_configuration(&reader_file_path(), ReaderStore::open)?;
    let preferences = Arc::new(PreferencesService::new(
        initial_settings,
        accounts.clone(),
        active_account,
        carries_0_1_6_data(&config_dir()),
    ));
    let artwork_path = custom_art_dir();
    let artwork = open_configuration(&artwork_path, ArtworkStore::open)?;
    if let Err(error) = artwork.collect_orphans(
        collections.poster_references(),
        std::time::SystemTime::now(),
    ) {
        tracing::warn!(target: "collections", "could not clean custom artwork: {error}");
    }
    let deletion_path = pending_deletions_file_path();
    let pending_deletions = open_configuration(&deletion_path, PendingDeletionService::open)?;
    let companion = Arc::new(CompanionSession::new(
        session.clone(),
        library.clone(),
        shell.clone(),
    ));
    if restored {
        warm_restored_session(session.clone(), companion.clone());
    }
    let ratings = Arc::new(RatingsService::new(&library, &session, &companion));
    let letterboxd = Arc::new(ReviewService::default());
    let sync = sync::spawn(library.clone(), session.clone(), shell.clone());
    let socket = socket::spawn(
        library.clone(),
        session.clone(),
        sync.clone(),
        shell.clone(),
    );
    tracing::info!(
        target: "jellyfin.session",
        restored,
        "library services started"
    );
    let services = Arc::new(Services {
        library,
        session,
        companion,
        ratings,
        letterboxd,
        sync,
        socket,
        accounts,
        collections,
        playback_preferences,
        reader,
        artwork,
        pending_deletions,
        preferences,
        shell,
        home_watched: Mutex::new(HashMap::new()),
        children_sources: ChildrenSources::default(),
        open_books: OpenBooks::default(),
        playback: RwLock::new(None),
    });
    connect_app_hooks(&services);
    let _ = finish_pending_deletions(&services);
    Ok(services)
}

fn open_library() -> Result<Arc<Library>, StartupFailure> {
    let path = paths::library_db_path();
    let error = match Library::open(&path) {
        Ok(library) => return Ok(Arc::new(library)),
        Err(error) => error,
    };
    let refusal = library_refusal(&error);
    let message = match refusal {
        Some(_) => format!("could not open {}: {error}", path.display()),
        None => format!(
            "could not open {}: {error}. Another program is using library.db: close it, then \
             restart MediaFlick",
            path.display()
        ),
    };
    tracing::error!(target: "library.db", "{message}");
    let full = format!("{error:#}");
    if full != error.to_string() {
        tracing::error!(target: "library.db", "the failed open in full: {full}");
    }
    Err(StartupFailure {
        reason: message,
        library: refusal,
    })
}

fn library_refusal(error: &OpenError) -> Option<LibraryRefusal> {
    let sqlite = match error {
        OpenError::NewerSchema { .. } => return Some(LibraryRefusal::NewerSchema),
        OpenError::Sqlite(error) | OpenError::Migration { error, .. } => error,
    };
    match sqlite.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => None,
        _ => Some(LibraryRefusal::Unusable),
    }
}

pub fn start(_: &InitPermit, services: &Arc<Services>) {
    crate::collections::scheduler::start(services.clone());
    tracing::info!(
        target: "library.db",
        path = %services.library.path().display(),
        "library database ready"
    );
}

fn warm_restored_session(session: Arc<Session>, companion: Arc<CompanionSession>) {
    crate::app::threads::spawn_named("session-restore", move || {
        session.refresh_user_policy();
        if let Err(error) = companion.probe(false) {
            tracing::debug!(target: "companion", "initial companion probe failed: {error}");
        }
    });
}

fn connect_app_hooks(services: &Arc<Services>) {
    let weak = Arc::downgrade(services);
    services.sync.on_cycle_completed(Arc::new(move || {
        if let Some(services) = weak.upgrade() {
            crate::collections::scheduler::request_after_library_sync(services);
        }
    }));
    let weak = Arc::downgrade(services);
    services.socket.on_remote_command(Arc::new(move |command| {
        if let Some(services) = weak.upgrade() {
            crate::jellyfin::remote::handle(&services, command);
        }
    }));
}

pub fn finish_pending_deletions(services: &Arc<Services>) -> Result<(), String> {
    let mut first_error = None;
    for deletion in services.pending_deletions.pending() {
        let _ = services.session.begin_account_deletion(&deletion.account);
        if let Err(error) = complete_pending_deletion(services, &deletion) {
            tracing::warn!(
                target: "account-deletion",
                server_id = deletion.account.server_id(),
                user_id = deletion.account.user_id(),
                "could not resume local account deletion: {error}"
            );
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn open_configuration<T>(
    path: &Path,
    open: impl FnOnce(PathBuf) -> std::io::Result<T>,
) -> Result<Arc<T>, String> {
    open(path.to_path_buf()).map(Arc::new).map_err(|error| {
        initialization_failure(format!("could not open {}: {error}", path.display()))
    })
}

fn initialization_failure(message: String) -> String {
    tracing::error!(target: "config", "{message}");
    message
}

fn import_legacy_account_data(
    library: &Library,
    accounts: &AccountConfigurationService,
    playback_preferences: &PlaybackPreferenceService,
) -> Result<(), String> {
    let profiles = library
        .legacy_external_profiles()
        .map_err(|error| format!("could not read legacy Letterboxd profiles: {error}"))?;
    let preferences = library
        .legacy_playback_preferences()
        .map_err(|error| format!("could not read legacy playback preferences: {error}"))?;
    if profiles.is_empty() && preferences.is_empty() {
        return Ok(());
    }

    for profile in profiles {
        let account = AccountKey::new(
            profile.jellyfin_server_id.clone(),
            profile.jellyfin_user_id.clone(),
        )
        .ok_or_else(|| "a legacy Letterboxd profile has no account identity".to_string())?;
        accounts
            .save_letterboxd_profile(&account, &profile)
            .map_err(|error| format!("could not import a Letterboxd profile: {error}"))?;
    }

    let credentials = library.credentials();
    for legacy in preferences {
        let server_id = if credentials.server_url.as_deref() == Some(&legacy.server_key) {
            credentials
                .server_id
                .as_deref()
                .unwrap_or(&legacy.server_key)
        } else {
            &legacy.server_key
        };
        let account = AccountKey::new(server_id, legacy.user_id)
            .ok_or_else(|| "a legacy playback preference has no account identity".to_string())?;
        playback_preferences
            .save(&account, &legacy.item_id, &legacy.preference)
            .map_err(|error| format!("could not import a playback preference: {error}"))?;
    }

    library
        .finish_legacy_account_import()
        .map_err(|error| format!("could not finish the legacy account import: {error}"))
}

pub fn delete_local_account_data(services: &Arc<Services>) -> Result<(), String> {
    let account = services
        .session
        .account_key()
        .ok_or_else(|| "sign in to delete local account data".to_string())?;
    let artwork_ids = services.collections.artwork_ids_for_account(&account);
    let deletion = PendingDeletion {
        account,
        artwork_ids,
    };
    services
        .pending_deletions
        .begin(deletion.clone())
        .map_err(|error| format!("could not start the deletion journal: {error}"))?;
    let _ = services.session.begin_account_deletion(&deletion.account);
    complete_pending_deletion(services, &deletion)
}

fn complete_pending_deletion(
    services: &Arc<Services>,
    deletion: &PendingDeletion,
) -> Result<(), String> {
    services
        .accounts
        .remove_account(&deletion.account)
        .map_err(|error| format!("could not remove account settings: {error}"))?;
    services
        .collections
        .remove_account(&deletion.account)
        .map_err(|error| format!("could not remove collection settings: {error}"))?;
    services
        .playback_preferences
        .remove_account(&deletion.account)
        .map_err(|error| format!("could not remove playback preferences: {error}"))?;
    services
        .reader
        .remove_account(&deletion.account)
        .map_err(|error| format!("could not remove reading places: {error}"))?;
    crate::collections::snapshots::SnapshotRepository::new(&services.library)
        .remove_account(&deletion.account)
        .map_err(|error| format!("could not remove collection cache rows: {error}"))?;
    for artwork_id in &deletion.artwork_ids {
        services
            .artwork
            .remove(artwork_id)
            .map_err(|error| format!("could not remove custom artwork: {error}"))?;
    }
    if services.session.account_key().as_ref() == Some(&deletion.account) {
        services
            .session
            .clear_local(true)
            .map_err(|error| format!("could not clear persistent session data: {error}"))?;
        services
            .preferences
            .activate_account(None)
            .map_err(|error| format!("could not clear active account settings: {error}"))?;
    }
    services
        .pending_deletions
        .finish(&deletion.account)
        .map_err(|error| format!("could not finish the deletion journal: {error}"))?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::app::ids::random_hex;

    pub(crate) struct TestServices {
        pub services: Arc<Services>,
        directory: PathBuf,
    }

    impl TestServices {
        pub fn signed_out() -> Self {
            Self::new(None, "user")
        }

        pub fn signed_in(server_url: &str) -> Self {
            Self::new(Some(server_url), "user")
        }

        pub fn signed_in_as(server_url: &str, user_id: &str) -> Self {
            Self::new(Some(server_url), user_id)
        }

        pub fn accounts_path(&self) -> PathBuf {
            self.directory.join("accounts.json")
        }

        fn new(server_url: Option<&str>, user_id: &str) -> Self {
            let directory =
                std::env::temp_dir().join(format!("mediaflick-api-test-{}", random_hex(8)));
            std::fs::create_dir_all(&directory).expect("test directory");
            let library = Arc::new(Library::open_in_memory().expect("library"));
            if let Some(server_url) = server_url {
                let mut credentials = library.credentials();
                credentials.server_url = Some(server_url.to_string());
                credentials.server_id = Some("server".to_string());
                credentials.user_id = Some(user_id.to_string());
                credentials.token = Some("token".to_string());
                library.save_credentials(&credentials).expect("credentials");
            }
            let shell = Arc::new(ShellBridge::default());
            let session = Arc::new(Session::restore(library.clone(), shell.clone()));
            let accounts = Arc::new(
                AccountConfigurationService::open(directory.join("accounts.json"))
                    .expect("accounts"),
            );
            let preferences = Arc::new(PreferencesService::new(
                AppSettings::default(),
                accounts.clone(),
                session.account_key(),
                false,
            ));
            let companion = Arc::new(CompanionSession::new(
                session.clone(),
                library.clone(),
                shell.clone(),
            ));
            let services = Services {
                ratings: Arc::new(RatingsService::new(&library, &session, &companion)),
                letterboxd: Arc::new(ReviewService::default()),
                sync: SyncHandle::detached(),
                socket: SocketHandle::detached(),
                accounts,
                collections: Arc::new(
                    CollectionConfigurationService::open(directory.join("collections.json"))
                        .expect("collections"),
                ),
                playback_preferences: Arc::new(
                    PlaybackPreferenceService::open(directory.join("playback.json"))
                        .expect("playback preferences"),
                ),
                reader: Arc::new(ReaderStore::open(directory.join("reader.json")).expect("reader")),
                artwork: Arc::new(ArtworkStore::open(directory.join("art")).expect("artwork")),
                pending_deletions: Arc::new(
                    PendingDeletionService::open(directory.join("deletions.json"))
                        .expect("pending deletions"),
                ),
                preferences,
                shell,
                home_watched: Mutex::new(HashMap::new()),
                children_sources: ChildrenSources::default(),
                open_books: OpenBooks::default(),
                playback: RwLock::new(None),
                library,
                session,
                companion,
            };
            Self {
                services: Arc::new(services),
                directory,
            }
        }
    }

    impl Drop for TestServices {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite(code: i32) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None)
    }

    #[test]
    fn a_library_another_program_holds_is_never_offered_for_recreation() {
        for code in [5, 6, 261, 517, 773, 262] {
            assert_eq!(
                library_refusal(&OpenError::Sqlite(sqlite(code))),
                None,
                "{code}"
            );
            assert_eq!(
                library_refusal(&OpenError::Migration {
                    from: 23,
                    step: 23..24,
                    error: sqlite(code),
                }),
                None,
                "{code} during a migration"
            );
        }
        for code in [1, 14, 11, 26] {
            assert_eq!(
                library_refusal(&OpenError::Sqlite(sqlite(code))),
                Some(LibraryRefusal::Unusable),
                "{code}"
            );
            assert_eq!(
                library_refusal(&OpenError::Migration {
                    from: 0,
                    step: 0..23,
                    error: sqlite(code),
                }),
                Some(LibraryRefusal::Unusable),
                "{code} during a migration"
            );
        }
        assert_eq!(
            library_refusal(&OpenError::NewerSchema { found: 24 }),
            Some(LibraryRefusal::NewerSchema)
        );
    }
}
