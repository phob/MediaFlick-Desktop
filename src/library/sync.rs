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

mod cycle;
mod worker;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::jellyfin::api::ApiError;

use super::{Library, LibraryChangeBatch};

pub use cycle::run_cycle;
pub use worker::spawn;

/// Base delay between incremental sweeps; jittered per cycle.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// How often the identity-only pass runs. It mirrors watch state *and* detects
/// deletions from the same pages, so both stay this fresh for one pass' cost.
pub(super) const IDENTITY_SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// How often the whole library is re-paged. This is the only thing that notices
/// an in-place metadata edit, and thin pages are cheap, so it runs daily.
pub(super) const REBOOTSTRAP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// Identity-only pages can be much larger than metadata pages.
pub(super) const IDENTITY_PAGE_SIZE: i64 = 1_000;
/// Safety valve so a server that never reaches the watermark cannot loop.
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
    pub ready: bool,
    pub processed: i64,
    pub total: Option<i64>,
    pub initial: bool,
}

/// The catalog becomes usable after its first successful page commit. Existing
/// rows are an equally strong readiness signal during a migration or weekly
/// refresh, even if an older build never wrote the explicit marker.
pub fn bootstrap_progress(library: &Library) -> BootstrapProgress {
    let has_items = library.has_items();
    let complete = library.meta(META_BOOTSTRAP_DONE).as_deref() == Some("1");
    BootstrapProgress {
        complete,
        ready: complete || has_items || library.meta(META_CATALOG_READY).as_deref() == Some("1"),
        processed: meta_count(library, META_BOOTSTRAP_OFFSET).unwrap_or(0),
        total: meta_count(library, META_BOOTSTRAP_TOTAL),
        // A migration/backfill over an existing cache is never first-time setup.
        initial: library.meta(META_LAST_BOOTSTRAP).is_none() && !has_items,
    }
}

pub fn ownership_available(library: &Library) -> bool {
    library.meta(META_BOOTSTRAP_DONE).as_deref() == Some("1")
        && library.meta(META_LATEST_FAILURE).as_deref() != Some("1")
}

fn meta_count(library: &Library, key: &str) -> Option<i64> {
    library
        .meta(key)
        .and_then(|value| value.parse::<i64>().ok())
        .map(|value| value.max(0))
}

/// What a single cycle changed; surfaced by `/api/status` and `--library-stats`.
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
    /// The timer came around.
    Scheduled,
    /// A person asked: the refresh button, a fresh sign-in, or
    /// `--library-sync-once`.
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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncPhase {
    Catalog,
    Reconciling,
    Retrying,
    #[default]
    Complete,
}

#[derive(Debug, Clone, Default)]
pub(super) struct WorkerState {
    phase: SyncPhase,
    error: Option<String>,
    retry_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub active: bool,
    pub phase: SyncPhase,
    pub catalog: BootstrapProgress,
    pub error: Option<String>,
    pub retry_at: Option<i64>,
}

/// Handle used by the shell to nudge or stop the sync thread.
#[derive(Clone)]
pub struct SyncHandle {
    signal: Arc<Signal>,
    running: Arc<AtomicBool>,
    state: Arc<Mutex<WorkerState>>,
}

impl SyncHandle {
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

