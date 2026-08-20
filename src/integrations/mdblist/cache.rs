use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

use crate::jellyfin::api::ApiError;
use crate::library::{CachedRatings, IntegrationState, RatingTarget, now_unix};

use super::credentials::{
    MAX_QUOTA, MDBLIST_CREDENTIAL, bounded_nonnegative, bounded_timestamp, effective_origin,
    storage_error,
};
use super::schema::{normalize_media, normalize_rating_array, normalized_source_updated_at};
use super::transport::{MAX_BATCH_SIZE, MdbError};
use super::{Origin, RatingsError, RatingsService};

const CACHE_SCHEMA_VERSION: i64 = 1;
const MAX_REQUEST_IDS: usize = 500;
const FRESH_SECONDS: i64 = 7 * 24 * 60 * 60;
const NEGATIVE_FRESH_SECONDS: i64 = 24 * 60 * 60;
const EXPIRE_SECONDS: i64 = 30 * 24 * 60 * 60;
const PLUGIN_FRESH_SECONDS: i64 = 24 * 60 * 60;

impl RatingsService {
    /// Returns cached data immediately where possible and performs at most the
    /// bounded refresh work owned by this call. The catalog endpoints never
    /// invoke this method, so rating latency cannot delay progressive sync.
    pub fn batch(&self, item_ids: &[String]) -> Result<Value, RatingsError> {
        let item_ids = bounded_item_ids(item_ids);

        // A platform without a local vault can still consume the plugin's
        // capability/data boundary; a vault failure must not suppress it.
        let configured = self.credentials.get(MDBLIST_CREDENTIAL).unwrap_or(None);
        let state = self
            .library
            .integration_state(MDBLIST_CREDENTIAL)
            .map_err(storage_error)?
            .unwrap_or_default();
        let _ = self.companion.probe(false);
        let origin = effective_origin(
            configured.is_some(),
            state.valid,
            self.companion.supports("ratings-v1"),
        );
        let Some(origin) = origin else {
            return Ok(unavailable_ratings(&state));
        };

        let targets = self
            .library
            .rating_targets(&item_ids)
            .map_err(storage_error)?;
        let now = now_unix();
        let mut cache = self
            .library
            .cached_ratings(&targets, origin.as_str())
            .map_err(storage_error)?;
        self.sanitize_cached(&targets, origin, &mut cache)
            .map_err(storage_error)?;
        let mut refresh = targets
            .iter()
            .filter(|target| {
                cache.get(&target.item_id).is_none_or(|cached| {
                    cached.schema_version != CACHE_SCHEMA_VERSION || cached.stale_at <= now
                })
            })
            .cloned()
            .collect::<Vec<_>>();

        let backed_off = origin == Origin::Local && state.retry_at.is_some_and(|retry| retry > now);
        if backed_off {
            refresh.clear();
        }
        let owned = self.claim(origin, &refresh);
        let mut diagnostic = None;
        if !owned.is_empty() {
            let refreshed = match origin {
                Origin::Local => configured.as_deref().map_or_else(
                    || Err("The local MDBList credential is no longer available.".to_string()),
                    |credential| self.refresh_local(credential, &owned),
                ),
                Origin::Plugin => self.refresh_plugin(&owned),
            };
            if let Err(error) = refreshed {
                diagnostic = Some(error);
            }
            self.release(origin, &owned);
            cache = self
                .library
                .cached_ratings(&targets, origin.as_str())
                .map_err(storage_error)?;
            self.sanitize_cached(&targets, origin, &mut cache)
                .map_err(storage_error)?;
        }

        let items = targets
            .iter()
            .filter_map(|target| {
                let cached = cache.get(&target.item_id)?;
                (cached.expires_at > now).then(|| {
                    json!({
                        "id": target.item_id,
                        "ratings": cached.ratings,
                        "origin": origin.as_str(),
                        "fetchedAt": cached.fetched_at.max(0),
                        "sourceUpdatedAt": cached.source_updated_at,
                        "stale": cached.stale_at <= now,
                        "schemaVersion": cached.schema_version,
                    })
                })
            })
            .collect::<Vec<_>>();
        let latest_state = self
            .library
            .integration_state(MDBLIST_CREDENTIAL)
            .map_err(storage_error)?
            .unwrap_or(state);
        Ok(json!({
            "available": true,
            "effectiveOrigin": origin.as_str(),
            "items": items,
            "retryAt": bounded_timestamp(latest_state.retry_at),
            "quota": {
                "limit": bounded_nonnegative(latest_state.quota_limit, MAX_QUOTA),
                "remaining": bounded_nonnegative(latest_state.quota_remaining, MAX_QUOTA),
                "resetAt": bounded_timestamp(latest_state.quota_reset_at),
            },
            "diagnostic": diagnostic,
        }))
    }

