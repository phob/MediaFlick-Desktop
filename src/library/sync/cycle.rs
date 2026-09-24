use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::jellyfin::api::items::{self, PAGE_SIZE};
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::session::{Session, SessionScope};
use crate::library::{Library, LibraryChangeBatch, UserDataRecord};

use super::{
    IDENTITY_PAGE_SIZE, IDENTITY_SWEEP_INTERVAL, MAX_BOOTSTRAP_PAGES, MAX_IDENTITY_PAGES,
    MAX_INCREMENTAL_PAGES, META_BOOTSTRAP_DONE, META_BOOTSTRAP_OFFSET, META_BOOTSTRAP_TOTAL,
    META_CATALOG_READY, META_LAST_BOOTSTRAP, META_LAST_IDENTITY_SWEEP, META_LAST_SYNC,
    META_LATEST_FAILURE, META_WATERMARK, META_WATERMARK_IDS, REBOOTSTRAP_INTERVAL, SyncHandle,
    SyncPhase, SyncReport, Trigger, now_unix,
};

/// Runs one full cycle. Public so `--library-sync-once` can reuse it.
pub fn run_cycle(
    library: &Library,
    session: &Session,
    trigger: Trigger,
) -> Result<SyncReport, ApiError> {
    run_cycle_inner(library, session, trigger, None)
}

pub(super) fn run_cycle_inner(
    library: &Library,
    session: &Session,
    trigger: Trigger,
    control: Option<&SyncHandle>,
) -> Result<SyncReport, ApiError> {
    let started = Instant::now();
    let scope = session.scope()?;
    let client = scope.client();
    let user_id = scope.user_id();
    let initial_catalog = meta(library, META_BOOTSTRAP_DONE)?.as_deref() != Some("1")
        && meta(library, META_LAST_BOOTSTRAP)?.is_none();
    let recovering_ownership = meta(library, META_LATEST_FAILURE)?.as_deref() == Some("1");
    let mut report = SyncReport::default();

    let result = (|| -> Result<(), ApiError> {
        if full_bootstrap_due(library)? {
            // Re-page everything. Upserts are idempotent, so this refreshes
            // metadata in place rather than churning rows.
            commit(session, &scope, || {
                library
                    .set_meta(META_BOOTSTRAP_DONE, "0")
                    .map_err(|error| storage_error(&error))?;
                library
                    .set_meta(META_BOOTSTRAP_OFFSET, "0")
                    .map_err(|error| storage_error(&error))?;
                // An empty value deliberately means "the first page has not told
                // us the new total yet"; retaining the previous week's total would
                // make the determinate bar move against stale information.
                library
                    .set_meta(META_BOOTSTRAP_TOTAL, "")
                    .map_err(|error| storage_error(&error))?;
                Ok(())
            })?;
        }
        if meta(library, META_BOOTSTRAP_DONE)?.as_deref() != Some("1")
            && let Some(control) = control
        {
            control.set_phase(SyncPhase::Catalog);
        }
        report.bootstrapped = bootstrap(library, session, &scope, client, user_id, control)?;
        cancelled(control, session, &scope)?;
        if let Some(control) = control {
            control.set_phase(SyncPhase::Reconciling);
        }
        report.updated = incremental(
            library,
            session,
            &scope,
            client,
            user_id,
            &mut report.changes,
            control,
        )?;
        if initial_catalog {
            // The just-completed catalog itself is a complete identity and user
            // data observation; repeating every page immediately would double
            // first-run request load without finding a stale row.
            commit(session, &scope, || touch(library, META_LAST_IDENTITY_SWEEP))?;
        } else if trigger.forces_identity_sweep()
            || due(library, META_LAST_IDENTITY_SWEEP, IDENTITY_SWEEP_INTERVAL)?
        {
            let (refreshed, deletion_changes) =
                identity_sweep(library, session, &scope, client, user_id, control)?;
            report.user_data_refreshed = refreshed;
            report.deleted = deletion_changes.item_ids.len();
            report.changes.merge(deletion_changes);
            commit(session, &scope, || touch(library, META_LAST_IDENTITY_SWEEP))?;
        }
        Ok(())
    })();

    if let Err(error) = &result {
        let _ = commit(session, &scope, || {
            library
                .set_meta(META_LATEST_FAILURE, "1")
                .map_err(|storage| storage_error(&storage))
        });
        session.note_scoped_error(&scope, error);
        // Bootstrap/incremental pages commit independently. A later network
        // failure must not strand already-visible SQLite changes without the
        // same batched UI notification a fully successful cycle receives.
        if !report.changes.is_empty()
            && session.scope_is_current(&scope)
            && let Some(control) = control
        {
            control.shell().library_changed(report.changes.clone());
        }
    }
    result?;

    report.elapsed_ms = started.elapsed().as_millis() as u64;
    let serialized_report = serde_json::to_string(&report).unwrap_or_default();
    commit(session, &scope, || {
        library
            .set_meta(META_LATEST_FAILURE, "0")
            .map_err(|error| storage_error(&error))?;
        touch(library, META_LAST_SYNC)?;
        library
            .set_meta("sync.last_report", &serialized_report)
            .map_err(|error| storage_error(&error))?;
        Ok(())
    })?;
    if let Some(control) = control {
        if recovering_ownership {
            control.shell().collections_changed();
        }
        if !report.changes.is_empty() {
            control.shell().library_changed(report.changes.clone());
        }
        if initial_catalog || report.changed() || recovering_ownership {
            control.announce_cycle_completed();
        }
    }
    if (initial_catalog || report.changed())
        && let Err(error) = library.optimize()
    {
        // Stale statistics only slow queries down; the sync itself succeeded.
        tracing::warn!(target: "library.sync", "could not refresh query statistics: {error}");
    }
    Ok(report)
}

