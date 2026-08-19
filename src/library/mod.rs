//! Local catalog index.
//!
//! The UI browses, sorts, filters, and searches exclusively from here; rich
//! metadata (synopsis, cast, technical streams) is fetched live from Jellyfin
//! by the surfaces that need it. A background thread ([`sync`]) keeps the thin
//! index current.

pub mod db;
pub mod headless;
pub mod model;
pub mod sync;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde_json::{Value, json};

use crate::app::ids::new_device_id;
use crate::jellyfin::api::model::BaseItemDto;

use model::is_synced_kind;
pub use model::{
    ItemPlaybackPreference, ItemRecord, LibraryStats, UserDataRecord, resolve_playback_preference,
};

/// Which cached rows one committed mutation touched, so the UI can invalidate
/// exactly the affected items and their parent/series/season contexts.
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

fn record_change(changes: &mut LibraryChangeBatch, dto: &BaseItemDto) {
    record_change_identity(
        changes,
        &dto.id,
        [
            dto.parent_id.as_deref(),
            dto.series_id.as_deref(),
            dto.season_id.as_deref(),
        ],
    );
}

fn record_change_identity(
    changes: &mut LibraryChangeBatch,
    item_id: &str,
    contexts: [Option<&str>; 3],
) {
    changes.item_ids.push(item_id.to_string());
    for context in contexts.into_iter().flatten() {
        if !context.trim().is_empty() {
            changes.context_ids.push(context.to_string());
        }
    }
    normalize_ids(&mut changes.item_ids);
    normalize_ids(&mut changes.context_ids);
}

fn normalize_ids(ids: &mut Vec<String>) {
    ids.sort_unstable();
    ids.dedup();
}

/// Session details persisted across restarts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredCredentials {
    pub server_url: Option<String>,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub server_id: Option<String>,
    pub device_id: String,
    pub token: Option<String>,
}

/// Stable external identity used to batch rating lookups.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RatingTarget {
    pub item_id: String,
    pub kind: String,
    pub media_type: String,
    pub provider: String,
    pub provider_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CachedRatings {
    pub ratings: Value,
    pub source_updated_at: Option<String>,
    pub fetched_at: i64,
    pub stale_at: i64,
    pub expires_at: i64,
    pub schema_version: i64,
    pub origin: String,
}

/// Non-secret validation and quota state. API keys are held by the OS vault.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntegrationState {
    pub service: String,
    pub validation: String,
    pub valid: bool,
    pub detail: Option<String>,
    pub quota_limit: Option<i64>,
    pub quota_remaining: Option<i64>,
    pub quota_reset_at: Option<i64>,
    pub retry_at: Option<i64>,
    pub failure_count: i64,
    pub updated_at: i64,
}

/// An optional, public profile associated with one Jellyfin account.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalProfile {
    pub id: String,
    pub provider: String,
    pub profile_key: String,
    pub display_name: String,
    pub canonical_url: String,
    pub enabled: bool,
    pub verification_status: String,
    pub created_at: i64,
    pub last_checked_at: Option<i64>,
    #[serde(skip)]
    pub jellyfin_server_id: String,
    #[serde(skip)]
    pub jellyfin_user_id: String,
}

impl StoredCredentials {
    pub fn is_authenticated(&self) -> bool {
        self.token.is_some() && self.user_id.is_some() && self.server_url.is_some()
    }
}

/// The Seerr link, persisted as the single row of `seerr_config`.
///
/// `jellyfin_server_id` / `jellyfin_user_id` are the Jellyfin account the link
/// was made under; everything that acquires a Seerr client checks them, so an
/// account switch cannot leave one user's Seerr cookie serving another's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeerrConfig {
    pub base_url: Option<String>,
    /// The whole cookie set as JSON, not just the session cookie: a
    /// CSRF-protected instance needs its `_csrf` / `XSRF-TOKEN` pair back too.
    pub cookies: Option<String>,
    pub user_id: Option<i64>,
    pub user_name: Option<String>,
    pub jellyfin_server_id: Option<String>,
    pub jellyfin_user_id: Option<String>,
    pub movie_4k_enabled: bool,
    pub series_4k_enabled: bool,
    pub partial_requests_enabled: bool,
}

/// How the library grid is ordered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ItemSort {
    #[default]
    Name,
    Year,
    DateAdded,
    CommunityRating,
}

impl ItemSort {
    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "name" => Some(Self::Name),
            "year" => Some(Self::Year),
            "added" | "dateadded" => Some(Self::DateAdded),
            "rating" | "communityrating" => Some(Self::CommunityRating),
            _ => None,
        }
    }

    fn order_clause(self) -> &'static str {
        match self {
            Self::Name => "i.sort_name COLLATE NOCASE ASC, i.name COLLATE NOCASE ASC, i.id ASC",
            Self::Year => "i.year DESC NULLS LAST, i.sort_name COLLATE NOCASE ASC, i.id ASC",
            Self::DateAdded => "i.date_created DESC NULLS LAST, i.id DESC",
            Self::CommunityRating => {
                "i.community_rating DESC NULLS LAST, i.sort_name COLLATE NOCASE ASC, i.id ASC"
            }
        }
    }
}

/// Filters for the library grid.
#[derive(Debug, Clone, Default)]
pub struct ItemQuery {
    pub search: Option<String>,
    pub kinds: Vec<String>,
    pub genre: Option<String>,
    /// Inclusive first year of a standard ten-year release decade.
    pub release_decade: Option<i64>,
    pub parent_id: Option<String>,
    pub series_id: Option<String>,
    pub watched: Option<bool>,
    pub favorite: Option<bool>,
    pub sort: ItemSort,
    pub offset: i64,
    pub limit: i64,
}

pub const EARLIEST_RELEASE_DECADE: i64 = 1900;

/// The newest standard decade that can be selected by the library UI.
pub fn current_release_decade() -> i64 {
    let days = now_unix().div_euclid(86_400);
    let (year, _, _) = civil_from_days(days);
    year.div_euclid(10) * 10
}

/// Parses only decade starts that the library UI can represent. Keeping this
/// at the API boundary means hand-written URLs cannot turn a future or
/// five-year bucket into an accidental query contract.
pub fn release_decade_from_id(value: &str) -> Option<i64> {
    let decade = value.trim().parse::<i64>().ok()?;
    (decade >= EARLIEST_RELEASE_DECADE
        && decade <= current_release_decade()
        && decade.rem_euclid(10) == 0)
        .then_some(decade)
}

#[derive(Debug, Clone)]
pub struct ItemPage {
    pub items: Vec<Value>,
    pub total: i64,
}

pub struct Library {
    db: db::Database,
}

