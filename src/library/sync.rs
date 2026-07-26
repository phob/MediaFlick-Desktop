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

use super::{Library, UserDataRecord};

/// Base delay between incremental sweeps; jittered per cycle.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Delay used while waiting for the user to sign in.
const IDLE_INTERVAL: Duration = Duration::from_secs(30);
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
const META_BOOTSTRAP_DONE: &str = "sync.bootstrap_done";
const META_WATERMARK: &str = "sync.watermark";
const META_LAST_IDENTITY_SWEEP: &str = "sync.identity_sweep_at";
const META_LAST_BOOTSTRAP: &str = "sync.bootstrap_at";
const META_LAST_SYNC: &str = "sync.completed_at";

/// What a single cycle changed; surfaced by `/api/status` and `--library-stats`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub bootstrapped: usize,
    pub updated: usize,
    pub user_data_refreshed: usize,
    pub deleted: usize,
    pub elapsed_ms: u64,
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
    // The cycle that runs at start-up is an ordinary one. `due` already forces
    // the identity sweep whenever the app was closed for longer than the sweep
    // interval, which is every launch that could have missed a deletion.
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

        handle.running.store(true, Ordering::Relaxed);
        let outcome = run_cycle(&library, &session, trigger);
        trigger = Trigger::Scheduled;
        handle.running.store(false, Ordering::Relaxed);

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
                IDLE_INTERVAL
            }
            Err(error) => {
                tracing::warn!(target: "library.sync", "library sync cycle failed: {error}");
                backoff = (backoff * 2).min(Duration::from_secs(30 * 60));
                jittered(backoff)
            }
        };
        match wait(&handle, delay) {
            Wake::Stopped => return,
            Wake::Requested => trigger = Trigger::Requested,
            Wake::Elapsed => {}
        }
    }
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
        if due(library, META_LAST_BOOTSTRAP, REBOOTSTRAP_INTERVAL) {
            // Re-page everything. Upserts are idempotent, so this refreshes
            // metadata in place rather than churning rows.
            let _ = library.set_meta(META_BOOTSTRAP_DONE, "0");
            let _ = library.set_meta(META_BOOTSTRAP_OFFSET, "0");
        }
        report.bootstrapped = bootstrap(library, &client, &user_id)?;
        report.updated = incremental(library, &client, &user_id)?;
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
    }
    result?;

    report.elapsed_ms = started.elapsed().as_millis() as u64;
    touch(library, META_LAST_SYNC);
    let _ = library.set_meta(
        "sync.last_report",
        &serde_json::to_string(&report).unwrap_or_default(),
    );
    Ok(report)
}

/// Pages the whole library once, resuming from the stored offset after a crash
/// or a mid-sync sign-out.
fn bootstrap(library: &Library, client: &JellyfinClient, user_id: &str) -> Result<usize, ApiError> {
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

    let mut pages = 0;
    let truncated = loop {
        if pages >= MAX_BOOTSTRAP_PAGES {
            break true;
        }
        let page = items::fetch_items_page(client, user_id, offset, "DateCreated", "Ascending")?;
        if page.items.is_empty() {
            break false;
        }
        advance_watermark(&mut watermark, &page.items);
        written += library.upsert_page(&page.items).map_err(storage_error)?;
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
) -> Result<usize, ApiError> {
    let watermark = library.meta(META_WATERMARK);
    let mut offset = 0;
    let mut written = 0;
    let mut newest = watermark.clone();

    for _ in 0..MAX_INCREMENTAL_PAGES {
        let page = items::fetch_items_page(client, user_id, offset, "DateCreated", "Descending")?;
        if page.items.is_empty() {
            break;
        }
        advance_watermark(&mut newest, &page.items);

        let fresh = page
            .items
            .iter()
            .take_while(|item| is_newer(item.date_created.as_deref(), watermark.as_deref()))
            .cloned()
            .collect::<Vec<_>>();
        let reached_watermark = fresh.len() < page.items.len();
        written += library.upsert_page(&fresh).map_err(storage_error)?;

        if reached_watermark || (page.items.len() as i64) < PAGE_SIZE {
            break;
        }
        offset += page.items.len() as i64;
    }

    if let Some(newest) = newest {
        let _ = library.set_meta(META_WATERMARK, &newest);
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

fn advance_watermark(
    watermark: &mut Option<String>,
    items: &[crate::jellyfin::api::model::BaseItemDto],
) {
    for item in items {
        if is_newer(item.date_created.as_deref(), watermark.as_deref()) {
            *watermark = item.date_created.clone();
        }
    }
}

/// Jellyfin timestamps are ISO-8601 UTC, so lexicographic order is chronological.
fn is_newer(candidate: Option<&str>, watermark: Option<&str>) -> bool {
    match (candidate, watermark) {
        (Some(candidate), Some(watermark)) => candidate > watermark,
        (Some(_), None) => true,
        (None, _) => false,
    }
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
        Flags, MAX_INCREMENTAL_PAGES, PAGE_SIZE, SYNC_INTERVAL, Signal, SyncHandle, SyncReport,
        Trigger, Wake, advance_watermark, is_newer, jittered, wait,
    };
    use crate::jellyfin::api::model::BaseItemDto;
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
    fn watermark_advances_to_the_newest_timestamp_seen() {
        let mut watermark = Some("2024-01-01T00:00:00Z".to_string());
        advance_watermark(
            &mut watermark,
            &items(&["2024-03-01T00:00:00Z", "2024-02-01T00:00:00Z"]),
        );
        assert_eq!(watermark.as_deref(), Some("2024-03-01T00:00:00Z"));
    }

    #[test]
    fn watermark_starts_from_the_first_timestamp_when_unset() {
        let mut watermark = None;
        advance_watermark(&mut watermark, &items(&["2024-01-01T00:00:00Z"]));
        assert_eq!(watermark.as_deref(), Some("2024-01-01T00:00:00Z"));
    }

    #[test]
    fn watermark_ignores_items_without_a_timestamp() {
        let mut watermark = Some("2024-01-01T00:00:00Z".to_string());
        let undated: Vec<BaseItemDto> = vec![serde_json::from_str(r#"{"Id":"a"}"#).expect("dto")];
        advance_watermark(&mut watermark, &undated);
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
        advance_watermark(&mut watermark, &saved_only);
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
    fn the_incremental_sweep_covers_a_full_library_but_stays_bounded() {
        // Enough pages to walk a very large library in one cycle, few enough
        // that a server which never reaches the watermark cannot spin forever.
        assert_eq!(MAX_INCREMENTAL_PAGES as i64 * PAGE_SIZE, 20_000);
    }
}
