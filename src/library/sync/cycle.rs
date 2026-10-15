use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::jellyfin::api::items::{self, PAGE_SIZE};
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::session::{Session, SessionScope};
use crate::library::catalog_libraries::{META_SWEEP_OWED, MembershipReplaced};
use crate::library::{Library, LibraryChangeBatch, UserDataRecord};

use super::libraries::{membership_pass, refresh_libraries};
use super::{
    IDENTITY_PAGE_SIZE, IDENTITY_SWEEP_INTERVAL, MAX_BOOTSTRAP_PAGES, MAX_CONFIRM_AGE,
    MAX_IDENTITY_PAGES, MAX_INCREMENTAL_PAGES, META_BOOTSTRAP_DONE, META_BOOTSTRAP_OFFSET,
    META_BOOTSTRAP_TOTAL, META_CATALOG_READY, META_EMPTY_SWEEP_AT, META_LAST_BOOTSTRAP,
    META_LAST_IDENTITY_SWEEP, META_LAST_SYNC, META_LATEST_FAILURE, META_WATERMARK,
    META_WATERMARK_IDS, MIN_CONFIRM_GAP, REBOOTSTRAP_INTERVAL, SyncHandle, SyncPhase, SyncReport,
    Trigger, best_effort, cancelled, catalog_downloading, commit, now_unix, storage_error,
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
    let mut walked = MembershipReplaced::default();

    let result = (|| -> Result<(), ApiError> {
        let no_catalog_library = list_libraries(library, session, &scope, control)?;
        if full_bootstrap_due(library)? {
            restart_bootstrap(library, session, &scope)?;
        }
        let downloading = catalog_downloading(library).map_err(|error| storage_error(&error))?;
        if downloading && let Some(control) = control {
            control.set_phase(SyncPhase::Catalog);
        }
        if initial_catalog {
            walk_membership(library, session, &scope, true, control, &mut walked)?;
        }
        report.bootstrapped = bootstrap(library, session, &scope, client, user_id, control)?;
        walked.settled |=
            downloading && !catalog_downloading(library).map_err(|error| storage_error(&error))?;
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
            || meta(library, META_SWEEP_OWED)?.as_deref() == Some("1")
            || due(library, META_LAST_IDENTITY_SWEEP, IDENTITY_SWEEP_INTERVAL)?
        {
            sweep_identity(
                library,
                session,
                &scope,
                no_catalog_library,
                control,
                &mut report,
            )?
        } else {
            false
        };
        if !initial_catalog {
            let owe_all = report.changed() || swept || trigger == Trigger::Requested;
            walk_membership(library, session, &scope, owe_all, control, &mut walked)?;
        }
        Ok(())
    })();

    if result.is_err() {
        record_failure(library, session, &scope);
    }
    let result = result.and_then(|()| {
        report.elapsed_ms = started.elapsed().as_millis() as u64;
        record_success(library, session, &scope, &report)
    });
    let succeeded = result.is_ok();
    let completed = succeeded
        && (initial_catalog || report.changed() || recovering_ownership || walked.changed);
    announce_outcome(session, &scope, control, &result, |control| {
        if succeeded && recovering_ownership {
            control.shell().collections_changed();
        }
        if !report.changes.is_empty() {
            control.shell().library_changed(report.changes.clone());
        }
        if walked.settled {
            tracing::debug!(target: "library.sync", "announced settled library memberships");
            control.shell().library_memberships_settled();
        }
        if completed {
            control.announce_cycle_completed();
        } else if walked.changed {
            control.shell().library_sync_completed();
        }
    });
    result?;

    if (initial_catalog || report.changed())
        && let Err(error) = library.optimize()
    {
        tracing::warn!(target: "library.sync", "could not refresh query statistics: {error}");
    }
    Ok(report)
}

