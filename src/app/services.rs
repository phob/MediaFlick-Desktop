//! Process-wide services the app-scheme API reaches from CEF's background
//! threads, where the UI-thread browser state is not available.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock, mpsc};

use crate::app::paths;
use crate::collections::artwork::{ArtworkStore, custom_art_dir};
use crate::companion::CompanionSession;
use crate::integrations::letterboxd::ReviewService;
use crate::integrations::mdblist::RatingsService;
use crate::jellyfin::session::Session;
use crate::jellyfin::socket::{self, SocketHandle};
use crate::library::Library;
use crate::library::LibraryChangeBatch;
use crate::library::sync::{self, SyncHandle};
use crate::playback::PlaybackCoordinator;
use crate::preferences::{
    AccountConfigurationService, AccountKey, AppSettings, CollectionConfigurationService,
    PendingDeletion, PendingDeletionService, PlaybackPreferenceService, PreferencesService,
    accounts_file_path, collections_file_path, pending_deletions_file_path,
    playback_preferences_file_path,
};

static SERVICES: OnceLock<Arc<Services>> = OnceLock::new();
static INIT_ERROR: OnceLock<String> = OnceLock::new();
/// `OnceLock::set` only makes the *result* single-shot, not the work leading up
/// to it, so the whole sequence is serialized here instead.
static INIT_LOCK: Mutex<()> = Mutex::new(());

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
    pub artwork: Arc<ArtworkStore>,
    pub pending_deletions: Arc<PendingDeletionService>,
    pub preferences: Arc<PreferencesService>,
    pub shell: Arc<ShellBridge>,
    pub home_watched: Mutex<HashMap<AccountKey, Option<crate::library::ItemDetail>>>,
    playback: RwLock<Option<Arc<PlaybackCoordinator>>>,
}

/// Native work requested by the React application. The app-scheme API runs on
/// CEF background threads, while file dialogs and player installation must run
/// on CEF's UI thread; this narrow queue is the boundary between the two.
#[derive(Debug, Clone)]
pub enum ShellRequest {
    /// The startup cover has left the DOM and the completed initial route has
    /// painted. Revealing the native window must happen on CEF's UI thread.
    MainWindowReady,
    /// Choose the external mpv executable for the Player settings draft.
    FilePicker {
        request_id: String,
    },
    InstallMpv {
        request_id: String,
    },
    /// A committed cache mutation changed catalog rows.
    LibraryChanged {
        item_ids: Vec<String>,
        context_ids: Vec<String>,
    },
    /// A bootstrap page changed local catalog projections only.
    CatalogChanged {
        item_ids: Vec<String>,
        context_ids: Vec<String>,
    },
    /// A committed provider snapshot changed collection projections.
    CollectionsChanged,
    /// Any native Jellyfin call observed an authorization rejection.
    /// Relay it so the UI re-reads the canonical session state immediately,
    /// even when the failing request itself is silent by design.
    SessionExpired,
}

/// The queue from background work to the CEF UI thread. The services that
/// change what the UI shows hold it directly; until CEF subscribes, requests
/// are dropped, which is safe because no page can have read the old state.
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

    /// A committed cache batch. An empty batch has nothing to refresh.
    pub fn library_changed(&self, changes: LibraryChangeBatch) {
        if changes.item_ids.is_empty() {
            return;
        }
        let _ = self.request(ShellRequest::LibraryChanged {
            item_ids: changes.item_ids,
            context_ids: changes.context_ids,
        });
    }

    pub fn catalog_changed(&self, changes: LibraryChangeBatch) {
        let _ = self.request(ShellRequest::CatalogChanged {
            item_ids: changes.item_ids,
            context_ids: changes.context_ids,
        });
    }

    pub fn session_expired(&self) {
        let _ = self.request(ShellRequest::SessionExpired);
    }

    pub fn collections_changed(&self) {
        let _ = self.request(ShellRequest::CollectionsChanged);
    }

    /// Refreshes aggregate watch-state projections once after a full catalog
    /// observation, including user data learned during a daily rebootstrap.
    pub fn library_sync_completed(&self) {
        let _ = self.request(ShellRequest::LibraryChanged {
            item_ids: Vec::new(),
            context_ids: Vec::new(),
        });
    }
}

impl Services {
    /// The playback coordinator is created with the browser state, after the
    /// services exist, so it is attached rather than injected.
    pub fn attach_playback(&self, playback: Arc<PlaybackCoordinator>) {
        if let Ok(mut slot) = self.playback.write() {
            *slot = Some(playback);
        }
    }

    pub fn playback(&self) -> Option<Arc<PlaybackCoordinator>> {
        self.playback.read().ok().and_then(|slot| slot.clone())
    }
}

/// Opens the library database and restores the session. Safe to call twice;
/// only the first call does the work.
pub fn init() -> Option<Arc<Services>> {
    services().or_else(|| init_with_settings(AppSettings::load()))
}