impl Library {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let library = Self {
            db: db::Database::open(path)?,
        };
        library.ensure_device_id()?;
        Ok(library)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let library = Self {
            db: db::Database::open_in_memory()?,
        };
        library.ensure_device_id()?;
        Ok(library)
    }

    pub fn path(&self) -> &Path {
        self.db.path()
    }

    // ---------------------------------------------------------------- session

    fn ensure_device_id(&self) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "INSERT INTO credentials (id, device_id, updated_at) VALUES (1, ?1, ?2)
                 ON CONFLICT(id) DO NOTHING",
                params![new_device_id(), now_unix()],
            )?;
            Ok(())
        })
    }

    pub fn credentials(&self) -> StoredCredentials {
        self.db
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT server_url, user_id, user_name, server_id, device_id, token
                     FROM credentials WHERE id = 1",
                    [],
                    |row| {
                        Ok(StoredCredentials {
                            server_url: row.get(0)?,
                            user_id: row.get(1)?,
                            user_name: row.get(2)?,
                            server_id: row.get(3)?,
                            device_id: row.get(4)?,
                            token: row.get(5)?,
                        })
                    },
                )
            })
            .unwrap_or_default()
    }

    pub fn save_credentials(&self, credentials: &StoredCredentials) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "UPDATE credentials SET server_url = ?1, user_id = ?2, user_name = ?3,
                     server_id = ?4, token = ?5, updated_at = ?6 WHERE id = 1",
                params![
                    credentials.server_url,
                    credentials.user_id,
                    credentials.user_name,
                    credentials.server_id,
                    credentials.token,
                    now_unix(),
                ],
            )?;
            Ok(())
        })
    }

    /// Drops the token but keeps the server URL so the login screen stays
    /// prefilled, and wipes cached metadata that belonged to that account.
    pub fn clear_session(&self, forget_library: bool) -> rusqlite::Result<()> {
        self.db.with_transaction(|transaction| {
            transaction.execute(
                "UPDATE credentials SET user_id = NULL, user_name = NULL, token = NULL,
                     updated_at = ?1 WHERE id = 1",
                params![now_unix()],
            )?;
            if forget_library {
                transaction.execute("DELETE FROM items", [])?;
                transaction.execute("DELETE FROM user_data", [])?;
                transaction.execute("DELETE FROM meta", [])?;
            }
            Ok(())
        })
    }

    // -------------------------------------------------------- external profiles

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

    // ------------------------------------------------------------------- seerr

    #[cfg(test)]
    pub fn seerr_config(&self) -> SeerrConfig {
        self.seerr_config_snapshot().0
    }

    /// Reads the link together with the opaque revision used to prevent a
    /// long-running status probe from overwriting a newer link.
    pub(crate) fn seerr_config_snapshot(&self) -> (SeerrConfig, i64) {
        self.try_seerr_config_snapshot()
            .unwrap_or_else(|error| {
                tracing::warn!(
                    target: "library.db",
                    "could not read the Seerr config: {error}"
                );
                None
            })
            .unwrap_or_default()
    }

    /// Unlike [`Self::seerr_config_snapshot`], preserves storage failures so a
    /// live session does not mistake a transient read error for an empty row.
    pub(crate) fn try_seerr_config_snapshot(&self) -> rusqlite::Result<Option<(SeerrConfig, i64)>> {
        match self.db.with_connection(|connection| {
            connection.query_row(
                "SELECT base_url, cookies, user_id, user_name, jellyfin_server_id,
                     jellyfin_user_id, movie_4k_enabled, series_4k_enabled,
                     partial_requests_enabled, updated_at
                 FROM seerr_config WHERE id = 1",
                [],
                |row| {
                    Ok((
                        SeerrConfig {
                            base_url: row.get(0)?,
                            cookies: row.get(1)?,
                            user_id: row.get(2)?,
                            user_name: row.get(3)?,
                            jellyfin_server_id: row.get(4)?,
                            jellyfin_user_id: row.get(5)?,
                            movie_4k_enabled: row.get(6)?,
                            series_4k_enabled: row.get(7)?,
                            partial_requests_enabled: row.get(8)?,
                        },
                        row.get(9)?,
                    ))
                },
            )
        }) {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Upserts rather than updates: unlike `credentials`, this row is only
    /// created when a Seerr instance is first configured.
    #[cfg(test)]
    pub fn save_seerr_config(&self, config: &SeerrConfig) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "INSERT INTO seerr_config (id, base_url, cookies, user_id, user_name,
                     jellyfin_server_id, jellyfin_user_id, movie_4k_enabled,
                     series_4k_enabled, partial_requests_enabled, updated_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                     base_url = excluded.base_url,
                     cookies = excluded.cookies,
                     user_id = excluded.user_id,
                     user_name = excluded.user_name,
                     jellyfin_server_id = excluded.jellyfin_server_id,
                     jellyfin_user_id = excluded.jellyfin_user_id,
                     movie_4k_enabled = excluded.movie_4k_enabled,
                     series_4k_enabled = excluded.series_4k_enabled,
                     partial_requests_enabled = excluded.partial_requests_enabled,
                     updated_at = seerr_config.updated_at + 1",
                params![
                    config.base_url,
                    config.cookies,
                    config.user_id,
                    config.user_name,
                    config.jellyfin_server_id,
                    config.jellyfin_user_id,
                    config.movie_4k_enabled,
                    config.series_4k_enabled,
                    config.partial_requests_enabled,
                    1,
                ],
            )?;
            Ok(())
        })
    }

    /// Saves only if nobody has changed the link since `expected_revision` was
    /// read. `None` means the caller's snapshot is stale and was not written.
    pub(crate) fn save_seerr_config_if_revision(
        &self,
        config: &SeerrConfig,
        expected_revision: i64,
    ) -> rusqlite::Result<Option<i64>> {
        self.db.with_connection(|connection| {
            let changed = connection.execute(
                "INSERT INTO seerr_config (id, base_url, cookies, user_id, user_name,
                     jellyfin_server_id, jellyfin_user_id, movie_4k_enabled,
                     series_4k_enabled, partial_requests_enabled, updated_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)
                 ON CONFLICT(id) DO UPDATE SET
                     base_url = excluded.base_url,
                     cookies = excluded.cookies,
                     user_id = excluded.user_id,
                     user_name = excluded.user_name,
                     jellyfin_server_id = excluded.jellyfin_server_id,
                     jellyfin_user_id = excluded.jellyfin_user_id,
                     movie_4k_enabled = excluded.movie_4k_enabled,
                     series_4k_enabled = excluded.series_4k_enabled,
                     partial_requests_enabled = excluded.partial_requests_enabled,
                     updated_at = seerr_config.updated_at + 1
                 WHERE seerr_config.updated_at = ?10",
                params![
                    config.base_url,
                    config.cookies,
                    config.user_id,
                    config.user_name,
                    config.jellyfin_server_id,
                    config.jellyfin_user_id,
                    config.movie_4k_enabled,
                    config.series_4k_enabled,
                    config.partial_requests_enabled,
                    expected_revision,
                ],
            )?;
            if changed == 0 {
                return Ok(None);
            }
            connection
                .query_row(
                    "SELECT updated_at FROM seerr_config WHERE id = 1",
                    [],
                    |row| row.get(0),
                )
                .map(Some)
        })
    }

    /// Drops the Seerr session and the account binding, keeping the instance
    /// address so re-linking does not mean retyping it.
    #[cfg(test)]
    pub fn clear_seerr_link(&self) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "UPDATE seerr_config SET cookies = NULL, user_id = NULL, user_name = NULL,
                     jellyfin_server_id = NULL, jellyfin_user_id = NULL,
                     updated_at = updated_at + 1
                 WHERE id = 1",
                [],
            )?;
            Ok(())
        })
    }

    // ------------------------------------------------------------------- meta

    pub fn meta(&self, key: &str) -> Option<String> {
        self.db
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT value FROM meta WHERE key = ?1",
                    params![key],
                    |row| row.get::<_, String>(0),
                )
            })
            .ok()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    // --------------------------------------------------------------- ratings

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
            // Expired data has no stale-while-revalidate value. Keep cleanup
            // bounded to successful cache writes rather than startup/catalog.
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

    // ------------------------------------------------------------------ write

    /// Ingests server DTOs into the thin index (plus their user data) in one
    /// transaction, reporting which rows and contexts actually moved — a DTO
    /// identical to its cached row is not a change, so the periodic full
    /// re-page of a quiet library invalidates nothing. Non-library kinds —
    /// people, folders — are skipped so live single-item fetches cannot
    /// pollute browsing.
    pub fn ingest_page(&self, dtos: &[BaseItemDto]) -> rusqlite::Result<LibraryChangeBatch> {
        self.db.with_transaction(|transaction| {
            let mut changes = LibraryChangeBatch::default();
            for dto in dtos {
                let kind = dto.item_type.as_deref().unwrap_or_default();
                if dto.id.trim().is_empty() || !is_synced_kind(kind) {
                    continue;
                }
                let mut moved = upsert_item(transaction, &ItemRecord::from_dto(dto))?;
                if let Some(user_data) = &dto.user_data {
                    moved |= upsert_user_data(
                        transaction,
                        &UserDataRecord::from_dto(&dto.id, user_data),
                    )?;
                }
                if moved {
                    record_change(&mut changes, dto);
                }
            }
            Ok(changes)
        })
    }

    /// Upserts a page of items (and their user data) in one transaction.
    pub fn upsert_page(&self, items: &[BaseItemDto]) -> rusqlite::Result<usize> {
        self.ingest_page(items)
            .map(|changes| changes.item_ids.len())
    }

    /// Applies pushed watch-state changes for items already in the catalog,
    /// returning the change batch so the UI can invalidate the items and
    /// their series/season contexts. Unknown ids are skipped: the item sweep
    /// delivers the row and its watch state together, and an orphan
    /// user-data row would notify nothing anyone can see.
    pub fn apply_user_data(
        &self,
        records: &[UserDataRecord],
    ) -> rusqlite::Result<LibraryChangeBatch> {
        self.db.with_transaction(|transaction| {
            let mut changes = LibraryChangeBatch::default();
            for record in records {
                let contexts = transaction
                    .query_row(
                        "SELECT parent_id, series_id, season_id FROM items WHERE jellyfin_id = ?1",
                        params![record.jellyfin_id],
                        |row| {
                            Ok((
                                row.get::<_, Option<String>>(0)?,
                                row.get::<_, Option<String>>(1)?,
                                row.get::<_, Option<String>>(2)?,
                            ))
                        },
                    )
                    .optional()?;
                let Some((parent_id, series_id, season_id)) = contexts else {
                    continue;
                };
                if upsert_user_data(transaction, record)? {
                    record_change_identity(
                        &mut changes,
                        &record.jellyfin_id,
                        [
                            parent_id.as_deref(),
                            series_id.as_deref(),
                            season_id.as_deref(),
                        ],
                    );
                }
            }
            Ok(changes)
        })
    }

    /// Mirrors a page of watch state, returning how many rows actually moved.
    pub fn upsert_user_data(&self, records: &[UserDataRecord]) -> rusqlite::Result<usize> {
        self.db.with_transaction(|transaction| {
            let mut refreshed = 0;
            for record in records {
                if upsert_user_data(transaction, record)? {
                    refreshed += 1;
                }
            }
            Ok(refreshed)
        })
    }

    /// Removes cached items the server no longer reports, retaining their old
    /// hierarchy long enough for every affected UI context to be invalidated.
    pub fn retain_ids(&self, keep: &HashSet<String>) -> rusqlite::Result<LibraryChangeBatch> {
        self.delete_missing(&self.all_ids()?, keep)
    }

    /// Atomically replaces one live container's direct child snapshot. No
    /// page is exposed on its own: either the complete server answer commits,
    /// including deletions, or the cached list remains untouched.
    pub fn reconcile_children(
        &self,
        parent_id: &str,
        dtos: &[BaseItemDto],
    ) -> rusqlite::Result<LibraryChangeBatch> {
        self.db.with_transaction(|transaction| {
            let existing = child_ids_from_connection(transaction, parent_id)?;
            let mut keep = HashSet::new();
            let mut changes = LibraryChangeBatch::default();

            for dto in dtos {
                let kind = dto.item_type.as_deref().unwrap_or_default();
                if dto.id.trim().is_empty() || !is_synced_kind(kind) {
                    continue;
                }
                keep.insert(dto.id.clone());
                let mut moved = upsert_item(transaction, &ItemRecord::from_dto(dto))?;
                if let Some(user_data) = &dto.user_data {
                    moved |= upsert_user_data(
                        transaction,
                        &UserDataRecord::from_dto(&dto.id, user_data),
                    )?;
                }
                if moved {
                    record_change(&mut changes, dto);
                }
            }

            delete_missing_from_transaction(transaction, &existing, &keep, &mut changes)?;
            Ok(changes)
        })
    }

    fn delete_missing(
        &self,
        existing: &HashSet<String>,
        keep: &HashSet<String>,
    ) -> rusqlite::Result<LibraryChangeBatch> {
        let stale = existing.difference(keep).cloned().collect::<Vec<_>>();
        if stale.is_empty() {
            return Ok(LibraryChangeBatch::default());
        }
        self.db.with_transaction(|transaction| {
            let mut changes = LibraryChangeBatch::default();
            delete_items_from_transaction(transaction, &stale, &mut changes)?;
            Ok(changes)
        })
    }

    /// Maps each requested card id to the item whose media streams answer its
    /// technical badge. Movies and episodes answer for themselves; Series and
    /// Season rows are containers Jellyfin reports without streams, so they
    /// are represented by their first episode in season/episode order, with
    /// specials sorting last. A container with no cached episode is omitted —
    /// no live query could describe it. Ids the cache has never seen (deep
    /// links) are probed as themselves.
    pub fn technical_stream_sources(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<(String, String)>> {
        self.db.with_connection(|connection| {
            let mut sources = Vec::with_capacity(ids.len());
            for id in ids {
                let kind = connection
                    .query_row(
                        "SELECT kind FROM items WHERE jellyfin_id = ?1",
                        params![id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                let source = match kind.as_deref() {
                    Some("Series") => connection
                        .query_row(
                            "SELECT jellyfin_id FROM items
                             WHERE kind = 'Episode' AND series_id = ?1
                             ORDER BY parent_index_number IS NULL OR parent_index_number < 1,
                                      parent_index_number, index_number
                             LIMIT 1",
                            params![id],
                            |row| row.get::<_, String>(0),
                        )
                        .optional()?,
                    // The same episode predicate the season reconcile reads.
                    Some("Season") => connection
                        .query_row(
                            "SELECT jellyfin_id FROM items
                             WHERE kind = 'Episode' AND (parent_id = ?1 OR season_id = ?1)
                             ORDER BY index_number LIMIT 1",
                            params![id],
                            |row| row.get::<_, String>(0),
                        )
                        .optional()?,
                    _ => Some(id.clone()),
                };
                if let Some(source) = source {
                    sources.push((id.clone(), source));
                }
            }
            Ok(sources)
        })
    }

    /// The item type of a cached item, used to decide whether a parent is worth
    /// reconciling against the server at all.
    pub fn kind(&self, item_id: &str) -> Option<String> {
        self.db
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT kind FROM items WHERE jellyfin_id = ?1",
                    params![item_id],
                    |row| row.get::<_, String>(0),
                )
            })
            .ok()
    }

    /// Resolves TMDB ids to the cached items that carry them, for one item kind.
    ///
    /// The kind is a parameter rather than an afterthought because TMDB numbers
    /// movies and series in separate namespaces: id 603 is *The Matrix* and also
    /// a completely unrelated series, so a join on the id alone would offer
    /// Play on the wrong title. Callers pass `Movie` or `Series` and get back
    /// only ids of that kind.
    pub fn ids_by_tmdb(
        &self,
        kind: &str,
        tmdb_ids: &[String],
    ) -> rusqlite::Result<HashMap<String, String>> {
        self.ids_by_provider(kind, "tmdb_id", tmdb_ids)
    }

    /// Resolves TVDB ids to cached items of one kind. Calendar episodes often
    /// have a TVDB id even where Sonarr cannot provide a TMDB episode id.
    pub fn ids_by_tvdb(
        &self,
        kind: &str,
        tvdb_ids: &[String],
    ) -> rusqlite::Result<HashMap<String, String>> {
        self.ids_by_provider(kind, "tvdb_id", tvdb_ids)
    }

    fn ids_by_provider(
        &self,
        kind: &str,
        column: &str,
        provider_ids: &[String],
    ) -> rusqlite::Result<HashMap<String, String>> {
        if provider_ids.is_empty() {
            return Ok(HashMap::new());
        }
        debug_assert!(matches!(column, "tmdb_id" | "tvdb_id"));
        self.db.with_connection(|connection| {
            let placeholders = vec!["?"; provider_ids.len()].join(", ");
            let mut statement = connection.prepare(&format!(
                "SELECT {column}, jellyfin_id FROM items
                 WHERE kind = ?1 AND {column} IN ({placeholders})"
            ))?;
            let arguments = std::iter::once(kind)
                .chain(provider_ids.iter().map(String::as_str))
                .collect::<Vec<_>>();
            let rows = statement
                .query_map(params_from_iter(arguments), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<HashMap<_, _>>>()?;
            Ok(rows)
        })
    }

    /// Drops one item the server has authoritatively disowned.
    ///
    /// Jellyfin re-creates an item with a new id when its file is replaced, so
    /// the old row would otherwise linger — offering a dead poster and a dead
    /// Play button — until the next daily deletion sweep.
    pub fn forget(&self, item_id: &str) -> rusqlite::Result<LibraryChangeBatch> {
        self.db.with_transaction(|transaction| {
            let mut changes = LibraryChangeBatch::default();
            delete_items_from_transaction(transaction, &[item_id.to_string()], &mut changes)?;
            Ok(changes)
        })
    }

    pub fn all_ids(&self) -> rusqlite::Result<HashSet<String>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare("SELECT jellyfin_id FROM items")?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<HashSet<_>>>()?;
            Ok(ids)
        })
    }

    /// Mirrors a played/favorite toggle locally so the UI updates without a
    /// server round trip.
    pub fn set_local_played(&self, item_id: &str, played: bool) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "INSERT INTO user_data (jellyfin_id, played, play_count, playback_position_ticks, updated_at)
                 VALUES (?1, ?2, ?3, 0, ?4)
                 ON CONFLICT(jellyfin_id) DO UPDATE SET
                     played = excluded.played,
                     play_count = CASE WHEN excluded.played = 1
                         THEN max(user_data.play_count, 1) ELSE 0 END,
                     playback_position_ticks = 0,
                     updated_at = excluded.updated_at",
                params![item_id, played, i64::from(played), now_unix()],
            )?;
            Ok(())
        })
    }

    pub fn set_local_favorite(&self, item_id: &str, favorite: bool) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "INSERT INTO user_data (jellyfin_id, is_favorite, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(jellyfin_id) DO UPDATE SET
                     is_favorite = excluded.is_favorite, updated_at = excluded.updated_at",
                params![item_id, favorite, now_unix()],
            )?;
            Ok(())
        })
    }

    // ----------------------------------------------------- playback preference

    /// The source and tracks explicitly saved for one exact Jellyfin item and
    /// the currently signed-in Jellyfin account.
    /// Invalid legacy/corrupt JSON degrades to no preference so detail and play
    /// paths retain their normal server defaults instead of failing.
    pub fn playback_preference(
        &self,
        item_id: &str,
    ) -> rusqlite::Result<Option<ItemPlaybackPreference>> {
        let row = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT preference.media_source, preference.audio_track,
                            preference.subtitle_track
                     FROM item_playback_preferences preference
                     JOIN credentials account ON account.id = 1
                     WHERE preference.jellyfin_id = ?1
                       AND preference.jellyfin_server_key =
                           COALESCE(NULLIF(account.server_id, ''), NULLIF(account.server_url, ''))
                       AND preference.jellyfin_user_id = account.user_id",
                    params![item_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()
        })?;
        let Some((source, audio, subtitle)) = row else {
            return Ok(None);
        };
        let Ok(media_source) = serde_json::from_str(&source) else {
            return Ok(None);
        };
        let audio_track = match audio {
            Some(raw) => match serde_json::from_str(&raw) {
                Ok(track) => Some(track),
                Err(_) => return Ok(None),
            },
            None => None,
        };
        let subtitle_track = match subtitle {
            Some(raw) => match serde_json::from_str(&raw) {
                Ok(track) => Some(track),
                Err(_) => return Ok(None),
            },
            None => None,
        };
        Ok(Some(ItemPlaybackPreference {
            media_source,
            audio_track,
            subtitle_track,
        }))
    }

    /// Atomically replaces both selections for an item. Keeping source, audio,
    /// and subtitle in one row prevents a source change from leaving track
    /// indices that belong to the previous file.
    pub fn save_playback_preference(
        &self,
        item_id: &str,
        preference: &ItemPlaybackPreference,
    ) -> rusqlite::Result<()> {
        let source = serde_json::to_string(&preference.media_source)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let audio = preference
            .audio_track
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let subtitle = preference
            .subtitle_track
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        self.db.with_connection(|connection| {
            let (server_key, user_id) = connection.query_row(
                "SELECT COALESCE(NULLIF(server_id, ''), NULLIF(server_url, '')), user_id
                 FROM credentials
                 WHERE id = 1 AND user_id IS NOT NULL",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?;
            connection.execute(
                "INSERT INTO item_playback_preferences (
                     jellyfin_id, jellyfin_server_key, jellyfin_user_id,
                     media_source, audio_track, subtitle_track, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(jellyfin_id, jellyfin_server_key, jellyfin_user_id) DO UPDATE SET
                     media_source = excluded.media_source,
                     audio_track = excluded.audio_track,
                     subtitle_track = excluded.subtitle_track,
                     updated_at = excluded.updated_at",
                params![
                    item_id,
                    server_key,
                    user_id,
                    source,
                    audio,
                    subtitle,
                    now_unix()
                ],
            )?;
            Ok(())
        })
    }

    // -------------------------------------------------------------------read

    pub fn stats(&self) -> LibraryStats {
        self.db
            .with_connection(|connection| {
                let count = |kind: &str| -> rusqlite::Result<i64> {
                    connection.query_row(
                        "SELECT count(*) FROM items WHERE kind = ?1",
                        params![kind],
                        |row| row.get(0),
                    )
                };
                Ok(LibraryStats {
                    movies: count("Movie")?,
                    series: count("Series")?,
                    seasons: count("Season")?,
                    episodes: count("Episode")?,
                    total: connection
                        .query_row("SELECT count(*) FROM items", [], |row| row.get(0))?,
                })
            })
            .unwrap_or_default()
    }

    pub fn query(&self, query: &ItemQuery) -> rusqlite::Result<ItemPage> {
        let (from_clause, conditions, mut arguments) = query_base(query);
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        let total: i64 = self.db.with_connection(|connection| {
            connection.query_row(
                &format!("SELECT count(*) {from_clause}{where_clause}"),
                params_from_iter(arguments.iter()),
                |row| row.get(0),
            )
        })?;

        // Relevance beats alphabetical order while the user is typing.
        let order = if query.search.is_some() {
            "bm25(items_fts), i.id ASC".to_string()
        } else {
            query.sort.order_clause().to_string()
        };
        arguments.push(SqlValue::Integer(query.limit.clamp(1, 500)));
        arguments.push(SqlValue::Integer(query.offset.max(0)));

        let items = self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {SUMMARY_COLUMNS} {from_clause}{where_clause} ORDER BY {order} LIMIT ? OFFSET ?"
            ))?;
            let rows = statement
                .query_map(params_from_iter(arguments.iter()), summary_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;

        Ok(ItemPage { items, total })
    }

    pub fn item(&self, item_id: &str) -> rusqlite::Result<Option<Value>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {DETAIL_COLUMNS} FROM items i
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 WHERE i.jellyfin_id = ?1"
            ))?;
            let mut rows = statement.query(params![item_id])?;
            match rows.next()? {
                Some(row) => Ok(Some(detail_row(row)?)),
                None => Ok(None),
            }
        })
    }

    /// Seasons of a series, or episodes of a season, in broadcast order.
    ///
    /// Summary rows only: episode synopses are not cached. The children API
    /// handler overlays them from its live server reconcile when online.
    pub fn children(&self, parent_id: &str) -> rusqlite::Result<Vec<Value>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM items i
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 WHERE i.parent_id = ?1 OR (i.season_id = ?1 AND i.kind = 'Episode')
                 ORDER BY i.parent_index_number ASC NULLS LAST,
                          i.index_number ASC NULLS LAST,
                          i.sort_name COLLATE NOCASE ASC"
            ))?;
            let rows = statement
                .query_map(params![parent_id], summary_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn continue_watching(&self, limit: i64) -> rusqlite::Result<Vec<Value>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM items i
                 JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 WHERE u.playback_position_ticks > 0 AND u.played = 0
                   AND i.kind IN ('Movie', 'Episode')
                 ORDER BY u.last_played_date DESC NULLS LAST, u.updated_at DESC
                 LIMIT ?1"
            ))?;
            let rows = statement
                .query_map(params![limit], summary_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn recently_added(&self, limit: i64) -> rusqlite::Result<Vec<Value>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM items i
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 WHERE i.kind IN ('Movie', 'Series')
                 ORDER BY i.date_created DESC NULLS LAST, i.id DESC
                 LIMIT ?1"
            ))?;
            let rows = statement
                .query_map(params![limit], summary_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// A fresh set of movies and series for the home billboard.
    ///
    /// The artwork predicate lives in the query rather than in the UI so five
    /// blank candidates cannot crowd out five useful ones. Image tag keys are
    /// case-insensitive in Jellyfin, hence the small `json_each` lookup for a
    /// Thumb fallback.
    pub fn random_billboard_titles(&self, limit: i64) -> rusqlite::Result<Vec<Value>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM items i
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 WHERE i.kind IN ('Movie', 'Series')
                   AND (
                       COALESCE(i.backdrop_image_tag, '') <> ''
                       OR EXISTS (
                           SELECT 1 FROM json_each(i.image_tags) image
                           WHERE lower(image.key) = 'thumb'
                             AND COALESCE(image.value, '') <> ''
                       )
                   )
                 ORDER BY random()
                 LIMIT ?1"
            ))?;
            let rows = statement
                .query_map(params![limit.clamp(1, 50)], summary_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn genres(&self) -> rusqlite::Result<Vec<String>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT DISTINCT genre.value FROM items, json_each(items.genres) AS genre
                 WHERE genre.value <> '' ORDER BY genre.value COLLATE NOCASE ASC",
            )?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// The earliest episode of a series in broadcast order.
    ///
    /// This is what a series' Play button falls back to once the server has no
    /// Next Up left to offer — a fully watched show would otherwise leave the
    /// page with nothing to play.
    pub fn first_episode(&self, series_id: &str) -> rusqlite::Result<Option<Value>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM items i
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 WHERE i.kind = 'Episode' AND i.series_id = ?1
                 ORDER BY i.parent_index_number ASC NULLS LAST,
                          i.index_number ASC NULLS LAST,
                          i.sort_name COLLATE NOCASE ASC
                 LIMIT 1"
            ))?;
            let mut rows = statement.query(params![series_id])?;
            match rows.next()? {
                Some(row) => Ok(Some(summary_row(row)?)),
                None => Ok(None),
            }
        })
    }

    /// The episode that follows `item_id` inside its series, used when mpv
    /// reports end-of-file or a mark-watched-and-next hotkey.
    pub fn next_episode(&self, item_id: &str) -> rusqlite::Result<Option<Value>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM items i
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 JOIN items current ON current.jellyfin_id = ?1
                 WHERE i.kind = 'Episode'
                   AND i.series_id = current.series_id
                   AND (COALESCE(i.parent_index_number, 0), COALESCE(i.index_number, 0))
                       > (COALESCE(current.parent_index_number, 0), COALESCE(current.index_number, 0))
                 ORDER BY i.parent_index_number ASC, i.index_number ASC
                 LIMIT 1"
            ))?;
            let mut rows = statement.query(params![item_id])?;
            match rows.next()? {
                Some(row) => Ok(Some(summary_row(row)?)),
                None => Ok(None),
            }
        })
    }
}

