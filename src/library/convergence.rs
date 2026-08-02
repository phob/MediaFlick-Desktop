//! Durable metadata convergence for Jellyfin's asynchronous enrichment window.
//!
//! Correctness is entirely the persisted REST queue below: no event has replay,
//! and a desktop can be offline for days. A Jellyfin WebSocket accelerator is
//! intentionally deferred until the project has a shared socket runtime that
//! can provide authenticated header transport, application keepalive, robust
//! proxy URL conversion, and session-switch cancellation as one unit. Adding a
//! partial socket here would weaken security without improving correctness.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::jellyfin::api::ApiError;
use crate::jellyfin::api::items;
use crate::jellyfin::api::model::BaseItemDto;
use crate::jellyfin::session::Session;

use super::model::{MetadataQuality, metadata_convergence_initial_delay, metadata_quality};
use super::{ItemRecord, Library, UserDataRecord, now_unix, upsert_item, upsert_user_data};

pub const REQUEST_BATCH_SIZE: usize = 20;
pub const MAX_BATCHES_PER_WAKE: usize = 2;
pub const MAX_ITEMS_PER_WAKE: usize = REQUEST_BATCH_SIZE * MAX_BATCHES_PER_WAKE;
pub const DRAIN_COOLDOWN_SECS: u64 = 10;

const DORMANCY_MIN_UNCHANGED: i64 = 8;
const DORMANCY_MIN_AGE_SECS: i64 = 14 * 24 * 60 * 60;
const META_LAST_RUN: &str = "convergence.last_run";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryChangeBatch {
    pub item_ids: Vec<String>,
    pub context_ids: Vec<String>,
}

impl LibraryChangeBatch {
    pub fn is_empty(&self) -> bool {
        self.item_ids.is_empty()
    }

    pub fn merge(&mut self, other: Self) {
        self.item_ids.extend(other.item_ids);
        self.context_ids.extend(other.context_ids);
        normalize_ids(&mut self.item_ids);
        normalize_ids(&mut self.context_ids);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvergenceRunReport {
    pub selected: usize,
    pub requests: usize,
    pub completed: usize,
    pub progressed: usize,
    pub unchanged: usize,
    pub dormant: usize,
    pub failed: usize,
    pub elapsed_ms: u64,
    #[serde(skip)]
    pub changes: LibraryChangeBatch,
    #[serde(skip)]
    pub more_due: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvergenceDiagnostics {
    pub pending: i64,
    pub due: i64,
    pub dormant: i64,
    pub oldest_pending_at: Option<i64>,
    pub next_due_at: Option<i64>,
    pub last_run: Option<ConvergenceRunReport>,
}

#[derive(Debug, Clone)]
struct StateRow {
    state: String,
    quality_score: i64,
    first_observed_at: i64,
    last_progress_at: i64,
    next_attempt_at: Option<i64>,
    unchanged_count: i64,
    successful_unchanged: i64,
    signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservationSource {
    Authoritative,
    Convergence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservationOutcome {
    Complete,
    Progressed,
    Unchanged,
    Dormant,
}

impl Library {
    /// Ingests authoritative full DTOs and observes convergence in the same
    /// SQLite transaction. Metadata-identical DTOs skip the item rewrite.
    pub fn ingest_page(&self, dtos: &[BaseItemDto]) -> rusqlite::Result<LibraryChangeBatch> {
        self.ingest_page_at(dtos, now_unix())
    }

    pub(crate) fn ingest_page_at(
        &self,
        dtos: &[BaseItemDto],
        now: i64,
    ) -> rusqlite::Result<LibraryChangeBatch> {
        self.db.with_transaction(|transaction| {
            let mut changes = LibraryChangeBatch::default();
            for dto in dtos {
                if dto.id.trim().is_empty() {
                    continue;
                }
                let quality = metadata_quality(dto);
                if !quality.supported {
                    continue;
                }
                let existed: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM items WHERE jellyfin_id = ?1)",
                    [&dto.id],
                    |row| row.get(0),
                )?;
                // The queue row has an immediate foreign key to `items`, so a
                // first observation establishes the cache row before its state.
                if !existed {
                    upsert_item(transaction, &ItemRecord::from_dto(dto), false)?;
                }
                let (outcome, write_metadata) = observe(
                    transaction,
                    dto,
                    &quality,
                    now,
                    ObservationSource::Authoritative,
                )?;
                if write_metadata && existed {
                    upsert_item(transaction, &ItemRecord::from_dto(dto), false)?;
                }
                if write_metadata {
                    record_change(&mut changes, dto);
                }
                if let Some(user_data) = &dto.user_data {
                    upsert_user_data(transaction, &UserDataRecord::from_dto(&dto.id, user_data))?;
                }
                tracing::trace!(
                    target: "library.convergence",
                    item_id = dto.id,
                    ?outcome,
                    score = quality.score,
                    missing = quality.missing,
                    "observed authoritative metadata quality"
                );
            }
            Ok(changes)
        })
    }

    pub(crate) fn due_convergence_ids(
        &self,
        now: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<String>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT jellyfin_id FROM metadata_convergence
                 WHERE state = 'pending' AND next_attempt_at <= ?1
                 ORDER BY next_attempt_at ASC, first_observed_at ASC, jellyfin_id ASC
                 LIMIT ?2",
            )?;
            statement
                .query_map(
                    params![now, i64::try_from(limit).unwrap_or(i64::MAX)],
                    |row| row.get(0),
                )?
                .collect()
        })
    }

