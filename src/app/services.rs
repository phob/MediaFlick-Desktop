//! Process-wide services the app-scheme API reaches from CEF's background
//! threads, where the UI-thread browser state is not available.

use std::sync::{Arc, Mutex, OnceLock, RwLock, mpsc};

use crate::app::paths;
use crate::companion::{CompanionSession, RequestsProvider};
use crate::jellyfin::session::Session;
use crate::library::Library;
use crate::library::sync::{self, SyncHandle};
use crate::playback::PlaybackCoordinator;
use crate::preferences::{AppSettings, PreferencesService};
use crate::seerr::SeerrSession;

static SERVICES: OnceLock<Arc<Services>> = OnceLock::new();
static INIT_ERROR: OnceLock<String> = OnceLock::new();
/// `OnceLock::set` only makes the *result* single-shot, not the work leading up
/// to it, so the whole sequence is serialized here instead.
static INIT_LOCK: Mutex<()> = Mutex::new(());

pub struct Services {
    pub library: Arc<Library>,
    pub session: Arc<Session>,
    pub companion: Arc<CompanionSession>,
    pub seerr: Arc<SeerrSession>,
    pub sync: SyncHandle,
    pub preferences: Arc<PreferencesService>,
    pub shell: ShellBridge,
    playback: RwLock<Option<Arc<PlaybackCoordinator>>>,
}

/// Native work requested by the React application. The app-scheme API runs on
/// CEF background threads, while file dialogs and player installation must run
/// on CEF's UI thread; this narrow queue is the boundary between the two.
#[derive(Debug, Clone)]
pub enum ShellRequest {
    FilePicker {
        request_id: String,
        target: ShellFilePickerTarget,
    },
    InstallMpv {
        request_id: String,
    },
    /// A background exact-ID repair changed metadata/artwork already queried
    /// by React. The shell relays this after SQLite commits so active cache
    /// surfaces can invalidate without deleting any local data.
    MetadataRepaired {
        item_ids: Vec<String>,
    },
    /// The one-shot background repair observed an authorization rejection.
    /// Relay it so the UI re-reads the canonical session state immediately.
    SessionExpired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFilePickerTarget {
    Mpv,
    Mpchc,
}

pub struct ShellBridge {
    sender: Mutex<Option<mpsc::Sender<ShellRequest>>>,
}

impl ShellBridge {
    fn new() -> Self {
        Self {
            sender: Mutex::new(None),
        }
    }

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

    pub fn requests_provider(&self) -> RequestsProvider {
        // Cached once probed; only the first caller after a sign-in (or one
        // racing the startup warm-up) pays a network round-trip.
        let _ = self.companion.probe(false);
        RequestsProvider::select(self.companion.clone(), self.seerr.clone())
    }
}

/// Opens the library database and restores the session. Safe to call twice;
/// only the first call does the work.
pub fn init() -> Option<Arc<Services>> {
    init_with_settings(AppSettings::load())
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
    let session = Arc::new(Session::restore(library.clone()));
    let companion = Arc::new(CompanionSession::new(session.clone(), library.clone()));
    let seerr = Arc::new(SeerrSession::restore(library.clone()));
    // A link left behind by a Jellyfin account that is no longer signed in is
    // dropped before anything can reach it.
    seerr.revalidate();
    if restored {
        // `init` runs on the CEF UI thread and holds INIT_LOCK, so the probe
        // must not wait on the network here; it warms the cache from its own
        // thread and the API paths re-check lazily.
        let companion = companion.clone();
        let session = session.clone();
        std::thread::spawn(move || {
            if let Err(error) = companion.probe(false) {
                session.note_error(&error);
                tracing::debug!(target: "companion", "initial companion probe failed: {error}");
            }
        });
    }
    let sync = sync::spawn(library.clone(), session.clone());
    tracing::info!(
        target: "jellyfin.session",
        restored,
        "library services started"
    );
    let services = Arc::new(Services {
        library,
        session,
        companion,
        seerr,
        sync,
        preferences: Arc::new(PreferencesService::new(initial_settings)),
        shell: ShellBridge::new(),
        playback: RwLock::new(None),
    });
    let _ = SERVICES.set(services.clone());
    tracing::info!(target: "library.db", path = %path.display(), "library database ready");
    Some(services)
}

pub fn services() -> Option<Arc<Services>> {
    SERVICES.get().cloned()
}

/// Best-effort UI notification for background cache repair. If CEF has not
/// subscribed yet, no query could have observed the old row through that
/// browser, so dropping the event is safe.
pub fn notify_metadata_repaired(item_ids: Vec<String>) {
    if item_ids.is_empty() {
        return;
    }
    if let Some(services) = services() {
        let _ = services
            .shell
            .request(ShellRequest::MetadataRepaired { item_ids });
    }
}

pub fn notify_session_expired() {
    if let Some(services) = services() {
        let _ = services.shell.request(ShellRequest::SessionExpired);
    }
}

/// Why [`init`] failed, for the error the UI shows instead of a blank window.
pub fn init_error() -> Option<&'static str> {
    INIT_ERROR.get().map(String::as_str)
}
