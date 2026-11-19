//! The process-wide `Services`, reachable only from the CEF shell entry points.
//! Everything below the shell receives what it needs by injection.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::app::paths;
use crate::app::services::{self as app_services, LibraryRefusal, Services, StartupFailure};
use crate::library;
use crate::preferences::AppSettings;

static SERVICES: OnceLock<Arc<Services>> = OnceLock::new();
/// `OnceLock::set` only makes the *result* single-shot, not the work leading up
/// to it, so the whole sequence is serialized here instead, with what it left.
static INIT_LOCK: Mutex<InitState> = Mutex::new(InitState {
    failed: None,
    on_open: None,
});

struct InitState {
    /// Why the services failed to open. It stays for the process: a failed
    /// open is not retried on the next request. A retry would only redo the
    /// same work under `INIT_LOCK`, which the UI thread waits on, and a failed
    /// `library.db` migration would run and roll back again on every request.
    /// Only the user's Recreate library ([`recreate_library`]) clears it.
    failed: Option<Failed>,
    /// The shell's wiring for services that open after its window exists.
    on_open: Option<OnOpen>,
}

struct Failed {
    failure: StartupFailure,
    /// What the failed open was given, for the open after a recreate.
    settings: AppSettings,
}

type OnOpen = Box<dyn FnOnce(&Arc<Services>) + Send>;

/// Proof that the caller holds `INIT_LOCK`. Only this module can create one,
/// so `app::services::open` and `start` cannot run outside this module.
pub struct InitPermit {
    state: MutexGuard<'static, InitState>,
}

impl InitPermit {
    fn take() -> Self {
        // A poisoned lock only means a previous attempt panicked, which the
        // checks after taking it already handle.
        Self {
            state: INIT_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        }
    }
}

pub(in crate::shell) fn services() -> Option<Arc<Services>> {
    SERVICES.get().cloned()
}

/// How `library.db` was refused, when that is why the services are down.
pub(in crate::shell) fn library_refusal() -> Option<LibraryRefusal> {
    InitPermit::take()
        .state
        .failed
        .as_ref()
        .and_then(|failed| failed.failure.library)
}

/// The services, or why they failed to open.
#[expect(
    clippy::disallowed_methods,
    reason = "the services are not open yet, so settings.json is read here to open them"
)]
pub(in crate::shell) fn init() -> Result<Arc<Services>, String> {
    open_once(AppSettings::load_at_startup)
}

pub(in crate::shell) fn init_with_settings(initial_settings: AppSettings) -> Option<Arc<Services>> {
    open_once(|| initial_settings).ok()
}

fn open_once(settings: impl FnOnce() -> AppSettings) -> Result<Arc<Services>, String> {
    if let Some(services) = services() {
        return Ok(services);
    }
    // Several CEF threads can reach the API at once before anything is up.
    // Whoever takes this lock and still finds no services is the one that
    // builds them; without it, both would open the database and spawn a sync
    // thread, and the loser of the `SERVICES.set` race would leak its thread
    // against a `Library` nothing else can see.
    let permit = InitPermit::take();
    if let Some(services) = services() {
        return Ok(services);
    }
    if let Some(failed) = &permit.state.failed {
        return Err(failed.failure.reason.clone());
    }
    open(permit, settings())
}

/// Opens the services, or records why they failed.
fn open(mut permit: InitPermit, settings: AppSettings) -> Result<Arc<Services>, String> {
    let services = match app_services::open(&permit, settings.clone()) {
        Ok(services) => services,
        Err(failure) => {
            let reason = failure.reason.clone();
            permit.state.failed = Some(Failed { failure, settings });
            return Err(reason);
        }
    };
    let _ = SERVICES.set(services.clone());
    app_services::start(&permit, &services);
    let on_open = permit.state.on_open.take();
    drop(permit);
    if let Some(connect) = on_open {
        connect(&services);
    }
    Ok(services)
}

/// Runs `connect` with the services: now if they are open, or once they open
/// after a Recreate library.
pub(in crate::shell) fn on_services(connect: impl FnOnce(&Arc<Services>) + Send + 'static) {
    let mut permit = InitPermit::take();
    match services() {
        Some(services) => {
            drop(permit);
            connect(&services);
        }
        None => permit.state.on_open = Some(Box::new(connect)),
    }
}

pub(in crate::shell) enum RecreateError {
    /// The services are open, so `library.db` is in use.
    Open,
    /// The services are down for a reason other than `library.db`.
    NotRefused,
    /// The refused file could not be moved aside; it is where it was.
    SetAside(std::io::Error),
    /// The file was moved aside, and the services still failed to open.
    Reopen(String),
}

/// The user's Recreate library: moves the refused `library.db` and its
/// sidecars aside, then opens the services on a new one, once. Only this
/// clears a recorded failure, and only when `library.db` was what failed.
/// Answers where the old file went.
pub(in crate::shell) fn recreate_library() -> Result<PathBuf, RecreateError> {
    let mut permit = InitPermit::take();
    if services().is_some() {
        return Err(RecreateError::Open);
    }
    let Some(failed) = permit
        .state
        .failed
        .take_if(|failed| failed.failure.library.is_some())
    else {
        return Err(RecreateError::NotRefused);
    };
    let path = paths::library_db_path();
    let backup = library::db::set_aside_path(&path, library::now_unix());
    if let Err(error) = library::db::set_aside(&path, &backup) {
        tracing::error!(
            target: "library.db",
            "could not move {} aside to {}: {error}",
            path.display(),
            backup.display()
        );
        permit.state.failed = Some(failed);
        return Err(RecreateError::SetAside(error));
    }
    tracing::warn!(
        target: "library.db",
        "Recreate library: moved the refused {} and its sidecars to {}; opening a new one",
        path.display(),
        backup.display()
    );
    open(permit, failed.settings)
        .map(|_| backup)
        .map_err(RecreateError::Reopen)
}