    pub fn convergence_diagnostics(&self) -> ConvergenceDiagnostics {
        let now = now_unix();
        let mut diagnostics = self
            .db
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT
                         count(*) FILTER (WHERE state = 'pending'),
                         count(*) FILTER (WHERE state = 'pending' AND next_attempt_at <= ?1),
                         count(*) FILTER (WHERE state = 'dormant'),
                         min(first_observed_at) FILTER (WHERE state = 'pending'),
                         min(next_attempt_at) FILTER (WHERE state = 'pending')
                     FROM metadata_convergence",
                    [now],
                    |row| {
                        Ok(ConvergenceDiagnostics {
                            pending: row.get(0)?,
                            due: row.get(1)?,
                            dormant: row.get(2)?,
                            oldest_pending_at: row.get(3)?,
                            next_due_at: row.get(4)?,
                            last_run: None,
                        })
                    },
                )
            })
            .unwrap_or_default();
        diagnostics.last_run = self
            .meta(META_LAST_RUN)
            .and_then(|value| serde_json::from_str(&value).ok());
        diagnostics
    }

    fn apply_convergence_response(
        &self,
        selected: &[String],
        dtos: Vec<BaseItemDto>,
        now: i64,
    ) -> rusqlite::Result<ConvergenceRunReport> {
        let selected_set = selected.iter().map(String::as_str).collect::<HashSet<_>>();
        let mut by_id = HashMap::new();
        for dto in dtos {
            if selected_set.contains(dto.id.as_str()) {
                by_id.entry(dto.id.clone()).or_insert(dto);
            }
        }
        self.db.with_transaction(|transaction| {
            let mut report = ConvergenceRunReport::default();
            for item_id in selected {
                let Some(dto) = by_id.remove(item_id) else {
                    mark_error(transaction, item_id, now)?;
                    report.failed += 1;
                    continue;
                };
                let quality = metadata_quality(&dto);
                if !quality.supported
                    || dto
                        .name
                        .as_deref()
                        .is_none_or(|name| name.trim().is_empty())
                {
                    mark_error(transaction, item_id, now)?;
                    report.failed += 1;
                    continue;
                }
                let (outcome, write_metadata) = observe(
                    transaction,
                    &dto,
                    &quality,
                    now,
                    ObservationSource::Convergence,
                )?;
                match outcome {
                    ObservationOutcome::Complete => report.completed += 1,
                    ObservationOutcome::Progressed => report.progressed += 1,
                    ObservationOutcome::Unchanged => report.unchanged += 1,
                    ObservationOutcome::Dormant => {
                        report.unchanged += 1;
                        report.dormant += 1;
                    }
                }
                if write_metadata {
                    // Exact-ID reads occasionally omit fields. Only equal or
                    // better observations reach this branch, and missing
                    // values merge instead of erasing richer cached metadata.
                    upsert_item(transaction, &ItemRecord::from_dto(&dto), true)?;
                    record_change(&mut report.changes, &dto);
                }
                if let Some(user_data) = &dto.user_data {
                    upsert_user_data(transaction, &UserDataRecord::from_dto(&dto.id, user_data))?;
                }
            }
            Ok(report)
        })
    }

    fn mark_convergence_batch_error(&self, item_ids: &[String], now: i64) -> rusqlite::Result<()> {
        self.db.with_transaction(|transaction| {
            for item_id in item_ids {
                mark_error(transaction, item_id, now)?;
            }
            Ok(())
        })
    }
}

