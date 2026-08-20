use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde_json::json;

use super::{CachedRatings, ExternalProfile, IntegrationState, Library, RatingTarget, now_unix};

impl Library {
    pub fn external_profiles(
        &self,
        provider: &str,
        jellyfin_server_id: &str,
        jellyfin_user_id: &str,
    ) -> rusqlite::Result<Vec<ExternalProfile>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, provider, profile_key, display_name, canonical_url, enabled,
                        verification_status, created_at, last_checked_at,
                        jellyfin_server_id, jellyfin_user_id
                 FROM external_profiles
                 WHERE provider = ?1 AND jellyfin_server_id = ?2 AND jellyfin_user_id = ?3
                 ORDER BY created_at DESC",
            )?;
            statement
                .query_map(
                    params![provider, jellyfin_server_id, jellyfin_user_id],
                    external_profile_from_row,
                )?
                .collect()
        })
    }

    pub fn save_external_profile(
        &self,
        profile: &ExternalProfile,
    ) -> rusqlite::Result<ExternalProfile> {
        self.db.with_connection(|connection| {
            connection.execute(
                "INSERT INTO external_profiles (
                     id, provider, profile_key, display_name, canonical_url,
                     jellyfin_server_id, jellyfin_user_id, enabled,
                     verification_status, created_at, last_checked_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(provider, profile_key, jellyfin_server_id, jellyfin_user_id)
                 DO UPDATE SET display_name = excluded.display_name,
                               canonical_url = excluded.canonical_url,
                               enabled = excluded.enabled,
                               verification_status = excluded.verification_status,
                               last_checked_at = excluded.last_checked_at",
                params![
                    profile.id,
                    profile.provider,
                    profile.profile_key,
                    profile.display_name,
                    profile.canonical_url,
                    profile.jellyfin_server_id,
                    profile.jellyfin_user_id,
                    profile.enabled,
                    profile.verification_status,
                    profile.created_at,
                    profile.last_checked_at,
                ],
            )?;
            connection.query_row(
                "SELECT id, provider, profile_key, display_name, canonical_url, enabled,
                        verification_status, created_at, last_checked_at,
                        jellyfin_server_id, jellyfin_user_id
                 FROM external_profiles
                 WHERE provider = ?1 AND profile_key = ?2
                   AND jellyfin_server_id = ?3 AND jellyfin_user_id = ?4",
                params![
                    profile.provider,
                    profile.profile_key,
                    profile.jellyfin_server_id,
                    profile.jellyfin_user_id,
                ],
                external_profile_from_row,
            )
        })
    }

    pub fn set_external_profile_enabled(
        &self,
        id: &str,
        jellyfin_server_id: &str,
        jellyfin_user_id: &str,
        enabled: bool,
    ) -> rusqlite::Result<Option<ExternalProfile>> {
        self.db.with_connection(|connection| {
            connection.execute(
                "UPDATE external_profiles SET enabled = ?1
                 WHERE id = ?2 AND jellyfin_server_id = ?3 AND jellyfin_user_id = ?4",
                params![enabled, id, jellyfin_server_id, jellyfin_user_id],
            )?;
            external_profile_by_id(connection, id, jellyfin_server_id, jellyfin_user_id)
        })
    }

    pub fn remove_external_profile(
        &self,
        id: &str,
        jellyfin_server_id: &str,
        jellyfin_user_id: &str,
    ) -> rusqlite::Result<bool> {
        self.db.with_connection(|connection| {
            let changed = connection.execute(
                "DELETE FROM external_profiles
                 WHERE id = ?1 AND jellyfin_server_id = ?2 AND jellyfin_user_id = ?3",
                params![id, jellyfin_server_id, jellyfin_user_id],
            )?;
            Ok(changed > 0)
        })
    }

    pub fn external_profile(
        &self,
        id: &str,
        jellyfin_server_id: &str,
        jellyfin_user_id: &str,
    ) -> rusqlite::Result<Option<ExternalProfile>> {
        self.db.with_connection(|connection| {
            external_profile_by_id(connection, id, jellyfin_server_id, jellyfin_user_id)
        })
    }

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
        origin: &str,
    ) -> rusqlite::Result<HashMap<String, CachedRatings>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT ratings, source_updated_at, fetched_at, stale_at, expires_at,
                        schema_version, origin
                 FROM rating_cache
                 WHERE provider = ?1 AND provider_id = ?2 AND media_type = ?3 AND origin = ?4",
            )?;
            let mut result = HashMap::new();
            for target in targets {
                let cached = statement
                    .query_row(
                        params![
                            target.provider,
                            target.provider_id,
                            target.media_type,
                            origin
                        ],
                        |row| {
                            let raw: String = row.get(0)?;
                            Ok(CachedRatings {
                                ratings: serde_json::from_str(&raw).unwrap_or_else(|_| json!([])),
                                source_updated_at: row.get(1)?,
                                fetched_at: row.get(2)?,
                                stale_at: row.get(3)?,
                                expires_at: row.get(4)?,
                                schema_version: row.get(5)?,
                                origin: row.get(6)?,
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
                         fetched_at, stale_at, expires_at, schema_version, origin
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT(provider, provider_id, media_type, origin) DO UPDATE SET
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
                        cached.origin,
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

    pub fn integration_state(&self, service: &str) -> rusqlite::Result<Option<IntegrationState>> {
        self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT service, validation, valid, detail, quota_limit, quota_remaining,
                            quota_reset_at, retry_at, failure_count, updated_at
                     FROM integration_state WHERE service = ?1",
                    params![service],
                    |row| {
                        Ok(IntegrationState {
                            service: row.get(0)?,
                            validation: row.get(1)?,
                            valid: row.get::<_, i64>(2)? != 0,
                            detail: row.get(3)?,
                            quota_limit: row.get(4)?,
                            quota_remaining: row.get(5)?,
                            quota_reset_at: row.get(6)?,
                            retry_at: row.get(7)?,
                            failure_count: row.get(8)?,
                            updated_at: row.get(9)?,
                        })
                    },
                )
                .optional()
        })
    }

    pub fn save_integration_state(&self, state: &IntegrationState) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "INSERT INTO integration_state (
                     service, validation, valid, detail, quota_limit, quota_remaining,
                     quota_reset_at, retry_at, failure_count, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(service) DO UPDATE SET
                     validation = excluded.validation,
                     valid = excluded.valid,
                     detail = excluded.detail,
                     quota_limit = excluded.quota_limit,
                     quota_remaining = excluded.quota_remaining,
                     quota_reset_at = excluded.quota_reset_at,
                     retry_at = excluded.retry_at,
                     failure_count = excluded.failure_count,
                     updated_at = excluded.updated_at",
                params![
                    state.service,
                    state.validation,
                    state.valid,
                    state.detail,
                    state.quota_limit,
                    state.quota_remaining,
                    state.quota_reset_at,
                    state.retry_at,
                    state.failure_count,
                    state.updated_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn clear_integration_state(&self, service: &str) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "DELETE FROM integration_state WHERE service = ?1",
                params![service],
            )?;
            Ok(())
        })
    }
}