    /// Rewrites legacy/tampered cache rows before the cache can feed a desktop
    /// response. The persisted representation receives the same positive
    /// allowlist as fresh local and plugin results.
    fn sanitize_cached(
        &self,
        targets: &[RatingTarget],
        origin: Origin,
        cache: &mut HashMap<String, CachedRatings>,
    ) -> rusqlite::Result<()> {
        let mut replacements = Vec::new();
        for target in targets {
            let Some(cached) = cache.get_mut(&target.item_id) else {
                continue;
            };
            let sanitized = CachedRatings {
                ratings: normalize_rating_array(&cached.ratings),
                source_updated_at: normalized_source_updated_at(
                    cached.source_updated_at.as_deref(),
                ),
                fetched_at: cached.fetched_at.max(0),
                stale_at: cached.stale_at.max(0),
                expires_at: cached.expires_at.max(0),
                schema_version: CACHE_SCHEMA_VERSION,
                origin: origin.as_str().to_string(),
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

    fn claim(&self, origin: Origin, targets: &[RatingTarget]) -> Vec<RatingTarget> {
        let Ok(mut in_flight) = self.in_flight.lock() else {
            return Vec::new();
        };
        targets
            .iter()
            .filter(|target| in_flight.insert(target_key(origin, target)))
            .cloned()
            .collect()
    }

    fn release(&self, origin: Origin, targets: &[RatingTarget]) {
        if let Ok(mut in_flight) = self.in_flight.lock() {
            for target in targets {
                in_flight.remove(&target_key(origin, target));
            }
        }
    }

    fn refresh_local(&self, key: &str, targets: &[RatingTarget]) -> Result<(), String> {
        let mut groups = BTreeMap::<(String, String), Vec<RatingTarget>>::new();
        for target in targets {
            groups
                .entry((target.provider.clone(), target.media_type.clone()))
                .or_default()
                .push(target.clone());
        }
        for ((provider, media_type), group) in groups {
            for chunk in group.chunks(MAX_BATCH_SIZE) {
                let ids = chunk
                    .iter()
                    .map(|target| target.provider_id.clone())
                    .collect::<Vec<_>>();
                match self.transport.batch(key, &provider, &media_type, &ids) {
                    Ok(response) => {
                        self.note_quota(&response.quota)
                            .map_err(|error| error.to_string())?;
                        let entries = normalize_batch(
                            chunk,
                            &provider,
                            &response.body,
                            Origin::Local,
                            now_unix(),
                        );
                        self.library.save_rating_cache(&entries).map_err(|_| {
                            "could not cache MDBList ratings; cached ratings remain usable."
                                .to_string()
                        })?;
                    }
                    Err(error) => {
                        self.note_fetch_error(&error)
                            .map_err(|state_error| state_error.to_string())?;
                        return Err(fetch_error_message(&error));
                    }
                }
            }
        }
        Ok(())
    }

    fn refresh_plugin(&self, targets: &[RatingTarget]) -> Result<(), String> {
        let body = json!({
            "boundaryVersion": 1,
            "items": targets,
        });
        let response = self
            .companion
            .ratings_v1(&body)
            .map_err(|error| plugin_rating_error(&error))?;
        let entries = normalize_plugin_batch(targets, &response, now_unix());
        self.library.save_rating_cache(&entries).map_err(|_| {
            "could not cache server ratings; cached plugin ratings remain usable.".to_string()
        })
    }
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

fn unavailable_ratings(state: &IntegrationState) -> Value {
    json!({
        "available": false,
        "effectiveOrigin": "none",
        "items": [],
        "retryAt": bounded_timestamp(state.retry_at),
        "diagnostic": "No valid local MDBList key or compatible plugin rating capability is available.",
    })
}

fn plugin_rating_error(error: &ApiError) -> String {
    match error {
        ApiError::NotConfigured
        | ApiError::Status { status: 404 }
        | ApiError::Remote { status: 404, .. } => {
            "The server rating capability is not available yet; cached plugin ratings remain usable."
                .to_string()
        }
        _ => "The server rating capability is temporarily unavailable; cached plugin ratings remain usable."
            .to_string(),
    }
}

fn fetch_error_message(error: &MdbError) -> String {
    match error {
        MdbError::Unauthorized(_) => "MDBList rejected the saved credential.".to_string(),
        MdbError::RateLimited(_) => {
            "MDBList quota is exhausted; retry timing is being respected.".to_string()
        }
        MdbError::Transport => "MDBList is unreachable; stale ratings are being used.".to_string(),
        MdbError::Decode => {
            "MDBList returned an unreadable response; stale ratings are being used.".to_string()
        }
        MdbError::Remote { status, .. } => {
            format!("MDBList returned HTTP {status}; stale ratings are being used.")
        }
    }
}

fn target_key(origin: Origin, target: &RatingTarget) -> String {
    format!(
        "{}:{}:{}:{}",
        origin.as_str(),
        target.provider,
        target.media_type,
        target.provider_id
    )
}

fn normalize_batch(
    targets: &[RatingTarget],
    provider: &str,
    body: &Value,
    origin: Origin,
    now: i64,
) -> Vec<(RatingTarget, CachedRatings)> {
    let media = body.as_array().cloned().unwrap_or_default();
    let mut by_id = HashMap::<String, (Value, Option<String>)>::new();
    for item in media {
        let Some(provider_id) = item
            .get("ids")
            .and_then(|ids| ids.get(provider))
            .and_then(value_id)
        else {
            continue;
        };
        let source_updated_at =
            normalized_source_updated_at(item.get("updated").and_then(Value::as_str));
        by_id.insert(provider_id, (normalize_media(&item), source_updated_at));
    }
    targets
        .iter()
        .cloned()
        .map(|target| {
            let (ratings, source_updated_at) = by_id
                .get(&target.provider_id)
                .cloned()
                .unwrap_or_else(|| (json!([]), None));
            let fresh = if ratings.as_array().is_some_and(Vec::is_empty) {
                NEGATIVE_FRESH_SECONDS
            } else {
                FRESH_SECONDS
            };
            (
                target,
                CachedRatings {
                    ratings,
                    source_updated_at,
                    fetched_at: now,
                    stale_at: now.saturating_add(fresh),
                    expires_at: now.saturating_add(EXPIRE_SECONDS),
                    schema_version: CACHE_SCHEMA_VERSION,
                    origin: origin.as_str().to_string(),
                },
            )
        })
        .collect()
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
        let ratings = normalize_rating_array(item.get("ratings").unwrap_or(&Value::Null));
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
                    origin: Origin::Plugin.as_str().to_string(),
                },
            ))
        })
        .collect()
}