/// Runs at most two serial logical GETs. The Jellyfin client's own retry/auth
/// behavior remains intact; this layer only advances durable per-item state.
pub(crate) fn process_due(
    library: &Library,
    session: &Session,
) -> Result<ConvergenceRunReport, ApiError> {
    let started = Instant::now();
    let now = now_unix();
    let (client, user_id) = session.client_and_user()?;
    let selected = library
        .due_convergence_ids(now, MAX_ITEMS_PER_WAKE)
        .map_err(storage_error)?;
    let mut report = ConvergenceRunReport {
        selected: selected.len(),
        ..ConvergenceRunReport::default()
    };

    for chunk in selected.chunks(REQUEST_BATCH_SIZE) {
        report.requests += 1;
        let ids = chunk.to_vec();
        match items::fetch_items(&client, &user_id, &ids) {
            Ok(response) => {
                let batch = library
                    .apply_convergence_response(&ids, response.items, now)
                    .map_err(storage_error)?;
                report.completed += batch.completed;
                report.progressed += batch.progressed;
                report.unchanged += batch.unchanged;
                report.dormant += batch.dormant;
                report.failed += batch.failed;
                report.changes.merge(batch.changes);
            }
            Err(error) => {
                library
                    .mark_convergence_batch_error(&ids, now)
                    .map_err(storage_error)?;
                report.failed += ids.len();
                session.note_error(&error);
                tracing::debug!(
                    target: "library.convergence",
                    selected = ids.len(),
                    "convergence REST batch failed: {error}"
                );
                if error == ApiError::Unauthorized {
                    break;
                }
            }
        }
    }
    report.more_due = !library
        .due_convergence_ids(now, 1)
        .map_err(storage_error)?
        .is_empty();
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    let _ = library.set_meta(
        META_LAST_RUN,
        &serde_json::to_string(&report).unwrap_or_default(),
    );
    tracing::info!(
        target: "library.convergence",
        selected = report.selected,
        requests = report.requests,
        completed = report.completed,
        progressed = report.progressed,
        unchanged = report.unchanged,
        dormant = report.dormant,
        failed = report.failed,
        elapsed_ms = report.elapsed_ms,
        "metadata convergence wake finished"
    );
    Ok(report)
}

