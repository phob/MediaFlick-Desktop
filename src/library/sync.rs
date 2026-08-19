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

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::app::ids::random_hex;
use crate::jellyfin::api::items::{self, PAGE_SIZE};
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::session::Session;

use super::{Library, LibraryChangeBatch, UserDataRecord};

/// Base delay between incremental sweeps; jittered per cycle.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Delay used while waiting for the user to sign in.
const IDLE_INTERVAL: Duration = Duration::from_secs(30);
/// How often the identity-only pass runs. It mirrors watch state *and* detects
/// deletions from the same pages, so both stay this fresh for one pass' cost.
const IDENTITY_SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// How often the whole library is re-paged. This is the only thing that notices
/// an in-place metadata edit, and thin pages are cheap, so it runs daily.
const REBOOTSTRAP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// Identity-only pages can be much larger than metadata pages.
const IDENTITY_PAGE_SIZE: i64 = 1_000;
/// Safety valve so a server that never reaches the watermark cannot loop.
const MAX_INCREMENTAL_PAGES: usize = 100;
/// The same valve for the two full passes, which end on the server's own idea
/// of where the library stops. Both caps sit far above any real library — a
/// server that reaches one is repeating pages or miscounting, not answering
/// honestly — so they only bound a runaway, never a normal cycle.
const MAX_BOOTSTRAP_PAGES: usize = 1_000;
const MAX_IDENTITY_PAGES: usize = 1_000;

const META_BOOTSTRAP_OFFSET: &str = "sync.bootstrap_offset";
const META_BOOTSTRAP_TOTAL: &str = "sync.bootstrap_total";
const META_BOOTSTRAP_DONE: &str = "sync.bootstrap_done";
const META_CATALOG_READY: &str = "sync.catalog_ready";
const META_WATERMARK: &str = "sync.watermark";
const META_WATERMARK_IDS: &str = "sync.watermark_ids";
const META_LAST_IDENTITY_SWEEP: &str = "sync.identity_sweep_at";
const META_LAST_BOOTSTRAP: &str = "sync.bootstrap_at";
const META_LAST_SYNC: &str = "sync.completed_at";

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
pub fn library_ready(library: &Library) -> bool {
    library.meta(META_CATALOG_READY).as_deref() == Some("1")
        || library.meta(META_BOOTSTRAP_DONE).as_deref() == Some("1")
        || library.stats().total > 0
}

pub fn bootstrap_progress(library: &Library) -> BootstrapProgress {
    let stats = library.stats();
    BootstrapProgress {
        complete: library.meta(META_BOOTSTRAP_DONE).as_deref() == Some("1"),
        ready: library_ready(library),
        processed: meta_count(library, META_BOOTSTRAP_OFFSET).unwrap_or(0),
        total: meta_count(library, META_BOOTSTRAP_TOTAL),
        // A migration/backfill over an existing cache is never first-time setup.
        initial: library.meta(META_LAST_BOOTSTRAP).is_none() && stats.total == 0,
    }
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
    fn forces_identity_sweep(self) -> bool {
        matches!(self, Self::Requested)
    }
}

struct Signal {
    flags: Mutex<Flags>,
    condvar: Condvar,
}

#[derive(Default)]
struct Flags {
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
struct WorkerState {
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