fn value_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|value| value.len() <= 32 && value.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .map(str::to_string)
        .or_else(|| {
            value
                .as_i64()
                .filter(|value| *value > 0)
                .map(|value| value.to_string())
        })
        .or_else(|| {
            value
                .as_u64()
                .filter(|value| *value > 0)
                .map(|value| value.to_string())
        })
}

#[cfg(test)]
mod tests {
    use crate::library::Library;

    use super::*;

    #[test]
    fn batch_normalization_keeps_partial_and_missing_results_independent() {
        let targets = [
            RatingTarget {
                item_id: "a".to_string(),
                kind: "Movie".to_string(),
                media_type: "movie".to_string(),
                provider: "tmdb".to_string(),
                provider_id: "603".to_string(),
            },
            RatingTarget {
                item_id: "b".to_string(),
                kind: "Movie".to_string(),
                media_type: "movie".to_string(),
                provider: "tmdb".to_string(),
                provider_id: "604".to_string(),
            },
        ];
        let entries = normalize_batch(
            &targets,
            "tmdb",
            &json!([{
                "ids": { "tmdb": 603 },
                "updated": "2026-08-04T20:00:00Z",
                "ratings": [{ "source": "imdb", "value": 8.7, "score": 87 }]
            }]),
            Origin::Local,
            100,
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1.ratings[0]["sourceId"], "imdb");
        assert_eq!(
            entries[0].1.source_updated_at.as_deref(),
            Some("2026-08-04T20:00:00Z")
        );
        assert_eq!(entries[1].1.ratings, json!([]));
        assert_eq!(entries[1].1.stale_at, 100 + NEGATIVE_FRESH_SECONDS);
    }