/// Same initialization path, with the already-normalized launch settings CEF
/// will use. This keeps CLI overrides and API writes in one snapshot.
pub fn init_with_settings(initial_settings: AppSettings) -> Option<Arc<Services>> {
    if let Some(services) = SERVICES.get() {
        return Some(services.clone());
    }
    // Several CEF threads can reach the API at once before anything is up.
    // Whoever takes this lock and still finds no services is the one that
    // builds them; without it, both would open the database and spawn a sync
    // thread, and the loser of the `SERVICES.set` race would leak its thread
    // against a `Library` nothing else can see. A poisoned lock only means a
    // previous attempt panicked, which the checks below already handle.
    let _guard = INIT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(services) = SERVICES.get() {
        return Some(services.clone());
    }
    let path = paths::library_db_path();
    let library = match Library::open(&path) {
        Ok(library) => Arc::new(library),
        Err(error) => {
            let message = format!("could not open {}: {error}", path.display());
            tracing::error!(target: "library.db", "{message}");
            let _ = INIT_ERROR.set(message);
            return None;
        }
    };
    let restored = library.credentials().is_authenticated();
    let shell = Arc::new(ShellBridge::default());
    let session = Arc::new(Session::restore(library.clone(), shell.clone()));
    let account_path = accounts_file_path();
    let accounts = open_configuration(&account_path, AccountConfigurationService::open)?;
    let active_account = session.account_key();
    if let Err(error) =
        accounts.import_legacy_appearance(active_account.as_ref(), &initial_settings.appearance)
    {
        return initialization_failure(format!(
            "could not import legacy appearance settings: {error}"
        ));
    }
    let collections_path = collections_file_path();
    let collections = open_configuration(&collections_path, CollectionConfigurationService::open)?;
    let playback_path = playback_preferences_file_path();
    let playback_preferences = open_configuration(&playback_path, PlaybackPreferenceService::open)?;
    if let Err(error) = import_legacy_account_data(&library, &accounts, &playback_preferences) {
        return initialization_failure(error);
    }
    let preferences = Arc::new(PreferencesService::new(
        initial_settings,
        accounts.clone(),
        active_account,
    ));
    let artwork_path = custom_art_dir();
    let artwork = open_configuration(&artwork_path, ArtworkStore::open)?;
    if let Err(error) = artwork.collect_orphans(&collections_path, std::time::SystemTime::now()) {
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
    let ratings = Arc::new(RatingsService::new(library.clone(), companion.clone()));
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
        artwork,
        pending_deletions,
        preferences,
        shell,
        home_watched: Mutex::new(HashMap::new()),
        playback: RwLock::new(None),
    });
    connect_app_hooks(&services);
    resume_pending_deletions(&services);
    let _ = SERVICES.set(services.clone());
    crate::collections::scheduler::start(services.clone());
    tracing::info!(target: "library.db", path = %path.display(), "library database ready");
    Some(services)
}

/// `init` holds INIT_LOCK, which the CEF UI thread waits on, so the probe must
/// not wait on the network there; it warms the cache from its own thread and
/// the API paths re-check lazily.
fn warm_restored_session(session: Arc<Session>, companion: Arc<CompanionSession>) {
    crate::app::threads::spawn_named("session-restore", move || {
        session.refresh_user_policy();
        if let Err(error) = companion.probe(false) {
            tracing::debug!(target: "companion", "initial companion probe failed: {error}");
        }
    });
}

/// The lower layers report work that only the application can act on through
/// hooks registered here, rather than looking the services up themselves. The
/// hooks hold weak references, so they never keep the services alive.
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

fn resume_pending_deletions(services: &Arc<Services>) {
    for deletion in services.pending_deletions.pending() {
        let _ = services.session.begin_account_deletion(&deletion.account);
        if let Err(error) = complete_pending_deletion(services, &deletion) {
            tracing::warn!(
                target: "account-deletion",
                server_id = deletion.account.server_id(),
                user_id = deletion.account.user_id(),
                "could not resume local account deletion: {error}"
            );
        }
    }
}

fn open_configuration<T>(
    path: &Path,
    open: impl FnOnce(PathBuf) -> std::io::Result<T>,
) -> Option<Arc<T>> {
    match open(path.to_path_buf()) {
        Ok(service) => Some(Arc::new(service)),
        Err(error) => {
            let message = format!("could not open {}: {error}", path.display());
            tracing::error!(target: "config", "{message}");
            let _ = INIT_ERROR.set(message);
            None
        }
    }
}

fn initialization_failure(message: String) -> Option<Arc<Services>> {
    tracing::error!(target: "config", "{message}");
    let _ = INIT_ERROR.set(message);
    None
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

pub fn services() -> Option<Arc<Services>> {
    SERVICES.get().cloned()
}

/// Why [`init`] failed, for the error the UI shows instead of a blank window.
pub fn init_error() -> Option<&'static str> {
    INIT_ERROR.get().map(String::as_str)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::app::ids::random_hex;

    /// Services over an in-memory catalog and a scratch configuration folder,
    /// without the sync and event-socket threads, so API tests never touch the
    /// user's data or reach a server the test did not start.
    pub(crate) struct TestServices {
        pub services: Arc<Services>,
        directory: PathBuf,
    }

    impl TestServices {
        pub fn signed_out() -> Self {
            Self::new(None)
        }

        /// Signed in as `user` on the Jellyfin server `server` at `server_url`.
        pub fn signed_in(server_url: &str) -> Self {
            Self::new(Some(server_url))
        }

        fn new(server_url: Option<&str>) -> Self {
            let directory =
                std::env::temp_dir().join(format!("mediaflick-api-test-{}", random_hex(8)));
            std::fs::create_dir_all(&directory).expect("test directory");
            let library = Arc::new(Library::open_in_memory().expect("library"));
            if let Some(server_url) = server_url {
                let mut credentials = library.credentials();
                credentials.server_url = Some(server_url.to_string());
                credentials.server_id = Some("server".to_string());
                credentials.user_id = Some("user".to_string());
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
            ));
            let companion = Arc::new(CompanionSession::new(
                session.clone(),
                library.clone(),
                shell.clone(),
            ));
            let services = Services {
                ratings: Arc::new(RatingsService::new(library.clone(), companion.clone())),
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
                artwork: Arc::new(ArtworkStore::open(directory.join("art")).expect("artwork")),
                pending_deletions: Arc::new(
                    PendingDeletionService::open(directory.join("deletions.json"))
                        .expect("pending deletions"),
                ),
                preferences,
                shell,
                home_watched: Mutex::new(HashMap::new()),
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
