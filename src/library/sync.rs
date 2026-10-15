//! Background synchronisation of the catalog index.
//!
//! One controlled worker owns the whole cycle: a resumable catalog fill, an
//! incremental `DateCreated` sweep, and a periodic identity/user-data/deletion
//! pass. It idles until credentials exist, resumes durable offsets after
//! restart, and pauses when the server rejects the token.
//!
//! Jellyfin offers no "changed since" ordering — `DateLastSaved` is a valid
//! `ItemFields` value but not a valid `ItemSortBy` one, and servers return it
//! empty. So the incremental sweep catches *new* items via `DateCreated`
//! (which also covers a replaced file, since Jellyfin re-creates the item with
//! a fresh id), and in-place metadata edits are picked up by the periodic
//! re-bootstrap instead.

mod children;
mod cycle;
mod libraries;
mod worker;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::app::services::ShellBridge;
use crate::jellyfin::api::ApiError;
use crate::jellyfin::session::{Session, SessionScope};

use super::{Library, LibraryChangeBatch};

pub use children::title_children;
pub use cycle::run_cycle;
pub use worker::spawn;

pub const SYNC_INTERVAL: Duration = Duration::from_secs(10 * 60);

pub(super) const IDENTITY_SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);
pub(super) const REBOOTSTRAP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub(super) const IDENTITY_PAGE_SIZE: i64 = 1_000;
pub(super) const MAX_INCREMENTAL_PAGES: usize = 100;
/// The same valve for the two full passes, which end on the server's own idea
/// of where the library stops. Both caps sit far above any real library — a
/// server that reaches one is repeating pages or miscounting, not answering
/// honestly — so they only bound a runaway, never a normal cycle.
pub(super) const MAX_BOOTSTRAP_PAGES: usize = 1_000;
pub(super) const MAX_IDENTITY_PAGES: usize = 1_000;

pub(super) const META_BOOTSTRAP_OFFSET: &str = "sync.bootstrap_offset";
pub(super) const META_BOOTSTRAP_TOTAL: &str = "sync.bootstrap_total";
pub(super) const META_BOOTSTRAP_DONE: &str = "sync.bootstrap_done";
pub(super) const META_CATALOG_READY: &str = "sync.catalog_ready";
pub(super) const META_WATERMARK: &str = "sync.watermark";
pub(super) const META_WATERMARK_IDS: &str = "sync.watermark_ids";
pub(super) const META_LAST_IDENTITY_SWEEP: &str = "sync.identity_sweep_at";
pub(super) const META_LAST_BOOTSTRAP: &str = "sync.bootstrap_at";
pub(super) const META_LAST_SYNC: &str = "sync.completed_at";
pub(super) const META_LATEST_FAILURE: &str = "sync.latest_failure";

/// Durable progress for the resumable initial cache fill.
///
/// Both values live in SQLite rather than in the worker handle so a restarted
/// app can report the same progress before the next page request begins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapProgress {
    pub complete: bool,
    #[serde(skip)]
    pub ready: bool,
    pub processed: i64,
    pub total: Option<i64>,
}

/// The catalog becomes usable after its first successful page commit.
///
/// This only reports progress; the sync cycle reads its own state fallibly.
/// An unreadable database is logged and reported as a catalog that is not
/// ready yet.
pub fn bootstrap_progress(library: &Library) -> BootstrapProgress {
    read_bootstrap_progress(library).unwrap_or_else(|error| {
        tracing::warn!(target: "library.sync", "could not read catalog progress: {error}");
        BootstrapProgress::default()
    })
}

fn read_bootstrap_progress(library: &Library) -> rusqlite::Result<BootstrapProgress> {
    let complete = flag(library, META_BOOTSTRAP_DONE)?;
    Ok(BootstrapProgress {
        complete,
        ready: complete || flag(library, META_CATALOG_READY)?,
        processed: meta_count(library, META_BOOTSTRAP_OFFSET)?.unwrap_or(0),
        total: meta_count(library, META_BOOTSTRAP_TOTAL)?,
    })
}