const SUMMARY_COLUMNS: &str = "i.jellyfin_id, i.kind, i.name, i.year, i.runtime_ticks, \
i.community_rating, i.official_rating, i.series_id, i.series_name, i.index_number, \
i.parent_index_number, i.primary_image_tag, i.child_count, i.premiere_date, i.season_id, \
COALESCE(u.played, 0), COALESCE(u.play_count, 0), COALESCE(u.playback_position_ticks, 0), \
COALESCE(u.is_favorite, 0), i.image_tags, i.backdrop_image_tag";

const DETAIL_COLUMNS: &str = "i.jellyfin_id, i.kind, i.name, i.year, i.runtime_ticks, \
i.community_rating, i.official_rating, i.series_id, i.series_name, i.index_number, \
i.parent_index_number, i.primary_image_tag, i.child_count, i.premiere_date, i.season_id, \
COALESCE(u.played, 0), COALESCE(u.play_count, 0), COALESCE(u.playback_position_ticks, 0), \
COALESCE(u.is_favorite, 0), i.image_tags, i.backdrop_image_tag, \
i.genres, i.original_title, i.tmdb_id, i.imdb_id, i.tvdb_id, i.parent_id, i.date_created";

/// Reads one entry out of Jellyfin's per-image-type map, whose key casing the
/// server does not guarantee. The live-DTO path in `src/shell/cef/api.rs` goes
/// through `BaseItemDto::image_tag` for the same types; this is the cached
/// equivalent, working on the map as it was stored.
fn cached_image_tag<'a>(image_tags: &'a Value, image_type: &str) -> Option<&'a str> {
    image_tags.as_object()?.iter().find_map(|(key, value)| {
        key.eq_ignore_ascii_case(image_type)
            .then(|| value.as_str())
            .flatten()
    })
}