/// Pages the whole library once, resuming from the stored offset after a crash
/// or a mid-sync sign-out.
fn bootstrap(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    client: &JellyfinClient,
    user_id: &str,
    control: Option<&SyncHandle>,
) -> Result<usize, ApiError> {
    if meta(library, META_BOOTSTRAP_DONE)?.as_deref() == Some("1") {
        return Ok(0);
    }

    let phase_started = Instant::now();
    let mut offset = meta(library, META_BOOTSTRAP_OFFSET)?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    let mut written = 0;
    let mut watermark = meta(library, META_WATERMARK)?;
    let mut watermark_ids = read_watermark_ids(library)?;

    let mut pages = 0;
    let truncated = loop {
        cancelled(control, session, scope)?;
        if pages >= MAX_BOOTSTRAP_PAGES {
            break true;
        }
        let fetch_started = Instant::now();
        let page = items::fetch_items_page(client, user_id, offset, "DateCreated", "Ascending")?;
        let fetch_ms = fetch_started.elapsed().as_millis() as u64;
        // Jellyfin normally reports the total on every page. Preserve the last
        // useful value if a trailing empty page omits it, while still recording
        // a real zero for an empty library.
        if page.items.is_empty() {
            commit_empty_bootstrap_page(
                library,
                session,
                scope,
                page.total_record_count,
                offset,
                phase_started.elapsed(),
            )?;
            break false;
        }
        advance_watermark_with_ids(&mut watermark, &mut watermark_ids, &page.items);
        let next_offset = offset + page.items.len() as i64;
        let ingest_started = Instant::now();
        let page_changes = commit_catalog_page(library, session, scope, &page, offset)?;
        written += page.items.len();
        offset = next_offset;
        pages += 1;
        if pages == 1 {
            tracing::info!(
                target: "library.sync",
                items = page.items.len(),
                total = page.total_record_count,
                ready_ms = phase_started.elapsed().as_millis() as u64,
                "first catalog page committed; library is ready"
            );
        }
        // Bootstrap commits invalidate local projections without refetching live Next Up.
        if !page_changes.is_empty()
            && let Some(control) = control
        {
            control.shell().catalog_changed(page_changes);
        }
        tracing::debug!(
            target: "library.sync",
            offset,
            total = page.total_record_count,
            fetch_ms,
            ingest_ms = ingest_started.elapsed().as_millis() as u64,
            "bootstrapped a library page"
        );
        if page.total_record_count > 0 && offset >= page.total_record_count {
            break false;
        }
        if (page.items.len() as i64) < PAGE_SIZE {
            break false;
        }
    };

    commit_bootstrap_watermark(
        library,
        session,
        scope,
        watermark.as_deref(),
        &watermark_ids,
    )?;
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
    finish_bootstrap(library, session, scope, written)
}

