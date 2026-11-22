use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use serde::Serialize;
use serde_json::{Value, json};

use crate::jellyfin::api::ApiError;
use crate::jellyfin::session::{SessionScope, Stale};
use crate::library::{CachedRatings, RatingTarget, now_unix};

use super::in_flight::{is_cancelled, wait_for_flights};
use super::schema::{
    NormalizedRating, normalize_rating_array, normalized_source_updated_at, rating_cache_value,
};
use super::{EffectiveOrigin, RatingOrigin, RatingsError, RatingsService};

const CACHE_SCHEMA_VERSION: i64 = 1;
const MAX_REQUEST_IDS: usize = 500;
const PLUGIN_FRESH_SECONDS: i64 = 24 * 60 * 60;
const EXPIRE_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RatingsBatchResponse {
    available: bool,
    effective_origin: EffectiveOrigin,
    items: Vec<ItemRatings>,
    retry_at: Option<i64>,
    diagnostic: Option<String>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct ItemRatings {
    id: String,
    ratings: Vec<NormalizedRating>,
    origin: RatingOrigin,
    fetched_at: i64,
    source_updated_at: Option<String>,
    stale: bool,
    schema_version: i64,
}

impl RatingsService {
    pub fn batch(
        &self,
        item_ids: &[String],
        cancelled: Option<&AtomicBool>,
    ) -> Result<RatingsBatchResponse, RatingsError> {
        if is_cancelled(cancelled) {
            return Err(RatingsError::Cancelled);
        }
        let item_ids = bounded_item_ids(item_ids);
        let _ = self.companion.probe(false);
        if is_cancelled(cancelled) {
            return Err(RatingsError::Cancelled);
        }
        if !self.companion.supports("ratings-v1") {
            return Ok(unavailable_ratings());
        }
        let Ok(scope) = self.session.scope() else {
            return Ok(unavailable_ratings());
        };

        let targets = rating_targets(&self.library, &item_ids).map_err(storage_error)?;
        let now = now_unix();
        let mut cache = self
            .library
            .cached_ratings(&targets)
            .map_err(storage_error)?;
        self.sanitize_cached(&targets, &mut cache)
            .map_err(storage_error)?;
        let refresh = targets
            .iter()
            .filter(|target| {
                cache
                    .get(&target.item_id)
                    .is_none_or(|cached| cached.stale_at <= now)
            })
            .cloned()
            .collect::<Vec<_>>();

        let claim = self.in_flight.claim(&scope, &refresh);
        let own_diagnostic = if let Some(guard) = claim.guard {
            let result = self.refresh_plugin(&scope, &claim.owned).err();
            guard.finish(result.clone());
            result
        } else {
            None
        };
        let awaited_diagnostic = if claim.awaited.is_empty() {
            None
        } else {
            wait_for_flights(&self.session, &scope, &claim.awaited, cancelled)?
        };
        let diagnostic = own_diagnostic.or(awaited_diagnostic);

        if is_cancelled(cancelled) {
            return Err(RatingsError::Cancelled);
        }
        if !self.session.scope_is_current(&scope) {
            return Err(RatingsError::AccountChanged);
        }
        if !claim.owned.is_empty() || !claim.awaited.is_empty() {
            cache = self
                .library
                .cached_ratings(&targets)
                .map_err(storage_error)?;
            self.sanitize_cached(&targets, &mut cache)
                .map_err(storage_error)?;
        }

        let items = targets
            .iter()
            .filter_map(|target| {
                let cached = cache.get(&target.item_id)?;
                (cached.expires_at > now).then(|| ItemRatings {
                    id: target.item_id.clone(),
                    ratings: normalize_rating_array(&cached.ratings),
                    origin: RatingOrigin::Plugin,
                    fetched_at: cached.fetched_at.max(0),
                    source_updated_at: cached.source_updated_at.clone(),
                    stale: cached.stale_at <= now,
                    schema_version: cached.schema_version,
                })
            })
            .collect();
        if !self.session.scope_is_current(&scope) {
            return Err(RatingsError::AccountChanged);
        }
        Ok(RatingsBatchResponse {
            available: true,
            effective_origin: EffectiveOrigin::Plugin,
            items,
            retry_at: None,
            diagnostic,
        })
    }

    fn sanitize_cached(
        &self,
        targets: &[RatingTarget],
        cache: &mut HashMap<String, CachedRatings>,
    ) -> rusqlite::Result<()> {
        cache.retain(|_, cached| cached.schema_version == CACHE_SCHEMA_VERSION);
        let mut replacements = Vec::new();
        for target in targets {
            let Some(cached) = cache.get_mut(&target.item_id) else {
                continue;
            };
            let sanitized = CachedRatings {
                ratings: rating_cache_value(&normalize_rating_array(&cached.ratings)),
                source_updated_at: normalized_source_updated_at(
                    cached.source_updated_at.as_deref(),
                ),
                fetched_at: cached.fetched_at.max(0),
                stale_at: cached.stale_at.max(0),
                expires_at: cached.expires_at.max(0),
                schema_version: cached.schema_version,
            };
            if *cached != sanitized {
                *cached = sanitized.clone();
                replacements.push((target.clone(), sanitized));
            }
        }
        if replacements.is_empty() {
            Ok(())
        } else {
            self.library.save_rating_cache(&replacements)
        }
    }

    fn refresh_plugin(&self, scope: &SessionScope, targets: &[RatingTarget]) -> Result<(), String> {
        let body = json!({
            "boundaryVersion": 1,
            "items": targets,
        });
        let response = self
            .companion
            .ratings_v1(scope, &body)
            .map_err(|error| plugin_rating_error(&error))?;
        let account_changed = || ApiError::AccountChanged.to_string();
        self.session
            .commit_if_current(scope, account_changed, |live| {
                let response = live.open(response).map_err(|Stale| account_changed())?;
                let entries = normalize_plugin_batch(targets, &response, now_unix());
                self.library.save_rating_cache(&entries).map_err(|_| {
                    "could not cache server ratings; cached ratings remain usable".to_string()
                })
            })
    }
}

fn rating_targets(
    library: &crate::library::Library,
    item_ids: &[String],
) -> rusqlite::Result<Vec<RatingTarget>> {
    let (discovery_ids, library_ids): (Vec<_>, Vec<_>) = item_ids
        .iter()
        .cloned()
        .partition(|id| id.starts_with("tmdb:"));
    let mut targets = library.rating_targets(&library_ids)?;
    targets.extend(discovery_ids.iter().filter_map(|id| discovery_target(id)));
    Ok(targets)
}

fn discovery_target(item_id: &str) -> Option<RatingTarget> {
    let (media_type, provider_id) = item_id.strip_prefix("tmdb:")?.split_once(':')?;
    let (kind, media_type) = match media_type {
        "movie" => ("Movie", "movie"),
        "tv" => ("Series", "show"),
        _ => return None,
    };
    let id = provider_id.parse::<i64>().ok().filter(|id| *id > 0)?;
    if id.to_string() != provider_id {
        return None;
    }
    Some(RatingTarget {
        item_id: item_id.to_string(),
        kind: kind.to_string(),
        media_type: media_type.to_string(),
        provider: "tmdb".to_string(),
        provider_id: provider_id.to_string(),
    })
}

fn bounded_item_ids(item_ids: &[String]) -> Vec<String> {
    let mut item_ids = item_ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .collect::<Vec<_>>();
    item_ids.sort();
    item_ids.dedup();
    item_ids.truncate(MAX_REQUEST_IDS);
    item_ids
}

fn unavailable_ratings() -> RatingsBatchResponse {
    RatingsBatchResponse {
        available: false,
        effective_origin: EffectiveOrigin::None,
        items: Vec::new(),
        retry_at: None,
        diagnostic: Some("Server ratings are unavailable.".to_string()),
    }
}

fn plugin_rating_error(error: &ApiError) -> String {
    match error {
        ApiError::NotConfigured
        | ApiError::Status { status: 404 }
        | ApiError::Remote { status: 404, .. } => {
            "The server rating capability is not available yet; cached ratings remain usable."
                .to_string()
        }
        _ => {
            "The server rating capability is temporarily unavailable; cached ratings remain usable."
                .to_string()
        }
    }
}

fn storage_error(_: rusqlite::Error) -> RatingsError {
    RatingsError::Storage
}

fn normalize_plugin_batch(
    targets: &[RatingTarget],
    body: &Value,
    now: i64,
) -> Vec<(RatingTarget, CachedRatings)> {
    if body.get("boundaryVersion").and_then(Value::as_i64) != Some(1) {
        return Vec::new();
    }
    let mut by_item = HashMap::<String, (Value, Option<String>)>::new();
    for item in body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let Some(item_id) = item
            .get("itemId")
            .or_else(|| item.get("id"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let ratings = rating_cache_value(&normalize_rating_array(
            item.get("ratings").unwrap_or(&Value::Null),
        ));
        let source_updated_at =
            normalized_source_updated_at(item.get("sourceUpdatedAt").and_then(Value::as_str));
        by_item.insert(item_id.to_string(), (ratings, source_updated_at));
    }
    targets
        .iter()
        .filter_map(|target| {
            let (ratings, source_updated_at) = by_item.get(&target.item_id)?.clone();
            Some((
                target.clone(),
                CachedRatings {
                    ratings,
                    source_updated_at,
                    fetched_at: now,
                    stale_at: now.saturating_add(PLUGIN_FRESH_SECONDS),
                    expires_at: now.saturating_add(EXPIRE_SECONDS),
                    schema_version: CACHE_SCHEMA_VERSION,
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::library::Library;
    use crate::library::test_support::dto;

    use super::*;

    #[test]
    fn discovery_and_library_cards_share_cache_without_mixing_movies_and_series() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../fixtures/ratings/discovery-identities.json"
        ))
        .expect("discovery identities");
        let entries = fixture.as_array().expect("identities");
        let library = Library::open_in_memory().expect("library");
        library.upsert_page(&crate::jellyfin::session::Live::for_test(), &[
            dto(r#"{"Id":"local-movie","Name":"Movie","Type":"Movie","ProviderIds":{"Tmdb":"603"}}"#),
            dto(r#"{"Id":"local-series","Name":"Series","Type":"Series","ProviderIds":{"Tmdb":"603"}}"#),
        ]).expect("seed");
        let ids = entries
            .iter()
            .map(|entry| entry["ratingId"].as_str().expect("id").to_string())
            .collect::<Vec<_>>();
        let targets = rating_targets(&library, &bounded_item_ids(&ids)).expect("targets");
        assert_eq!(targets.len(), entries.len());
        for entry in entries {
            let target = targets
                .iter()
                .find(|target| target.item_id == entry["ratingId"])
                .expect("target");
            assert_eq!(target.kind, entry["kind"]);
            assert_eq!(target.media_type, entry["providerMediaType"]);
            assert_eq!(target.provider, "tmdb");
            assert_eq!(target.provider_id, "603");
        }
        let response = json!({
            "boundaryVersion": 1,
            "items": [
                { "itemId": "tmdb:movie:603", "ratings": [{ "sourceId": "imdb", "value": 8.7 }] },
                { "itemId": "tmdb:tv:603", "ratings": [{ "sourceId": "imdb", "value": 7.2 }] }
            ]
        });
        library
            .save_rating_cache(&normalize_plugin_batch(&targets, &response, now_unix()))
            .expect("cache");
        let cache = library.cached_ratings(&targets).expect("read cache");
        assert_eq!(cache["tmdb:movie:603"], cache["local-movie"]);
        assert_eq!(cache["tmdb:tv:603"], cache["local-series"]);
        assert_eq!(cache["local-movie"].ratings[0]["value"], 8.7);
        assert_eq!(cache["local-series"].ratings[0]["value"], 7.2);
    }

    #[test]
    fn discovery_identities_reject_invalid_or_noncanonical_targets() {
        for id in [
            "tmdb:person:603",
            "tmdb:show:603",
            "tmdb:movie:0",
            "tmdb:tv:-1",
            "tmdb:movie:0603",
            "tmdb:movie:+603",
            "tmdb:movie:603/credits",
            "tmdb:movie:9223372036854775808",
            "tmdb:movie:",
            "tmdb:movie: 603",
        ] {
            assert!(discovery_target(id).is_none(), "accepted {id}");
        }
        let ids = vec!["tmdb:movie:603".to_string(); 600];
        assert_eq!(bounded_item_ids(&ids), ["tmdb:movie:603"]);
        let ids = (1..=600)
            .map(|id| format!("tmdb:movie:{id}"))
            .collect::<Vec<_>>();
        assert_eq!(bounded_item_ids(&ids).len(), 500);
    }

    #[test]
    fn credential_shaped_plugin_data_never_reaches_desktop_or_cache() {
        const SERVER_MDBLIST_KEY: &str = "server-mdb-key-must-never-reach-desktop";
        const SERVER_TMDB_KEY: &str = "0123456789abcdef0123456789abcdef";
        let targets = [RatingTarget {
            item_id: "matrix".to_string(),
            kind: "Movie".to_string(),
            media_type: "movie".to_string(),
            provider: "tmdb".to_string(),
            provider_id: "603".to_string(),
        }];
        let cache_now = now_unix();
        let entries = normalize_plugin_batch(
            &targets,
            &json!({
                "boundaryVersion": 1,
                "items": [{
                    "itemId": "matrix",
                    "sourceUpdatedAt": SERVER_MDBLIST_KEY,
                    "diagnostic": SERVER_TMDB_KEY,
                    "ratings": [
                        { "sourceId": SERVER_MDBLIST_KEY, "value": 99, "trace": SERVER_TMDB_KEY },
                        { "sourceId": "imdb", "value": 8.7, "rawSource": SERVER_MDBLIST_KEY,
                          "error": SERVER_TMDB_KEY }
                    ]
                }]
            }),
            cache_now,
        );
        let library = Library::open_in_memory().expect("rating cache");
        library
            .save_rating_cache(&entries)
            .expect("persist plugin cache");
        let persisted = library.cached_ratings(&targets).expect("read plugin cache");
        let serialized = json!({
            "ratings": &persisted["matrix"].ratings,
            "sourceUpdatedAt": &persisted["matrix"].source_updated_at,
        })
        .to_string();
        assert!(serialized.contains("\"imdb\""));
        assert!(!serialized.contains(SERVER_MDBLIST_KEY));
        assert!(!serialized.contains(SERVER_TMDB_KEY));
        assert_eq!(entries[0].1.source_updated_at, None);

        let diagnostic = plugin_rating_error(&ApiError::Remote {
            status: 502,
            message: format!("upstream said {SERVER_MDBLIST_KEY} / {SERVER_TMDB_KEY}"),
        });
        assert!(!diagnostic.contains(SERVER_MDBLIST_KEY));
        assert!(!diagnostic.contains(SERVER_TMDB_KEY));
    }

    fn loopback_companion(
        ratings_answer: (&'static str, &'static str),
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Value>>>) {
        use std::io::{BufRead, BufReader, Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let batches = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = batches.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().map_while(Result::ok) {
                let mut reader = BufReader::new(&mut stream);
                let mut request_line = String::new();
                reader.read_line(&mut request_line).expect("request line");
                let mut length = 0;
                let mut header = String::new();
                while reader.read_line(&mut header).expect("header") > 2 {
                    if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:")
                    {
                        length = value.trim().parse().expect("content length");
                    }
                    header.clear();
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).expect("body");
                let (status, answer) = if request_line.contains(" /MediaFlick/info") {
                    (
                        "200 OK",
                        r#"{"pluginVersion":"0.2.2","apiVersion":1,"capabilities":["ratings-v1"]}"#,
                    )
                } else if request_line.contains(" /MediaFlick/ratings/v1/batch") {
                    recorded
                        .lock()
                        .expect("batches")
                        .push(serde_json::from_slice(&body).expect("batch body"));
                    ratings_answer
                } else {
                    ("404 Not Found", "{}")
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                )
                .expect("answer");
            }
        });
        (url, batches)
    }

    fn seed_three_schemas(library: &Library) -> Vec<RatingTarget> {
        let now = now_unix();
        let entries = [
            (603, 0),
            (604, CACHE_SCHEMA_VERSION + 1),
            (605, CACHE_SCHEMA_VERSION),
        ]
        .map(|(id, schema_version)| {
            (
                discovery_target(&format!("tmdb:movie:{id}")).expect("target"),
                CachedRatings {
                    ratings: rating_cache_value(&normalize_rating_array(
                        &json!([{ "sourceId": "imdb", "value": 5 }]),
                    )),
                    source_updated_at: None,
                    fetched_at: now,
                    stale_at: now + 3600,
                    expires_at: now + 86_400,
                    schema_version,
                },
            )
        });
        library.save_rating_cache(&entries).expect("seed");
        entries.map(|(target, _)| target).to_vec()
    }

    fn imdb_value(ratings: &[NormalizedRating]) -> Value {
        let ratings = serde_json::to_value(ratings).expect("ratings");
        ratings[0]["value"].clone()
    }

    #[test]
    fn rows_at_another_schema_version_are_refetched_not_restamped() {
        const NINE: &str = r#"{"boundaryVersion":1,"items":[
            {"itemId":"tmdb:movie:603","ratings":[{"sourceId":"imdb","value":9}]},
            {"itemId":"tmdb:movie:604","ratings":[{"sourceId":"imdb","value":9}]},
            {"itemId":"tmdb:movie:605","ratings":[{"sourceId":"imdb","value":9}]}]}"#;
        let (url, batches) = loopback_companion(("200 OK", NINE));
        let fixture = crate::app::services::test_support::TestServices::signed_in(&url);
        let targets = seed_three_schemas(&fixture.services.library);
        let ids = targets
            .iter()
            .map(|target| target.item_id.clone())
            .collect::<Vec<_>>();

        let response = fixture.services.ratings.batch(&ids, None).expect("batch");

        let batches = batches.lock().expect("batches").clone();
        assert_eq!(batches.len(), 1, "{batches:?}");
        let requested = batches[0]["items"]
            .as_array()
            .expect("items")
            .iter()
            .map(|item| item["itemId"].as_str().expect("itemId"))
            .collect::<Vec<_>>();
        assert_eq!(requested, ["tmdb:movie:603", "tmdb:movie:604"]);
        let served = response
            .items
            .iter()
            .map(|item| {
                (
                    item.id.as_str(),
                    imdb_value(&item.ratings),
                    item.schema_version,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            served,
            [
                ("tmdb:movie:603", json!(9.0), CACHE_SCHEMA_VERSION),
                ("tmdb:movie:604", json!(9.0), CACHE_SCHEMA_VERSION),
                ("tmdb:movie:605", json!(5.0), CACHE_SCHEMA_VERSION),
            ]
        );
    }

    #[test]
    fn a_failed_refetch_neither_serves_nor_restamps_another_schemas_rows() {
        let (url, batches) = loopback_companion(("503 Service Unavailable", "{}"));
        let fixture = crate::app::services::test_support::TestServices::signed_in(&url);
        let targets = seed_three_schemas(&fixture.services.library);
        let ids = targets
            .iter()
            .map(|target| target.item_id.clone())
            .collect::<Vec<_>>();

        let response = fixture.services.ratings.batch(&ids, None).expect("batch");

        assert_eq!(batches.lock().expect("batches").len(), 1);
        let served = response
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(served, ["tmdb:movie:605"]);
        let stored = fixture
            .services
            .library
            .cached_ratings(&targets)
            .expect("read cache");
        assert_eq!(stored["tmdb:movie:603"].schema_version, 0);
        assert_eq!(
            stored["tmdb:movie:604"].schema_version,
            CACHE_SCHEMA_VERSION + 1
        );
    }
}