pub fn ownership_available(library: &Library) -> bool {
    let available = flag(library, META_BOOTSTRAP_DONE)
        .and_then(|done| Ok(done && !flag(library, META_LATEST_FAILURE)?));
    available.unwrap_or_else(|error| {
        tracing::warn!(target: "library.sync", "could not read catalog ownership state: {error}");
        false
    })
}

fn flag(library: &Library, key: &str) -> rusqlite::Result<bool> {
    Ok(library.meta(key)?.as_deref() == Some("1"))
}

fn meta_count(library: &Library, key: &str) -> rusqlite::Result<Option<i64>> {
    Ok(library
        .meta(key)?
        .and_then(|value| value.parse::<i64>().ok())
        .map(|value| value.max(0)))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub bootstrapped: usize,
    pub updated: usize,
    pub user_data_refreshed: usize,
    pub deleted: usize,
    pub elapsed_ms: u64,
    #[serde(skip)]
    pub changes: LibraryChangeBatch,
}

impl SyncReport {
    pub fn changed(&self) -> bool {
        self.bootstrapped > 0 || self.updated > 0 || self.deleted > 0
    }
}

/// What set this cycle off.
///
/// The distinction only matters to the identity sweep. Its hourly gate is a
/// bandwidth budget for the *timer*, not a correctness rule, and it is the only
/// thing that notices a deletion — so an explicit ask has to be able to skip it.
/// Without that, pressing refresh after deleting something on the server does
/// nothing observable until the hour is up, which reads as a broken button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Scheduled,
    Requested,
}

impl Trigger {
    pub(super) fn forces_identity_sweep(self) -> bool {
        matches!(self, Self::Requested)
    }
}

pub(super) struct Signal {
    flags: Mutex<Flags>,
    condvar: Condvar,
}