    pub fn progress(&self, library: &Library) -> SyncProgress {
        let catalog = bootstrap_progress(library);
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

    fn set_phase(&self, phase: SyncPhase) {
        if let Ok(mut state) = self.state.lock() {
            state.phase = phase;
            state.error = None;
            state.retry_at = None;
        }
    }

    fn set_retry(&self, error: &ApiError, delay: Duration) {
        if let Ok(mut state) = self.state.lock() {
            state.phase = SyncPhase::Retrying;
            state.error = Some(error.to_string());
            state.retry_at = Some(now_unix() as i64 + delay.as_secs() as i64);
        }
    }

    fn is_stopped(&self) -> bool {
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

pub fn spawn(library: Arc<Library>, session: Arc<Session>) -> SyncHandle {
    let handle = SyncHandle {
        signal: Arc::new(Signal {
            flags: Mutex::new(Flags::default()),
            condvar: Condvar::new(),
        }),
        running: Arc::new(AtomicBool::new(false)),
        state: Arc::new(Mutex::new(WorkerState::default())),
    };
    let worker = handle.clone();
    if let Err(error) = thread::Builder::new()
        .name("library-sync".to_string())
        .spawn(move || run(&library, &session, &worker))
    {
        tracing::warn!(target: "library.sync", "failed to start the library sync thread: {error}");
    }
    handle
}

fn run(library: &Arc<Library>, session: &Arc<Session>, handle: &SyncHandle) {
    let mut backoff = SYNC_INTERVAL;
    let mut normal_deadline = Instant::now();
    let mut trigger = Trigger::Scheduled;
    loop {
        if !session.is_authenticated() {
            handle.running.store(false, Ordering::Relaxed);
            match wait(handle, IDLE_INTERVAL) {
                Wake::Stopped => return,
                // Held until a cycle can actually consume it: this is the
                // sign-in nudge arriving just before the session goes live.
                Wake::Requested => trigger = Trigger::Requested,
                Wake::Elapsed => {}
            }
            continue;
        }

        let normal_due = trigger == Trigger::Requested || Instant::now() >= normal_deadline;
        handle.running.store(normal_due, Ordering::Relaxed);

        if normal_due {
            let outcome = run_cycle_inner(library, session, trigger, Some(handle));
            trigger = Trigger::Scheduled;
            let delay = match outcome {
                Ok(report) => {
                    backoff = SYNC_INTERVAL;
                    if report.changed() {
                        tracing::info!(
                            target: "library.sync",
                            catalogued = report.bootstrapped,
                            updated = report.updated,
                            deleted = report.deleted,
                            elapsed_ms = report.elapsed_ms,
                            "library sync cycle finished"
                        );
                    }
                    jittered(SYNC_INTERVAL)
                }
                Err(ApiError::Cancelled) if handle.is_stopped() => return,
                Err(ApiError::Unauthorized) => {
                    // `mark_expired` itself notifies the UI, exactly once.
                    session.mark_expired();
                    IDLE_INTERVAL
                }
                Err(error) => {
                    tracing::warn!(target: "library.sync", "library sync cycle failed: {error}");
                    backoff = (backoff * 2).min(Duration::from_secs(30 * 60));
                    let delay = retry_delay(&error, jittered(backoff));
                    handle.set_retry(&error, delay);
                    delay
                }
            };
            normal_deadline = Instant::now() + delay;
        }
        handle.running.store(false, Ordering::Relaxed);

        let delay = normal_deadline.saturating_duration_since(Instant::now());
        match wait(handle, delay) {
            Wake::Stopped => return,
            Wake::Requested => trigger = Trigger::Requested,
            Wake::Elapsed => {}
        }
    }
}

fn retry_delay(error: &ApiError, fallback: Duration) -> Duration {
    error.retry_after().unwrap_or(fallback).max(fallback)
}

/// Why the sync thread stopped waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wake {
    /// Someone called [`SyncHandle::request`].
    Requested,
    /// The timeout elapsed on its own.
    Elapsed,
    /// The thread should exit.
    Stopped,
}

fn wait(handle: &SyncHandle, timeout: Duration) -> Wake {
    let Ok(mut flags) = handle.signal.flags.lock() else {
        return Wake::Stopped;
    };
    if flags.stopped {
        return Wake::Stopped;
    }
    if flags.requested {
        flags.requested = false;
        return Wake::Requested;
    }
    let (mut flags, _) = handle
        .signal
        .condvar
        .wait_timeout(flags, timeout)
        .unwrap_or_else(|error| error.into_inner());
    if flags.stopped {
        return Wake::Stopped;
    }
    // A request that landed during the wait still counts as one: the condvar
    // cannot distinguish it from a plain timeout, but the flag can.
    if flags.requested {
        flags.requested = false;
        return Wake::Requested;
    }
    Wake::Elapsed
}

/// Runs one full cycle. Public so `--library-sync-once` can reuse it.
pub fn run_cycle(
    library: &Library,
    session: &Session,
    trigger: Trigger,
) -> Result<SyncReport, ApiError> {
    run_cycle_inner(library, session, trigger, None)
}

fn run_cycle_inner(
    library: &Library,
    session: &Session,
    trigger: Trigger,
    control: Option<&SyncHandle>,
) -> Result<SyncReport, ApiError> {
    let started = Instant::now();
    let (client, user_id) = session.client_and_user()?;
    let initial_catalog = library.meta(META_BOOTSTRAP_DONE).as_deref() != Some("1")
        && library.meta(META_LAST_BOOTSTRAP).is_none();
    let mut report = SyncReport::default();

    let result = (|| -> Result<(), ApiError> {
        if full_bootstrap_due(library) {
            // Re-page everything. Upserts are idempotent, so this refreshes
            // metadata in place rather than churning rows.
            let _ = library.set_meta(META_BOOTSTRAP_DONE, "0");
            let _ = library.set_meta(META_BOOTSTRAP_OFFSET, "0");
            // An empty value deliberately means "the first page has not told
            // us the new total yet"; retaining the previous week's total would
            // make the determinate bar move against stale information.
            let _ = library.set_meta(META_BOOTSTRAP_TOTAL, "");
        }
        if library.meta(META_BOOTSTRAP_DONE).as_deref() != Some("1")
            && let Some(control) = control
        {
            control.set_phase(SyncPhase::Catalog);
        }
        report.bootstrapped = bootstrap(library, &client, &user_id, control)?;
        cancelled(control)?;
        if let Some(control) = control {
            control.set_phase(SyncPhase::Reconciling);
        }
        report.updated = incremental(library, &client, &user_id, &mut report.changes, control)?;
        if initial_catalog {
            // The just-completed catalog itself is a complete identity and user
            // data observation; repeating every page immediately would double
            // first-run request load without finding a stale row.
            touch(library, META_LAST_IDENTITY_SWEEP);
        } else if trigger.forces_identity_sweep()
            || due(library, META_LAST_IDENTITY_SWEEP, IDENTITY_SWEEP_INTERVAL)
        {
            let (refreshed, deletion_changes) =
                identity_sweep(library, &client, &user_id, control)?;
            report.user_data_refreshed = refreshed;
            report.deleted = deletion_changes.item_ids.len();
            report.changes.merge(deletion_changes);
            touch(library, META_LAST_IDENTITY_SWEEP);
        }
        Ok(())
    })();

    if let Err(error) = &result {
        session.note_error(error);
        // Bootstrap/incremental pages commit independently. A later network
        // failure must not strand already-visible SQLite changes without the
        // same batched UI notification a fully successful cycle receives.
        if !report.changes.is_empty() {
            crate::app::services::notify_library_changed(report.changes.clone());
        }
    }
    result?;

    report.elapsed_ms = started.elapsed().as_millis() as u64;
    touch(library, META_LAST_SYNC);
    let _ = library.set_meta(
        "sync.last_report",
        &serde_json::to_string(&report).unwrap_or_default(),
    );
    if !report.changes.is_empty() {
        crate::app::services::notify_library_changed(report.changes.clone());
    }
    Ok(report)
}

/// Pages the whole library once, resuming from the stored offset after a crash
/// or a mid-sync sign-out.
fn bootstrap(
    library: &Library,
    client: &JellyfinClient,
    user_id: &str,
    control: Option<&SyncHandle>,
) -> Result<usize, ApiError> {
    if library.meta(META_BOOTSTRAP_DONE).as_deref() == Some("1") {
        return Ok(0);
    }

    let phase_started = Instant::now();
    let mut offset = library
        .meta(META_BOOTSTRAP_OFFSET)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    let mut written = 0;
    let mut watermark = library.meta(META_WATERMARK);
    let mut watermark_ids = read_watermark_ids(library);

    let mut pages = 0;
    let truncated = loop {
        cancelled(control)?;
        if pages >= MAX_BOOTSTRAP_PAGES {
            break true;
        }
        let page = items::fetch_items_page(client, user_id, offset, "DateCreated", "Ascending")?;
        // Jellyfin normally reports the total on every page. Preserve the last
        // useful value if a trailing empty page omits it, while still recording
        // a real zero for an empty library.
        if page.total_record_count > 0 || offset == 0 {
            let _ = library.set_meta(
                META_BOOTSTRAP_TOTAL,
                &page.total_record_count.max(0).to_string(),
            );
        }
        if page.items.is_empty() {
            // Even an empty library has a successful first catalog page.
            let _ = library.set_meta(META_CATALOG_READY, "1");
            if offset == 0 {
                tracing::info!(
                    target: "library.sync",
                    ready_ms = phase_started.elapsed().as_millis() as u64,
                    "empty catalog confirmed; library is ready"
                );
            }
            break false;
        }
        advance_watermark_with_ids(&mut watermark, &mut watermark_ids, &page.items);
        let page_changes = library
            .ingest_page(&page.items)
            .map_err(|error| storage_error(&error))?;
        // Readiness is written only after the page transaction commits.
        let _ = library.set_meta(META_CATALOG_READY, "1");
        written += page.items.len();
        offset += page.items.len() as i64;
        pages += 1;
        let _ = library.set_meta(META_BOOTSTRAP_OFFSET, &offset.to_string());
        if pages == 1 {
            tracing::info!(
                target: "library.sync",
                items = page.items.len(),
                total = page.total_record_count,
                ready_ms = phase_started.elapsed().as_millis() as u64,
                "first catalog page committed; library is ready"
            );
        }
        // One page is one SQLite transaction and one UI invalidation. This is
        // progressive without degenerating into an event per item.
        if !page_changes.is_empty() {
            crate::app::services::notify_library_changed(page_changes);
        }
        tracing::debug!(
            target: "library.sync",
            offset,
            total = page.total_record_count,
            "bootstrapped a library page"
        );
        if page.total_record_count > 0 && offset >= page.total_record_count {
            break false;
        }
        if (page.items.len() as i64) < PAGE_SIZE {
            break false;
        }
    };

    if let Some(watermark) = watermark {
        let _ = library.set_meta(META_WATERMARK, &watermark);
        write_watermark_ids(library, &watermark_ids);
    }
    if truncated {
        // The offset is stored, so the next cycle picks up where this one
        // stopped. Marking the bootstrap done here would instead declare a
        // half-paged library complete.
        tracing::warn!(
            target: "library.sync",
            pages,
            offset,
            "stopped bootstrapping at the page cap; resuming next cycle"
        );
        return Ok(written);
    }
    let _ = library.set_meta(META_BOOTSTRAP_DONE, "1");
    touch(library, META_LAST_BOOTSTRAP);
    tracing::info!(target: "library.sync", items = written, "library bootstrap complete");
    Ok(written)
}

/// Walks `DateCreated` descending until it reaches items already cached.
///
/// This is what makes a replaced file appear: Jellyfin deletes the old item and
/// adds a new one with a new id and a fresh `DateCreated`.
fn incremental(
    library: &Library,
    client: &JellyfinClient,
    user_id: &str,
    changes: &mut LibraryChangeBatch,
    control: Option<&SyncHandle>,
) -> Result<usize, ApiError> {
    let watermark = library.meta(META_WATERMARK);
    let known_watermark_ids = read_watermark_ids(library);
    let mut offset = 0;
    let mut written = 0;
    let mut newest = watermark.clone();
    let mut newest_ids = known_watermark_ids.clone();

    for _ in 0..MAX_INCREMENTAL_PAGES {
        cancelled(control)?;
        let page = items::fetch_items_page(client, user_id, offset, "DateCreated", "Descending")?;
        if page.items.is_empty() {
            break;
        }
        advance_watermark_with_ids(&mut newest, &mut newest_ids, &page.items);

        let fresh = page
            .items
            .iter()
            .filter(|item| {
                is_incremental_candidate(item, watermark.as_deref(), &known_watermark_ids)
            })
            .cloned()
            .collect::<Vec<_>>();
        let reached_watermark = page.items.iter().any(|item| {
            item.date_created
                .as_deref()
                .zip(watermark.as_deref())
                .is_some_and(|(candidate, watermark)| candidate < watermark)
                || item.date_created.is_none()
        });
        let page_changes = library
            .ingest_page(&fresh)
            .map_err(|error| storage_error(&error))?;
        written += fresh.len();
        changes.merge(page_changes);

        if reached_watermark || (page.items.len() as i64) < PAGE_SIZE {
            break;
        }
        offset += page.items.len() as i64;
    }

    if let Some(newest) = newest {
        let _ = library.set_meta(META_WATERMARK, &newest);
        write_watermark_ids(library, &newest_ids);
    }
    Ok(written)
}

/// One identity-only pass over the library that both mirrors watch state and
/// drops items the server no longer reports.
///
/// These used to be two sweeps requesting the identical pages on different
/// schedules, which meant deletions were only noticed once a day. Folding them
/// together makes deletions as fresh as watch state for no extra requests.
///
/// Returns the refreshed user-data count and a complete deletion batch so the
/// same cycle can notify item and hierarchy caches after committing removals.
fn identity_sweep(
    library: &Library,
    client: &JellyfinClient,
    user_id: &str,
    control: Option<&SyncHandle>,
) -> Result<(usize, LibraryChangeBatch), ApiError> {
    let mut offset = 0;
    let mut refreshed = 0;
    let mut seen = HashSet::new();
    let mut pages = 0;
    let truncated = loop {
        cancelled(control)?;
        if pages >= MAX_IDENTITY_PAGES {
            break true;
        }
        let page = items::fetch_identity_page(client, user_id, offset, IDENTITY_PAGE_SIZE)?;
        if page.items.is_empty() {
            break false;
        }
        seen.extend(page.items.iter().map(|item| item.id.clone()));
        let records = page
            .items
            .iter()
            .filter_map(|item| {
                item.user_data
                    .as_ref()
                    .map(|user_data| UserDataRecord::from_dto(&item.id, user_data))
            })
            .collect::<Vec<_>>();
        refreshed += library
            .upsert_user_data(&records)
            .map_err(|error| storage_error(&error))?;
        offset += page.items.len() as i64;
        pages += 1;
        if (page.items.len() as i64) < IDENTITY_PAGE_SIZE {
            break false;
        }
    };
    if truncated {
        // `seen` is what decides which items still exist, so a partial one must
        // never reach `retain_ids` — everything past the cap would be deleted.
        tracing::warn!(
            target: "library.sync",
            pages,
            offset,
            "stopped the identity sweep at the page cap; skipping the deletion pass"
        );
        return Ok((refreshed, LibraryChangeBatch::default()));
    }
    if seen.is_empty() {
        // An empty answer is far more likely to be a server hiccup than an
        // emptied library, so never treat it as "delete everything".
        return Ok((refreshed, LibraryChangeBatch::default()));
    }
    let deletion_changes = library
        .retain_ids(&seen)
        .map_err(|error| storage_error(&error))?;
    Ok((refreshed, deletion_changes))
}

fn advance_watermark_with_ids(
    watermark: &mut Option<String>,
    ids: &mut HashSet<String>,
    items: &[crate::jellyfin::api::model::BaseItemDto],
) {
    for item in items {
        match (item.date_created.as_deref(), watermark.as_deref()) {
            (Some(candidate), Some(current)) if candidate > current => {
                *watermark = Some(candidate.to_string());
                ids.clear();
                ids.insert(item.id.clone());
            }
            (Some(candidate), Some(current)) if candidate == current => {
                ids.insert(item.id.clone());
            }
            (Some(candidate), None) => {
                *watermark = Some(candidate.to_string());
                ids.clear();
                ids.insert(item.id.clone());
            }
            _ => {}
        }
    }
}

fn read_watermark_ids(library: &Library) -> HashSet<String> {
    library
        .meta(META_WATERMARK_IDS)
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn write_watermark_ids(library: &Library, ids: &HashSet<String>) {
    let mut ids = ids.iter().cloned().collect::<Vec<_>>();
    ids.sort_unstable();
    let _ = library.set_meta(
        META_WATERMARK_IDS,
        &serde_json::to_string(&ids).unwrap_or_default(),
    );
}

/// Jellyfin timestamps are ISO-8601 UTC, so lexicographic order is chronological.
fn is_newer(candidate: Option<&str>, watermark: Option<&str>) -> bool {
    match (candidate, watermark) {
        (Some(candidate), Some(watermark)) => candidate > watermark,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn is_incremental_candidate(
    item: &crate::jellyfin::api::model::BaseItemDto,
    watermark: Option<&str>,
    known_watermark_ids: &HashSet<String>,
) -> bool {
    is_newer(item.date_created.as_deref(), watermark)
        || (item.date_created.as_deref() == watermark && !known_watermark_ids.contains(&item.id))
}

fn due(library: &Library, key: &str, interval: Duration) -> bool {
    let Some(last) = library
        .meta(key)
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return true;
    };
    now_unix().saturating_sub(last) >= interval.as_secs()
}

fn full_bootstrap_due(library: &Library) -> bool {
    // A missing/false done marker means the resumable initial pass is already
    // underway. Reset only a previously completed cache whose weekly refresh
    // is due; otherwise a retry after a network failure would jump back to
    // zero and make both the stored offset and the progress UI dishonest.
    library.meta(META_BOOTSTRAP_DONE).as_deref() == Some("1")
        && due(library, META_LAST_BOOTSTRAP, REBOOTSTRAP_INTERVAL)
}

fn touch(library: &Library, key: &str) {
    let _ = library.set_meta(key, &now_unix().to_string());
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Storage failures are reported through the same channel as API failures so a
/// cycle stops on the first problem instead of half-applying a page.
fn cancelled(control: Option<&SyncHandle>) -> Result<(), ApiError> {
    if control.is_some_and(SyncHandle::is_stopped) {
        Err(ApiError::Cancelled)
    } else {
        Ok(())
    }
}

fn storage_error(error: &rusqlite::Error) -> ApiError {
    ApiError::Decode(format!("library storage failed: {error}"))
}

/// Spreads restarts across clients so a server is not hit by a thundering herd.
fn jittered(base: Duration) -> Duration {
    let entropy = u64::from_str_radix(&random_hex(2), 16).unwrap_or(0);
    let spread = base.as_secs().max(1) / 5;
    base + Duration::from_secs(entropy % spread.max(1))
}

#[cfg(test)]
mod tests {
    use super::{
        Flags, MAX_INCREMENTAL_PAGES, META_BOOTSTRAP_DONE, META_BOOTSTRAP_OFFSET,
        META_BOOTSTRAP_TOTAL, META_LAST_BOOTSTRAP, META_LAST_IDENTITY_SWEEP, PAGE_SIZE,
        SYNC_INTERVAL, Signal, SyncHandle, SyncReport, Trigger, Wake, WorkerState,
        advance_watermark_with_ids, bootstrap_progress, full_bootstrap_due,
        is_incremental_candidate, is_newer, jittered, library_ready, wait,
    };
    use crate::jellyfin::api::model::BaseItemDto;
    use crate::jellyfin::session::Session;
    use crate::library::{Library, StoredCredentials};
    use std::collections::HashSet;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Condvar, Mutex, mpsc};
    use std::thread;
    use std::time::Duration;

    fn items(dates: &[&str]) -> Vec<BaseItemDto> {
        dates
            .iter()
            .enumerate()
            .map(|(index, date)| {
                serde_json::from_str(&format!(r#"{{"Id":"item{index}","DateCreated":"{date}"}}"#))
                    .expect("dto")
            })
            .collect()
    }

    fn receive_target(listener: &TcpListener) -> (TcpStream, String) {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 2_048];
        loop {
            let read = stream.read(&mut buffer).expect("read request");
            assert!(read > 0, "connection closed before headers");
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let target = String::from_utf8(request)
            .expect("utf8 request")
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .expect("request target")
            .to_string();
        (stream, target)
    }

    fn send_json(mut stream: TcpStream, body: &str) {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("write response");
    }

    fn authenticated_session(library: &Arc<Library>, server_url: &str) -> Session {
        let previous = library.credentials();
        library
            .save_credentials(&StoredCredentials {
                server_url: Some(server_url.to_string()),
                user_id: Some("user-1".to_string()),
                user_name: Some("Test User".to_string()),
                server_id: Some("server-1".to_string()),
                device_id: previous.device_id,
                token: Some("token-1".to_string()),
            })
            .expect("credentials");
        Session::restore(library.clone())
    }

    #[test]
    fn bootstrap_progress_is_durable_and_distinguishes_the_initial_fill() {
        let library = Library::open_in_memory().expect("library");
        assert_eq!(
            bootstrap_progress(&library),
            super::BootstrapProgress {
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
    fn first_catalog_page_is_ready_while_background_paging_continues() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        library
            .set_meta(META_LAST_IDENTITY_SWEEP, &super::now_unix().to_string())
            .expect("skip unrelated identity sweep");

        let first_items = (0..PAGE_SIZE)
            .map(|index| {
                serde_json::json!({
                    "Id": format!("item-{index:03}"),
                    "Name": format!("Item {index}"),
                    "Type": "Movie",
                    "DateCreated": format!("2024-01-01T00:{:02}:00Z", index % 60),
                    "ImageTags": { "Primary": format!("poster-{index}") },
                    "UserData": { "Played": false, "IsFavorite": index == 0 },
                })
            })
            .collect::<Vec<_>>();
        let first_body = serde_json::json!({
            "Items": first_items,
            "TotalRecordCount": PAGE_SIZE + 1,
            "StartIndex": 0,
        })
        .to_string();
        let second_body = serde_json::json!({
            "Items": [{
                "Id": "item-last",
                "Name": "Last item",
                "Type": "Movie",
                "DateCreated": "2024-02-01T00:00:00Z"
            }],
            "TotalRecordCount": PAGE_SIZE + 1,
            "StartIndex": PAGE_SIZE,
        })
        .to_string();
        let empty_body = r#"{"Items":[],"TotalRecordCount":201}"#.to_string();

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let (second_started_tx, second_started_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut targets = Vec::new();
            let (stream, target) = receive_target(&listener);
            targets.push(target);
            send_json(stream, &first_body);

            let (stream, target) = receive_target(&listener);
            targets.push(target);
            second_started_tx.send(()).expect("signal second page");
            continue_rx.recv().expect("release second page");
            send_json(stream, &second_body);

            let (stream, target) = receive_target(&listener);
            targets.push(target);
            send_json(stream, &empty_body);
            targets
        });
        let session = authenticated_session(&library, &format!("http://{address}"));
        let worker_library = library.clone();
        let worker =
            thread::spawn(move || super::run_cycle(&worker_library, &session, Trigger::Scheduled));

        second_started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("worker continued to page two");
        let first_page = bootstrap_progress(&library);
        assert!(first_page.ready);
        assert!(!first_page.complete);
        assert_eq!(first_page.processed, PAGE_SIZE);
        assert_eq!(first_page.total, Some(PAGE_SIZE + 1));
        assert_eq!(library.stats().total, PAGE_SIZE);

        continue_tx.send(()).expect("continue catalog");
        let report = worker.join().expect("worker").expect("sync cycle");
        assert_eq!(report.bootstrapped as i64, PAGE_SIZE + 1);
        assert!(bootstrap_progress(&library).complete);
        assert_eq!(library.stats().total, PAGE_SIZE + 1);

        let targets = server.join().expect("server");
        assert_eq!(targets.len(), 3);
        assert!(targets[0].contains("StartIndex=0"));
        assert!(targets[1].contains(&format!("StartIndex={PAGE_SIZE}")));
        assert!(targets[2].contains("SortOrder=Descending"));
        assert!(targets[..2].iter().all(|target| !target.contains("People")));
        assert!(
            targets[..2]
                .iter()
                .all(|target| !target.contains("MediaStreams"))
        );
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

        assert!(library_ready(&library));
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

        let filling = handle.progress(&library);
        assert!(filling.active);
        assert!(filling.catalog.ready);
        assert_eq!(filling.phase, super::SyncPhase::Catalog);

        handle.set_retry(
            &crate::jellyfin::api::ApiError::Transport("offline".to_string()),
            Duration::from_secs(60),
        );
        let retrying = handle.progress(&library);
        assert_eq!(retrying.phase, super::SyncPhase::Retrying);
        assert!(
            retrying
                .error
                .as_deref()
                .is_some_and(|error| error.contains("offline"))
        );
        assert!(retrying.retry_at.is_some());
    }

    #[test]
    fn an_incomplete_bootstrap_resumes_instead_of_resetting_its_progress() {
        let library = Library::open_in_memory().expect("library");
        library
            .set_meta(META_BOOTSTRAP_OFFSET, "400")
            .expect("offset");
        library
            .set_meta(META_BOOTSTRAP_TOTAL, "1250")
            .expect("total");
        library
            .set_meta(META_BOOTSTRAP_DONE, "0")
            .expect("incomplete");
        assert!(!full_bootstrap_due(&library));

        // Only an old pass which actually completed is eligible for the
        // periodic reset.
        library
            .set_meta(META_BOOTSTRAP_DONE, "1")
            .expect("complete");
        library
            .set_meta(META_LAST_BOOTSTRAP, "0")
            .expect("old bootstrap");
        assert!(full_bootstrap_due(&library));
    }

    #[test]
    fn watermark_advances_to_the_newest_timestamp_seen() {
        let mut watermark = Some("2024-01-01T00:00:00Z".to_string());
        let mut ids = HashSet::new();
        advance_watermark_with_ids(
            &mut watermark,
            &mut ids,
            &items(&["2024-03-01T00:00:00Z", "2024-02-01T00:00:00Z"]),
        );
        assert_eq!(watermark.as_deref(), Some("2024-03-01T00:00:00Z"));
        assert_eq!(ids, HashSet::from(["item0".to_string()]));
    }

    #[test]
    fn watermark_starts_from_the_first_timestamp_when_unset() {
        let mut watermark = None;
        advance_watermark_with_ids(
            &mut watermark,
            &mut HashSet::new(),
            &items(&["2024-01-01T00:00:00Z"]),
        );
        assert_eq!(watermark.as_deref(), Some("2024-01-01T00:00:00Z"));
    }

    #[test]
    fn watermark_ignores_items_without_a_timestamp() {
        let mut watermark = Some("2024-01-01T00:00:00Z".to_string());
        let undated: Vec<BaseItemDto> = vec![serde_json::from_str(r#"{"Id":"a"}"#).expect("dto")];
        advance_watermark_with_ids(&mut watermark, &mut HashSet::new(), &undated);
        assert_eq!(watermark.as_deref(), Some("2024-01-01T00:00:00Z"));
    }

    /// Regression: the sweep used to key on `DateLastSaved`, which servers
    /// return empty. Every item then looked "not newer", so `take_while` cut
    /// the first page to nothing and the cache silently stopped updating.
    #[test]
    fn watermark_ignores_date_last_saved() {
        let mut watermark = None;
        let saved_only: Vec<BaseItemDto> = vec![
            serde_json::from_str(r#"{"Id":"a","DateLastSaved":"2024-05-01T00:00:00Z"}"#)
                .expect("dto"),
        ];
        advance_watermark_with_ids(&mut watermark, &mut HashSet::new(), &saved_only);
        assert_eq!(watermark, None);
    }

    #[test]
    fn the_watermark_cut_off_is_strict() {
        assert!(is_newer(Some("2024-02-01"), Some("2024-01-01")));
        assert!(!is_newer(Some("2024-01-01"), Some("2024-01-01")));
        assert!(!is_newer(Some("2023-12-31"), Some("2024-01-01")));
        assert!(is_newer(Some("2024-01-01"), None));
        assert!(!is_newer(None, Some("2024-01-01")));
    }

    #[test]
    fn an_unseen_id_tied_at_the_watermark_is_not_skipped() {
        let tied = items(&["2024-01-01"]).remove(0);
        assert!(is_incremental_candidate(
            &tied,
            Some("2024-01-01"),
            &HashSet::new()
        ));
        assert!(!is_incremental_candidate(
            &tied,
            Some("2024-01-01"),
            &HashSet::from([tied.id.clone()])
        ));
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

    #[test]
    fn jitter_only_ever_delays_the_next_cycle() {
        for _ in 0..20 {
            let delay = jittered(SYNC_INTERVAL);
            assert!(delay >= SYNC_INTERVAL);
            assert!(delay < SYNC_INTERVAL + SYNC_INTERVAL / 5 + std::time::Duration::from_secs(1));
        }
    }

    /// Only an explicit ask may skip the identity sweep's hourly gate; letting
    /// the timer skip it would re-page the whole library every ten minutes.
    #[test]
    fn only_a_requested_cycle_forces_the_identity_sweep() {
        assert!(Trigger::Requested.forces_identity_sweep());
        assert!(!Trigger::Scheduled.forces_identity_sweep());
    }

    /// The refresh button is only a "reconcile now" lever if the request
    /// survives the wait it interrupts — a request that reads back as a plain
    /// timeout would silently fall back to the hourly gate.
    #[test]
    fn a_request_is_distinguishable_from_a_timeout_and_is_consumed_once() {
        let handle = SyncHandle {
            signal: Arc::new(Signal {
                flags: Mutex::new(Flags::default()),
                condvar: Condvar::new(),
            }),
            running: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(WorkerState::default())),
        };

        handle.request();
        assert_eq!(wait(&handle, Duration::ZERO), Wake::Requested);
        // Consumed: the next wait is an ordinary scheduled one.
        assert_eq!(wait(&handle, Duration::from_millis(1)), Wake::Elapsed);

        handle.stop();
        assert_eq!(wait(&handle, Duration::ZERO), Wake::Stopped);
    }

    #[test]
    fn the_incremental_sweep_covers_a_full_library_but_stays_bounded() {
        // Enough pages to walk a very large library in one cycle, few enough
        // that a server which never reaches the watermark cannot spin forever.
        assert_eq!(MAX_INCREMENTAL_PAGES as i64 * PAGE_SIZE, 20_000);
    }
}
