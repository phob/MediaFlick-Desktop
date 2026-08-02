//! Background synchronisation of the metadata cache.
//!
//! One thread owns the whole cycle: a resumable bootstrap that re-runs
//! periodically, an incremental `DateCreated` sweep, a periodic user-data
//! mirror, and a daily deletion sweep. It idles until credentials exist and
//! pauses when the server rejects the token.
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
/// A storage failure can leave an already-due queue row unchanged. Without a
/// local floor the worker computes a zero wait from that row and spins.
const CONVERGENCE_FAILURE_COOLDOWN: Duration = Duration::from_secs(5 * 60);
/// How often the identity-only pass runs. It mirrors watch state *and* detects
/// deletions from the same pages, so both stay this fresh for one pass' cost.
const IDENTITY_SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// How often the whole library is re-paged. This is the only thing that notices
/// an in-place metadata edit, so it trades bandwidth for eventual correctness.
const REBOOTSTRAP_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
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
    pub processed: i64,
    pub total: Option<i64>,
    pub initial: bool,
}

pub fn bootstrap_progress(library: &Library) -> BootstrapProgress {
    BootstrapProgress {
        complete: library.meta(META_BOOTSTRAP_DONE).as_deref() == Some("1"),
        processed: meta_count(library, META_BOOTSTRAP_OFFSET).unwrap_or(0),
        total: meta_count(library, META_BOOTSTRAP_TOTAL),
        initial: library.meta(META_LAST_BOOTSTRAP).is_none(),
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

/// Handle used by the shell to nudge or stop the sync thread.
#[derive(Clone)]
pub struct SyncHandle {
    signal: Arc<Signal>,
    running: Arc<AtomicBool>,
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
}

pub fn spawn(library: Arc<Library>, session: Arc<Session>) -> SyncHandle {
    let handle = SyncHandle {
        signal: Arc::new(Signal {
            flags: Mutex::new(Flags::default()),
            condvar: Condvar::new(),
        }),
        running: Arc::new(AtomicBool::new(false)),
    };
    let worker = handle.clone();
    if let Err(error) = thread::Builder::new()
        .name("library-sync".to_string())
        .spawn(move || run(library, session, worker))
    {
        tracing::warn!(target: "library.sync", "failed to start the library sync thread: {error}");
    }
    handle
}

fn run(library: Arc<Library>, session: Arc<Session>, handle: SyncHandle) {
    let mut backoff = SYNC_INTERVAL;
    let mut normal_deadline = Instant::now();
    let mut convergence_not_before = Instant::now();
    let mut trigger = Trigger::Scheduled;
    loop {
        if !session.is_authenticated() {
            match wait(&handle, IDLE_INTERVAL) {
                Wake::Stopped => return,
                // Held until a cycle can actually consume it: this is the
                // sign-in nudge arriving just before the session goes live.
                Wake::Requested => trigger = Trigger::Requested,
                Wake::Elapsed => {}
            }
            continue;
        }

        let normal_due = trigger == Trigger::Requested || Instant::now() >= normal_deadline;
        let convergence_due = library.meta(META_BOOTSTRAP_DONE).as_deref() == Some("1")
            && Instant::now() >= convergence_not_before
            && library.convergence_diagnostics().due > 0;
        handle
            .running
            .store(normal_due || convergence_due, Ordering::Relaxed);

        if normal_due {
            let outcome = run_cycle(&library, &session, trigger);
            trigger = Trigger::Scheduled;
            let delay = match outcome {
                Ok(report) => {
                    backoff = SYNC_INTERVAL;
                    if report.changed() {
                        tracing::info!(
                            target: "library.sync",
                            bootstrapped = report.bootstrapped,
                            updated = report.updated,
                            deleted = report.deleted,
                            elapsed_ms = report.elapsed_ms,
                            "library sync cycle finished"
                        );
                    }
                    jittered(SYNC_INTERVAL)
                }
                Err(ApiError::Unauthorized) => {
                    session.mark_expired();
                    crate::app::services::notify_session_expired();
                    IDLE_INTERVAL
                }
                Err(error) => {
                    tracing::warn!(target: "library.sync", "library sync cycle failed: {error}");
                    backoff = (backoff * 2).min(Duration::from_secs(30 * 60));
                    jittered(backoff)
                }
            };
            normal_deadline = Instant::now() + delay;
        }

        if session.is_authenticated()
            && library.meta(META_BOOTSTRAP_DONE).as_deref() == Some("1")
            && Instant::now() >= convergence_not_before
            && library.convergence_diagnostics().due > 0
        {
            match super::convergence::process_due(&library, &session) {
                Ok(report) => {
                    if !report.changes.is_empty() {
                        crate::app::services::notify_library_changed(report.changes);
                    }
                    if !session.is_authenticated() {
                        crate::app::services::notify_session_expired();
                    }
                    convergence_not_before = if report.more_due {
                        Instant::now()
                            + Duration::from_secs(super::convergence::DRAIN_COOLDOWN_SECS)
                    } else {
                        Instant::now()
                    };
                }
                Err(ApiError::Unauthorized) => {
                    session.mark_expired();
                    crate::app::services::notify_session_expired();
                }
                Err(error) => {
                    tracing::warn!(
                        target: "library.convergence",
                        "could not process metadata convergence: {error}"
                    );
                    convergence_not_before = convergence_failure_deadline(Instant::now());
                }
            }
        }
        handle.running.store(false, Ordering::Relaxed);

        let diagnostics = library.convergence_diagnostics();
        let convergence_delay = if library.meta(META_BOOTSTRAP_DONE).as_deref() == Some("1") {
            diagnostics.next_due_at.map_or(SYNC_INTERVAL, |next| {
                let seconds = next.saturating_sub(now_unix() as i64).max(0) as u64;
                Duration::from_secs(seconds)
            })
        } else {
            SYNC_INTERVAL
        };
        let convergence_delay =
            convergence_delay.max(convergence_not_before.saturating_duration_since(Instant::now()));
        let delay = normal_deadline
            .saturating_duration_since(Instant::now())
            .min(convergence_delay);
        match wait(&handle, delay) {
            Wake::Stopped => return,
            Wake::Requested => trigger = Trigger::Requested,
            Wake::Elapsed => {}
        }
    }
}

fn convergence_failure_deadline(now: Instant) -> Instant {
    now + CONVERGENCE_FAILURE_COOLDOWN
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
    let started = Instant::now();
    let (client, user_id) = session.client_and_user()?;
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
        report.bootstrapped = bootstrap(library, &client, &user_id, &mut report.changes)?;
        report.updated = incremental(library, &client, &user_id, &mut report.changes)?;
        if trigger.forces_identity_sweep()
            || due(library, META_LAST_IDENTITY_SWEEP, IDENTITY_SWEEP_INTERVAL)
        {
            let (refreshed, deleted) = identity_sweep(library, &client, &user_id)?;
            report.user_data_refreshed = refreshed;
            report.deleted = deleted;
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
    changes: &mut LibraryChangeBatch,
) -> Result<usize, ApiError> {
    if library.meta(META_BOOTSTRAP_DONE).as_deref() == Some("1") {
        return Ok(0);
    }

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
            break false;
        }
        advance_watermark_with_ids(&mut watermark, &mut watermark_ids, &page.items);
        let page_changes = library.ingest_page(&page.items).map_err(storage_error)?;
        written += page.items.len();
        // The caller emits one event after the complete ordinary cycle.
        changes.merge(page_changes);
        offset += page.items.len() as i64;
        pages += 1;
        let _ = library.set_meta(META_BOOTSTRAP_OFFSET, &offset.to_string());
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
) -> Result<usize, ApiError> {
    let watermark = library.meta(META_WATERMARK);
    let known_watermark_ids = read_watermark_ids(library);
    let mut offset = 0;
    let mut written = 0;
    let mut newest = watermark.clone();
    let mut newest_ids = known_watermark_ids.clone();

    for _ in 0..MAX_INCREMENTAL_PAGES {
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
        let page_changes = library.ingest_page(&fresh).map_err(storage_error)?;
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
/// Returns `(user data rows refreshed, items deleted)`.
fn identity_sweep(
    library: &Library,
    client: &JellyfinClient,
    user_id: &str,
) -> Result<(usize, usize), ApiError> {
    let mut offset = 0;
    let mut refreshed = 0;
    let mut seen = HashSet::new();
    let mut pages = 0;
    let truncated = loop {
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
        refreshed += library.upsert_user_data(&records).map_err(storage_error)?;
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
        return Ok((refreshed, 0));
    }
    if seen.is_empty() {
        // An empty answer is far more likely to be a server hiccup than an
        // emptied library, so never treat it as "delete everything".
        return Ok((refreshed, 0));
    }
    let deleted = library.retain_ids(&seen).map_err(storage_error)?;
    Ok((refreshed, deleted))
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
fn storage_error(error: rusqlite::Error) -> ApiError {
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
        CONVERGENCE_FAILURE_COOLDOWN, Flags, MAX_INCREMENTAL_PAGES, META_BOOTSTRAP_DONE,
        META_BOOTSTRAP_OFFSET, META_BOOTSTRAP_TOTAL, META_LAST_BOOTSTRAP, PAGE_SIZE, SYNC_INTERVAL,
        Signal, SyncHandle, SyncReport, Trigger, Wake, advance_watermark_with_ids,
        bootstrap_progress, convergence_failure_deadline, full_bootstrap_due,
        is_incremental_candidate, is_newer, jittered, wait,
    };
    use crate::jellyfin::api::model::BaseItemDto;
    use crate::library::Library;
    use std::collections::HashSet;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Condvar, Mutex};
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

    #[test]
    fn bootstrap_progress_is_durable_and_distinguishes_the_initial_fill() {
        let library = Library::open_in_memory().expect("library");
        assert_eq!(
            bootstrap_progress(&library),
            super::BootstrapProgress {
                complete: false,
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
        assert!(!completed.initial);
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
        };

        handle.request();
        assert_eq!(wait(&handle, Duration::ZERO), Wake::Requested);
        // Consumed: the next wait is an ordinary scheduled one.
        assert_eq!(wait(&handle, Duration::from_millis(1)), Wake::Elapsed);

        handle.stop();
        assert_eq!(wait(&handle, Duration::ZERO), Wake::Stopped);
    }

    #[test]
    fn convergence_failures_have_a_nonzero_local_retry_floor() {
        let now = std::time::Instant::now();
        assert_eq!(
            convergence_failure_deadline(now).duration_since(now),
            CONVERGENCE_FAILURE_COOLDOWN
        );
        assert!(!CONVERGENCE_FAILURE_COOLDOWN.is_zero());
    }

    #[test]
    fn the_incremental_sweep_covers_a_full_library_but_stays_bounded() {
        // Enough pages to walk a very large library in one cycle, few enough
        // that a server which never reaches the watermark cannot spin forever.
        assert_eq!(MAX_INCREMENTAL_PAGES as i64 * PAGE_SIZE, 20_000);
    }
}