#[derive(Default)]
pub(super) struct Flags {
    requested: bool,
    stopped: bool,
    window_ready: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncPhase {
    Catalog,
    Reconciling,
    #[default]
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncFailure {
    Unreachable,
    Server,
    Storage,
}

impl SyncFailure {
    fn of(error: &ApiError) -> Self {
        match error {
            ApiError::Transport(_) => Self::Unreachable,
            ApiError::Storage(_) => Self::Storage,
            ApiError::Unauthorized
            | ApiError::Status { .. }
            | ApiError::RateLimited { .. }
            | ApiError::Remote { .. }
            | ApiError::Decode(_)
            | ApiError::NotConfigured
            | ApiError::Cancelled => Self::Server,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Retry {
    pub reason: SyncFailure,
    pub retry_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Bootstrap {
    Loading,
    Retrying(Retry),
    Ready,
}

#[derive(Debug, Clone, Copy)]
struct Failure {
    generation: u64,
    retry: Retry,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct WorkerState {
    phase: SyncPhase,
    failure: Option<Failure>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub active: bool,
    pub phase: SyncPhase,
    pub catalog: BootstrapProgress,
    pub retry: Option<Retry>,
}

pub struct SyncStatus {
    pub progress: SyncProgress,
    pub bootstrap: Bootstrap,
}

pub type CycleCompleted = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct SyncHandle {
    signal: Arc<Signal>,
    running: Arc<AtomicBool>,
    state: Arc<Mutex<WorkerState>>,
    shell: Arc<ShellBridge>,
    cycle_completed: Arc<Mutex<Option<CycleCompleted>>>,
}

impl SyncHandle {
    fn new(shell: Arc<ShellBridge>) -> Self {
        Self {
            signal: Arc::new(Signal {
                flags: Mutex::new(Flags::default()),
                condvar: Condvar::new(),
            }),
            running: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(WorkerState::default())),
            shell,
            cycle_completed: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    pub fn detached() -> Self {
        Self::new(Arc::default())
    }

    pub fn on_cycle_completed(&self, hook: CycleCompleted) {
        if let Ok(mut slot) = self.cycle_completed.lock() {
            *slot = Some(hook);
        }
    }

    pub(super) fn shell(&self) -> &ShellBridge {
        &self.shell
    }

    pub(super) fn announce_cycle_completed(&self) {
        self.shell.library_sync_completed();
        let hook = self
            .cycle_completed
            .lock()
            .ok()
            .and_then(|slot| slot.clone());
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Asks for a cycle as soon as possible: sign-in, the refresh button, or an
    /// eviction that proved the cache is behind.
    ///
    /// The resulting cycle runs as [`Trigger::Requested`], so it also reconciles
    /// deletions instead of waiting out the identity sweep's hourly gate.
    pub fn request(&self) {
        if let Ok(mut flags) = self.signal.flags.lock() {
            flags.requested = true;
        }
        self.signal.condvar.notify_all();
    }

    pub fn release_startup_hold(&self) {
        if let Ok(mut flags) = self.signal.flags.lock() {
            flags.window_ready = true;
        }
        self.signal.condvar.notify_all();
    }

    pub(super) fn window_ready(&self) -> bool {
        self.signal
            .flags
            .lock()
            .map(|flags| flags.window_ready)
            .unwrap_or(true)
    }

    pub fn stop(&self) {
        if let Ok(mut flags) = self.signal.flags.lock() {
            flags.stopped = true;
        }
        self.signal.condvar.notify_all();
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn status(&self, catalog: BootstrapProgress, generation: u64) -> SyncStatus {
        let state = self.state.lock().map(|state| *state).unwrap_or_default();
        let retry = state
            .failure
            .filter(|failure| failure.generation == generation)
            .map(|failure| failure.retry);
        let running = self.is_running();
        let active = running || !catalog.complete;
        let phase = if running {
            state.phase
        } else if !catalog.complete {
            SyncPhase::Catalog
        } else {
            SyncPhase::Complete
        };
        let bootstrap = if catalog.ready {
            Bootstrap::Ready
        } else {
            retry.map_or(Bootstrap::Loading, Bootstrap::Retrying)
        };
        SyncStatus {
            progress: SyncProgress {
                active,
                phase,
                catalog,
                retry: retry.filter(|_| active && !running),
            },
            bootstrap,
        }
    }

    pub(super) fn set_phase(&self, phase: SyncPhase) {
        if let Ok(mut state) = self.state.lock() {
            state.phase = phase;
        }
    }

    pub(super) fn set_retry(&self, error: &ApiError, delay: Duration, generation: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.failure = Some(Failure {
                generation,
                retry: Retry {
                    reason: SyncFailure::of(error),
                    retry_at: now_unix() as i64 + delay.as_secs() as i64,
                },
            });
        }
    }

    pub(super) fn clear_failure(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.failure = None;
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal
            .flags
            .lock()
            .map(|flags| flags.stopped)
            .unwrap_or(true)
    }
}

pub(super) fn cancelled(
    control: Option<&SyncHandle>,
    session: &Session,
    scope: &SessionScope,
) -> Result<(), ApiError> {
    if control.is_some_and(SyncHandle::is_stopped) || !session.scope_is_current(scope) {
        Err(ApiError::Cancelled)
    } else {
        Ok(())
    }
}

pub(super) fn commit<T>(
    session: &Session,
    scope: &SessionScope,
    work: impl FnOnce() -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    session.commit_if_current(scope, || ApiError::Cancelled, work)
}

pub(super) fn storage_error(error: &rusqlite::Error) -> ApiError {
    ApiError::Storage(error.to_string())
}

pub(super) fn best_effort<T>(
    step: &str,
    result: Result<T, ApiError>,
) -> Result<Option<T>, ApiError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error @ (ApiError::Unauthorized | ApiError::Cancelled)) => Err(error),
        Err(error) => {
            tracing::warn!(target: "library.sync", "could not sync {step}; keeping what is stored: {error}");
            Ok(None)
        }
    }
}

pub(super) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}