fn restart_bootstrap(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
) -> Result<(), ApiError> {
    commit(session, scope, || {
        library
            .set_meta(META_BOOTSTRAP_DONE, "0")
            .map_err(|error| storage_error(&error))?;
        library
            .set_meta(META_BOOTSTRAP_OFFSET, "0")
            .map_err(|error| storage_error(&error))?;
        library
            .set_meta(META_BOOTSTRAP_TOTAL, "")
            .map_err(|error| storage_error(&error))?;
        Ok(())
    })
}

fn record_failure(library: &Library, session: &Session, scope: &SessionScope) {
    let _ = commit(session, scope, || {
        library
            .set_meta(META_LATEST_FAILURE, "1")
            .map_err(|storage| storage_error(&storage))
    });
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
    no_catalog_library: bool,
    control: Option<&SyncHandle>,
    report: &mut SyncReport,
) -> Result<bool, ApiError> {
    let (walk, failure) = match walk_identity(library, session, scope, control, &mut report.changes)
    {
        Ok(walk) => (walk, None),
        Err(error) => (
            IdentityWalk {
                refreshed: 0,
                seen: HashSet::new(),
                answer: IdentityAnswer::Incomplete,
            },
            Some(error),
        ),
    };
    let observed_at = meta(library, META_EMPTY_SWEEP_AT)?.and_then(|at| at.parse::<u64>().ok());
    let verdict = sweep_verdict(walk.answer, no_catalog_library, observed_at, now_unix());
    let deletion_changes = commit(session, scope, || {
        apply_sweep_verdict(library, verdict, &walk.seen)
    })?;
    report.user_data_refreshed = walk.refreshed;
    let deletion_ran = deletion_changes.is_some();
    if let Some(deletion_changes) = deletion_changes {
        report.deleted = deletion_changes.item_ids.len();
        report.changes.merge(deletion_changes);
    }
    if let Some(error) = failure {
        return Err(error);
    }
    commit(session, scope, || touch(library, META_LAST_IDENTITY_SWEEP))?;
    Ok(deletion_ran)
}

fn list_libraries(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    control: Option<&SyncHandle>,
) -> Result<bool, ApiError> {
    let refreshed = refresh_libraries(library, session, scope);
    let Some(listed) = best_effort("the library list", refreshed)? else {
        return Ok(false);
    };
    if listed.changed {
        announce(session, scope, control, |control| {
            tracing::debug!(target: "library.sync", "announced a changed library list");
            control.shell().libraries_changed();
        });
    }
    Ok(listed.lists_no_catalog_library())
}

fn announce(
    session: &Session,
    scope: &SessionScope,
    control: Option<&SyncHandle>,
    send: impl FnOnce(&SyncHandle),
) {
    if let Some(control) = control
        && session.scope_is_current(scope)
    {
        send(control);
    }
}

/// Announces before noting a failure: a rejected token expires the session,
/// which would silence any later announcement.
fn announce_outcome<T>(
    session: &Session,
    scope: &SessionScope,
    control: Option<&SyncHandle>,
    result: &Result<T, ApiError>,
    send: impl FnOnce(&SyncHandle),
) {
    announce(session, scope, control, send);
    if let Err(error) = result {
        session.note_scoped_error(scope, error);
    }
}

