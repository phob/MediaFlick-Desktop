use std::collections::{HashMap, HashSet};

use rusqlite::{OptionalExtension, params, params_from_iter};
use serde_json::json;

use super::{CachedRatings, Library, RatingTarget, now_unix};

impl Library {
    /// Resolves requested cards to one preferred MDBList lookup identity.
    /// TMDB is numeric and cheapest to batch; IMDb is the stable fallback.
    pub fn rating_targets(&self, item_ids: &[String]) -> rusqlite::Result<Vec<RatingTarget>> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut unique = Vec::new();
        let mut seen = HashSet::new();
        for id in item_ids
            .iter()
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
        {
            if seen.insert(id.to_string()) {
                unique.push(id.to_string());
            }
            if unique.len() == 500 {
                break;
            }
        }
        if unique.is_empty() {
            return Ok(Vec::new());
        }
        self.db.with_connection(|connection| {
            let placeholders = vec!["?"; unique.len()].join(", ");
            let mut statement = connection.prepare(&format!(
                "SELECT jellyfin_id, kind, tmdb_id, imdb_id FROM items
                 WHERE kind IN ('Movie', 'Series') AND jellyfin_id IN ({placeholders})"
            ))?;
            statement
                .query_map(params_from_iter(unique.iter()), |row| {
                    let item_id: String = row.get(0)?;
                    let kind: String = row.get(1)?;
                    let tmdb_id = row.get::<_, Option<String>>(2)?.and_then(|id| {
                        let id = id.trim();
                        (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
                            .then(|| id.to_string())
                    });
                    let imdb_id = row.get::<_, Option<String>>(3)?.and_then(|id| {
                        let id = id.trim();
                        (id.len() <= 32
                            && id.starts_with("tt")
                            && id[2..].bytes().all(|byte| byte.is_ascii_digit()))
                        .then(|| id.to_string())
                    });
                    let (provider, provider_id) = tmdb_id
                        .map(|id| ("tmdb".to_string(), id))
                        .or_else(|| imdb_id.map(|id| ("imdb".to_string(), id)))
                        .unwrap_or_default();
                    Ok(RatingTarget {
                        item_id,
                        media_type: if kind == "Movie" { "movie" } else { "show" }.to_string(),
                        kind,
                        provider,
                        provider_id,
                    })
                })?
                .filter_map(|row| match row {
                    Ok(target) if !target.provider_id.is_empty() => Some(Ok(target)),
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                })
                .collect()
        })
    }

    pub fn cached_ratings(
        &self,
        targets: &[RatingTarget],
    ) -> rusqlite::Result<HashMap<String, CachedRatings>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT ratings, source_updated_at, fetched_at, stale_at, expires_at,
                        schema_version
                 FROM rating_cache
                 WHERE provider = ?1 AND provider_id = ?2 AND media_type = ?3",
            )?;
            let mut result = HashMap::new();
            for target in targets {
                let cached = statement
                    .query_row(
                        params![target.provider, target.provider_id, target.media_type],
                        |row| {
                            let raw: String = row.get(0)?;
                            Ok(CachedRatings {
                                ratings: serde_json::from_str(&raw).unwrap_or_else(|_| json!([])),
                                source_updated_at: row.get(1)?,
                                fetched_at: row.get(2)?,
                                stale_at: row.get(3)?,
                                expires_at: row.get(4)?,
                                schema_version: row.get(5)?,
                            })
                        },
                    )
                    .optional()?;
                if let Some(cached) = cached {
                    result.insert(target.item_id.clone(), cached);
                }
            }
            Ok(result)
        })
    }

    pub fn save_rating_cache(
        &self,
        entries: &[(RatingTarget, CachedRatings)],
    ) -> rusqlite::Result<()> {
        self.db.with_transaction(|transaction| {
            for (target, cached) in entries {
                transaction.execute(
                    "INSERT INTO rating_cache (
                         provider, provider_id, media_type, ratings, source_updated_at,
                         fetched_at, stale_at, expires_at, schema_version
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT(provider, provider_id, media_type) DO UPDATE SET
                         ratings = excluded.ratings,
                         source_updated_at = excluded.source_updated_at,
                         fetched_at = excluded.fetched_at,
                         stale_at = excluded.stale_at,
                         expires_at = excluded.expires_at,
                         schema_version = excluded.schema_version",
                    params![
                        target.provider,
                        target.provider_id,
                        target.media_type,
                        cached.ratings.to_string(),
                        cached.source_updated_at,
                        cached.fetched_at,
                        cached.stale_at,
                        cached.expires_at,
                        cached.schema_version,
                    ],
                )?;
            }
            transaction.execute(
                "DELETE FROM rating_cache WHERE expires_at < ?1",
                params![now_unix()],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CachedRatings, Library};
    use crate::library::test_support::dto;

    #[test]
    fn rating_targets_prefer_tmdb_and_cache_by_stable_identity() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(r#"{"Id":"m1","Name":"Movie","Type":"Movie","ProviderIds":{"Tmdb":"603","Imdb":"tt0133093"}}"#),
                dto(r#"{"Id":"s1","Name":"Show","Type":"Series","ProviderIds":{"Imdb":"tt0903747"}}"#),
                dto(r#"{"Id":"e1","Name":"Episode","Type":"Episode","ProviderIds":{"Tmdb":"1"}}"#),
            ])
            .expect("seed");
        let targets = library
            .rating_targets(&["m1".to_string(), "s1".to_string(), "e1".to_string()])
            .expect("targets");
        assert_eq!(targets.len(), 2);
        let movie = targets
            .iter()
            .find(|target| target.item_id == "m1")
            .expect("movie");
        assert_eq!(
            (movie.provider.as_str(), movie.provider_id.as_str()),
            ("tmdb", "603")
        );
        let show = targets
            .iter()
            .find(|target| target.item_id == "s1")
            .expect("show");
        assert_eq!(
            (show.provider.as_str(), show.provider_id.as_str()),
            ("imdb", "tt0903747")
        );

        let cached = CachedRatings {
            ratings: json!([{ "sourceId": "letterboxd", "value": 4.2 }]),
            fetched_at: 10,
            stale_at: 20,
            expires_at: i64::MAX,
            schema_version: 1,
            source_updated_at: Some("2026-08-04T20:00:00Z".to_string()),
        };
        library
            .save_rating_cache(&[(movie.clone(), cached.clone())])
            .expect("cache");
        assert_eq!(
            library
                .cached_ratings(std::slice::from_ref(movie))
                .expect("cached")["m1"],
            cached
        );
    }
}
