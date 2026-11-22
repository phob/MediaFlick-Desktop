use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::app::paths;
use crate::app::services::{self as app_services, LibraryRefusal, Services, StartupFailure};
use crate::library;
use crate::preferences::AppSettings;

static SERVICES: OnceLock<Arc<Services>> = OnceLock::new();
static INIT_LOCK: Mutex<InitState> = Mutex::new(InitState {
    failed: None,
    on_open: None,
});

struct InitState {
    failed: Option<Failed>,
    on_open: Option<OnOpen>,
}

struct Failed {
    failure: StartupFailure,
    settings: AppSettings,
}

type OnOpen = Box<dyn FnOnce(&Arc<Services>) + Send>;

pub struct InitPermit {
    state: MutexGuard<'static, InitState>,
}

impl InitPermit {
    fn take() -> Self {
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

pub(in crate::shell) fn library_refusal() -> Option<LibraryRefusal> {
    InitPermit::take()
        .state
        .failed
        .as_ref()
        .and_then(|failed| failed.failure.library)
}

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
    let permit = InitPermit::take();
    if let Some(services) = services() {
        return Ok(services);
    }
    if let Some(failed) = &permit.state.failed {
        return Err(failed.failure.reason.clone());
    }
    open(permit, settings())
}

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
    Open,
    NotRefused,
    SetAside(std::io::Error),
    Reopen(String),
}

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
