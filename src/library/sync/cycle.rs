use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::jellyfin::api::items::{self, PAGE_SIZE};
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::session::{Session, SessionScope};
use crate::library::{Library, LibraryChangeBatch, UserDataRecord};

use super::membership::{membership_pass, refresh_libraries};
use super::{
    IDENTITY_PAGE_SIZE, IDENTITY_SWEEP_INTERVAL, MAX_BOOTSTRAP_PAGES, MAX_IDENTITY_PAGES,
    MAX_INCREMENTAL_PAGES, META_BOOTSTRAP_DONE, META_BOOTSTRAP_OFFSET, META_BOOTSTRAP_TOTAL,
    META_CATALOG_READY, META_LAST_BOOTSTRAP, META_LAST_IDENTITY_SWEEP, META_LAST_SYNC,
    META_LATEST_FAILURE, META_WATERMARK, META_WATERMARK_IDS, REBOOTSTRAP_INTERVAL, SyncHandle,
    SyncPhase, SyncReport, Trigger, now_unix,
};

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
    let mut membership_changed = false;

    let result = (|| -> Result<(), ApiError> {
        let libraries = refresh_libraries(library, session, &scope)?;
        if full_bootstrap_due(library)? {
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
        if initial_catalog {
            // Before the first fill, so library grids fill page by page with it.
            membership_changed |= membership_pass(library, session, &scope, control)?;
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
        let swept = if initial_catalog {
            commit(session, &scope, || touch(library, META_LAST_IDENTITY_SWEEP))?;
            false
        } else if trigger.forces_identity_sweep()
            // A removed library's titles leave the cache now, not within the hour.
            || !libraries.removed.is_empty()
            || due(library, META_LAST_IDENTITY_SWEEP, IDENTITY_SWEEP_INTERVAL)?
        {
            sweep_identity(library, session, &scope, control, &mut report)?;
            true
        } else {
            false
        };
        if !libraries.is_empty() || report.changed() || swept || trigger == Trigger::Requested {
            membership_changed |= membership_pass(library, session, &scope, control)?;
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
        if !report.changes.is_empty()
            && session.scope_is_current(&scope)
            && let Some(control) = control
        {
            control.shell().library_changed(report.changes.clone());
        }
    }
    result?;

    report.elapsed_ms = started.elapsed().as_millis() as u64;
    record_success(library, session, &scope, &report)?;
    if let Some(control) = control {
        if recovering_ownership {
            control.shell().collections_changed();
        }
        if !report.changes.is_empty() {
            control.shell().library_changed(report.changes.clone());
        }
        if initial_catalog || report.changed() || recovering_ownership || membership_changed {
            control.announce_cycle_completed();
        }
    }
    if (initial_catalog || report.changed())
        && let Err(error) = library.optimize()
    {
        tracing::warn!(target: "library.sync", "could not refresh query statistics: {error}");
    }
    Ok(report)
}

fn record_success(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    report: &SyncReport,
) -> Result<(), ApiError> {
    let serialized_report = serde_json::to_string(report).unwrap_or_default();
    commit(session, scope, || {
        library
            .set_meta(META_LATEST_FAILURE, "0")
            .map_err(|error| storage_error(&error))?;
        touch(library, META_LAST_SYNC)?;
        library
            .set_meta("sync.last_report", &serialized_report)
            .map_err(|error| storage_error(&error))?;
        Ok(())
    })
}

fn sweep_identity(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    control: Option<&SyncHandle>,
    report: &mut SyncReport,
) -> Result<(), ApiError> {
    let (refreshed, deletion_changes) = identity_sweep(
        library,
        session,
        scope,
        scope.client(),
        scope.user_id(),
        control,
    )?;
    report.user_data_refreshed = refreshed;
    report.deleted = deletion_changes.item_ids.len();
    report.changes.merge(deletion_changes);
    commit(session, scope, || touch(library, META_LAST_IDENTITY_SWEEP))
}

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

fn meta(library: &Library, key: &str) -> Result<Option<String>, ApiError> {
    library.meta(key).map_err(|error| storage_error(&error))
}

fn touch(library: &Library, key: &str) -> Result<(), ApiError> {
    library
        .set_meta(key, &now_unix().to_string())
        .map_err(|error| storage_error(&error))
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

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, mpsc};
    use std::thread;
    use std::time::Duration;

    use serde_json::json;

    use super::PAGE_SIZE;
    use crate::jellyfin::api::ApiError;
    use crate::jellyfin::session::Session;
    use crate::library::libraries::{LibraryId, LibraryRegistry};
    use crate::library::sync::{
        META_BOOTSTRAP_DONE, META_LAST_BOOTSTRAP, META_LAST_IDENTITY_SWEEP, SyncReport, Trigger,
        bootstrap_progress, now_unix, ownership_available,
    };
    use crate::library::{ItemQuery, Library, StoredCredentials};

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

    fn send_json(stream: TcpStream, body: &str) {
        send(stream, 200, body);
    }

    fn send(mut stream: TcpStream, status: u16, body: &str) {
        write!(
            stream,
            "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
            send_json(stream, NO_LIBRARIES);

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
        assert!(targets[0].starts_with("/UserViews?"), "{}", targets[0]);
        assert!(targets[1].contains("StartIndex=0"));
        assert!(targets[2].contains(&format!("StartIndex={PAGE_SIZE}")));
        for field in ["ProviderIds", "Genres", "SortName", "ParentId"] {
            assert!(
                targets[1].contains(field),
                "{field} missing: {}",
                targets[1]
            );
        }
    }

    const FINISHED: &str = "/finished";

    struct FakeJellyfin {
        url: String,
        server: thread::JoinHandle<Vec<String>>,
    }

    impl FakeJellyfin {
        /// Every request target in arrival order. A request past the script
        /// was answered 404 and is listed too.
        fn requests(self) -> Vec<String> {
            let mut stream =
                TcpStream::connect(self.url.trim_start_matches("http://")).expect("connect");
            write!(stream, "GET {FINISHED} HTTP/1.1\r\n\r\n").expect("finish");
            self.server.join().expect("server")
        }
    }

    fn serve(replies: Vec<(u16, String)>) -> FakeJellyfin {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let server = thread::spawn(move || {
            let mut replies = replies.into_iter();
            let mut targets = Vec::new();
            loop {
                let (stream, target) = receive_target(&listener);
                if target == FINISHED {
                    return targets;
                }
                let (status, body) = replies.next().unwrap_or((404, String::new()));
                send(stream, status, &body);
                targets.push(target);
            }
        });
        FakeJellyfin { url, server }
    }

    fn ok(body: &str) -> (u16, String) {
        (200, body.to_string())
    }

    const EMPTY_PAGE: &str = r#"{"Items":[],"TotalRecordCount":0}"#;
    const NO_LIBRARIES: &str = r#"{"Items":[],"TotalRecordCount":0}"#;
    const NOT_JSON: &str = "not json";
    const MOVIES: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
    const MORE_MOVIES: &str = "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";
    const SHOWS: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
    const ONE_MOVIE: &str = r#"{"Items":[{"Id":"m1","Name":"M1","Type":"Movie","DateCreated":"2024-01-01T00:00:00Z"}],"TotalRecordCount":1}"#;

    /// Runs one cycle against a server that answers `replies` in order.
    fn cycle(
        library: &Arc<Library>,
        trigger: Trigger,
        replies: Vec<(u16, String)>,
    ) -> (Result<SyncReport, ApiError>, Vec<String>) {
        let server = serve(replies);
        let session = authenticated_session(library, &server.url);
        let result = super::run_cycle(library, &session, trigger);
        (result, server.requests())
    }

    /// What each request was for.
    fn steps(targets: &[String]) -> Vec<&str> {
        targets
            .iter()
            .map(|target| {
                let parent = target
                    .split(['?', '&'])
                    .find_map(|pair| pair.strip_prefix("parentId="));
                match parent {
                    Some(MOVIES) => "walk movies",
                    Some(MORE_MOVIES) => "walk more movies",
                    Some(SHOWS) => "walk shows",
                    Some(_) => "walk elsewhere",
                    None if target.starts_with("/UserViews") => "views",
                    None if target.contains("SortOrder=Descending") => "incremental",
                    None if target.contains("EnableImages=false") => "sweep",
                    None if target.starts_with("/Items") => "bootstrap",
                    None => target.as_str(),
                }
            })
            .collect()
    }

    fn views(libraries: &[(&str, &str)]) -> (u16, String) {
        let items = libraries
            .iter()
            .map(|(id, collection_type)| {
                json!({ "Id": id, "Name": collection_type, "CollectionType": collection_type })
            })
            .collect::<Vec<_>>();
        ok(&json!({ "Items": items, "TotalRecordCount": items.len() }).to_string())
    }

    fn members(ids: &[&str], total: usize) -> (u16, String) {
        let items = ids.iter().map(|id| json!({ "Id": id })).collect::<Vec<_>>();
        ok(&json!({ "Items": items, "TotalRecordCount": total }).to_string())
    }

    /// A signed-in cache that already holds `titles` and a finished catalog.
    fn settled_library(titles: &[(&str, &str)]) -> Arc<Library> {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let dtos = titles
            .iter()
            .map(|(id, kind)| {
                serde_json::from_value(json!({ "Id": id, "Name": id, "Type": kind })).expect("dto")
            })
            .collect::<Vec<_>>();
        library.ingest_page(&dtos).expect("seed");
        settled_catalog(&library);
        library
    }

    /// One cycle that stores `libraries`, all movies or TV shows libraries,
    /// with the members each lists.
    fn sync_libraries(library: &Arc<Library>, libraries: &[(&str, &str, &[&str])]) {
        let listed = libraries
            .iter()
            .map(|(id, collection_type, _)| (*id, *collection_type))
            .collect::<Vec<_>>();
        let mut replies = vec![views(&listed), ok(EMPTY_PAGE)];
        replies.extend(libraries.iter().map(|(_, _, ids)| members(ids, ids.len())));
        let (result, _) = cycle(library, Trigger::Scheduled, replies);
        result.expect("libraries synced");
    }

    fn listed(library: &Library) -> Option<Vec<String>> {
        match library.libraries().expect("registry") {
            LibraryRegistry::Unknown => None,
            LibraryRegistry::Known(libraries) => Some(
                libraries
                    .iter()
                    .map(|library| library.id.as_str().to_string())
                    .collect(),
            ),
        }
    }

    /// The cached titles a library's grid lists.
    fn grid(library: &Library, id: &str) -> Vec<String> {
        let mut ids = library
            .query(&ItemQuery {
                library: LibraryId::parse(id),
                limit: 500,
                ..ItemQuery::default()
            })
            .expect("grid")
            .items
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }

    fn set_meta(library: &Library, key: &str, value: &str) {
        library.set_meta(key, value).expect("meta");
    }

    fn settled_catalog(library: &Library) {
        let now = now_unix().to_string();
        set_meta(library, META_BOOTSTRAP_DONE, "1");
        set_meta(library, META_LAST_BOOTSTRAP, &now);
        set_meta(library, META_LAST_IDENTITY_SWEEP, &now);
    }

    #[test]
    fn an_unreadable_sync_state_stops_the_cycle_instead_of_restarting_the_catalog() {
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
    fn a_requested_cycle_reconciles_server_deletions() {
        let server = serve(vec![
            ok(NO_LIBRARIES),
            ok(EMPTY_PAGE),
            ok(r#"{"Items":[{"Id":"kept"}],"TotalRecordCount":1}"#),
        ]);
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let session = authenticated_session(&library, &server.url);
        library
            .ingest_page(&[
                serde_json::from_str(r#"{"Id":"kept","Name":"Kept","Type":"Movie"}"#).expect("dto"),
                serde_json::from_str(r#"{"Id":"gone","Name":"Gone","Type":"Movie"}"#).expect("dto"),
            ])
            .expect("seed");
        settled_catalog(&library);

        let requested = super::run_cycle(&library, &session, Trigger::Requested).expect("cycle");
        assert_eq!(requested.deleted, 1);
        assert!(library.item("gone").expect("query").is_none());
        assert!(library.item("kept").expect("query").is_some());

        server.requests();
    }

    #[test]
    fn a_failed_library_list_still_fills_a_fresh_catalog() {
        let library = Arc::new(Library::open_in_memory().expect("library"));

        let (result, requests) = cycle(
            &library,
            Trigger::Requested,
            vec![ok(NOT_JSON), ok(ONE_MOVIE), ok(ONE_MOVIE)],
        );

        result.expect("the catalog synced without the library list");
        assert_eq!(steps(&requests), ["views", "bootstrap", "incremental"]);
        assert!(bootstrap_progress(&library).complete);
        assert!(ownership_available(&library));
        assert_eq!(listed(&library), None);
    }

    #[test]
    fn a_failed_library_list_keeps_the_stored_libraries_and_membership() {
        let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
        sync_libraries(&library, &[(MOVIES, "movies", &["m1", "m2"])]);

        let (result, requests) = cycle(
            &library,
            Trigger::Scheduled,
            vec![ok(NOT_JSON), ok(EMPTY_PAGE)],
        );

        result.expect("the catalog synced without the library list");
        assert_eq!(steps(&requests), ["views", "incremental"]);
        assert_eq!(listed(&library), Some(vec![MOVIES.to_string()]));
        assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
        assert!(ownership_available(&library));
    }

    #[test]
    fn a_failed_membership_walk_keeps_its_membership_and_walks_again_next_cycle() {
        let library = settled_library(&[
            ("m1", "Movie"),
            ("m2", "Movie"),
            ("m3", "Movie"),
            ("s1", "Series"),
            ("s2", "Series"),
        ]);
        sync_libraries(
            &library,
            &[
                (MOVIES, "movies", &["m1", "m2"]),
                (SHOWS, "tvshows", &["s1"]),
            ],
        );
        let listing = || views(&[(MOVIES, "movies"), (SHOWS, "tvshows")]);

        let (result, requests) = cycle(
            &library,
            Trigger::Requested,
            vec![
                listing(),
                ok(EMPTY_PAGE),
                members(&["m1", "m2", "m3", "s1", "s2"], 5),
                ok(NOT_JSON),
                members(&["s1", "s2"], 2),
            ],
        );
        result.expect("the catalog synced without one library's members");
        assert_eq!(
            steps(&requests),
            ["views", "incremental", "sweep", "walk movies", "walk shows"]
        );
        assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
        assert_eq!(grid(&library, SHOWS), ["s1", "s2"]);
        assert!(ownership_available(&library));

        let (result, requests) = cycle(
            &library,
            Trigger::Scheduled,
            vec![
                listing(),
                ok(EMPTY_PAGE),
                members(&["m1", "m2", "m3"], 3),
                members(&["s1", "s2"], 2),
            ],
        );
        result.expect("the next cycle");
        assert_eq!(
            steps(&requests),
            ["views", "incremental", "walk movies", "walk shows"]
        );
        assert_eq!(grid(&library, MOVIES), ["m1", "m2", "m3"]);

        let (result, requests) = cycle(
            &library,
            Trigger::Scheduled,
            vec![listing(), ok(EMPTY_PAGE)],
        );
        result.expect("a cycle with nothing owed");
        assert_eq!(steps(&requests), ["views", "incremental"]);
    }

    #[test]
    fn a_library_added_by_a_failed_cycle_is_walked_by_the_next() {
        let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
        let listing = || views(&[(MOVIES, "movies")]);

        let (result, requests) = cycle(&library, Trigger::Scheduled, vec![listing(), ok(NOT_JSON)]);
        assert!(matches!(result, Err(ApiError::Decode(_))), "{result:?}");
        assert_eq!(steps(&requests), ["views", "incremental"]);

        let (result, requests) = cycle(
            &library,
            Trigger::Scheduled,
            vec![listing(), ok(EMPTY_PAGE), members(&["m1", "m2"], 2)],
        );
        result.expect("the next cycle");
        assert_eq!(steps(&requests), ["views", "incremental", "walk movies"]);
        assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
    }

    #[test]
    fn a_library_removed_by_a_failed_cycle_is_swept_by_the_next() {
        let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
        sync_libraries(
            &library,
            &[
                (MOVIES, "movies", &["m1"]),
                (MORE_MOVIES, "movies", &["m2"]),
            ],
        );
        let remaining = || views(&[(MOVIES, "movies")]);

        let (result, _) = cycle(
            &library,
            Trigger::Scheduled,
            vec![remaining(), ok(NOT_JSON)],
        );
        assert!(matches!(result, Err(ApiError::Decode(_))), "{result:?}");

        let (result, requests) = cycle(
            &library,
            Trigger::Scheduled,
            vec![
                remaining(),
                ok(EMPTY_PAGE),
                members(&["m1"], 1),
                members(&["m1"], 1),
            ],
        );
        let report = result.expect("the next cycle");
        assert_eq!(
            steps(&requests),
            ["views", "incremental", "sweep", "walk movies"]
        );
        assert_eq!(report.deleted, 1);
        assert!(library.item("m2").expect("query").is_none());

        let (result, requests) = cycle(
            &library,
            Trigger::Scheduled,
            vec![remaining(), ok(EMPTY_PAGE)],
        );
        result.expect("a cycle with nothing owed");
        assert_eq!(steps(&requests), ["views", "incremental"]);
    }

    #[test]
    fn renaming_reordering_or_new_artwork_does_not_walk_membership() {
        let library = settled_library(&[("m1", "Movie"), ("s1", "Series")]);
        sync_libraries(
            &library,
            &[(MOVIES, "movies", &["m1"]), (SHOWS, "tvshows", &["s1"])],
        );
        let relisted = json!({
            "Items": [
                { "Id": SHOWS, "Name": "tvshows", "CollectionType": "tvshows" },
                {
                    "Id": MOVIES,
                    "Name": "Films",
                    "CollectionType": "movies",
                    "ImageTags": { "Primary": "new-artwork" },
                },
            ],
            "TotalRecordCount": 2,
        });

        let (result, requests) = cycle(
            &library,
            Trigger::Scheduled,
            vec![ok(&relisted.to_string()), ok(EMPTY_PAGE)],
        );

        result.expect("cycle");
        assert_eq!(steps(&requests), ["views", "incremental"]);
        assert_eq!(
            listed(&library),
            Some(vec![SHOWS.to_string(), MOVIES.to_string()])
        );
        assert_eq!(grid(&library, MOVIES), ["m1"]);
    }

    #[test]
    fn the_first_sync_walks_each_library_once() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let listing = || views(&[(MOVIES, "movies"), (SHOWS, "tvshows")]);

        let (result, requests) = cycle(
            &library,
            Trigger::Requested,
            vec![
                listing(),
                members(&["m1"], 1),
                members(&[], 0),
                ok(ONE_MOVIE),
                ok(ONE_MOVIE),
            ],
        );
        result.expect("the first sync");
        assert_eq!(
            steps(&requests),
            [
                "views",
                "walk movies",
                "walk shows",
                "bootstrap",
                "incremental"
            ]
        );
        assert_eq!(grid(&library, MOVIES), ["m1"]);

        let (result, requests) =
            cycle(&library, Trigger::Scheduled, vec![listing(), ok(ONE_MOVIE)]);
        result.expect("the second sync");
        assert_eq!(steps(&requests), ["views", "incremental"]);
    }

    #[test]
    fn a_membership_walk_that_ends_short_of_its_total_keeps_the_previous_membership() {
        let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
        sync_libraries(&library, &[(MOVIES, "movies", &["m1", "m2"])]);
        let listing = || views(&[(MOVIES, "movies")]);
        // 1,500 ids received against a total of 1,500, but the second page
        // repeats 100 of the first, so only 1,400 are distinct.
        let first = std::iter::once("m1".to_string())
            .chain((1..1_000).map(|n| format!("x{n:04}")))
            .collect::<Vec<_>>();
        let second = (900..1_400).map(|n| format!("x{n:04}")).collect::<Vec<_>>();
        let page =
            |ids: &[String]| members(&ids.iter().map(String::as_str).collect::<Vec<_>>(), 1_500);

        let (result, requests) = cycle(
            &library,
            Trigger::Requested,
            vec![
                listing(),
                ok(EMPTY_PAGE),
                members(&["m1", "m2"], 2),
                page(&first),
                page(&second),
            ],
        );
        result.expect("cycle");
        assert_eq!(
            steps(&requests),
            [
                "views",
                "incremental",
                "sweep",
                "walk movies",
                "walk movies"
            ]
        );
        assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
        assert!(
            requests[3].contains("SortBy=DateCreated%2CSortName"),
            "{}",
            requests[3]
        );

        let (result, requests) = cycle(
            &library,
            Trigger::Scheduled,
            vec![listing(), ok(EMPTY_PAGE), members(&["m1"], 1)],
        );
        result.expect("the next cycle");
        assert_eq!(steps(&requests), ["views", "incremental", "walk movies"]);
        assert_eq!(grid(&library, MOVIES), ["m1"]);
    }

    #[test]
    fn a_rejected_token_stops_the_cycle_at_any_library_step() {
        let library = settled_library(&[("m1", "Movie"), ("s1", "Series")]);
        sync_libraries(
            &library,
            &[(MOVIES, "movies", &["m1"]), (SHOWS, "tvshows", &["s1"])],
        );

        let (result, requests) = cycle(&library, Trigger::Scheduled, vec![(401, String::new())]);
        assert_eq!(result.err(), Some(ApiError::Unauthorized));
        assert_eq!(steps(&requests), ["views"]);

        let (result, requests) = cycle(
            &library,
            Trigger::Requested,
            vec![
                views(&[(MOVIES, "movies"), (SHOWS, "tvshows")]),
                ok(EMPTY_PAGE),
                members(&["m1", "s1"], 2),
                (401, String::new()),
            ],
        );
        assert_eq!(result.err(), Some(ApiError::Unauthorized));
        assert_eq!(
            steps(&requests),
            ["views", "incremental", "sweep", "walk movies"]
        );
    }
}