    pub fn stop(&self) {
        if let Ok(mut flags) = self.signal.flags.lock() {
            flags.stopped = true;
        }
        self.signal.condvar.notify_all();
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn progress(&self, catalog: BootstrapProgress) -> SyncProgress {
        let state = self
            .state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default();
        let active = self.is_running() || !catalog.complete;
        let phase = if state.phase == SyncPhase::Retrying && active {
            SyncPhase::Retrying
        } else if self.is_running() {
            state.phase
        } else if !catalog.complete {
            SyncPhase::Catalog
        } else {
            SyncPhase::Complete
        };
        SyncProgress {
            active,
            phase,
            catalog,
            error: state.error,
            retry_at: state.retry_at,
        }
    }

    pub(super) fn set_phase(&self, phase: SyncPhase) {
        if let Ok(mut state) = self.state.lock() {
            state.phase = phase;
            state.error = None;
            state.retry_at = None;
        }
    }

    pub(super) fn set_retry(&self, error: &ApiError, delay: Duration) {
        if let Ok(mut state) = self.state.lock() {
            state.phase = SyncPhase::Retrying;
            state.error = Some(error.to_string());
            state.retry_at = Some(now_unix() as i64 + delay.as_secs() as i64);
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

/// A handle no worker listens to, so tests can exercise consumers of sync
/// nudges without spawning the sync thread.
#[cfg(test)]
pub(crate) fn detached_handle() -> SyncHandle {
    SyncHandle {
        signal: Arc::new(Signal {
            flags: Mutex::new(Flags::default()),
            condvar: Condvar::new(),
        }),
        running: Arc::new(AtomicBool::new(false)),
        state: Arc::new(Mutex::new(WorkerState::default())),
    }
}

pub(super) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    use super::{
        BootstrapProgress, Flags, META_BOOTSTRAP_DONE, META_BOOTSTRAP_OFFSET, META_BOOTSTRAP_TOTAL,
        META_LAST_BOOTSTRAP, Signal, SyncHandle, SyncPhase, SyncReport, WorkerState,
        bootstrap_progress,
    };
    use crate::library::Library;

    #[test]
    fn bootstrap_progress_is_durable_and_distinguishes_the_initial_fill() {
        let library = Library::open_in_memory().expect("library");
        assert_eq!(
            bootstrap_progress(&library),
            BootstrapProgress {
                complete: false,
                ready: false,
                processed: 0,
                total: None,
                initial: true,
            }
        );

        library
            .set_meta(META_BOOTSTRAP_OFFSET, "400")
            .expect("offset");
        library
            .set_meta(META_BOOTSTRAP_TOTAL, "1250")
            .expect("total");
        assert_eq!(bootstrap_progress(&library).processed, 400);
        assert_eq!(bootstrap_progress(&library).total, Some(1250));

        library
            .set_meta(META_BOOTSTRAP_DONE, "1")
            .expect("complete");
        library
            .set_meta(META_LAST_BOOTSTRAP, "123")
            .expect("last bootstrap");
        let completed = bootstrap_progress(&library);
        assert!(completed.complete);
        assert!(completed.ready);
        assert!(!completed.initial);
    }

    #[test]
    fn existing_cache_is_ready_even_while_a_refresh_or_migration_is_incomplete() {
        let library = Library::open_in_memory().expect("library");
        library
            .ingest_page(&[serde_json::from_str(
                r#"{"Id":"cached","Name":"Cached","Type":"Movie"}"#,
            )
            .expect("dto")])
            .expect("seed cache");
        library
            .set_meta(META_BOOTSTRAP_DONE, "0")
            .expect("refresh in progress");

        assert!(bootstrap_progress(&library).ready);
        let progress = bootstrap_progress(&library);
        assert!(progress.ready);
        assert!(!progress.complete);
        assert!(!progress.initial);
    }

    #[test]
    fn progress_reports_retry_without_hiding_the_ready_cache() {
        let library = Library::open_in_memory().expect("library");
        library
            .ingest_page(&[serde_json::from_str(
                r#"{"Id":"cached","Name":"Cached","Type":"Movie"}"#,
            )
            .expect("dto")])
            .expect("seed");
        let handle = SyncHandle {
            signal: Arc::new(Signal {
                flags: Mutex::new(Flags::default()),
                condvar: Condvar::new(),
            }),
            running: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(WorkerState::default())),
        };

        let filling = handle.progress(bootstrap_progress(&library));
        assert!(filling.active);
        assert!(filling.catalog.ready);
        assert_eq!(filling.phase, SyncPhase::Catalog);

        handle.set_retry(
            &crate::jellyfin::api::ApiError::Transport("offline".to_string()),
            Duration::from_secs(60),
        );
        let retrying = handle.progress(bootstrap_progress(&library));
        assert_eq!(retrying.phase, SyncPhase::Retrying);
        assert!(
            retrying
                .error
                .as_deref()
                .is_some_and(|error| error.contains("offline"))
        );
        assert!(retrying.retry_at.is_some());
    }

    #[test]
    fn a_report_only_counts_as_a_change_when_rows_moved() {
        assert!(!SyncReport::default().changed());
        assert!(
            !SyncReport {
                user_data_refreshed: 5,
                ..Default::default()
            }
            .changed()
        );
        assert!(
            SyncReport {
                updated: 1,
                ..Default::default()
            }
            .changed()
        );
    }
}
