//! Background synchronisation of the metadata cache.
//!
//! One thread owns the whole cycle: a resumable bootstrap, an incremental
//! `DateLastSaved` sweep, a periodic user-data mirror, and a daily deletion
//! sweep. It idles until credentials exist and pauses when the server rejects
//! the token.

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
const USER_DATA_SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);
const DELETION_SWEEP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// Identity-only pages can be much larger than metadata pages.
const IDENTITY_PAGE_SIZE: i64 = 1_000;
/// Safety valve so a server that never reaches the watermark cannot loop.
const MAX_INCREMENTAL_PAGES: usize = 100;

const META_BOOTSTRAP_OFFSET: &str = "sync.bootstrap_offset";
const META_BOOTSTRAP_DONE: &str = "sync.bootstrap_done";
const META_WATERMARK: &str = "sync.watermark";
const META_LAST_USER_DATA_SWEEP: &str = "sync.user_data_sweep_at";
const META_LAST_DELETION_SWEEP: &str = "sync.deletion_sweep_at";
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
    /// Asks for a cycle as soon as possible (login, window focus, manual refresh).
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
    loop {
        if !session.is_authenticated() {
            if !wait(&handle, IDLE_INTERVAL) {
                return;
            }
            continue;
        }

        handle.running.store(true, Ordering::Relaxed);
        let outcome = run_cycle(&library, &session);
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
        if !wait(&handle, delay) {
            return;
        }
    }
}

/// Returns false when the thread should exit.
fn wait(handle: &SyncHandle, timeout: Duration) -> bool {
    let Ok(mut flags) = handle.signal.flags.lock() else {
        return false;
    };
    if flags.stopped {
        return false;
    }
    if flags.requested {
        flags.requested = false;
        return true;
    }
    let (mut flags, _) = handle
        .signal
        .condvar
        .wait_timeout(flags, timeout)
        .unwrap_or_else(|error| error.into_inner());
    if flags.stopped {
        return false;
    }
    flags.requested = false;
    true
}

/// Runs one full cycle. Public so `--library-sync-once` can reuse it.
pub fn run_cycle(library: &Library, session: &Session) -> Result<SyncReport, ApiError> {
    let started = Instant::now();
    let (client, user_id) = session.client_and_user()?;
    let mut report = SyncReport::default();

    let result = (|| -> Result<(), ApiError> {
        report.bootstrapped = bootstrap(library, &client, &user_id)?;
        report.updated = incremental(library, &client, &user_id)?;
        if due(library, META_LAST_USER_DATA_SWEEP, USER_DATA_SWEEP_INTERVAL) {
            report.user_data_refreshed = user_data_sweep(library, &client, &user_id)?;
            touch(library, META_LAST_USER_DATA_SWEEP);
        }
        if due(library, META_LAST_DELETION_SWEEP, DELETION_SWEEP_INTERVAL) {
            report.deleted = deletion_sweep(library, &client, &user_id)?;
            touch(library, META_LAST_DELETION_SWEEP);
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

    loop {
        let page = items::fetch_items_page(client, user_id, offset, "DateCreated", "Ascending")?;
        if page.items.is_empty() {
            break;
        }
        advance_watermark(&mut watermark, &page.items);
        written += library.upsert_page(&page.items).map_err(storage_error)?;
        offset += page.items.len() as i64;
        let _ = library.set_meta(META_BOOTSTRAP_OFFSET, &offset.to_string());
        tracing::debug!(
            target: "library.sync",
            offset,
            total = page.total_record_count,
            "bootstrapped a library page"
        );
        if page.total_record_count > 0 && offset >= page.total_record_count {
            break;
        }
        if (page.items.len() as i64) < PAGE_SIZE {
            break;
        }
    }

    if let Some(watermark) = watermark {
        let _ = library.set_meta(META_WATERMARK, &watermark);
    }
    let _ = library.set_meta(META_BOOTSTRAP_DONE, "1");
    tracing::info!(target: "library.sync", items = written, "library bootstrap complete");
    Ok(written)
}

/// Walks `DateLastSaved` descending until it reaches items already cached.
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
        let page = items::fetch_items_page(client, user_id, offset, "DateLastSaved", "Descending")?;
        if page.items.is_empty() {
            break;
        }
        advance_watermark(&mut newest, &page.items);

        let fresh = page
            .items
            .iter()
            .take_while(|item| is_newer(item.date_last_saved.as_deref(), watermark.as_deref()))
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

/// Cheap sweep that catches watch-state changes made on other devices.
fn user_data_sweep(
    library: &Library,
    client: &JellyfinClient,
    user_id: &str,
) -> Result<usize, ApiError> {
    let mut offset = 0;
    let mut refreshed = 0;
    loop {
        let page = items::fetch_identity_page(client, user_id, offset, IDENTITY_PAGE_SIZE)?;
        if page.items.is_empty() {
            break;
        }
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
        if (page.items.len() as i64) < IDENTITY_PAGE_SIZE {
            break;
        }
    }
    Ok(refreshed)
}

/// Diffs the server's full id list against the cache and drops the leftovers.
fn deletion_sweep(
    library: &Library,
    client: &JellyfinClient,
    user_id: &str,
) -> Result<usize, ApiError> {
    let mut offset = 0;
    let mut seen = HashSet::new();
    loop {
        let page = items::fetch_identity_page(client, user_id, offset, IDENTITY_PAGE_SIZE)?;
        if page.items.is_empty() {
            break;
        }
        seen.extend(page.items.iter().map(|item| item.id.clone()));
        offset += page.items.len() as i64;
        if (page.items.len() as i64) < IDENTITY_PAGE_SIZE {
            break;
        }
    }
    if seen.is_empty() {
        // An empty answer is far more likely to be a server hiccup than an
        // emptied library, so never treat it as "delete everything".
        return Ok(0);
    }
    library.retain_ids(&seen).map_err(storage_error)
}

fn advance_watermark(
    watermark: &mut Option<String>,
    items: &[crate::jellyfin::api::model::BaseItemDto],
) {
    for item in items {
        if is_newer(item.date_last_saved.as_deref(), watermark.as_deref()) {
            *watermark = item.date_last_saved.clone();
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
        MAX_INCREMENTAL_PAGES, PAGE_SIZE, SYNC_INTERVAL, SyncReport, advance_watermark, is_newer,
        jittered,
    };
    use crate::jellyfin::api::model::BaseItemDto;

    fn items(dates: &[&str]) -> Vec<BaseItemDto> {
        dates
            .iter()
            .enumerate()
            .map(|(index, date)| {
                serde_json::from_str(&format!(
                    r#"{{"Id":"item{index}","DateLastSaved":"{date}"}}"#
                ))
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

    #[test]
    fn the_incremental_sweep_covers_a_full_library_but_stays_bounded() {
        // Enough pages to walk a very large library in one cycle, few enough
        // that a server which never reaches the watermark cannot spin forever.
        assert_eq!(MAX_INCREMENTAL_PAGES as i64 * PAGE_SIZE, 20_000);
    }
}