fn commit_catalog_page(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    page: &crate::jellyfin::api::model::ItemsResponse,
    offset: i64,
) -> Result<LibraryChangeBatch, ApiError> {
    commit(session, scope, || {
        let changes = library
            .ingest_page(&page.items)
            .map_err(|error| storage_error(&error))?;
        if page.total_record_count > 0 || offset == 0 {
            library
                .set_meta(
                    META_BOOTSTRAP_TOTAL,
                    &page.total_record_count.max(0).to_string(),
                )
                .map_err(|error| storage_error(&error))?;
        }
        library
            .set_meta(META_CATALOG_READY, "1")
            .map_err(|error| storage_error(&error))?;
        library
            .set_meta(
                META_BOOTSTRAP_OFFSET,
                &(offset + page.items.len() as i64).to_string(),
            )
            .map_err(|error| storage_error(&error))?;
        Ok(changes)
    })
}

fn finish_bootstrap(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    written: usize,
) -> Result<usize, ApiError> {
    commit(session, scope, || {
        library
            .set_meta(META_BOOTSTRAP_DONE, "1")
            .map_err(|error| storage_error(&error))?;
        touch(library, META_LAST_BOOTSTRAP)
    })?;
    tracing::info!(target: "library.sync", items = written, "library bootstrap complete");
    Ok(written)
}

fn commit_bootstrap_watermark(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    watermark: Option<&str>,
    watermark_ids: &HashSet<String>,
) -> Result<(), ApiError> {
    let Some(watermark) = watermark else {
        return Ok(());
    };
    commit(session, scope, || {
        library
            .set_meta(META_WATERMARK, watermark)
            .map_err(|error| storage_error(&error))?;
        write_watermark_ids(library, watermark_ids)
    })
}

fn commit_empty_bootstrap_page(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    total_record_count: i64,
    offset: i64,
    elapsed: Duration,
) -> Result<(), ApiError> {
    commit(session, scope, || {
        if total_record_count > 0 || offset == 0 {
            library
                .set_meta(META_BOOTSTRAP_TOTAL, &total_record_count.max(0).to_string())
                .map_err(|error| storage_error(&error))?;
        }
        // Even an empty library has a successful first catalog page.
        library
            .set_meta(META_CATALOG_READY, "1")
            .map_err(|error| storage_error(&error))?;
        Ok(())
    })?;
    if offset == 0 {
        tracing::info!(
            target: "library.sync",
            ready_ms = elapsed.as_millis() as u64,
            "empty catalog confirmed; library is ready"
        );
    }
    Ok(())
}