    #[test]
    fn credential_shaped_upstream_plugin_cache_and_error_text_never_reach_desktop_data() {
        const SERVER_MDBLIST_KEY: &str = "server-mdb-key-must-never-reach-desktop";
        const SERVER_TMDB_KEY: &str = "0123456789abcdef0123456789abcdef";
        let targets = [RatingTarget {
            item_id: "matrix".to_string(),
            kind: "Movie".to_string(),
            media_type: "movie".to_string(),
            provider: "tmdb".to_string(),
            provider_id: "603".to_string(),
        }];

        // Plugin-key-only fallback: malicious server response fields and cache
        // records are rebuilt rather than copied into persisted entries.
        let cache_now = now_unix();
        assert_eq!(effective_origin(false, false, true), Some(Origin::Plugin));
        let plugin_entries = normalize_plugin_batch(
            &targets,
            &json!({
                "boundaryVersion": 1,
                "items": [{
                    "itemId": "matrix",
                    "sourceUpdatedAt": SERVER_MDBLIST_KEY,
                    "diagnostic": SERVER_TMDB_KEY,
                    "ratings": [
                        { "source": SERVER_MDBLIST_KEY, "value": 99, "trace": SERVER_TMDB_KEY },
                        { "source": "imdb", "value": 8.7, "rawSource": SERVER_MDBLIST_KEY,
                          "error": SERVER_TMDB_KEY }
                    ]
                }]
            }),
            cache_now,
        );
        let library = Library::open_in_memory().expect("rating cache");
        library
            .save_rating_cache(&plugin_entries)
            .expect("persist plugin cache");
        let persisted_plugin = library
            .cached_ratings(&targets, Origin::Plugin.as_str())
            .expect("read plugin cache");
        let persisted_plugin_entry = json!({
            "ratings": &persisted_plugin["matrix"].ratings,
            "sourceUpdatedAt": &persisted_plugin["matrix"].source_updated_at,
        })
        .to_string();
        assert!(persisted_plugin_entry.contains("\"imdb\""));
        assert!(!persisted_plugin_entry.contains(SERVER_MDBLIST_KEY));
        assert!(!persisted_plugin_entry.contains(SERVER_TMDB_KEY));
        assert_eq!(plugin_entries[0].1.source_updated_at, None);

        // A valid local Desktop credential wins precedence, but local upstream
        // text follows the same policy rather than creating a second leak path.
        assert_eq!(effective_origin(true, true, true), Some(Origin::Local));
        let local_entries = normalize_batch(
            &targets,
            "tmdb",
            &json!([{ "ids": { "tmdb": 603 }, "updated": SERVER_TMDB_KEY,
                "ratings": [{ "source": SERVER_MDBLIST_KEY, "value": 99 }] }]),
            Origin::Local,
            cache_now,
        );
        library
            .save_rating_cache(&local_entries)
            .expect("persist local cache");
        let persisted_local = library
            .cached_ratings(&targets, Origin::Local.as_str())
            .expect("read local cache");
        let persisted_local_entry = json!({
            "ratings": &persisted_local["matrix"].ratings,
            "sourceUpdatedAt": &persisted_local["matrix"].source_updated_at,
        })
        .to_string();
        assert!(!persisted_local_entry.contains(SERVER_MDBLIST_KEY));
        assert!(!persisted_local_entry.contains(SERVER_TMDB_KEY));
        assert_eq!(local_entries[0].1.ratings, json!([]));
        assert_eq!(local_entries[0].1.source_updated_at, None);

        let diagnostic = plugin_rating_error(&ApiError::Remote {
            status: 502,
            message: format!("upstream said {SERVER_MDBLIST_KEY} / {SERVER_TMDB_KEY}"),
        });
        assert!(!diagnostic.contains(SERVER_MDBLIST_KEY));
        assert!(!diagnostic.contains(SERVER_TMDB_KEY));
    }
}
