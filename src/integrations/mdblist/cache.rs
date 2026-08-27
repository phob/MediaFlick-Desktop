use std::collections::HashMap;

use serde_json::{Value, json};

use crate::jellyfin::api::ApiError;
use crate::library::{CachedRatings, RatingTarget, now_unix};

use super::schema::{normalize_rating_array, normalized_source_updated_at};
use super::{CACHE_ORIGIN, RatingsError, RatingsService};

const CACHE_SCHEMA_VERSION: i64 = 1;
const MAX_REQUEST_IDS: usize = 500;
const PLUGIN_FRESH_SECONDS: i64 = 24 * 60 * 60;
const EXPIRE_SECONDS: i64 = 30 * 24 * 60 * 60;

impl RatingsService {
    /// Returns cached data immediately where possible and performs at most the
    /// bounded refresh work owned by this call. Catalog endpoints never invoke
    /// this method, so rating latency cannot delay progressive sync.
    pub fn batch(&self, item_ids: &[String]) -> Result<Value, RatingsError> {
        let item_ids = bounded_item_ids(item_ids);
        let _ = self.companion.probe(false);
        if !self.companion.supports("ratings-v1") {
            return Ok(unavailable_ratings());
        }

        let targets = self
            .library
            .rating_targets(&item_ids)
            .map_err(storage_error)?;
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
                cache.get(&target.item_id).is_none_or(|cached| {
                    cached.schema_version != CACHE_SCHEMA_VERSION || cached.stale_at <= now
                })
            })
            .cloned()
            .collect::<Vec<_>>();

        let owned = self.claim(&refresh);
        let diagnostic = if owned.is_empty() {
            None
        } else {
            let result = self.refresh_plugin(&owned).err();
            self.release(&owned);
            cache = self
                .library
                .cached_ratings(&targets)
                .map_err(storage_error)?;
            self.sanitize_cached(&targets, &mut cache)
                .map_err(storage_error)?;
            result
        };

        let items = targets
            .iter()
            .filter_map(|target| {
                let cached = cache.get(&target.item_id)?;
                (cached.expires_at > now).then(|| {
                    json!({
                        "id": target.item_id,
                        "ratings": cached.ratings,
                        "origin": CACHE_ORIGIN,
                        "fetchedAt": cached.fetched_at.max(0),
                        "sourceUpdatedAt": cached.source_updated_at,
                        "stale": cached.stale_at <= now,
                        "schemaVersion": cached.schema_version,
                    })
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "available": true,
            "effectiveOrigin": CACHE_ORIGIN,
            "items": items,
            "retryAt": null,
            "diagnostic": diagnostic,
        }))
    }

    /// Rewrites tampered cache rows before they can feed a Desktop response.
    /// Fresh Companion results and persisted entries use the same allowlist.
    fn sanitize_cached(
        &self,
        targets: &[RatingTarget],
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

    fn claim(&self, targets: &[RatingTarget]) -> Vec<RatingTarget> {
        let Ok(mut in_flight) = self.in_flight.lock() else {
            return Vec::new();
        };
        targets
            .iter()
            .filter(|target| in_flight.insert(target_key(target)))
            .cloned()
            .collect()
    }

    fn release(&self, targets: &[RatingTarget]) {
        if let Ok(mut in_flight) = self.in_flight.lock() {
            for target in targets {
                in_flight.remove(&target_key(target));
            }
        }
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
        self.library
            .save_rating_cache(&entries)
            .map_err(|_| "could not cache server ratings; cached ratings remain usable".to_string())
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

fn unavailable_ratings() -> Value {
    json!({
        "available": false,
        "effectiveOrigin": "none",
        "items": [],
        "retryAt": null,
        "diagnostic": "Server ratings are unavailable.",
    })
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
    RatingsError::new("ratings storage is unavailable")
}

fn target_key(target: &RatingTarget) -> String {
    format!(
        "{}:{}:{}",
        target.provider, target.media_type, target.provider_id
    )
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
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::library::Library;

    use super::*;

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
}