fn summary_object(row: &Row<'_>) -> rusqlite::Result<serde_json::Map<String, Value>> {
    let image_tags = parsed_json(&row.get::<_, String>(19)?);
    let Value::Object(object) = json!({
        "id": row.get::<_, String>(0)?,
        "kind": row.get::<_, String>(1)?,
        "name": row.get::<_, String>(2)?,
        "year": row.get::<_, Option<i64>>(3)?,
        "runtimeTicks": row.get::<_, Option<i64>>(4)?,
        "communityRating": row.get::<_, Option<f64>>(5)?,
        "officialRating": row.get::<_, Option<String>>(6)?,
        "seriesId": row.get::<_, Option<String>>(7)?,
        "seriesName": row.get::<_, Option<String>>(8)?,
        "indexNumber": row.get::<_, Option<i64>>(9)?,
        "parentIndexNumber": row.get::<_, Option<i64>>(10)?,
        "primaryImageTag": row.get::<_, Option<String>>(11)?,
        "childCount": row.get::<_, Option<i64>>(12)?,
        "premiereDate": row.get::<_, Option<String>>(13)?,
        "seasonId": row.get::<_, Option<String>>(14)?,
        "played": row.get::<_, i64>(15)? != 0,
        "playCount": row.get::<_, i64>(16)?,
        "positionTicks": row.get::<_, i64>(17)?,
        "favorite": row.get::<_, i64>(18)? != 0,
        "thumbImageTag": cached_image_tag(&image_tags, "Thumb"),
        // The title treatment — the show's own wordmark on transparency. The
        // billboard and the hover preview draw it in place of typeset text,
        // which is what makes a hero read as artwork rather than as a heading.
        "logoImageTag": cached_image_tag(&image_tags, "Logo"),
        "backdropImageTag": row.get::<_, Option<String>>(20)?,
    }) else {
        unreachable!("a JSON object literal always produces an object");
    };
    Ok(object)
}

fn summary_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    summary_object(row).map(Value::Object)
}

fn detail_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let mut object = summary_object(row)?;
    object.insert(
        "genres".to_string(),
        parsed_json(&row.get::<_, String>(21)?),
    );
    object.insert(
        "originalTitle".to_string(),
        json!(row.get::<_, Option<String>>(22)?),
    );
    object.insert(
        "providerIds".to_string(),
        json!({
            "tmdb": row.get::<_, Option<String>>(23)?,
            "imdb": row.get::<_, Option<String>>(24)?,
            "tvdb": row.get::<_, Option<String>>(25)?,
        }),
    );
    object.insert(
        "parentId".to_string(),
        json!(row.get::<_, Option<String>>(26)?),
    );
    object.insert(
        "dateCreated".to_string(),
        json!(row.get::<_, Option<String>>(27)?),
    );
    Ok(Value::Object(object))
}

fn parsed_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| json!([]))
}