fn external_profile_by_id(
    connection: &Connection,
    id: &str,
    jellyfin_server_id: &str,
    jellyfin_user_id: &str,
) -> rusqlite::Result<Option<ExternalProfile>> {
    match connection.query_row(
        "SELECT id, provider, profile_key, display_name, canonical_url, enabled,
                verification_status, created_at, last_checked_at,
                jellyfin_server_id, jellyfin_user_id
         FROM external_profiles
         WHERE id = ?1 AND jellyfin_server_id = ?2 AND jellyfin_user_id = ?3",
        params![id, jellyfin_server_id, jellyfin_user_id],
        external_profile_from_row,
    ) {
        Ok(profile) => Ok(Some(profile)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error),
    }
}

fn external_profile_from_row(row: &Row<'_>) -> rusqlite::Result<ExternalProfile> {
    Ok(ExternalProfile {
        id: row.get(0)?,
        provider: row.get(1)?,
        profile_key: row.get(2)?,
        display_name: row.get(3)?,
        canonical_url: row.get(4)?,
        enabled: row.get::<_, i64>(5)? != 0,
        verification_status: row.get(6)?,
        created_at: row.get(7)?,
        last_checked_at: row.get(8)?,
        jellyfin_server_id: row.get(9)?,
        jellyfin_user_id: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CachedRatings, ExternalProfile, IntegrationState, Library};
    use crate::library::test_support::dto;

    #[test]
    fn external_profiles_are_scoped_to_the_jellyfin_account() {
        let library = Library::open_in_memory().expect("library");
        let profile = ExternalProfile {
            id: "profile-a".to_string(),
            provider: "letterboxd".to_string(),
            profile_key: "alice".to_string(),
            display_name: "alice".to_string(),
            canonical_url: "https://letterboxd.com/alice/".to_string(),
            enabled: true,
            verification_status: "verified".to_string(),
            created_at: 1,
            last_checked_at: Some(2),
            jellyfin_server_id: "server-a".to_string(),
            jellyfin_user_id: "user-a".to_string(),
        };
        let saved = library.save_external_profile(&profile).expect("save");
        assert_eq!(saved.profile_key, "alice");
        assert_eq!(
            library
                .external_profiles("letterboxd", "server-a", "user-a")
                .expect("profiles")
                .len(),
            1
        );
        assert!(
            library
                .external_profiles("letterboxd", "server-a", "user-b")
                .expect("other account")
                .is_empty()
        );
        assert!(
            library
                .set_external_profile_enabled("profile-a", "server-a", "user-b", false)
                .expect("set other account")
                .is_none()
        );
    }

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
            origin: "local_mdblist".to_string(),
            source_updated_at: Some("2026-08-04T20:00:00Z".to_string()),
        };
        library
            .save_rating_cache(&[(movie.clone(), cached.clone())])
            .expect("cache");
        assert_eq!(
            library
                .cached_ratings(std::slice::from_ref(movie), "local_mdblist")
                .expect("cached")["m1"],
            cached
        );
    }

    #[test]
    fn integration_health_persists_without_a_secret_column() {
        let library = Library::open_in_memory().expect("library");
        let state = IntegrationState {
            service: "mdblist-api-key".to_string(),
            validation: "rate_limited".to_string(),
            valid: true,
            detail: Some("quota exhausted".to_string()),
            quota_limit: Some(1000),
            quota_remaining: Some(0),
            retry_at: Some(1234),
            updated_at: 100,
            ..IntegrationState::default()
        };
        library.save_integration_state(&state).expect("save state");
        assert_eq!(
            library.integration_state("mdblist-api-key").expect("state"),
            Some(state)
        );
        library
            .clear_integration_state("mdblist-api-key")
            .expect("clear");
        assert_eq!(
            library.integration_state("mdblist-api-key").expect("state"),
            None
        );
    }
}
