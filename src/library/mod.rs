//! Local catalog index.
//!
//! The UI browses, sorts, filters, and searches exclusively from here; rich
//! metadata (synopsis, cast, technical streams) is fetched live from Jellyfin
//! by the surfaces that need it. A background thread ([`sync`]) keeps the thin
//! index current.

pub mod browse;
pub mod db;
pub mod headless;
pub mod libraries;
pub mod model;
pub mod sync;

mod catalog;
mod catalog_libraries;
mod integrations;
mod legacy;
mod query;
mod session_store;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{OptionalExtension, params};
use serde::{Serialize, Serializer};
use serde_json::Value;

use crate::app::ids::new_device_id;
use libraries::LibraryId;

pub use model::{
    CatalogKind, DetailView, GenericItemDetail, ItemDetail, ItemPlaybackPreference, ItemRecord,
    ItemSummary, KindFilter, LibraryStats, ProviderIds, TitleDetail, TitleKind, UserDataRecord,
    resolve_playback_preference,
};

pub const EARLIEST_RELEASE_DECADE: i64 = 1900;

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

    pub fn unnamed<'a>(&self, ids: impl IntoIterator<Item = &'a String>) -> Vec<String> {
        let mut unnamed = ids
            .into_iter()
            .filter(|id| !self.item_ids.contains(id) && !self.context_ids.contains(id))
            .cloned()
            .collect::<Vec<_>>();
        normalize_ids(&mut unnamed);
        unnamed
    }
}

fn normalize_ids(ids: &mut Vec<String>) {
    ids.sort_unstable();
    ids.dedup();
}

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
        self.token.is_some()
            && self.user_id.is_some()
            && self.server_id.is_some()
            && self.server_url.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TmdbCandidate {
    pub tmdb_id: i64,
    pub kind: String,
    pub item_id: String,
}

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "asc" => Some(Self::Ascending),
            "desc" => Some(Self::Descending),
            _ => None,
        }
    }

    fn sql(self) -> &'static str {
        match self {
            Self::Ascending => "ASC",
            Self::Descending => "DESC",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ItemSort {
    #[default]
    Name,
    ReleaseDate,
    DateAdded,
    EpisodeAdded,
    CommunityRating,
    DatePlayed,
    PlayCount,
    Runtime,
    Random {
        seed: u32,
    },
}

/// Serialized as [`SortKey::id`]; the TypeScript names repeat those ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
pub enum SortKey {
    #[ts(rename = "name")]
    Name,
    #[ts(rename = "year")]
    ReleaseDate,
    #[ts(rename = "added")]
    DateAdded,
    #[ts(rename = "episode-added")]
    EpisodeAdded,
    #[ts(rename = "rating")]
    CommunityRating,
    #[ts(rename = "played")]
    DatePlayed,
    #[ts(rename = "plays")]
    PlayCount,
    #[ts(rename = "runtime")]
    Runtime,
    #[ts(rename = "random")]
    Random,
}

impl SortKey {
    const ALL: [Self; 9] = [
        Self::Name,
        Self::ReleaseDate,
        Self::DateAdded,
        Self::EpisodeAdded,
        Self::CommunityRating,
        Self::DatePlayed,
        Self::PlayCount,
        Self::Runtime,
        Self::Random,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::ReleaseDate => "year",
            Self::DateAdded => "added",
            Self::EpisodeAdded => "episode-added",
            Self::CommunityRating => "rating",
            Self::DatePlayed => "played",
            Self::PlayCount => "plays",
            Self::Runtime => "runtime",
            Self::Random => "random",
        }
    }

    fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dateadded" => Some(Self::DateAdded),
            "communityrating" => Some(Self::CommunityRating),
            id => Self::ALL.into_iter().find(|key| key.id() == id),
        }
    }
}

impl Serialize for SortKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.id())
    }
}

impl ItemSort {
    pub fn from_id(value: &str, seed: Option<u32>) -> Option<Self> {
        Some(match SortKey::from_id(value)? {
            SortKey::Name => Self::Name,
            SortKey::ReleaseDate => Self::ReleaseDate,
            SortKey::DateAdded => Self::DateAdded,
            SortKey::EpisodeAdded => Self::EpisodeAdded,
            SortKey::CommunityRating => Self::CommunityRating,
            SortKey::DatePlayed => Self::DatePlayed,
            SortKey::PlayCount => Self::PlayCount,
            SortKey::Runtime => Self::Runtime,
            SortKey::Random => Self::Random {
                seed: seed.unwrap_or_default(),
            },
        })
    }