/// Builds the FROM clause, WHERE conditions, and bound arguments for a query.
fn query_base(query: &ItemQuery) -> (String, Vec<String>, Vec<SqlValue>) {
    let mut conditions = Vec::new();
    let mut arguments = Vec::new();

    let search = query.search.as_deref().and_then(fts_match_expression);
    let from_clause = if search.is_some() {
        "FROM items_fts JOIN items i ON i.id = items_fts.rowid \
         LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id"
            .to_string()
    } else {
        "FROM items i LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id".to_string()
    };
    if let Some(search) = search {
        conditions.push("items_fts MATCH ?".to_string());
        arguments.push(SqlValue::Text(search));
    }

    if !query.kinds.is_empty() {
        let placeholders = vec!["?"; query.kinds.len()].join(", ");
        conditions.push(format!("i.kind IN ({placeholders})"));
        for kind in &query.kinds {
            arguments.push(SqlValue::Text(kind.clone()));
        }
    }
    if let Some(genre) = &query.genre {
        conditions
            .push("EXISTS (SELECT 1 FROM json_each(i.genres) AS g WHERE g.value = ?)".to_string());
        arguments.push(SqlValue::Text(genre.clone()));
    }
    if let Some(decade) = query.release_decade {
        // Two bound comparisons intentionally avoid strftime/substr so the
        // (kind, year) index remains usable for large libraries.
        conditions.push("i.year >= ? AND i.year < ?".to_string());
        arguments.push(SqlValue::Integer(decade));
        arguments.push(SqlValue::Integer(decade.saturating_add(10)));
    }
    if let Some(parent_id) = &query.parent_id {
        conditions.push("i.parent_id = ?".to_string());
        arguments.push(SqlValue::Text(parent_id.clone()));
    }
    if let Some(series_id) = &query.series_id {
        conditions.push("i.series_id = ?".to_string());
        arguments.push(SqlValue::Text(series_id.clone()));
    }
    if let Some(watched) = query.watched {
        conditions.push(if watched {
            "COALESCE(u.played, 0) = 1".to_string()
        } else {
            "COALESCE(u.played, 0) = 0".to_string()
        });
    }
    if let Some(favorite) = query.favorite {
        conditions.push(if favorite {
            "COALESCE(u.is_favorite, 0) = 1".to_string()
        } else {
            "COALESCE(u.is_favorite, 0) = 0".to_string()
        });
    }

    (from_clause, conditions, arguments)
}

/// Turns free text into an FTS5 prefix query. Splitting on non-alphanumeric
/// characters is also what keeps FTS operators out of the expression.
fn fts_match_expression(input: &str) -> Option<String> {
    let tokens = input
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\"*"))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" AND "))
}

fn child_ids_from_connection(
    connection: &Connection,
    parent_id: &str,
) -> rusqlite::Result<HashSet<String>> {
    let mut statement = connection.prepare(
        "SELECT jellyfin_id FROM items
         WHERE parent_id = ?1 OR (season_id = ?1 AND kind = 'Episode')",
    )?;
    statement
        .query_map(params![parent_id], |row| row.get::<_, String>(0))?
        .collect()
}

fn delete_missing_from_transaction(
    connection: &Connection,
    existing: &HashSet<String>,
    keep: &HashSet<String>,
    changes: &mut LibraryChangeBatch,
) -> rusqlite::Result<()> {
    let stale = existing.difference(keep).cloned().collect::<Vec<_>>();
    delete_items_from_transaction(connection, &stale, changes)
}