/// Walks `DateCreated` descending until it reaches items already cached.
///
/// This is what makes a replaced file appear: Jellyfin deletes the old item and
/// adds a new one with a new id and a fresh `DateCreated`.
fn incremental(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    client: &JellyfinClient,
    user_id: &str,
    changes: &mut LibraryChangeBatch,
    control: Option<&SyncHandle>,
) -> Result<usize, ApiError> {
    let watermark = meta(library, META_WATERMARK)?;
    let known_watermark_ids = read_watermark_ids(library)?;
    let mut offset = 0;
    let mut written = 0;
    let mut newest = watermark.clone();
    let mut newest_ids = known_watermark_ids.clone();

    for _ in 0..MAX_INCREMENTAL_PAGES {
        cancelled(control, session, scope)?;
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
        let page_changes = commit(session, scope, || {
            library
                .ingest_page(&fresh)
                .map_err(|error| storage_error(&error))
        })?;
        written += fresh.len();
        changes.merge(page_changes);

        if reached_watermark || (page.items.len() as i64) < PAGE_SIZE {
            break;
        }
        offset += page.items.len() as i64;
    }

    if let Some(newest) = newest {
        commit(session, scope, || {
            library
                .set_meta(META_WATERMARK, &newest)
                .map_err(|error| storage_error(&error))?;
            write_watermark_ids(library, &newest_ids)
        })?;
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
    session: &Session,
    scope: &SessionScope,
    client: &JellyfinClient,
    user_id: &str,
    control: Option<&SyncHandle>,
) -> Result<(usize, LibraryChangeBatch), ApiError> {
    let mut offset = 0;
    let mut refreshed = 0;
    let mut seen = HashSet::new();
    let mut pages = 0;
    let truncated = loop {
        cancelled(control, session, scope)?;
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
        refreshed += commit(session, scope, || {
            library
                .upsert_user_data(&records)
                .map_err(|error| storage_error(&error))
        })?;
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
    let deletion_changes = commit(session, scope, || {
        library
            .retain_ids(&seen)
            .map_err(|error| storage_error(&error))
    })?;
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

fn read_watermark_ids(library: &Library) -> Result<HashSet<String>, ApiError> {
    Ok(meta(library, META_WATERMARK_IDS)?
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .collect())
}

fn write_watermark_ids(library: &Library, ids: &HashSet<String>) -> Result<(), ApiError> {
    let mut ids = ids.iter().cloned().collect::<Vec<_>>();
    ids.sort_unstable();
    library
        .set_meta(
            META_WATERMARK_IDS,
            &serde_json::to_string(&ids).unwrap_or_default(),
        )
        .map_err(|error| storage_error(&error))
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

fn due(library: &Library, key: &str, interval: Duration) -> Result<bool, ApiError> {
    let Some(last) = meta(library, key)?.and_then(|value| value.parse::<u64>().ok()) else {
        return Ok(true);
    };
    Ok(now_unix().saturating_sub(last) >= interval.as_secs())
}

fn full_bootstrap_due(library: &Library) -> Result<bool, ApiError> {
    // A missing/false done marker means the resumable initial pass is already
    // underway. Reset only a previously completed cache whose daily
    // re-bootstrap is due; otherwise a retry after a network failure would jump back to
    // zero and make both the stored offset and the progress UI dishonest.
    Ok(meta(library, META_BOOTSTRAP_DONE)?.as_deref() == Some("1")
        && due(library, META_LAST_BOOTSTRAP, REBOOTSTRAP_INTERVAL)?)
}

/// A sync-state read. A failed read stops the cycle; treating it as absent
/// would restart the catalog from its first page.
fn meta(library: &Library, key: &str) -> Result<Option<String>, ApiError> {
    library.meta(key).map_err(|error| storage_error(&error))
}

fn touch(library: &Library, key: &str) -> Result<(), ApiError> {
    library
        .set_meta(key, &now_unix().to_string())
        .map_err(|error| storage_error(&error))
}

/// Storage failures are reported through the same channel as API failures so a
/// cycle stops on the first problem instead of half-applying a page.
fn cancelled(
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

fn commit<T>(
    session: &Session,
    scope: &SessionScope,
    work: impl FnOnce() -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    session.commit_if_current(scope, || ApiError::Cancelled, work)
}

fn storage_error(error: &rusqlite::Error) -> ApiError {
    ApiError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, mpsc};
    use std::thread;
    use std::time::Duration;

    use super::PAGE_SIZE;
    use crate::jellyfin::api::ApiError;
    use crate::jellyfin::session::Session;
    use crate::library::sync::{
        META_BOOTSTRAP_DONE, META_BOOTSTRAP_OFFSET, META_BOOTSTRAP_TOTAL, META_LAST_BOOTSTRAP,
        META_LAST_IDENTITY_SWEEP, META_WATERMARK, META_WATERMARK_IDS, Trigger, bootstrap_progress,
        now_unix,
    };
    use crate::library::{Library, StoredCredentials};

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
        Session::restore(library.clone(), Arc::default())
    }

    #[test]
    fn first_catalog_page_is_ready_while_background_paging_continues() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        library
            .set_meta(META_LAST_IDENTITY_SWEEP, &now_unix().to_string())
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
        // The index filters, sorts, and joins on these; without them the
        // cache silently loses genres, provider matches, and name order.
        for field in ["ProviderIds", "Genres", "SortName", "ParentId"] {
            assert!(
                targets[0].contains(field),
                "{field} missing: {}",
                targets[0]
            );
        }
        assert!(targets[..2].iter().all(|target| !target.contains("People")));
        assert!(
            targets[..2]
                .iter()
                .all(|target| !target.contains("MediaStreams"))
        );
    }

    /// Answers one request per body, in order, returning the request targets.
    fn serve(bodies: Vec<String>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let server = thread::spawn(move || {
            bodies
                .into_iter()
                .map(|body| {
                    let (stream, target) = receive_target(&listener);
                    send_json(stream, &body);
                    target
                })
                .collect()
        });
        (url, server)
    }

    const EMPTY_PAGE: &str = r#"{"Items":[],"TotalRecordCount":0}"#;

    fn set_meta(library: &Library, key: &str, value: &str) {
        library.set_meta(key, value).expect("meta");
    }

    /// A completed catalog whose daily re-page and hourly identity sweep
    /// just ran, so a scheduled cycle only walks the incremental sweep.
    fn settled_catalog(library: &Library) {
        let now = now_unix().to_string();
        set_meta(library, META_BOOTSTRAP_DONE, "1");
        set_meta(library, META_LAST_BOOTSTRAP, &now);
        set_meta(library, META_LAST_IDENTITY_SWEEP, &now);
    }

    #[test]
    fn an_interrupted_catalog_resumes_and_a_stale_complete_one_repages() {
        let (url, server) = serve(vec![
            r#"{"Items":[],"TotalRecordCount":1250}"#.to_string(),
            EMPTY_PAGE.to_string(),
            EMPTY_PAGE.to_string(),
            EMPTY_PAGE.to_string(),
        ]);
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let session = authenticated_session(&library, &url);

        // A fill interrupted at 400 of 1250 continues from 400.
        set_meta(&library, META_BOOTSTRAP_OFFSET, "400");
        set_meta(&library, META_BOOTSTRAP_TOTAL, "1250");
        set_meta(&library, META_BOOTSTRAP_DONE, "0");
        super::run_cycle(&library, &session, Trigger::Scheduled).expect("resumed cycle");
        let resumed = bootstrap_progress(&library);
        assert!(resumed.complete);
        assert_eq!(resumed.processed, 400);

        // A completed pass whose daily re-page is due starts over at zero.
        set_meta(&library, META_LAST_BOOTSTRAP, "0");
        set_meta(&library, META_LAST_IDENTITY_SWEEP, &now_unix().to_string());
        super::run_cycle(&library, &session, Trigger::Scheduled).expect("re-page cycle");

        let targets = server.join().expect("server");
        assert!(targets[0].contains("StartIndex=400"), "{}", targets[0]);
        assert!(
            targets[1].contains("SortOrder=Descending"),
            "{}",
            targets[1]
        );
        assert!(targets[2].contains("StartIndex=0"), "{}", targets[2]);
        assert!(targets[2].contains("SortOrder=Ascending"), "{}", targets[2]);
    }

    #[test]
    fn an_unreadable_sync_state_stops_the_cycle_instead_of_restarting_the_catalog() {
        // Nothing listens here: a cycle that misreads the broken state as
        // "never bootstrapped" fails on the network instead of on storage.
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let session = authenticated_session(&library, "http://127.0.0.1:9");
        settled_catalog(&library);
        library
            .db
            .with_connection(|connection| connection.execute_batch("DROP TABLE meta"))
            .expect("break the sync state");

        assert!(matches!(
            super::run_cycle(&library, &session, Trigger::Requested),
            Err(ApiError::Storage(_))
        ));
        assert!(!bootstrap_progress(&library).complete);
    }

    #[test]
    fn the_incremental_sweep_ingests_only_items_past_the_watermark() {
        let page = r#"{"Items":[
            {"Id":"new","Name":"New","Type":"Movie","DateCreated":"2024-02-01T00:00:00Z"},
            {"Id":"tied","Name":"Tied","Type":"Movie","DateCreated":"2024-01-01T00:00:00Z"},
            {"Id":"known","Name":"Known","Type":"Movie","DateCreated":"2024-01-01T00:00:00Z"},
            {"Id":"old","Name":"Old","Type":"Movie","DateCreated":"2023-12-01T00:00:00Z"}
        ],"TotalRecordCount":4}"#;
        let (url, server) = serve(vec![page.to_string(), page.to_string()]);
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let session = authenticated_session(&library, &url);
        settled_catalog(&library);
        set_meta(&library, META_WATERMARK, "2024-01-01T00:00:00Z");
        set_meta(&library, META_WATERMARK_IDS, r#"["known"]"#);

        let first = super::run_cycle(&library, &session, Trigger::Scheduled).expect("cycle");
        // Newer than the watermark, or tied with it but never seen.
        assert_eq!(first.updated, 2);
        for id in ["new", "tied"] {
            assert!(library.item(id).expect("query").is_some(), "{id} missed");
        }
        for id in ["known", "old"] {
            assert!(library.item(id).expect("query").is_none(), "{id} re-read");
        }

        // The watermark moved to the newest item, so the same page is old news.
        let second = super::run_cycle(&library, &session, Trigger::Scheduled).expect("cycle");
        assert_eq!(second.updated, 0);

        let targets = server.join().expect("server");
        assert!(
            targets
                .iter()
                .all(|target| target.contains("SortOrder=Descending"))
        );
    }

    /// Only an explicit ask may skip the identity sweep's hourly gate, which
    /// is the only pass that notices a deletion on the server.
    #[test]
    fn only_a_requested_cycle_reconciles_deletions_before_the_hourly_sweep() {
        let (url, server) = serve(vec![
            EMPTY_PAGE.to_string(),
            EMPTY_PAGE.to_string(),
            r#"{"Items":[{"Id":"kept"}],"TotalRecordCount":1}"#.to_string(),
        ]);
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let session = authenticated_session(&library, &url);
        library
            .ingest_page(&[
                serde_json::from_str(r#"{"Id":"kept","Name":"Kept","Type":"Movie"}"#).expect("dto"),
                serde_json::from_str(r#"{"Id":"gone","Name":"Gone","Type":"Movie"}"#).expect("dto"),
            ])
            .expect("seed");
        settled_catalog(&library);

        let scheduled = super::run_cycle(&library, &session, Trigger::Scheduled).expect("cycle");
        assert_eq!(scheduled.deleted, 0);
        assert!(library.item("gone").expect("query").is_some());

        let requested = super::run_cycle(&library, &session, Trigger::Requested).expect("cycle");
        assert_eq!(requested.deleted, 1);
        assert!(library.item("gone").expect("query").is_none());
        assert!(library.item("kept").expect("query").is_some());

        let targets = server.join().expect("server");
        assert!(targets[2].contains("EnableImages=false"), "{}", targets[2]);
    }
}
