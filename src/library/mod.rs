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

mod catalog;
mod integrations;
mod playback_preferences;
mod query;
mod session_store;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde_json::Value;

use crate::app::ids::new_device_id;

pub use model::{
    ItemPlaybackPreference, ItemRecord, LibraryStats, UserDataRecord, resolve_playback_preference,
};

pub const EARLIEST_RELEASE_DECADE: i64 = 1900;

/// The newest standard decade that can be selected by the library UI.
pub fn current_release_decade() -> i64 {
    let days = now_unix().div_euclid(86_400);
    let (year, _, _) = query::civil_from_days(days);
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

impl StoredCredentials {
    pub fn is_authenticated(&self) -> bool {
        self.token.is_some() && self.user_id.is_some() && self.server_url.is_some()
    }
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
}

pub(crate) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
pub(super) mod test_support {
    use super::Library;
    use crate::jellyfin::api::model::BaseItemDto;

    pub fn dto(json: &str) -> BaseItemDto {
        serde_json::from_str(json).expect("dto")
    }

    pub fn seeded() -> Library {
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
}

#[cfg(test)]
mod tests {
    use super::Library;

    #[test]
    fn meta_values_round_trip() {
        let library = Library::open_in_memory().expect("library");
        assert_eq!(library.meta("watermark"), None);
        library.set_meta("watermark", "2024-01-01").expect("set");
        library.set_meta("watermark", "2024-06-01").expect("update");
        assert_eq!(library.meta("watermark").as_deref(), Some("2024-06-01"));
    }
}
