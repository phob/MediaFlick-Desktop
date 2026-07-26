//! Process-wide services the app-scheme API reaches from CEF's background
//! threads, where the UI-thread browser state is not available.

use std::sync::{Arc, OnceLock, RwLock};

use crate::app::paths;
use crate::jellyfin::session::Session;
use crate::library::Library;
use crate::library::sync::{self, SyncHandle};
use crate::playback::PlaybackCoordinator;

static SERVICES: OnceLock<Arc<Services>> = OnceLock::new();
static INIT_ERROR: OnceLock<String> = OnceLock::new();

pub struct Services {
    pub library: Arc<Library>,
    pub session: Arc<Session>,
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
    let sync = sync::spawn(library.clone(), session.clone());
    tracing::info!(
        target: "jellyfin.session",
        restored,
        "library services started"
    );
    let services = Arc::new(Services {
        library,
        session,
        sync,
        playback: RwLock::new(None),
    });
    let _ = SERVICES.set(services);
    tracing::info!(target: "library.db", path = %path.display(), "library database ready");
    SERVICES.get().cloned()
}

pub fn services() -> Option<Arc<Services>> {
    SERVICES.get().cloned()
}

/// Why [`init`] failed, for the error the UI shows instead of a blank window.
pub fn init_error() -> Option<&'static str> {
    INIT_ERROR.get().map(String::as_str)
}
