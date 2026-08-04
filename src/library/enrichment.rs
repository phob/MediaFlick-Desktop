//! Durable finite rich-metadata enrichment after the lightweight catalog fill.
//!
//! Catalog pages stay small and serial. Every committed row is queued here in
//! the same SQLite transaction, then exact-id batches add prose, cast, studios,
//! tags, ratings, and technical streams after browsing is already available.
//! Queue time and backoff are wall-clock values, so a restart resumes rather
//! than beginning the backfill again.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::jellyfin::api::ApiError;
use crate::jellyfin::api::items;
use crate::jellyfin::api::model::BaseItemDto;
use crate::jellyfin::session::Session;

use super::{Library, LibraryChangeBatch, now_unix};

pub const REQUEST_BATCH_SIZE: usize = 20;
pub const MAX_BATCHES_PER_WAKE: usize = 2;
pub const MAX_ITEMS_PER_WAKE: usize = REQUEST_BATCH_SIZE * MAX_BATCHES_PER_WAKE;
pub const DRAIN_COOLDOWN_SECS: u64 = 2;

const META_LAST_RUN: &str = "enrichment.last_run";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentRunReport {
    pub selected: usize,
    pub requests: usize,
    pub completed: usize,
    pub failed: usize,
    pub elapsed_ms: u64,
    pub last_error: Option<String>,
    pub retry_at: Option<i64>,
    #[serde(skip)]
    pub changes: LibraryChangeBatch,
    #[serde(skip)]
    pub more_due: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentDiagnostics {
    pub total: i64,
    pub completed: i64,
    pub pending: i64,
    pub failed: i64,
    pub due: i64,
    pub next_due_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_run: Option<EnrichmentRunReport>,
}

impl Library {
    pub(crate) fn due_enrichment_ids(
        &self,
        now: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<String>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT jellyfin_id FROM catalog_enrichment
                 WHERE state = 'pending' AND next_attempt_at <= ?1
                 ORDER BY next_attempt_at ASC, jellyfin_id ASC
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

    pub fn enrichment_diagnostics(&self) -> EnrichmentDiagnostics {
        let now = now_unix();
        let mut diagnostics = self
            .db
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*),
                            count(*) FILTER (WHERE state = 'complete'),
                            count(*) FILTER (WHERE state = 'pending'),
                            count(*) FILTER (WHERE state = 'pending' AND error_count > 0),
                            count(*) FILTER (WHERE state = 'pending' AND next_attempt_at <= ?1),
                            min(next_attempt_at) FILTER (WHERE state = 'pending')
                     FROM catalog_enrichment",
                    [now],
                    |row| {
                        Ok(EnrichmentDiagnostics {
                            total: row.get(0)?,
                            completed: row.get(1)?,
                            pending: row.get(2)?,
                            failed: row.get(3)?,
                            due: row.get(4)?,
                            next_due_at: row.get(5)?,
                            ..EnrichmentDiagnostics::default()
                        })
                    },
                )
            })
            .unwrap_or_default();
        diagnostics.last_error = self
            .db
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT last_error FROM catalog_enrichment
                         WHERE state = 'pending' AND last_error IS NOT NULL
                         ORDER BY updated_at DESC LIMIT 1",
                        [],
                        |row| row.get(0),
                    )
                    .optional()
            })
            .ok()
            .flatten();
        diagnostics.last_run = self
            .meta(META_LAST_RUN)
            .and_then(|value| serde_json::from_str(&value).ok());
        diagnostics
    }

    /// Moves one opened item ahead of ordinary due rows. `0` is older than any
    /// wall-clock schedule and remains durable if the app exits before the
    /// worker can consume the request. A failed row keeps its server/local
    /// backoff so repeatedly opening it cannot bypass `Retry-After`.
    pub(crate) fn prioritize_enrichment(&self, item_id: &str) -> rusqlite::Result<bool> {
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE catalog_enrichment SET next_attempt_at = 0
                     WHERE jellyfin_id = ?1 AND state = 'pending' AND error_count = 0",
                    [item_id],
                )
                .map(|changed| changed > 0)
        })
    }

    fn mark_enrichment_complete(&self, item_ids: &[String], now: i64) -> rusqlite::Result<()> {
        self.db.with_transaction(|transaction| {
            for item_id in item_ids {
                transaction.execute(
                    "UPDATE catalog_enrichment
                     SET state = 'complete', next_attempt_at = NULL,
                         attempt_count = attempt_count + 1, error_count = 0,
                         last_error = NULL, updated_at = ?2
                     WHERE jellyfin_id = ?1",
                    params![item_id, now],
                )?;
            }
            Ok(())
        })
    }

    fn mark_enrichment_error(
        &self,
        item_ids: &[String],
        error: &ApiError,
        now: i64,
    ) -> rusqlite::Result<Option<i64>> {
        let message = bounded_error(error);
        self.db.with_transaction(|transaction| {
            let mut earliest_retry: Option<i64> = None;
            for item_id in item_ids {
                let Some(previous_errors) = transaction
                    .query_row(
                        "SELECT error_count FROM catalog_enrichment
                         WHERE jellyfin_id = ?1 AND state = 'pending'",
                        [item_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?
                else {
                    continue;
                };
                let errors = previous_errors + 1;
                let retry_at = now + retry_delay(errors, error);
                earliest_retry = Some(earliest_retry.map_or(retry_at, |old| old.min(retry_at)));
                transaction.execute(
                    "UPDATE catalog_enrichment
                     SET next_attempt_at = ?2, attempt_count = attempt_count + 1,
                         error_count = ?3, last_error = ?4, updated_at = ?5
                     WHERE jellyfin_id = ?1 AND state = 'pending'",
                    params![item_id, retry_at, errors, message, now],
                )?;
            }
            Ok(earliest_retry)
        })
    }
}

/// Runs at most two serial exact-id requests. There is intentionally no broad
/// item-level concurrency; the Jellyfin client's retry and Retry-After policy
/// applies to each logical request before this durable queue advances.
pub(crate) fn process_due(
    library: &Library,
    session: &Session,
    cancelled: impl Fn() -> bool,
) -> Result<EnrichmentRunReport, ApiError> {
    let started = Instant::now();
    let now = now_unix();
    let (client, user_id) = session.client_and_user()?;
    let selected = library
        .due_enrichment_ids(now, MAX_ITEMS_PER_WAKE)
        .map_err(storage_error)?;
    let mut report = EnrichmentRunReport {
        selected: selected.len(),
        ..EnrichmentRunReport::default()
    };

    for chunk in selected.chunks(REQUEST_BATCH_SIZE) {
        if cancelled() {
            break;
        }
        report.requests += 1;
        let ids = chunk.to_vec();
        match items::fetch_items(&client, &user_id, &ids) {
            Ok(response) => {
                let selected_ids = ids.iter().map(String::as_str).collect::<HashSet<_>>();
                let mut by_id = HashMap::new();
                for dto in response.items {
                    if selected_ids.contains(dto.id.as_str()) && usable_enrichment(&dto) {
                        by_id.entry(dto.id.clone()).or_insert(dto);
                    }
                }
                let received = ids
                    .iter()
                    .filter_map(|id| by_id.remove(id))
                    .collect::<Vec<_>>();
                let completed_ids = received
                    .iter()
                    .map(|dto| dto.id.clone())
                    .collect::<Vec<_>>();
                let received_ids = completed_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<HashSet<_>>();
                let missing_ids = ids
                    .iter()
                    .filter(|id| !received_ids.contains(id.as_str()))
                    .cloned()
                    .collect::<Vec<_>>();

                let changes = library
                    .ingest_enrichment_page(&received)
                    .map_err(storage_error)?;
                library
                    .mark_enrichment_complete(&completed_ids, now)
                    .map_err(storage_error)?;
                report.completed += completed_ids.len();
                report.changes.merge(changes);

                if !missing_ids.is_empty() {
                    let error = ApiError::Decode(
                        "an enrichment response omitted requested items".to_string(),
                    );
                    let retry_at = library
                        .mark_enrichment_error(&missing_ids, &error, now)
                        .map_err(storage_error)?;
                    report.failed += missing_ids.len();
                    report.last_error = Some(error.to_string());
                    report.retry_at = earliest(report.retry_at, retry_at);
                }
            }
            Err(error) => {
                let retry_at = library
                    .mark_enrichment_error(&ids, &error, now)
                    .map_err(storage_error)?;
                report.failed += ids.len();
                report.last_error = Some(error.to_string());
                report.retry_at = earliest(report.retry_at, retry_at);
                session.note_error(&error);
                tracing::debug!(
                    target: "library.enrichment",
                    selected = ids.len(),
                    "rich metadata batch failed: {error}"
                );
                // A dead/rate-limited server should not immediately receive a
                // second logical request from this wake.
                break;
            }
        }
    }

    report.more_due = !library
        .due_enrichment_ids(now, 1)
        .map_err(storage_error)?
        .is_empty();
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    let _ = library.set_meta(
        META_LAST_RUN,
        &serde_json::to_string(&report).unwrap_or_default(),
    );
    tracing::info!(
        target: "library.enrichment",
        selected = report.selected,
        requests = report.requests,
        completed = report.completed,
        failed = report.failed,
        elapsed_ms = report.elapsed_ms,
        "rich metadata enrichment wake finished"
    );
    Ok(report)
}

fn earliest(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}

fn usable_enrichment(dto: &BaseItemDto) -> bool {
    !dto.id.trim().is_empty()
        && dto
            .name
            .as_deref()
            .is_some_and(|name| !name.trim().is_empty())
        && matches!(
            dto.item_type.as_deref(),
            Some("Movie" | "Series" | "Season" | "Episode")
        )
}

fn retry_delay(errors: i64, error: &ApiError) -> i64 {
    const STEPS: [i64; 6] = [60, 300, 900, 3_600, 6 * 3_600, 24 * 3_600];
    let ordinary = STEPS[usize::try_from(errors.saturating_sub(1))
        .unwrap_or(usize::MAX)
        .min(STEPS.len() - 1)];
    error
        .retry_after()
        .map(|delay| i64::try_from(delay.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
        .max(ordinary)
}

fn bounded_error(error: &ApiError) -> String {
    error.to_string().chars().take(500).collect()
}

fn storage_error(error: rusqlite::Error) -> ApiError {
    ApiError::Decode(format!("library storage failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{MAX_ITEMS_PER_WAKE, REQUEST_BATCH_SIZE, retry_delay};
    use crate::jellyfin::api::ApiError;
    use crate::jellyfin::api::model::BaseItemDto;
    use crate::jellyfin::session::Session;
    use crate::library::{Library, StoredCredentials};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn request_load_is_serial_and_bounded() {
        assert_eq!(REQUEST_BATCH_SIZE, 20);
        assert_eq!(MAX_ITEMS_PER_WAKE, 40);
    }

    #[test]
    fn retry_after_wins_over_the_local_backoff() {
        let limited = ApiError::RateLimited {
            retry_after_secs: Some(600),
        };
        assert_eq!(retry_delay(1, &limited), 600);
        assert_eq!(limited.retry_after(), Some(Duration::from_secs(600)));
    }

    #[test]
    fn cancellation_between_batches_leaves_durable_work_pending() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let light: BaseItemDto =
            serde_json::from_str(r#"{"Id":"pending","Name":"Pending","Type":"Movie"}"#)
                .expect("dto");
        library
            .ingest_catalog_page_at(&[light], 1)
            .expect("catalog");
        let previous = library.credentials();
        library
            .save_credentials(&StoredCredentials {
                server_url: Some("http://127.0.0.1:1".to_string()),
                user_id: Some("user".to_string()),
                user_name: Some("User".to_string()),
                server_id: Some("server".to_string()),
                device_id: previous.device_id,
                token: Some("token".to_string()),
            })
            .expect("credentials");
        let session = Session::restore(library.clone());

        let report = super::process_due(&library, &session, || true).expect("cancel cleanly");

        assert_eq!(report.requests, 0);
        assert_eq!(report.completed, 0);
        assert_eq!(library.enrichment_diagnostics().pending, 1);
    }

    #[test]
    fn a_due_catalog_row_is_enriched_and_completed_in_one_bounded_request() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let light: BaseItemDto = serde_json::from_str(
            r#"{"Id":"movie","Name":"Movie","Type":"Movie","ProductionYear":2024}"#,
        )
        .expect("light dto");
        library
            .ingest_catalog_page_at(&[light], 1)
            .expect("catalog");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 2_048];
            loop {
                let read = stream.read(&mut buffer).expect("read");
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let target = String::from_utf8(request)
                .expect("request")
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .expect("target")
                .to_string();
            let body = r#"{"Items":[{"Id":"movie","Name":"Movie","Type":"Movie",
                "ProductionYear":2024,"Overview":"Rich overview",
                "ProviderIds":{"Tmdb":"1"},"ImageTags":{"Primary":"poster"},
                "People":[{"Name":"Actor","Type":"Actor"}],
                "MediaStreams":[{"Index":0,"Type":"Video","Codec":"hevc"}]}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("response");
            target
        });
        let previous = library.credentials();
        library
            .save_credentials(&StoredCredentials {
                server_url: Some(format!("http://{address}")),
                user_id: Some("user".to_string()),
                user_name: Some("User".to_string()),
                server_id: Some("server".to_string()),
                device_id: previous.device_id,
                token: Some("token".to_string()),
            })
            .expect("credentials");
        let session = Session::restore(library.clone());

        let report = super::process_due(&library, &session, || false).expect("enrich");

        assert_eq!(report.completed, 1);
        assert_eq!(report.requests, 1);
        let diagnostics = library.enrichment_diagnostics();
        assert_eq!((diagnostics.completed, diagnostics.pending), (1, 0));
        let item = library.item("movie").unwrap().unwrap();
        assert_eq!(item["overview"], "Rich overview");
        assert_eq!(item["people"][0]["name"], "Actor");
        assert_eq!(item["mediaStreams"][0]["codec"], "hevc");
        let target = server.join().expect("server");
        assert!(target.contains("People"));
        assert!(target.contains("MediaStreams"));
    }

    #[test]
    fn opened_item_is_moved_ahead_without_fetching_on_the_request_thread() {
        let library = Library::open_in_memory().expect("library");
        let dtos = [
            serde_json::from_str(r#"{"Id":"a","Name":"A","Type":"Movie"}"#).expect("a"),
            serde_json::from_str(r#"{"Id":"z","Name":"Z","Type":"Movie"}"#).expect("z"),
        ];
        library.ingest_catalog_page_at(&dtos, 100).expect("catalog");
        assert!(library.prioritize_enrichment("z").expect("prioritize"));
        assert_eq!(
            library.due_enrichment_ids(100, 2).expect("ordered queue"),
            vec!["z", "a"]
        );
    }

    #[test]
    fn pending_and_retry_state_survive_a_restart() {
        let path = std::env::temp_dir().join(format!(
            "mediaflick-enrichment-restart-{}.sqlite",
            crate::app::ids::random_hex(8)
        ));
        {
            let library = Library::open(&path).expect("library");
            let dto: BaseItemDto =
                serde_json::from_str(r#"{"Id":"resume","Name":"Resume","Type":"Movie"}"#)
                    .expect("dto");
            library
                .ingest_catalog_page_at(&[dto], 100)
                .expect("queue catalog row");
            library
                .mark_enrichment_error(
                    &["resume".to_string()],
                    &ApiError::Transport("offline".to_string()),
                    110,
                )
                .expect("persist retry");
        }

        let reopened = Library::open(&path).expect("reopen");
        let diagnostics = reopened.enrichment_diagnostics();
        assert_eq!(diagnostics.pending, 1);
        assert_eq!(diagnostics.failed, 1);
        assert!(diagnostics.next_due_at.is_some_and(|next| next >= 170));
        assert!(
            diagnostics
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("offline"))
        );

        drop(reopened);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
