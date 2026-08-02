//! One bounded repair pass for metadata cached during Jellyfin enrichment.
//!
//! The ordinary incremental sweep is intentionally keyed to `DateCreated`.
//! Jellyfin does not change that timestamp when its metadata providers finish,
//! so a sparse first DTO needs a durable path outside the watermark. Eligibility
//! lives on each row; this module only spends a small exact-ID request budget
//! once per process after authenticated synchronization is ready.

use crate::jellyfin::api::ApiError;
use crate::jellyfin::api::items;
use crate::jellyfin::session::Session;

use super::Library;
use super::model::materially_incomplete;

/// Exact-item requests allowed for one application launch. Sequential fetches
/// deliberately cap concurrency at one; together with this batch cap that keeps
/// the pass cheap even when a migrated library contains many sparse home items.
pub const MAX_STARTUP_REPAIR_CANDIDATES: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataRepairReport {
    pub candidates: usize,
    pub repaired_ids: Vec<String>,
    pub incomplete_responses: usize,
    pub failed: usize,
}

/// Re-fetches the launch's bounded candidate batch by exact Jellyfin ID.
///
/// Individual missing items, malformed/sparse responses, storage failures, and
/// ordinary request failures leave the cached row intact and do not prevent the
/// next ID from being tried. An authorization rejection is different: it is
/// routed through [`Session`] and stops the batch so the repair cannot hammer a
/// session that Jellyfin has already expired.
pub(crate) fn repair_once(
    library: &Library,
    session: &Session,
) -> Result<MetadataRepairReport, ApiError> {
    let (client, user_id) = session.client_and_user()?;
    let candidate_ids = library
        .metadata_repair_candidates(MAX_STARTUP_REPAIR_CANDIDATES)
        .map_err(storage_error)?;
    let mut report = MetadataRepairReport {
        candidates: candidate_ids.len(),
        ..MetadataRepairReport::default()
    };

    for item_id in candidate_ids {
        let response = items::fetch_item(&client, &user_id, &item_id);
        if let Err(error) = library.mark_metadata_repair_attempted(&item_id) {
            report.failed += 1;
            tracing::warn!(
                target: "library.repair",
                item_id,
                "could not record the metadata repair attempt: {error}"
            );
        }

        match response {
            Ok(Some(dto)) if dto.id == item_id && usable_repair_response(&dto) => {
                // The normal library upsert contract updates FTS, user data,
                // image tags, and the durable pending flag in one transaction.
                match library.upsert_metadata_repair(&dto) {
                    Ok(true) => report.repaired_ids.push(item_id),
                    Ok(false) => {
                        report.failed += 1;
                        tracing::debug!(
                            target: "library.repair",
                            item_id,
                            "the repaired Jellyfin DTO did not produce a cache write"
                        );
                    }
                    Err(error) => {
                        report.failed += 1;
                        tracing::warn!(
                            target: "library.repair",
                            item_id,
                            "could not upsert repaired Jellyfin metadata: {error}"
                        );
                    }
                }
            }
            Ok(Some(dto)) => {
                // A sparse/error-shaped success must never erase fields from
                // the existing row. A wrong ID is treated the same way.
                report.incomplete_responses += 1;
                tracing::debug!(
                    target: "library.repair",
                    item_id,
                    response_id = %dto.id,
                    "kept cached metadata after an incomplete repair response"
                );
            }
            Ok(None) => {
                // Deletion remains the identity sweep's responsibility. A
                // transient empty exact-ID response has no authority to erase.
                report.failed += 1;
                tracing::debug!(
                    target: "library.repair",
                    item_id,
                    "Jellyfin omitted a metadata repair candidate"
                );
            }
            Err(error) => {
                report.failed += 1;
                session.note_error(&error);
                tracing::debug!(
                    target: "library.repair",
                    item_id,
                    "could not repair cached metadata: {error}"
                );
                if error == ApiError::Unauthorized {
                    break;
                }
            }
        }
    }

    Ok(report)
}

fn usable_repair_response(dto: &crate::jellyfin::api::model::BaseItemDto) -> bool {
    matches!(dto.item_type.as_deref(), Some("Movie" | "Series"))
        && dto
            .name
            .as_deref()
            .is_some_and(|name| !name.trim().is_empty())
        && !materially_incomplete(dto)
}