/// Captures hierarchy before deletion so a committed removal can invalidate
/// the item itself and every aggregate container that used to own it.
fn delete_items_from_transaction(
    connection: &Connection,
    item_ids: &[String],
    changes: &mut LibraryChangeBatch,
) -> rusqlite::Result<()> {
    for item_id in item_ids {
        let contexts = connection
            .query_row(
                "SELECT parent_id, series_id, season_id FROM items WHERE jellyfin_id = ?1",
                params![item_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let removed =
            connection.execute("DELETE FROM items WHERE jellyfin_id = ?1", params![item_id])?;
        connection.execute(
            "DELETE FROM user_data WHERE jellyfin_id = ?1",
            params![item_id],
        )?;
        if removed > 0 {
            let (parent_id, series_id, season_id) = contexts.unwrap_or_default();
            record_change_identity(
                changes,
                item_id,
                [
                    parent_id.as_deref(),
                    series_id.as_deref(),
                    season_id.as_deref(),
                ],
            );
        }
    }
    Ok(())
}

/// A plain full-row upsert: every ingest source (catalog pages, live detail
/// fetches, children reconciles) requests a superset of the thin fields, so
/// the freshest observation simply wins.
/// Writes one catalog row, reporting whether the row is new or actually moved.
///
/// The conflict update carries a `WHERE` comparing every mirrored column, so
/// the periodic re-page of an unchanged library performs no row writes, fires
/// no FTS triggers, and reports nothing for the UI to invalidate. `synced_at`
/// sits outside the comparison on purpose: it would differ every cycle, and
/// nothing reads it back.
fn upsert_item(connection: &Connection, record: &ItemRecord) -> rusqlite::Result<bool> {
    let written = connection.execute(
        "INSERT INTO items (
            jellyfin_id, kind, name, original_title, sort_name, year, premiere_date,
            runtime_ticks, community_rating, official_rating, parent_id, series_id,
            series_name, season_id, index_number, parent_index_number, child_count,
            tmdb_id, imdb_id, tvdb_id, genres, image_tags, primary_image_tag,
            backdrop_image_tag, search_genres, date_created, date_last_saved, synced_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28
        )
        ON CONFLICT(jellyfin_id) DO UPDATE SET
            kind = excluded.kind,
            name = excluded.name,
            original_title = excluded.original_title,
            sort_name = excluded.sort_name,
            year = excluded.year,
            premiere_date = excluded.premiere_date,
            runtime_ticks = excluded.runtime_ticks,
            community_rating = excluded.community_rating,
            official_rating = excluded.official_rating,
            parent_id = excluded.parent_id,
            series_id = excluded.series_id,
            series_name = excluded.series_name,
            season_id = excluded.season_id,
            index_number = excluded.index_number,
            parent_index_number = excluded.parent_index_number,
            child_count = excluded.child_count,
            tmdb_id = excluded.tmdb_id,
            imdb_id = excluded.imdb_id,
            tvdb_id = excluded.tvdb_id,
            genres = excluded.genres,
            image_tags = excluded.image_tags,
            primary_image_tag = excluded.primary_image_tag,
            backdrop_image_tag = excluded.backdrop_image_tag,
            search_genres = excluded.search_genres,
            date_created = excluded.date_created,
            date_last_saved = excluded.date_last_saved,
            synced_at = excluded.synced_at
        WHERE kind IS NOT excluded.kind
            OR name IS NOT excluded.name
            OR original_title IS NOT excluded.original_title
            OR sort_name IS NOT excluded.sort_name
            OR year IS NOT excluded.year
            OR premiere_date IS NOT excluded.premiere_date
            OR runtime_ticks IS NOT excluded.runtime_ticks
            OR community_rating IS NOT excluded.community_rating
            OR official_rating IS NOT excluded.official_rating
            OR parent_id IS NOT excluded.parent_id
            OR series_id IS NOT excluded.series_id
            OR series_name IS NOT excluded.series_name
            OR season_id IS NOT excluded.season_id
            OR index_number IS NOT excluded.index_number
            OR parent_index_number IS NOT excluded.parent_index_number
            OR child_count IS NOT excluded.child_count
            OR tmdb_id IS NOT excluded.tmdb_id
            OR imdb_id IS NOT excluded.imdb_id
            OR tvdb_id IS NOT excluded.tvdb_id
            OR genres IS NOT excluded.genres
            OR image_tags IS NOT excluded.image_tags
            OR primary_image_tag IS NOT excluded.primary_image_tag
            OR backdrop_image_tag IS NOT excluded.backdrop_image_tag
            OR search_genres IS NOT excluded.search_genres
            OR date_created IS NOT excluded.date_created
            OR date_last_saved IS NOT excluded.date_last_saved",
        params![
            record.jellyfin_id,
            record.kind,
            record.name,
            record.original_title,
            record.sort_name,
            record.year,
            record.premiere_date,
            record.runtime_ticks,
            record.community_rating,
            record.official_rating,
            record.parent_id,
            record.series_id,
            record.series_name,
            record.season_id,
            record.index_number,
            record.parent_index_number,
            record.child_count,
            record.tmdb_id,
            record.imdb_id,
            record.tvdb_id,
            record.genres,
            record.image_tags,
            record.primary_image_tag,
            record.backdrop_image_tag,
            record.search_genres,
            record.date_created,
            record.date_last_saved,
            now_unix(),
        ],
    )?;
    Ok(written > 0)
}

/// Mirrors one watch-state row, reporting whether it is new or actually moved.
/// Like [`upsert_item`], an identical row performs no write; `updated_at` is
/// outside the comparison because it would differ on every sweep.
fn upsert_user_data(connection: &Connection, record: &UserDataRecord) -> rusqlite::Result<bool> {
    let written = connection.execute(
        "INSERT INTO user_data (jellyfin_id, played, play_count, playback_position_ticks,
             is_favorite, played_percentage, last_played_date, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(jellyfin_id) DO UPDATE SET
             played = excluded.played, play_count = excluded.play_count,
             playback_position_ticks = excluded.playback_position_ticks,
             is_favorite = excluded.is_favorite,
             played_percentage = excluded.played_percentage,
             last_played_date = excluded.last_played_date,
             updated_at = excluded.updated_at
         WHERE played IS NOT excluded.played
             OR play_count IS NOT excluded.play_count
             OR playback_position_ticks IS NOT excluded.playback_position_ticks
             OR is_favorite IS NOT excluded.is_favorite
             OR played_percentage IS NOT excluded.played_percentage
             OR last_played_date IS NOT excluded.last_played_date",
        params![
            record.jellyfin_id,
            record.played,
            record.play_count,
            record.playback_position_ticks,
            record.is_favorite,
            record.played_percentage,
            record.last_played_date,
            now_unix(),
        ],
    )?;
    Ok(written > 0)
}

pub(crate) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
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

/// Howard Hinnant's `civil_from_days`, the standard days-to-date conversion.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::{
        CachedRatings, ExternalProfile, IntegrationState, ItemPlaybackPreference, ItemQuery,
        ItemSort, Library, SeerrConfig, cached_image_tag, civil_from_days, current_release_decade,
        fts_match_expression, release_decade_from_id,
    };
    use crate::jellyfin::api::model::{BaseItemDto, MediaSourceInfo};
    use serde_json::json;
    use std::collections::HashSet;

    fn dto(json: &str) -> BaseItemDto {
        serde_json::from_str(json).expect("dto")
    }

    #[test]
    fn cached_image_tags_are_found_whatever_the_server_capitalised() {
        let tags = json!({ "primary": "p", "THUMB": "t", "Logo": "l" });
        assert_eq!(cached_image_tag(&tags, "Primary"), Some("p"));
        assert_eq!(cached_image_tag(&tags, "Thumb"), Some("t"));
        assert_eq!(cached_image_tag(&tags, "Logo"), Some("l"));
        assert_eq!(cached_image_tag(&tags, "Banner"), None);
        // A row stored before the column existed parses to `null`, not a map.
        assert_eq!(cached_image_tag(&json!(null), "Logo"), None);
    }

    /// The daily full re-page feeds every row through here; an unchanged
    /// library must produce zero UI invalidations and zero FTS churn from it.
    #[test]
    fn re_ingesting_an_identical_page_reports_no_changes() {
        let library = Library::open_in_memory().expect("library");
        let page = [dto(
            r#"{"Id":"m1","Name":"The Matrix","Type":"Movie","ProductionYear":1999,
                "Genres":["Action"],"UserData":{"Played":false}}"#,
        )];
        let first = library.ingest_page(&page).expect("first ingest");
        assert_eq!(first.item_ids, vec!["m1".to_string()]);

        let second = library.ingest_page(&page).expect("second ingest");
        assert!(second.is_empty());

        // A watch-state-only difference still reports the row…
        let watched = [dto(
            r#"{"Id":"m1","Name":"The Matrix","Type":"Movie","ProductionYear":1999,
                "Genres":["Action"],"UserData":{"Played":true}}"#,
        )];
        let third = library.ingest_page(&watched).expect("watched ingest");
        assert_eq!(third.item_ids, vec!["m1".to_string()]);

        // …and so does an in-place metadata edit.
        let edited = [dto(
            r#"{"Id":"m1","Name":"The Matrix Reloaded","Type":"Movie","ProductionYear":1999,
                "Genres":["Action"],"UserData":{"Played":true}}"#,
        )];
        let fourth = library.ingest_page(&edited).expect("edited ingest");
        assert_eq!(fourth.item_ids, vec!["m1".to_string()]);
    }

    #[test]
    fn technical_sources_resolve_containers_to_a_representative_episode() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(r#"{"Id":"m1","Name":"Movie","Type":"Movie"}"#),
                dto(r#"{"Id":"s1","Name":"Show","Type":"Series"}"#),
                dto(r#"{"Id":"season1","Name":"Season 1","Type":"Season",
                        "SeriesId":"s1","ParentId":"s1","IndexNumber":1}"#),
                // A special: numerically first, representative of nothing.
                dto(
                    r#"{"Id":"e0","Name":"Special","Type":"Episode","SeriesId":"s1",
                        "ParentIndexNumber":0,"IndexNumber":1}"#,
                ),
                dto(
                    r#"{"Id":"e2","Name":"S1E2","Type":"Episode","SeriesId":"s1",
                        "SeasonId":"season1","ParentId":"season1",
                        "ParentIndexNumber":1,"IndexNumber":2}"#,
                ),
                dto(
                    r#"{"Id":"e1","Name":"S1E1","Type":"Episode","SeriesId":"s1",
                        "SeasonId":"season1","ParentId":"season1",
                        "ParentIndexNumber":1,"IndexNumber":1}"#,
                ),
                dto(r#"{"Id":"empty","Name":"Empty Show","Type":"Series"}"#),
            ])
            .expect("seed");

        let sources = library
            .technical_stream_sources(&[
                "m1".to_string(),
                "s1".to_string(),
                "season1".to_string(),
                "empty".to_string(),
                "deep-link".to_string(),
            ])
            .expect("sources");
        assert_eq!(
            sources,
            vec![
                // Playable kinds and unknown (deep-linked) ids probe themselves;
                // containers resolve to their first regular episode, and a
                // container without a cached episode is omitted entirely.
                ("m1".to_string(), "m1".to_string()),
                ("s1".to_string(), "e1".to_string()),
                ("season1".to_string(), "e1".to_string()),
                ("deep-link".to_string(), "deep-link".to_string()),
            ]
        );
    }

    fn seeded() -> Library {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(
                    r#"{"Id":"m1","Name":"The Matrix","Type":"Movie","ProductionYear":1999,
                        "Genres":["Action"],"DateCreated":"2024-01-02","CommunityRating":8.7,
                        "ImageTags":{"Primary":"poster-tag","Thumb":"thumb-tag"},
                        "BackdropImageTags":["backdrop-tag"],
                        "People":[{"Name":"Keanu Reeves"}],
                        "UserData":{"Played":false,"PlaybackPositionTicks":600000000}}"#,
                ),
                dto(
                    r#"{"Id":"m2","Name":"Arrival","Type":"Movie","ProductionYear":2016,
                        "Genres":["Drama"],"DateCreated":"2024-03-04","CommunityRating":7.9,
                        "UserData":{"Played":true}}"#,
                ),
                dto(
                    r#"{"Id":"s1","Name":"Severance","Type":"Series","DateCreated":"2024-02-02",
                        "BackdropImageTags":["series-backdrop-tag"]}"#,
                ),
                dto(
                    r#"{"Id":"e1","Name":"Good News About Hell","Type":"Episode","SeriesId":"s1",
                        "ParentId":"season1","SeasonId":"season1","Overview":"Mark is promoted.",
                        "IndexNumber":1,"ParentIndexNumber":1}"#,
                ),
                dto(
                    r#"{"Id":"e2","Name":"Half Loop","Type":"Episode","SeriesId":"s1",
                        "ParentId":"season1","SeasonId":"season1",
                        "IndexNumber":2,"ParentIndexNumber":1}"#,
                ),
            ])
            .expect("seed");
        library
    }

    /// TMDB numbers movies and series separately, so id 603 addresses two
    /// unrelated titles. A join on the id alone would offer Play on the wrong
    /// one, which is why the kind is part of the lookup.
    #[test]
    fn tmdb_ids_resolve_only_within_the_kind_they_were_asked_for() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(r#"{"Id":"m1","Name":"The Matrix","Type":"Movie",
                        "ProviderIds":{"Tmdb":"603"}}"#),
                dto(r#"{"Id":"s1","Name":"Not The Matrix","Type":"Series",
                        "ProviderIds":{"Tmdb":"603"}}"#),
                dto(r#"{"Id":"m2","Name":"Arrival","Type":"Movie",
                        "ProviderIds":{"Tmdb":"329865"}}"#),
            ])
            .expect("seed");

        let ids = ["603".to_string(), "329865".to_string(), "1".to_string()];
        let movies = library.ids_by_tmdb("Movie", &ids).expect("movies");
        assert_eq!(movies.get("603").map(String::as_str), Some("m1"));
        assert_eq!(movies.get("329865").map(String::as_str), Some("m2"));
        // Not in the library at all: the caller offers a request for this one.
        assert_eq!(movies.get("1"), None);

        let series = library.ids_by_tmdb("Series", &ids).expect("series");
        assert_eq!(series.get("603").map(String::as_str), Some("s1"));
        assert_eq!(series.get("329865"), None);
    }

    #[test]
    fn tvdb_episode_ids_resolve_for_calendar_entries() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[dto(r#"{"Id":"e1","Name":"Half Loop","Type":"Episode",
                    "ProviderIds":{"Tvdb":"5161348"}}"#)])
            .expect("seed");

        let ids = ["5161348".to_string()];
        let episodes = library.ids_by_tvdb("Episode", &ids).expect("episodes");
        assert_eq!(episodes.get("5161348").map(String::as_str), Some("e1"));
        assert!(
            library
                .ids_by_tvdb("Movie", &ids)
                .expect("movies")
                .is_empty()
        );
    }

    #[test]
    fn asking_for_no_tmdb_ids_costs_no_query() {
        let library = Library::open_in_memory().expect("library");
        assert!(library.ids_by_tmdb("Movie", &[]).expect("empty").is_empty());
    }

    #[test]
    fn an_unconfigured_library_has_an_empty_seerr_config() {
        let library = Library::open_in_memory().expect("library");
        assert_eq!(library.seerr_config(), SeerrConfig::default());
    }

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
    fn the_seerr_config_round_trips_through_its_single_row() {
        let library = Library::open_in_memory().expect("library");
        let config = SeerrConfig {
            base_url: Some("https://seerr.test".to_string()),
            cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
            user_id: Some(7),
            user_name: Some("pho".to_string()),
            jellyfin_server_id: Some("srv".to_string()),
            jellyfin_user_id: Some("uid".to_string()),
            movie_4k_enabled: true,
            series_4k_enabled: false,
            partial_requests_enabled: true,
        };
        library.save_seerr_config(&config).expect("save");
        assert_eq!(library.seerr_config(), config);

        // Saving twice must update the row, not fail on the primary key.
        let moved = SeerrConfig {
            base_url: Some("https://other.test".to_string()),
            ..config
        };
        library.save_seerr_config(&moved).expect("re-save");
        assert_eq!(library.seerr_config(), moved);
    }

    #[test]
    fn a_stale_seerr_revision_cannot_replace_a_newer_config() {
        let library = Library::open_in_memory().expect("library");
        let original = SeerrConfig {
            base_url: Some("https://old.test".to_string()),
            ..SeerrConfig::default()
        };
        library.save_seerr_config(&original).expect("original");
        let (_, original_revision) = library.seerr_config_snapshot();

        let newer = SeerrConfig {
            base_url: Some("https://new.test".to_string()),
            ..SeerrConfig::default()
        };
        library.save_seerr_config(&newer).expect("newer");

        let stale = SeerrConfig {
            base_url: Some("https://stale.test".to_string()),
            ..SeerrConfig::default()
        };
        assert_eq!(
            library
                .save_seerr_config_if_revision(&stale, original_revision)
                .expect("conditional save"),
            None
        );
        assert_eq!(library.seerr_config(), newer);
    }

    #[test]
    fn clearing_the_seerr_link_keeps_the_instance_address() {
        let library = Library::open_in_memory().expect("library");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some("https://seerr.test".to_string()),
                cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                user_name: Some("pho".to_string()),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                partial_requests_enabled: true,
                ..SeerrConfig::default()
            })
            .expect("save");
        library.clear_seerr_link().expect("clear");

        let config = library.seerr_config();
        assert_eq!(config.base_url.as_deref(), Some("https://seerr.test"));
        assert!(config.partial_requests_enabled);
        assert_eq!(config.cookies, None);
        assert_eq!(config.user_id, None);
        assert_eq!(config.jellyfin_user_id, None);
    }

    #[test]
    fn upserts_are_idempotent() {
        let library = seeded();
        let before = library.stats();
        library
            .upsert_page(&[dto(r#"{"Id":"m1","Name":"The Matrix","Type":"Movie"}"#)])
            .expect("re-upsert");
        assert_eq!(library.stats(), before);
    }

    #[test]
    fn stats_count_each_kind() {
        let stats = seeded().stats();
        assert_eq!(stats.movies, 2);
        assert_eq!(stats.series, 1);
        assert_eq!(stats.episodes, 2);
        assert_eq!(stats.total, 5);
    }

    #[test]
    fn search_matches_titles_and_genres_by_prefix() {
        let library = seeded();
        let page = library
            .query(&ItemQuery {
                search: Some("matr".to_string()),
                limit: 10,
                ..Default::default()
            })
            .expect("query");
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0]["name"], "The Matrix");

        let by_genre = library
            .query(&ItemQuery {
                search: Some("acti".to_string()),
                limit: 10,
                ..Default::default()
            })
            .expect("query");
        assert_eq!(by_genre.total, 1);
        assert_eq!(by_genre.items[0]["id"], "m1");
    }

    #[test]
    fn filters_compose_across_kind_genre_and_watched_state() {
        let library = seeded();
        let page = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                genre: Some("Action".to_string()),
                watched: Some(false),
                limit: 10,
                ..Default::default()
            })
            .expect("query");
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0]["id"], "m1");

        let watched = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                watched: Some(true),
                limit: 10,
                ..Default::default()
            })
            .expect("query");
        assert_eq!(watched.total, 1);
        assert_eq!(watched.items[0]["id"], "m2");
    }

    #[test]
    fn favorite_filter_uses_the_mirrored_my_list_state() {
        let library = seeded();
        library.set_local_favorite("m1", true).expect("favorite");

        let favorites = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                favorite: Some(true),
                limit: 10,
                ..Default::default()
            })
            .expect("favorites");
        assert_eq!(favorites.total, 1);
        assert_eq!(favorites.items[0]["id"], "m1");

        let not_favorites = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                favorite: Some(false),
                limit: 10,
                ..Default::default()
            })
            .expect("not favorites");
        assert_eq!(not_favorites.total, 1);
        assert_eq!(not_favorites.items[0]["id"], "m2");
    }

    #[test]
    fn release_decade_is_bounded_and_composes_with_every_library_filter() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(r#"{"Id":"previous","Name":"Previous Decade","Type":"Movie","ProductionYear":1989,
                    "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":true}}"#),
                dto(r#"{"Id":"start","Name":"Decade Start","Type":"Movie","ProductionYear":1990,
                    "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":true}}"#),
                dto(r#"{"Id":"end","Name":"Decade End","Type":"Movie","ProductionYear":1999,
                    "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":true}}"#),
                dto(r#"{"Id":"next","Name":"Next Decade","Type":"Movie","ProductionYear":2000,
                    "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":true}}"#),
                dto(r#"{"Id":"watched","Name":"Watched","Type":"Movie","ProductionYear":1995,
                    "Genres":["Action"],"UserData":{"Played":true,"IsFavorite":true}}"#),
                dto(r#"{"Id":"drama","Name":"Drama","Type":"Movie","ProductionYear":1995,
                    "Genres":["Drama"],"UserData":{"Played":false,"IsFavorite":true}}"#),
                dto(r#"{"Id":"not-listed","Name":"Not Listed","Type":"Movie","ProductionYear":1995,
                    "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":false}}"#),
                dto(r#"{"Id":"unknown","Name":"Unknown Year","Type":"Movie",
                    "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":true}}"#),
            ])
            .expect("seed");

        let page = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                genre: Some("Action".to_string()),
                release_decade: Some(1990),
                watched: Some(false),
                favorite: Some(true),
                limit: 1,
                ..Default::default()
            })
            .expect("combined filters");

        // The count is over the complete filtered library even though only a
        // one-item page is returned, and both decade boundary years match.
        assert_eq!(page.total, 2);
        assert_eq!(page.items.len(), 1);
        let all = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                genre: Some("Action".to_string()),
                release_decade: Some(1990),
                watched: Some(false),
                favorite: Some(true),
                limit: 10,
                ..Default::default()
            })
            .expect("all combined filters");
        assert_eq!(
            all.items
                .iter()
                .map(|item| item["id"].as_str().expect("id"))
                .collect::<HashSet<_>>(),
            HashSet::from(["start", "end"])
        );
    }

    #[test]
    fn movie_and_series_decades_use_release_and_first_air_years() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(r#"{"Id":"movie","Name":"Film","Type":"Movie","ProductionYear":1999}"#),
                // A PremiereDate-only series exercises the first-air fallback.
                dto(r#"{"Id":"series","Name":"Show","Type":"Series",
                    "PremiereDate":"2017-02-15T00:00:00.0000000Z"}"#),
            ])
            .expect("seed");

        let movies = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                release_decade: Some(1990),
                limit: 10,
                ..Default::default()
            })
            .expect("movies");
        let series = library
            .query(&ItemQuery {
                kinds: vec!["Series".to_string()],
                release_decade: Some(2010),
                limit: 10,
                ..Default::default()
            })
            .expect("series");

        assert_eq!(movies.items[0]["id"], "movie");
        assert_eq!(series.items[0]["id"], "series");
        assert_eq!(series.items[0]["year"], 2017);
    }

    #[test]
    fn release_decade_ids_are_standard_and_not_in_the_future() {
        let current = current_release_decade();
        assert_eq!(release_decade_from_id("1900"), Some(1900));
        assert_eq!(release_decade_from_id(&current.to_string()), Some(current));
        assert_eq!(release_decade_from_id("1995"), None);
        assert_eq!(release_decade_from_id("1890"), None);
        assert_eq!(release_decade_from_id(&(current + 10).to_string()), None);
    }

    #[test]
    fn sorting_and_paging_are_stable() {
        let library = seeded();
        let by_year = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                sort: ItemSort::Year,
                limit: 1,
                ..Default::default()
            })
            .expect("query");
        assert_eq!(by_year.total, 2);
        assert_eq!(by_year.items[0]["id"], "m2");

        let second_page = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                sort: ItemSort::Year,
                limit: 1,
                offset: 1,
                ..Default::default()
            })
            .expect("query");
        assert_eq!(second_page.items[0]["id"], "m1");
    }

    #[test]
    fn continue_watching_lists_partially_played_items() {
        let rows = seeded().continue_watching(10).expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], "m1");
        assert_eq!(rows[0]["positionTicks"], 600_000_000i64);
        assert_eq!(rows[0]["thumbImageTag"], "thumb-tag");
        assert_eq!(rows[0]["backdropImageTag"], "backdrop-tag");
    }

    #[test]
    fn recently_added_orders_by_creation_date() {
        let rows = seeded().recently_added(10).expect("rows");
        assert_eq!(rows[0]["id"], "m2");
        assert_eq!(rows[1]["id"], "s1");
    }

    #[test]
    fn billboard_titles_include_movies_and_series_with_landscape_artwork() {
        let rows = seeded().random_billboard_titles(5).expect("rows");
        let mut kinds = rows
            .iter()
            .map(|row| (row["id"].as_str().unwrap(), row["kind"].as_str().unwrap()))
            .collect::<Vec<_>>();
        kinds.sort_unstable();
        assert_eq!(kinds, [("m1", "Movie"), ("s1", "Series")]);
    }

    #[test]
    fn children_returns_episodes_of_a_season_in_order() {
        let rows = seeded().children("season1").expect("children");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], "e1");
        assert_eq!(rows[1]["id"], "e2");
    }

    #[test]
    fn next_episode_follows_broadcast_order() {
        let library = seeded();
        let next = library.next_episode("e1").expect("next").expect("episode");
        assert_eq!(next["id"], "e2");
        assert!(library.next_episode("e2").expect("next").is_none());
    }

    /// The thin index deliberately carries no technical streams; card badges
    /// arrive over the live batch channel instead.
    #[test]
    fn summary_rows_are_thin_and_carry_no_rich_metadata() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[dto(
                r#"{"Id":"m1","Name":"Feature","Type":"Movie","MediaStreams":[
                    {"Index":0,"Type":"Video","Codec":"hevc","Width":3840,"Height":1608}],
                    "Overview":"Synopsis","People":[{"Name":"Actor"}]}"#,
            )])
            .expect("seed");

        let page = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                ..Default::default()
            })
            .expect("query");
        let row = page.items[0].as_object().expect("row object");
        assert!(!row.contains_key("mediaStreams"));
        assert!(!row.contains_key("overview"));
        assert!(!row.contains_key("people"));
    }

    #[test]
    fn item_detail_includes_genres_and_provider_ids() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[dto(r#"{"Id":"m1","Name":"The Matrix","Type":"Movie",
                    "OriginalTitle":"Matrix","Genres":["Action"],
                    "ProviderIds":{"Tmdb":"603"}}"#)])
            .expect("seed");
        let detail = library.item("m1").expect("query").expect("item");
        assert_eq!(detail["genres"][0], "Action");
        assert_eq!(detail["originalTitle"], "Matrix");
        assert_eq!(detail["providerIds"]["tmdb"], "603");
        assert!(library.item("missing").expect("query").is_none());
    }

    #[test]
    fn genres_are_deduplicated_across_items() {
        assert_eq!(seeded().genres().expect("genres"), vec!["Action", "Drama"]);
    }

    #[test]
    fn retain_ids_deletes_items_the_server_dropped() {
        let library = seeded();
        let keep = ["m1", "s1", "e1", "e2"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        let changes = library.retain_ids(&keep).expect("retain");
        assert_eq!(changes.item_ids, ["m2"]);
        assert!(changes.context_ids.is_empty());
        assert!(library.item("m2").expect("query").is_none());
        assert!(library.retain_ids(&keep).expect("retain").is_empty());
    }

    /// A season page reads its episodes back through `children`, so a live
    /// snapshot must evict anything that same predicate can still see.
    #[test]
    fn child_reconcile_drops_only_the_episodes_the_server_dropped() {
        let library = seeded();
        let live = [dto(
            r#"{"Id":"e2","Name":"Half Loop","Type":"Episode","SeriesId":"s1",
                "ParentId":"season1","SeasonId":"season1",
                "IndexNumber":2,"ParentIndexNumber":1}"#,
        )];

        let changes = library
            .reconcile_children("season1", &live)
            .expect("reconcile");
        assert_eq!(changes.item_ids, ["e1"]);
        assert_eq!(changes.context_ids, ["s1", "season1"]);
        let remaining = library.children("season1").expect("children");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0]["id"], "e2");
        // Untouched: the sweep is scoped to that one parent.
        assert!(library.item("m1").expect("query").is_some());
        assert!(
            library
                .reconcile_children("season1", &live)
                .expect("reconcile")
                .is_empty()
        );
    }

    /// The whole point of the season reconcile: a server that reports no
    /// children clears the cached ones instead of leaving ghosts behind.
    #[test]
    fn child_reconcile_clears_a_parent_the_server_reports_as_empty() {
        let library = seeded();
        let changes = library
            .reconcile_children("season1", &[])
            .expect("reconcile");
        assert_eq!(changes.item_ids, ["e1", "e2"]);
        assert_eq!(changes.context_ids, ["s1", "season1"]);
        assert!(library.children("season1").expect("children").is_empty());
    }

    #[test]
    fn child_reconcile_reports_upserts_deletions_and_parent_contexts_together() {
        let library = seeded();
        let live = [
            dto(
                r#"{"Id":"e2","Name":"Half Loop Updated","Type":"Episode","SeriesId":"s1",
                    "ParentId":"season1","SeasonId":"season1",
                    "IndexNumber":2,"ParentIndexNumber":1}"#,
            ),
            dto(
                r#"{"Id":"e3","Name":"In Perpetuity","Type":"Episode","SeriesId":"s1",
                    "ParentId":"season1","SeasonId":"season1",
                    "IndexNumber":3,"ParentIndexNumber":1}"#,
            ),
        ];

        let changes = library
            .reconcile_children("season1", &live)
            .expect("reconcile");
        assert_eq!(changes.item_ids, ["e1", "e2", "e3"]);
        assert_eq!(changes.context_ids, ["s1", "season1"]);
        let remaining = library.children("season1").expect("children");
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0]["id"], "e2");
        assert_eq!(remaining[0]["name"], "Half Loop Updated");
        assert_eq!(remaining[1]["id"], "e3");
    }

    #[test]
    fn kind_reads_back_the_item_type() {
        let library = seeded();
        assert_eq!(library.kind("s1").as_deref(), Some("Series"));
        assert_eq!(library.kind("e1").as_deref(), Some("Episode"));
        assert_eq!(library.kind("missing"), None);
    }

    /// Replacing a file in Jellyfin re-creates the item under a new id, so the
    /// old row has to go as soon as the server 404s it rather than waiting for
    /// the next daily deletion sweep.
    #[test]
    fn forget_drops_a_single_item_and_its_user_data() {
        let library = seeded();

        let changes = library.forget("m1").expect("forget");
        assert_eq!(changes.item_ids, ["m1"]);
        assert!(changes.context_ids.is_empty());
        assert!(library.item("m1").expect("query").is_none());
        assert!(
            !library
                .continue_watching(10)
                .expect("rows")
                .iter()
                .any(|row| row["id"] == "m1")
        );
        // Idempotent: a second eviction is not an error and reports no change.
        assert!(library.forget("m1").expect("forget").is_empty());
    }

    #[test]
    fn forgetting_an_episode_keeps_its_old_hierarchy_in_the_change_batch() {
        let library = seeded();
        let changes = library.forget("e1").expect("forget");
        assert_eq!(changes.item_ids, ["e1"]);
        assert_eq!(changes.context_ids, ["s1", "season1"]);
    }

    #[test]
    fn local_played_and_favorite_toggles_are_mirrored() {
        let library = seeded();
        library.set_local_played("m1", true).expect("played");
        library.set_local_favorite("m1", true).expect("favorite");
        let item = library.item("m1").expect("query").expect("item");
        assert_eq!(item["played"], true);
        assert_eq!(item["favorite"], true);
        assert_eq!(item["positionTicks"], 0);
    }

    #[test]
    fn item_playback_preferences_survive_reopen_and_are_keyed_to_one_item() {
        let path = std::env::temp_dir().join(format!(
            "mediaflick-track-preference-{}.sqlite",
            crate::app::ids::random_hex(8)
        ));
        let source: MediaSourceInfo = serde_json::from_str(
            r#"{"Id":"source-a","Name":"Main file","Container":"mkv","MediaStreams":[
                    {"Index":1,"Type":"Audio","Language":"jpn","Codec":"dts","Channels":6},
                    {"Index":2,"Type":"Subtitle","Language":"eng","Title":"English SDH",
                     "IsHearingImpaired":true,"IsForced":false}] }"#,
        )
        .expect("media source");
        let preference = ItemPlaybackPreference::capture(
            &source,
            0,
            source.streams_of_type("Audio").next(),
            source.streams_of_type("Subtitle").next(),
        );

        {
            let library = Library::open(&path).expect("create library");
            let mut credentials = library.credentials();
            credentials.server_url = Some("http://server.test".to_string());
            credentials.server_id = Some("server-a".to_string());
            credentials.user_id = Some("user-a".to_string());
            credentials.token = Some("token".to_string());
            library
                .save_credentials(&credentials)
                .expect("save account identity");
            library
                .upsert_page(&[
                    dto(r#"{"Id":"m1","Name":"One","Type":"Movie"}"#),
                    dto(r#"{"Id":"m2","Name":"Two","Type":"Movie"}"#),
                ])
                .expect("seed items");
            library
                .save_playback_preference("m1", &preference)
                .expect("save preference");
            assert_eq!(
                library.playback_preference("m1").expect("load"),
                Some(preference.clone())
            );
            assert_eq!(library.playback_preference("m2").expect("other item"), None);

            credentials.user_id = Some("user-b".to_string());
            library
                .save_credentials(&credentials)
                .expect("switch account");
            assert_eq!(
                library
                    .playback_preference("m1")
                    .expect("other account preference"),
                None
            );
            credentials.user_id = Some("user-a".to_string());
            library
                .save_credentials(&credentials)
                .expect("restore account");
        }

        {
            let library = Library::open(&path).expect("reopen library");
            assert_eq!(
                library.playback_preference("m1").expect("restored"),
                Some(preference)
            );
            assert!(!library.forget("m1").expect("forget item").is_empty());
            assert_eq!(
                library
                    .playback_preference("m1")
                    .expect("preference removed with item"),
                None
            );
        }

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[test]
    fn credentials_round_trip_and_keep_the_device_id_across_logout() {
        let library = Library::open_in_memory().expect("library");
        let mut credentials = library.credentials();
        let device_id = credentials.device_id.clone();
        assert!(!device_id.is_empty());
        assert!(!credentials.is_authenticated());

        credentials.server_url = Some("http://server:8096".to_string());
        credentials.user_id = Some("uid".to_string());
        credentials.user_name = Some("pho".to_string());
        credentials.token = Some("tok".to_string());
        library.save_credentials(&credentials).expect("save");

        let loaded = library.credentials();
        assert!(loaded.is_authenticated());
        assert_eq!(loaded.device_id, device_id);

        library
            .ingest_page(&[dto(
                r#"{"Id":"sparse","Name":"Sparse","Type":"Movie","ProviderIds":{"Tmdb":"1"}}"#,
            )])
            .expect("cache a row");
        assert_eq!(library.stats().total, 1);

        library.clear_session(true).expect("logout");
        let after = library.credentials();
        assert!(!after.is_authenticated());
        assert_eq!(after.device_id, device_id);
        assert_eq!(after.server_url.as_deref(), Some("http://server:8096"));
        assert_eq!(after.token, None);
        assert_eq!(library.stats().total, 0);
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

    #[test]
    fn meta_values_round_trip() {
        let library = Library::open_in_memory().expect("library");
        assert_eq!(library.meta("watermark"), None);
        library.set_meta("watermark", "2024-01-01").expect("set");
        library.set_meta("watermark", "2024-06-01").expect("update");
        assert_eq!(library.meta("watermark").as_deref(), Some("2024-06-01"));
    }

    #[test]
    fn search_expressions_are_prefix_matches_without_fts_operators() {
        assert_eq!(
            fts_match_expression("the matrix"),
            Some("\"the\"* AND \"matrix\"*".to_string())
        );
        assert_eq!(
            fts_match_expression("star OR \"wars\" -x"),
            Some("\"star\"* AND \"OR\"* AND \"wars\"* AND \"x\"*".to_string())
        );
        assert_eq!(fts_match_expression("   "), None);
        assert_eq!(fts_match_expression("***"), None);
    }

    #[test]
    fn civil_dates_convert_at_known_epochs() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }
}