fn observe(
    connection: &rusqlite::Connection,
    dto: &BaseItemDto,
    quality: &MetadataQuality,
    now: i64,
    source: ObservationSource,
) -> rusqlite::Result<(ObservationOutcome, bool)> {
    let previous = state_row(connection, &dto.id)?;
    let attempt_increment = i64::from(source == ObservationSource::Convergence);
    let changed = previous
        .as_ref()
        .is_none_or(|state| state.signature != quality.signature);
    let score_not_lower = previous
        .as_ref()
        .is_none_or(|state| i64::from(quality.score) >= state.quality_score);
    let write_metadata = previous.is_none()
        || (source == ObservationSource::Authoritative && changed)
        || (source == ObservationSource::Convergence && changed && score_not_lower);

    let first = previous
        .as_ref()
        .map_or(now, |state| state.first_observed_at);
    let old_progress = previous
        .as_ref()
        .map_or(now, |state| state.last_progress_at);
    let old_unchanged = previous.as_ref().map_or(0, |state| state.unchanged_count);
    let old_successful = previous
        .as_ref()
        .map_or(0, |state| state.successful_unchanged);
    let same_dormant = previous
        .as_ref()
        .is_some_and(|state| state.state == "dormant" && !changed);
    let deferred_by_backoff = source == ObservationSource::Authoritative
        && !changed
        && previous.as_ref().is_some_and(|state| {
            state.state == "pending" && state.next_attempt_at.is_some_and(|next| next > now)
        });

    let (state, next, unchanged, successful, last_progress, outcome) = if quality.complete {
        (
            "complete",
            None,
            0,
            old_successful,
            if changed { now } else { old_progress },
            ObservationOutcome::Complete,
        )
    } else if same_dormant {
        (
            "dormant",
            None,
            old_unchanged,
            old_successful,
            old_progress,
            ObservationOutcome::Dormant,
        )
    } else if deferred_by_backoff {
        (
            "pending",
            previous.as_ref().and_then(|state| state.next_attempt_at),
            old_unchanged,
            old_successful,
            old_progress,
            ObservationOutcome::Unchanged,
        )
    } else if changed && (source == ObservationSource::Authoritative || score_not_lower) {
        (
            "pending",
            Some(now + progress_delay(&dto.id)),
            0,
            0,
            now,
            ObservationOutcome::Progressed,
        )
    } else {
        let unchanged = old_unchanged + 1;
        let successful = old_successful + 1;
        if should_dormant(first, old_progress, successful, now) {
            (
                "dormant",
                None,
                unchanged,
                successful,
                old_progress,
                ObservationOutcome::Dormant,
            )
        } else {
            (
                "pending",
                Some(now + unchanged_delay(unchanged)),
                unchanged,
                successful,
                old_progress,
                ObservationOutcome::Unchanged,
            )
        }
    };

    connection.execute(
        "INSERT INTO metadata_convergence (
             jellyfin_id, state, missing_quality, quality_score,
             first_observed_at, last_observed_at, last_progress_at, next_attempt_at,
             attempt_count, unchanged_count, error_count, successful_unchanged, signature
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12)
         ON CONFLICT(jellyfin_id) DO UPDATE SET
             state = excluded.state,
             missing_quality = CASE WHEN ?13 THEN excluded.missing_quality
                                    ELSE metadata_convergence.missing_quality END,
             quality_score = CASE WHEN ?13 THEN excluded.quality_score
                                  ELSE metadata_convergence.quality_score END,
             last_observed_at = excluded.last_observed_at,
             last_progress_at = excluded.last_progress_at,
             next_attempt_at = excluded.next_attempt_at,
             attempt_count = metadata_convergence.attempt_count + ?9,
             unchanged_count = excluded.unchanged_count,
             error_count = CASE WHEN ?14 THEN 0 ELSE metadata_convergence.error_count END,
             successful_unchanged = excluded.successful_unchanged,
             signature = CASE WHEN ?13 THEN excluded.signature
                              ELSE metadata_convergence.signature END",
        params![
            dto.id,
            state,
            i64::from(quality.missing),
            i64::from(quality.score),
            first,
            now,
            last_progress,
            next,
            attempt_increment,
            unchanged,
            successful,
            quality.signature,
            write_metadata,
            !deferred_by_backoff,
        ],
    )?;
    Ok((outcome, write_metadata))
}

fn state_row(
    connection: &rusqlite::Connection,
    item_id: &str,
) -> rusqlite::Result<Option<StateRow>> {
    connection
        .query_row(
            "SELECT state, quality_score, first_observed_at, last_progress_at,
                    next_attempt_at, unchanged_count, successful_unchanged, signature
             FROM metadata_convergence WHERE jellyfin_id = ?1",
            [item_id],
            |row| {
                Ok(StateRow {
                    state: row.get(0)?,
                    quality_score: row.get(1)?,
                    first_observed_at: row.get(2)?,
                    last_progress_at: row.get(3)?,
                    next_attempt_at: row.get(4)?,
                    unchanged_count: row.get(5)?,
                    successful_unchanged: row.get(6)?,
                    signature: row.get(7)?,
                })
            },
        )
        .optional()
}

