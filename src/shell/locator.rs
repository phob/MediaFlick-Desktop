//! The process-wide `Services`, reachable only from the CEF shell entry points.
//! Everything below the shell receives what it needs by injection.

use std::sync::{Arc, Mutex, OnceLock};

use crate::app::services::{self as app_services, Services};
use crate::preferences::AppSettings;

static SERVICES: OnceLock<Arc<Services>> = OnceLock::new();
static INIT_ERROR: OnceLock<String> = OnceLock::new();
/// `OnceLock::set` only makes the *result* single-shot, not the work leading up
/// to it, so the whole sequence is serialized here instead.
static INIT_LOCK: Mutex<()> = Mutex::new(());

pub(in crate::shell) fn services() -> Option<Arc<Services>> {
    SERVICES.get().cloned()
}

pub(in crate::shell) fn init_error() -> Option<&'static str> {
    INIT_ERROR.get().map(String::as_str)
}

pub(in crate::shell) fn init() -> Option<Arc<Services>> {
    services().or_else(|| init_with_settings(AppSettings::load()))
}

pub(in crate::shell) fn init_with_settings(initial_settings: AppSettings) -> Option<Arc<Services>> {
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
    let services = match app_services::open(initial_settings) {
        Ok(services) => services,
        Err(message) => {
            let _ = INIT_ERROR.set(message);
            return None;
        }
    };
    let _ = SERVICES.set(services.clone());
    app_services::start(&services);
    Some(services)
}