fn walk_membership(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    owe_all: bool,
    control: Option<&SyncHandle>,
    walked: &mut MembershipReplaced,
) -> Result<(), ApiError> {
    let owed = if owe_all {
        commit(session, scope, || {
            library
                .owe_every_membership()
                .map_err(|error| storage_error(&error))
        })
    } else {
        Ok(())
    };
    let pass = owed.and_then(|()| membership_pass(library, session, scope, control, walked));
    best_effort("library membership", pass)?;
    Ok(())
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

struct IdentityWalk {
    refreshed: usize,
    seen: HashSet<String>,
    answer: IdentityAnswer,
}

#[derive(Clone, Copy)]
enum IdentityAnswer {
    Incomplete,
    Listed,
    EmptyCountingZero,
    EmptyNotCountedZero,
}

#[derive(Clone, Copy)]
enum SweepVerdict {
    Delete,
    ObserveEmpty(u64),
    ForgetEmpty,
    Wait,
}

/// An emptied catalog library can stay listed, and only the daily rebootstrap restores deleted
/// titles, so an empty zero-count answer deletes once an earlier one in the window agrees.
fn sweep_verdict(
    answer: IdentityAnswer,
    no_catalog_library: bool,
    observed_at: Option<u64>,
    now: u64,
) -> SweepVerdict {
    match answer {
        IdentityAnswer::Incomplete | IdentityAnswer::EmptyNotCountedZero => {
            SweepVerdict::ForgetEmpty
        }
        IdentityAnswer::Listed => SweepVerdict::Delete,
        IdentityAnswer::EmptyCountingZero if no_catalog_library => SweepVerdict::Delete,
        IdentityAnswer::EmptyCountingZero => match observed_at.and_then(|at| now.checked_sub(at)) {
            Some(age) if age < MIN_CONFIRM_GAP.as_secs() => SweepVerdict::Wait,
            Some(age) if age <= MAX_CONFIRM_AGE.as_secs() => SweepVerdict::Delete,
            _ => SweepVerdict::ObserveEmpty(now),
        },
    }
}

fn apply_sweep_verdict(
    library: &Library,
    verdict: SweepVerdict,
    seen: &HashSet<String>,
) -> Result<Option<LibraryChangeBatch>, ApiError> {
    let written = match verdict {
        SweepVerdict::Wait => return Ok(None),
        SweepVerdict::ObserveEmpty(at) => library
            .set_meta(META_EMPTY_SWEEP_AT, &at.to_string())
            .map(|()| None),
        SweepVerdict::ForgetEmpty => library.set_meta(META_EMPTY_SWEEP_AT, "").map(|()| None),
        SweepVerdict::Delete => library
            .retain_ids_setting_meta(seen, &[(META_SWEEP_OWED, "0"), (META_EMPTY_SWEEP_AT, "")])
            .map(Some),
    };
    written.map_err(|error| storage_error(&error))
}

/// One identity-only pass over the library that both mirrors watch state and
/// collects the ids the server still reports.
///
/// These used to be two sweeps requesting the identical pages on different
/// schedules, which meant deletions were only noticed once a day. Folding them
/// together makes deletions as fresh as watch state for no extra requests.
fn walk_identity(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    control: Option<&SyncHandle>,
    changes: &mut LibraryChangeBatch,
) -> Result<IdentityWalk, ApiError> {
    let mut offset = 0;
    let mut refreshed = 0;
    let mut seen = HashSet::new();
    let mut pages = 0;
    let mut counted_zero = false;
    let truncated = loop {
        cancelled(control, session, scope)?;
        if pages >= MAX_IDENTITY_PAGES {
            break true;
        }
        let page = items::fetch_identity_page(
            scope.client(),
            scope.user_id(),
            offset,
            IDENTITY_PAGE_SIZE,
        )?;
        if page.items.is_empty() {
            counted_zero = page.total_record_count == Some(0);
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
        let changed = commit(session, scope, || {
            library
                .apply_user_data(&records)
                .map_err(|error| storage_error(&error))
        })?;
        refreshed += changed.item_ids.len();
        changes.merge(changed);
        offset += page.items.len() as i64;
        pages += 1;
        if (page.items.len() as i64) < IDENTITY_PAGE_SIZE {
            break false;
        }
    };
    let answer = if truncated {
        // `seen` is what decides which items still exist, so a partial one must
        // never reach `retain_ids_setting_meta` — everything past the cap would be deleted.
        tracing::warn!(
            target: "library.sync",
            pages,
            offset,
            "stopped the identity sweep at the page cap; skipping the deletion pass"
        );
        IdentityAnswer::Incomplete
    } else if !seen.is_empty() {
        IdentityAnswer::Listed
    } else if counted_zero {
        IdentityAnswer::EmptyCountingZero
    } else {
        IdentityAnswer::EmptyNotCountedZero
    };
    Ok(IdentityWalk {
        refreshed,
        seen,
        answer,
    })
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

#[cfg(test)]
mod tests;
