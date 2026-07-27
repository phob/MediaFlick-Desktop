//! Process-wide services the app-scheme API reaches from CEF's background
//! threads, where the UI-thread browser state is not available.

use std::sync::{Arc, Mutex, OnceLock, RwLock};

use crate::app::paths;
use crate::jellyfin::session::Session;
use crate::library::Library;
use crate::library::sync::{self, SyncHandle};
use crate::playback::PlaybackCoordinator;
use crate::seer::SeerSession;

static SERVICES: OnceLock<Arc<Services>> = OnceLock::new();
static INIT_ERROR: OnceLock<String> = OnceLock::new();
/// `OnceLock::set` only makes the *result* single-shot, not the work leading up
/// to it, so the whole sequence is serialized here instead.
static INIT_LOCK: Mutex<()> = Mutex::new(());

pub struct Services {
    pub library: Arc<Library>,
    pub session: Arc<Session>,
    pub seer: Arc<SeerSession>,
    pub sync: SyncHandle,
    playback: RwLock<Option<Arc<PlaybackCoordinator>>>,
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
    let seer = Arc::new(SeerSession::restore(library.clone()));
    // A link left behind by a Jellyfin account that is no longer signed in is
    // dropped before anything can reach it.
    seer.revalidate();
    let sync = sync::spawn(library.clone(), session.clone());
    tracing::info!(
        target: "jellyfin.session",
        restored,
        "library services started"
    );
    let services = Arc::new(Services {
        library,
        session,
        seer,
        sync,
        playback: RwLock::new(None),
    });
    let _ = SERVICES.set(services.clone());
    tracing::info!(target: "library.db", path = %path.display(), "library database ready");
    Some(services)
}

pub fn services() -> Option<Arc<Services>> {
    SERVICES.get().cloned()
}

/// Why [`init`] failed, for the error the UI shows instead of a blank window.
pub fn init_error() -> Option<&'static str> {
    INIT_ERROR.get().map(String::as_str)
}