fn storage_error(error: rusqlite::Error) -> ApiError {
    ApiError::Decode(format!("library storage failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{MAX_STARTUP_REPAIR_CANDIDATES, repair_once};
    use crate::jellyfin::api::model::BaseItemDto;
    use crate::jellyfin::session::Session;
    use crate::library::{Library, StoredCredentials};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;

    fn dto(json: &str) -> BaseItemDto {
        serde_json::from_str(json).expect("dto")
    }

    struct MockResponse {
        status: u16,
        body: &'static str,
    }

    fn mock_jellyfin(responses: Vec<MockResponse>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock Jellyfin");
        let address = listener.local_addr().expect("mock address");
        let server = thread::spawn(move || {
            let mut targets = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept request");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                loop {
                    let read = stream.read(&mut buffer).expect("read request");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8(request).expect("request text");
                let target = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .expect("request target")
                    .to_string();
                targets.push(target);

                let reason = if response.status == 200 {
                    "OK"
                } else {
                    "Not Found"
                };
                write!(
                    stream,
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.status,
                    reason,
                    response.body.len(),
                    response.body
                )
                .expect("write response");
            }
            targets
        });
        (format!("http://{address}"), server)
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
    fn startup_repair_updates_an_unchanged_sparse_dto_by_exact_id() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let item_id = "race-item";
        let date_created = "2024-05-04T12:00:00Z";
        library
            .upsert_page(&[dto(&format!(
                r#"{{"Id":"{item_id}","Name":"Initially sparse","Type":"Movie",
                    "ProductionYear":1988,"DateCreated":"{date_created}"}}"#
            ))])
            .expect("sparse ingestion");

        let enriched = format!(
            r#"{{"Items":[{{"Id":"{item_id}","Name":"Enriched","Type":"Movie",
                "ProductionYear":1988,"DateCreated":"{date_created}",
                "Overview":"Metadata providers finished later.","Genres":["Horror"],
                "ProviderIds":{{"Tmdb":"123"}},
                "ImageTags":{{"Primary":"poster-tag","Thumb":"thumb-tag"}},
                "BackdropImageTags":["backdrop-tag"]}}],"TotalRecordCount":1}}"#
        );
        let enriched: &'static str = Box::leak(enriched.into_boxed_str());
        let (server_url, server) = mock_jellyfin(vec![MockResponse {
            status: 200,
            body: enriched,
        }]);
        let session = authenticated_session(&library, &server_url);

        let report = repair_once(&library, &session).expect("repair");

        assert_eq!(report.candidates, 1);
        assert_eq!(report.repaired_ids, vec![item_id]);
        let cached = library.item(item_id).expect("query").expect("cached item");
        assert_eq!(cached["name"], "Enriched");
        assert_eq!(cached["overview"], "Metadata providers finished later.");
        assert_eq!(cached["primaryImageTag"], "poster-tag");
        assert_eq!(cached["backdropImageTag"], "backdrop-tag");
        assert_eq!(cached["dateCreated"], date_created);
        assert!(library.metadata_repair_candidates(8).unwrap().is_empty());

        let targets = server.join().expect("mock server");
        assert_eq!(targets.len(), 1);
        assert!(targets[0].starts_with("/Items?"));
        assert!(targets[0].contains("userId=user-1"));
        assert!(targets[0].contains("ids=race-item"));
        assert!(targets[0].contains("Fields=ProviderIds%2COverview%2CGenres"));
        assert!(targets[0].contains("EnableImages=true"));
    }

    #[test]
    fn candidates_survive_a_week_or_longer_without_a_wall_clock_cutoff() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[dto(
                r#"{"Id":"old-sparse","Name":"Still pending","Type":"Series",
                    "DateCreated":"2018-01-01T00:00:00Z"}"#,
            )])
            .expect("seed");

        assert_eq!(
            library.metadata_repair_candidates(8).expect("candidates"),
            vec!["old-sparse"]
        );
    }

    #[test]
    fn candidate_selection_is_bounded_and_rotates_attempted_rows() {
        let library = Library::open_in_memory().expect("library");
        let items = (0..MAX_STARTUP_REPAIR_CANDIDATES + 3)
            .map(|index| {
                dto(&format!(
                    r#"{{"Id":"item-{index:02}","Name":"Sparse","Type":"Movie",
                        "DateCreated":"2024-01-{:02}T00:00:00Z"}}"#,
                    index + 1
                ))
            })
            .collect::<Vec<_>>();
        library.upsert_page(&items).expect("seed");

        let first = library
            .metadata_repair_candidates(MAX_STARTUP_REPAIR_CANDIDATES)
            .expect("first batch");
        assert_eq!(first.len(), MAX_STARTUP_REPAIR_CANDIDATES);
        for id in &first {
            library
                .mark_metadata_repair_attempted(id)
                .expect("mark attempted");
        }
        let next = library.metadata_repair_candidates(3).expect("next batch");
        assert_eq!(next.len(), 3);
        assert!(next.iter().all(|id| !first.contains(id)));
    }

    #[test]
    fn a_sparse_repair_response_never_erases_richer_cached_fields() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        library
            .upsert_page(&[dto(r#"{"Id":"sparse","Name":"Local title","Type":"Movie",
                    "ProductionYear":1988,"RunTimeTicks":12345,
                    "DateCreated":"2020-01-01T00:00:00Z"}"#)])
            .expect("seed");
        let (server_url, server) = mock_jellyfin(vec![MockResponse {
            status: 200,
            body: r#"{"Items":[{"Id":"sparse","Type":"Movie"}]}"#,
        }]);
        let session = authenticated_session(&library, &server_url);

        let report = repair_once(&library, &session).expect("repair");

        assert_eq!(report.incomplete_responses, 1);
        assert!(report.repaired_ids.is_empty());
        let cached = library.item("sparse").unwrap().unwrap();
        assert_eq!(cached["name"], "Local title");
        assert_eq!(cached["year"], 1988);
        assert_eq!(cached["runtimeTicks"], 12345);
        assert_eq!(
            library.metadata_repair_candidates(8).unwrap(),
            vec!["sparse"]
        );
        server.join().expect("mock server");
    }

    #[test]
    fn a_partial_enriched_response_merges_art_without_erasing_identity_metadata() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        library
            .upsert_page(&[dto(
                r#"{"Id":"partial","Name":"Cached title","Type":"Movie",
                    "ProductionYear":1988,"RunTimeTicks":12345,
                    "DateCreated":"2020-01-01T00:00:00Z"}"#,
            )])
            .expect("seed");
        let (server_url, server) = mock_jellyfin(vec![MockResponse {
            status: 200,
            body: r#"{"Items":[{"Id":"partial","Name":"Server title","Type":"Movie",
                "ImageTags":{"Primary":"new-poster"}}]}"#,
        }]);
        let session = authenticated_session(&library, &server_url);

        let report = repair_once(&library, &session).expect("repair");

        assert_eq!(report.repaired_ids, vec!["partial"]);
        let cached = library.item("partial").unwrap().unwrap();
        assert_eq!(cached["name"], "Server title");
        assert_eq!(cached["year"], 1988);
        assert_eq!(cached["runtimeTicks"], 12345);
        assert_eq!(cached["dateCreated"], "2020-01-01T00:00:00Z");
        assert_eq!(cached["primaryImageTag"], "new-poster");
        assert!(library.metadata_repair_candidates(8).unwrap().is_empty());
        server.join().expect("mock server");
    }

    #[test]
    fn one_item_failure_does_not_stop_the_bounded_batch() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        library
            .upsert_page(&[
                dto(r#"{"Id":"fails","Name":"Fails","Type":"Movie",
                        "DateCreated":"2024-02-02T00:00:00Z"}"#),
                dto(r#"{"Id":"repairs","Name":"Repairs","Type":"Movie",
                        "DateCreated":"2024-02-01T00:00:00Z"}"#),
            ])
            .expect("seed");
        let (server_url, server) = mock_jellyfin(vec![
            MockResponse {
                status: 404,
                body: r#"{}"#,
            },
            MockResponse {
                status: 200,
                body: r#"{"Items":[{"Id":"repairs","Name":"Repaired","Type":"Movie",
                    "Overview":"Now enriched","ImageTags":{"Primary":"poster"}}]}"#,
            },
        ]);
        let session = authenticated_session(&library, &server_url);

        let report = repair_once(&library, &session).expect("repair");

        assert_eq!(report.candidates, 2);
        assert_eq!(report.failed, 1);
        assert_eq!(report.repaired_ids, vec!["repairs"]);
        assert_eq!(
            library.item("repairs").unwrap().unwrap()["name"],
            "Repaired"
        );
        assert_eq!(library.item("fails").unwrap().unwrap()["name"], "Fails");
        assert_eq!(server.join().expect("mock server").len(), 2);
    }

    #[test]
    fn authorization_failure_expires_the_session_and_stops_the_batch() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        library
            .upsert_page(&[
                dto(r#"{"Id":"first","Name":"First","Type":"Movie",
                        "DateCreated":"2024-02-02T00:00:00Z"}"#),
                dto(r#"{"Id":"second","Name":"Second","Type":"Movie",
                        "DateCreated":"2024-02-01T00:00:00Z"}"#),
            ])
            .expect("seed");
        let (server_url, server) = mock_jellyfin(vec![MockResponse {
            status: 401,
            body: r#"{}"#,
        }]);
        let session = authenticated_session(&library, &server_url);

        let report = repair_once(&library, &session).expect("repair report");

        assert_eq!(report.candidates, 2);
        assert_eq!(report.failed, 1);
        assert!(!session.is_authenticated());
        assert_eq!(server.join().expect("mock server").len(), 1);
    }
}