fn mark_error(connection: &rusqlite::Connection, item_id: &str, now: i64) -> rusqlite::Result<()> {
    let errors: i64 = connection
        .query_row(
            "SELECT error_count FROM metadata_convergence
             WHERE jellyfin_id = ?1 AND state = 'pending'",
            [item_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or_default()
        + 1;
    connection.execute(
        "UPDATE metadata_convergence
         SET attempt_count = attempt_count + 1,
             error_count = ?2,
             next_attempt_at = ?3
         WHERE jellyfin_id = ?1 AND state = 'pending'",
        params![item_id, errors, now + error_delay(errors)],
    )?;
    Ok(())
}

fn progress_delay(item_id: &str) -> i64 {
    metadata_convergence_initial_delay(item_id)
}

fn unchanged_delay(unchanged: i64) -> i64 {
    const STEPS: [i64; 8] = [
        300,
        900,
        3_600,
        6 * 3_600,
        24 * 3_600,
        3 * 86_400,
        7 * 86_400,
        7 * 86_400,
    ];
    STEPS[usize::try_from(unchanged.saturating_sub(1))
        .unwrap_or(usize::MAX)
        .min(STEPS.len() - 1)]
}

fn error_delay(errors: i64) -> i64 {
    const STEPS: [i64; 6] = [300, 900, 3_600, 6 * 3_600, 24 * 3_600, 24 * 3_600];
    STEPS[usize::try_from(errors.saturating_sub(1))
        .unwrap_or(usize::MAX)
        .min(STEPS.len() - 1)]
}

fn should_dormant(first: i64, last_progress: i64, successful: i64, now: i64) -> bool {
    successful >= DORMANCY_MIN_UNCHANGED
        && now.saturating_sub(first) >= DORMANCY_MIN_AGE_SECS
        && now.saturating_sub(last_progress) >= DORMANCY_MIN_AGE_SECS
}

fn record_change(changes: &mut LibraryChangeBatch, dto: &BaseItemDto) {
    changes.item_ids.push(dto.id.clone());
    for context in [&dto.parent_id, &dto.series_id, &dto.season_id] {
        if let Some(id) = context.as_deref().filter(|id| !id.trim().is_empty()) {
            changes.context_ids.push(id.to_string());
        }
    }
    normalize_ids(&mut changes.item_ids);
    normalize_ids(&mut changes.context_ids);
}

fn normalize_ids(ids: &mut Vec<String>) {
    ids.sort_unstable();
    ids.dedup();
}

fn storage_error(error: rusqlite::Error) -> ApiError {
    ApiError::Decode(format!("library storage failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        DORMANCY_MIN_AGE_SECS, Library, error_delay, process_due, should_dormant, unchanged_delay,
    };
    use crate::jellyfin::api::model::BaseItemDto;
    use crate::jellyfin::session::Session;
    use crate::library::StoredCredentials;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;

    fn dto(json: &str) -> BaseItemDto {
        serde_json::from_str(json).expect("dto")
    }

    #[test]
    fn retry_steps_are_bounded_and_errors_do_not_feed_dormancy() {
        assert_eq!(unchanged_delay(1), 300);
        assert_eq!(unchanged_delay(4), 6 * 3_600);
        assert_eq!(unchanged_delay(100), 7 * 86_400);
        assert_eq!(error_delay(1), 300);
        assert_eq!(error_delay(100), 86_400);
        assert!(!should_dormant(0, 0, 7, DORMANCY_MIN_AGE_SECS * 2));
        assert!(should_dormant(0, 0, 8, DORMANCY_MIN_AGE_SECS));
    }

    #[test]
    fn restart_state_is_persisted_and_fairly_ordered_by_due_time() {
        let library = Library::open_in_memory().expect("library");
        library
            .ingest_page_at(
                &[
                    dto(r#"{"Id":"later","Name":"Later","Type":"Movie"}"#),
                    dto(r#"{"Id":"first","Name":"First","Type":"Movie"}"#),
                ],
                1_000,
            )
            .expect("ingest");
        library
            .db
            .with_connection(|connection| {
                connection.execute(
                    "UPDATE metadata_convergence SET next_attempt_at = 900 WHERE jellyfin_id = 'first'",
                    [],
                )?;
                connection.execute(
                    "UPDATE metadata_convergence SET next_attempt_at = 950 WHERE jellyfin_id = 'later'",
                    [],
                )?;
                Ok(())
            })
            .expect("schedule");
        assert_eq!(
            library.due_convergence_ids(1_000, 20).expect("due"),
            vec!["first", "later"]
        );
        assert_eq!(library.convergence_diagnostics().pending, 2);
    }

    #[test]
    fn sparse_items_become_dormant_only_after_successful_time_and_rearm_on_change() {
        let library = Library::open_in_memory().expect("library");
        let sparse = dto(r#"{"Id":"s","Name":"Sparse","Type":"Series"}"#);
        library
            .ingest_page_at(std::slice::from_ref(&sparse), 0)
            .expect("seed");
        let mut observation_at = DORMANCY_MIN_AGE_SECS;
        for _ in 0..8 {
            library
                .ingest_page_at(std::slice::from_ref(&sparse), observation_at)
                .expect("unchanged");
            observation_at = library
                .convergence_diagnostics()
                .next_due_at
                .unwrap_or(observation_at);
        }
        assert_eq!(library.convergence_diagnostics().dormant, 1);

        let changed = dto(r#"{"Id":"s","Name":"Sparse","Type":"Series","Genres":["Drama"]}"#);
        library
            .ingest_page_at(&[changed], DORMANCY_MIN_AGE_SECS * 2)
            .expect("rearm");
        let diagnostics = library.convergence_diagnostics();
        assert_eq!(diagnostics.dormant, 0);
        assert_eq!(diagnostics.pending, 1);
    }

    #[test]
    fn identical_observations_skip_cache_writes_but_progress_changes_write() {
        let library = Library::open_in_memory().expect("library");
        let sparse = dto(r#"{"Id":"s","Name":"Sparse","Type":"Season","SeriesId":"x"}"#);
        assert_eq!(
            library
                .ingest_page_at(std::slice::from_ref(&sparse), 1)
                .unwrap()
                .item_ids,
            vec!["s"]
        );
        assert!(library.ingest_page_at(&[sparse], 2).unwrap().is_empty());
        let richer = dto(
            r#"{"Id":"s","Name":"Sparse","Type":"Season","SeriesId":"x","IndexNumber":0,"ChildCount":1}"#,
        );
        assert_eq!(
            library.ingest_page_at(&[richer], 3).unwrap().item_ids,
            vec!["s"]
        );
    }

    #[test]
    fn ordinary_or_manual_ingestion_respects_the_items_persisted_backoff() {
        let library = Library::open_in_memory().expect("library");
        let sparse = dto(r#"{"Id":"m","Name":"Sparse","Type":"Movie"}"#);
        library
            .ingest_page_at(std::slice::from_ref(&sparse), 1_000)
            .expect("seed");
        for now in 1_001..1_010 {
            library
                .ingest_page_at(std::slice::from_ref(&sparse), now)
                .expect("manual observation");
        }
        let (unchanged, successful, next): (i64, i64, i64) = library
            .db
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT unchanged_count, successful_unchanged, next_attempt_at
                     FROM metadata_convergence WHERE jellyfin_id = 'm'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
            })
            .expect("state");
        assert_eq!((unchanged, successful), (0, 0));
        assert!(next >= 1_120);
        assert!(library.due_convergence_ids(1_010, 1).unwrap().is_empty());
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

    fn mock_jellyfin(responses: Vec<(u16, String)>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let mut targets = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 2048];
                loop {
                    let read = stream.read(&mut buffer).expect("read");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8(request).expect("request");
                targets.push(
                    request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .expect("target")
                        .to_string(),
                );
                let reason = if status == 200 { "OK" } else { "Unauthorized" };
                write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .expect("response");
            }
            targets
        });
        (format!("http://{address}"), server)
    }

    #[test]
    fn thirty_due_items_use_two_serial_get_batches_of_at_most_twenty() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let dtos = (0..30)
            .map(|index| {
                dto(&format!(
                    r#"{{"Id":"item-{index:02}","Name":"Sparse","Type":"Movie"}}"#
                ))
            })
            .collect::<Vec<_>>();
        library.ingest_page_at(&dtos, 1).expect("seed");
        library
            .db
            .with_connection(|connection| {
                connection.execute("UPDATE metadata_convergence SET next_attempt_at = 0", [])?;
                Ok(())
            })
            .expect("due");
        let (server_url, server) = mock_jellyfin(vec![
            (200, r#"{"Items":[]}"#.to_string()),
            (200, r#"{"Items":[]}"#.to_string()),
        ]);
        let session = authenticated_session(&library, &server_url);

        let report = process_due(&library, &session).expect("convergence");

        assert_eq!(report.selected, 30);
        assert_eq!(report.requests, 2);
        assert_eq!(report.failed, 30);
        let targets = server.join().expect("server");
        assert_eq!(targets.len(), 2);
        let counts = targets
            .iter()
            .map(|target| {
                target
                    .split("ids=")
                    .nth(1)
                    .and_then(|value| value.split('&').next())
                    .expect("ids")
                    .split("%2C")
                    .count()
            })
            .collect::<Vec<_>>();
        assert_eq!(counts, vec![20, 10]);
        assert!(targets.iter().all(|target| target.starts_with("/Items?")));
    }

    #[test]
    fn authorization_rejection_stops_before_the_second_batch() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let dtos = (0..25)
            .map(|index| {
                dto(&format!(
                    r#"{{"Id":"item-{index:02}","Name":"Sparse","Type":"Series"}}"#
                ))
            })
            .collect::<Vec<_>>();
        library.ingest_page_at(&dtos, 1).expect("seed");
        library
            .db
            .with_connection(|connection| {
                connection.execute("UPDATE metadata_convergence SET next_attempt_at = 0", [])?;
                Ok(())
            })
            .expect("due");
        let (server_url, server) = mock_jellyfin(vec![(401, "{}".to_string())]);
        let session = authenticated_session(&library, &server_url);

        let report = process_due(&library, &session).expect("report");

        assert_eq!(report.requests, 1);
        assert_eq!(report.failed, 20);
        assert!(!session.is_authenticated());
        assert_eq!(server.join().expect("server").len(), 1);
    }

    #[test]
    fn convergence_progress_merges_without_erasing_richer_cached_fields() {
        let library = Library::open_in_memory().expect("library");
        library
            .ingest_page_at(
                &[dto(
                    r#"{"Id":"m","Name":"Cached","Type":"Movie","ProductionYear":1988,
                        "RunTimeTicks":123,"DateCreated":"2020-01-01"}"#,
                )],
                1,
            )
            .expect("seed");
        let response =
            dto(r#"{"Id":"m","Name":"Server","Type":"Movie","ImageTags":{"Primary":"poster"}}"#);
        let report = library
            .apply_convergence_response(&["m".to_string()], vec![response], 2)
            .expect("apply");
        assert_eq!(report.progressed, 1);
        let cached = library.item("m").unwrap().unwrap();
        assert_eq!(cached["name"], "Server");
        assert_eq!(cached["year"], 1988);
        assert_eq!(cached["runtimeTicks"], 123);
        assert_eq!(cached["dateCreated"], "2020-01-01");
        assert_eq!(cached["primaryImageTag"], "poster");
    }

    #[test]
    fn a_rich_exact_id_observation_completes_a_v8_backfill_without_erasing_cache_fields() {
        let path = std::env::temp_dir().join(format!(
            "mediaflick-v8-convergence-{}.sqlite",
            crate::app::ids::random_hex(8)
        ));
        {
            let library = Library::open(&path).expect("create library");
            library
                .db
                .with_connection(|connection| {
                    connection.execute(
                        "INSERT INTO items (
                             jellyfin_id, kind, name, year, runtime_ticks, tmdb_id,
                             date_created, metadata_repair_pending, synced_at
                         ) VALUES ('blob', 'Movie', 'The Blob', 1988, 123, '9599',
                                   '2020-01-01', 0, 1)",
                        [],
                    )?;
                    connection.pragma_update(None, "user_version", 7)?;
                    Ok(())
                })
                .expect("seed v7 row");
        }

        let library = Library::open(&path).expect("migrate library");
        let queued: (String, i64) = library
            .db
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT state, missing_quality FROM metadata_convergence
                     WHERE jellyfin_id = 'blob'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .expect("backfilled state");
        assert_eq!(queued, ("pending".to_string(), 6));

        let response = dto(r#"{"Id":"blob","Name":"The Blob (1988)","Type":"Movie",
                "ProductionYear":1988,"Overview":"Rich server description",
                "ProviderIds":{"Tmdb":"9599"},"ImageTags":{"Primary":"poster"}}"#);
        let report = library
            .apply_convergence_response(&["blob".to_string()], vec![response], 2)
            .expect("apply rich response");
        assert_eq!(report.completed, 1);
        let cached = library.item("blob").unwrap().unwrap();
        assert_eq!(cached["name"], "The Blob (1988)");
        assert_eq!(cached["year"], 1988);
        assert_eq!(cached["runtimeTicks"], 123);
        assert_eq!(cached["dateCreated"], "2020-01-01");
        assert_eq!(cached["primaryImageTag"], "poster");
        let state: (String, Option<i64>) = library
            .db
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT state, next_attempt_at FROM metadata_convergence
                     WHERE jellyfin_id = 'blob'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .expect("complete state");
        assert_eq!(state, ("complete".to_string(), None));

        drop(library);
        let _ = std::fs::remove_file(path);
    }
}