    pub fn key(self) -> SortKey {
        match self {
            Self::Name => SortKey::Name,
            Self::ReleaseDate => SortKey::ReleaseDate,
            Self::DateAdded => SortKey::DateAdded,
            Self::EpisodeAdded => SortKey::EpisodeAdded,
            Self::CommunityRating => SortKey::CommunityRating,
            Self::DatePlayed => SortKey::DatePlayed,
            Self::PlayCount => SortKey::PlayCount,
            Self::Runtime => SortKey::Runtime,
            Self::Random { .. } => SortKey::Random,
        }
    }

    pub fn natural_direction(self) -> SortDirection {
        match self {
            Self::Name => SortDirection::Ascending,
            _ => SortDirection::Descending,
        }
    }

    fn order_clause(self, direction: SortDirection) -> String {
        const TIEBREAK: &str = "i.sort_name COLLATE NOCASE ASC, i.id ASC";
        let direction = direction.sql();
        let key = match self {
            Self::Random { seed } => return format!("mediaflick_shuffle(i.id, {seed}), i.id ASC"),
            Self::Name => {
                return format!(
                    "i.sort_name COLLATE NOCASE {direction}, i.name COLLATE NOCASE ASC, i.id ASC"
                );
            }
            Self::ReleaseDate => {
                return format!(
                    "i.year {direction} NULLS LAST, i.premiere_date {direction} NULLS LAST, {TIEBREAK}"
                );
            }
            Self::DateAdded => "i.date_created",
            Self::EpisodeAdded => {
                "COALESCE((SELECT max(e.date_created) FROM items e
                    WHERE e.kind = 'Episode' AND e.series_id = i.jellyfin_id
                      AND e.is_virtual = 0), i.date_created)"
            }
            Self::CommunityRating => "i.community_rating",
            // SQLite's two-argument max() is NULL when either side is, so
            // absent dates compare as '' and turn back into NULL afterwards.
            Self::DatePlayed => {
                "CASE WHEN i.kind = 'Series' THEN NULLIF(max(
                    COALESCE(u.last_played_date, ''),
                    COALESCE((SELECT max(ue.last_played_date) FROM items e
                        JOIN user_data ue ON ue.jellyfin_id = e.jellyfin_id
                        WHERE e.kind = 'Episode' AND e.series_id = i.jellyfin_id), '')
                ), '') ELSE u.last_played_date END"
            }
            Self::PlayCount => "COALESCE(u.play_count, 0)",
            Self::Runtime => "i.runtime_ticks",
        };
        format!("{key} {direction} NULLS LAST, {TIEBREAK}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchFilter {
    Unwatched,
    InProgress,
    Watched,
}

impl WatchFilter {
    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim() {
            "unwatched" => Some(Self::Unwatched),
            "progress" => Some(Self::InProgress),
            "watched" => Some(Self::Watched),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesStatus {
    Continuing,
    Ended,
}

impl SeriesStatus {
    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim() {
            "Continuing" => Some(Self::Continuing),
            "Ended" => Some(Self::Ended),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Continuing => "Continuing",
            Self::Ended => "Ended",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ItemQuery {
    pub search: Option<String>,
    pub kinds: KindFilter,
    pub library: Option<LibraryId>,
    pub genre: Option<String>,
    /// Inclusive first year of a standard ten-year release decade.
    pub release_decade: Option<i64>,
    pub watched: Option<WatchFilter>,
    pub favorite: Option<bool>,
    pub official_rating: Option<String>,
    pub series_status: Option<SeriesStatus>,
    pub sort: ItemSort,
    pub direction: Option<SortDirection>,
    pub offset: i64,
    pub limit: i64,
}

pub const DEFAULT_PAGE_SIZE: i64 = 60;

pub const MAX_CHILD_PAGE_LIMIT: i64 = 200;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct ItemPage {
    pub items: Vec<ItemSummary>,
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

    pub fn meta(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT value FROM meta WHERE key = ?1",
                    params![key],
                    |row| row.get::<_, String>(0),
                )
                .optional()
        })
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

    pub fn optimize(&self) -> rusqlite::Result<()> {
        self.db.optimize()
    }

    pub(crate) fn with_connection<T>(
        &self,
        work: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        self.db.with_connection(work)
    }

    pub(crate) fn with_transaction<T>(
        &self,
        work: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        self.db.with_transaction(work)
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
