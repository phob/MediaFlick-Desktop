//! Local metadata cache.
//!
//! The UI reads exclusively from here; only "what should I watch next" and the
//! play path need the server at request time. A background thread ([`sync`])
//! keeps the cache current.

pub mod db;
pub mod headless;
pub mod model;
pub mod sync;

use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, Row, params, params_from_iter};
use serde_json::{Value, json};

use crate::app::ids::new_device_id;
use crate::jellyfin::api::model::BaseItemDto;

pub use model::{ItemRecord, LibraryStats, UserDataRecord};

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
            Self::Name => "i.sort_name COLLATE NOCASE ASC, i.name COLLATE NOCASE ASC",
            Self::Year => "i.year DESC NULLS LAST, i.sort_name COLLATE NOCASE ASC",
            Self::DateAdded => "i.date_created DESC NULLS LAST, i.id DESC",
            Self::CommunityRating => {
                "i.community_rating DESC NULLS LAST, i.sort_name COLLATE NOCASE ASC"
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

    // ------------------------------------------------------------------ write

    /// Upserts a page of items (and their user data) in one transaction.
    pub fn upsert_page(&self, items: &[BaseItemDto]) -> rusqlite::Result<usize> {
        self.db.with_transaction(|transaction| {
            let mut written = 0;
            for dto in items {
                if dto.id.trim().is_empty() {
                    continue;
                }
                upsert_item(transaction, &ItemRecord::from_dto(dto))?;
                if let Some(user_data) = &dto.user_data {
                    upsert_user_data(transaction, &UserDataRecord::from_dto(&dto.id, user_data))?;
                }
                written += 1;
            }
            Ok(written)
        })
    }

    pub fn upsert_user_data(&self, records: &[UserDataRecord]) -> rusqlite::Result<usize> {
        self.db.with_transaction(|transaction| {
            for record in records {
                upsert_user_data(transaction, record)?;
            }
            Ok(records.len())
        })
    }

    /// Removes cached items the server no longer reports.
    pub fn retain_ids(&self, keep: &HashSet<String>) -> rusqlite::Result<usize> {
        let existing = self.all_ids()?;
        let stale = existing.difference(keep).cloned().collect::<Vec<_>>();
        if stale.is_empty() {
            return Ok(0);
        }
        self.db.with_transaction(|transaction| {
            for id in &stale {
                transaction.execute("DELETE FROM items WHERE jellyfin_id = ?1", params![id])?;
                transaction.execute("DELETE FROM user_data WHERE jellyfin_id = ?1", params![id])?;
            }
            Ok(stale.len())
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

    /// Mirrors our own playback reporting so Continue Watching reacts as soon
    /// as the player stops, without waiting for the next user-data sweep.
    pub fn record_playback_progress(
        &self,
        item_id: &str,
        position_ticks: i64,
        finished: bool,
    ) -> rusqlite::Result<()> {
        let position = if finished { 0 } else { position_ticks.max(0) };
        self.db.with_connection(|connection| {
            connection.execute(
                "INSERT INTO user_data (jellyfin_id, played, play_count, playback_position_ticks,
                     last_played_date, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(jellyfin_id) DO UPDATE SET
                     played = max(user_data.played, excluded.played),
                     play_count = CASE WHEN excluded.played = 1
                         THEN max(user_data.play_count, 1) ELSE user_data.play_count END,
                     playback_position_ticks = excluded.playback_position_ticks,
                     last_played_date = excluded.last_played_date,
                     updated_at = excluded.updated_at",
                params![
                    item_id,
                    finished,
                    i64::from(finished),
                    position,
                    iso_now(),
                    now_unix(),
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
            "bm25(items_fts)".to_string()
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
COALESCE(u.is_favorite, 0)";

const DETAIL_COLUMNS: &str = "i.jellyfin_id, i.kind, i.name, i.year, i.runtime_ticks, \
i.community_rating, i.official_rating, i.series_id, i.series_name, i.index_number, \
i.parent_index_number, i.primary_image_tag, i.child_count, i.premiere_date, i.season_id, \
COALESCE(u.played, 0), COALESCE(u.play_count, 0), COALESCE(u.playback_position_ticks, 0), \
COALESCE(u.is_favorite, 0), i.overview, i.genres, i.tags, i.studios, i.people, \
i.backdrop_image_tag, i.critic_rating, i.original_title, i.tmdb_id, i.imdb_id, i.tvdb_id, \
i.parent_id, i.date_created";

fn summary_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
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
    }))
}

fn detail_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let mut value = summary_row(row)?;
    let object = value
        .as_object_mut()
        .expect("summary rows serialize as objects");
    object.insert(
        "overview".to_string(),
        json!(row.get::<_, Option<String>>(19)?),
    );
    object.insert("genres".to_string(), parsed_json(row.get::<_, String>(20)?));
    object.insert("tags".to_string(), parsed_json(row.get::<_, String>(21)?));
    object.insert(
        "studios".to_string(),
        parsed_json(row.get::<_, String>(22)?),
    );
    object.insert("people".to_string(), parsed_json(row.get::<_, String>(23)?));
    object.insert(
        "backdropImageTag".to_string(),
        json!(row.get::<_, Option<String>>(24)?),
    );
    object.insert(
        "criticRating".to_string(),
        json!(row.get::<_, Option<f64>>(25)?),
    );
    object.insert(
        "originalTitle".to_string(),
        json!(row.get::<_, Option<String>>(26)?),
    );
    object.insert(
        "providerIds".to_string(),
        json!({
            "tmdb": row.get::<_, Option<String>>(27)?,
            "imdb": row.get::<_, Option<String>>(28)?,
            "tvdb": row.get::<_, Option<String>>(29)?,
        }),
    );
    object.insert(
        "parentId".to_string(),
        json!(row.get::<_, Option<String>>(30)?),
    );
    object.insert(
        "dateCreated".to_string(),
        json!(row.get::<_, Option<String>>(31)?),
    );
    Ok(value)
}

fn parsed_json(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap_or_else(|_| json!([]))
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
    if query.favorite == Some(true) {
        conditions.push("COALESCE(u.is_favorite, 0) = 1".to_string());
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

fn upsert_item(connection: &Connection, record: &ItemRecord) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO items (
            jellyfin_id, kind, name, original_title, sort_name, year, premiere_date,
            runtime_ticks, overview, community_rating, critic_rating, official_rating,
            parent_id, series_id, series_name, season_id, index_number, parent_index_number,
            child_count, tmdb_id, imdb_id, tvdb_id, genres, tags, studios, people,
            image_tags, primary_image_tag, backdrop_image_tag, search_genres, search_people,
            date_created, date_last_saved, synced_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34
        )
        ON CONFLICT(jellyfin_id) DO UPDATE SET
            kind = excluded.kind, name = excluded.name,
            original_title = excluded.original_title, sort_name = excluded.sort_name,
            year = excluded.year, premiere_date = excluded.premiere_date,
            runtime_ticks = excluded.runtime_ticks, overview = excluded.overview,
            community_rating = excluded.community_rating,
            critic_rating = excluded.critic_rating,
            official_rating = excluded.official_rating, parent_id = excluded.parent_id,
            series_id = excluded.series_id, series_name = excluded.series_name,
            season_id = excluded.season_id, index_number = excluded.index_number,
            parent_index_number = excluded.parent_index_number,
            child_count = excluded.child_count, tmdb_id = excluded.tmdb_id,
            imdb_id = excluded.imdb_id, tvdb_id = excluded.tvdb_id,
            genres = excluded.genres, tags = excluded.tags, studios = excluded.studios,
            people = excluded.people, image_tags = excluded.image_tags,
            primary_image_tag = excluded.primary_image_tag,
            backdrop_image_tag = excluded.backdrop_image_tag,
            search_genres = excluded.search_genres, search_people = excluded.search_people,
            date_created = excluded.date_created, date_last_saved = excluded.date_last_saved,
            synced_at = excluded.synced_at",
        params![
            record.jellyfin_id,
            record.kind,
            record.name,
            record.original_title,
            record.sort_name,
            record.year,
            record.premiere_date,
            record.runtime_ticks,
            record.overview,
            record.community_rating,
            record.critic_rating,
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
            record.tags,
            record.studios,
            record.people,
            record.image_tags,
            record.primary_image_tag,
            record.backdrop_image_tag,
            record.search_genres,
            record.search_people,
            record.date_created,
            record.date_last_saved,
            now_unix(),
        ],
    )?;
    Ok(())
}

fn upsert_user_data(connection: &Connection, record: &UserDataRecord) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO user_data (jellyfin_id, played, play_count, playback_position_ticks,
             is_favorite, played_percentage, last_played_date, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(jellyfin_id) DO UPDATE SET
             played = excluded.played, play_count = excluded.play_count,
             playback_position_ticks = excluded.playback_position_ticks,
             is_favorite = excluded.is_favorite,
             played_percentage = excluded.played_percentage,
             last_played_date = excluded.last_played_date,
             updated_at = excluded.updated_at",
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
    Ok(())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

/// Jellyfin timestamps are ISO-8601 UTC; this is enough precision for ordering.
fn iso_now() -> String {
    let seconds = now_unix();
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.0000000Z",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
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
    use super::{ItemQuery, ItemSort, Library, civil_from_days, fts_match_expression};
    use crate::jellyfin::api::model::BaseItemDto;
    use std::collections::HashSet;

    fn dto(json: &str) -> BaseItemDto {
        serde_json::from_str(json).expect("dto")
    }

    fn seeded() -> Library {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(
                    r#"{"Id":"m1","Name":"The Matrix","Type":"Movie","ProductionYear":1999,
                        "Genres":["Action"],"DateCreated":"2024-01-02","CommunityRating":8.7,
                        "People":[{"Name":"Keanu Reeves"}],
                        "UserData":{"Played":false,"PlaybackPositionTicks":600000000}}"#,
                ),
                dto(
                    r#"{"Id":"m2","Name":"Arrival","Type":"Movie","ProductionYear":2016,
                        "Genres":["Drama"],"DateCreated":"2024-03-04","CommunityRating":7.9,
                        "UserData":{"Played":true}}"#,
                ),
                dto(r#"{"Id":"s1","Name":"Severance","Type":"Series","DateCreated":"2024-02-02"}"#),
                dto(
                    r#"{"Id":"e1","Name":"Good News About Hell","Type":"Episode","SeriesId":"s1",
                        "ParentId":"season1","SeasonId":"season1",
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
    fn search_matches_titles_and_cast_by_prefix() {
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

        let by_actor = library
            .query(&ItemQuery {
                search: Some("keanu".to_string()),
                limit: 10,
                ..Default::default()
            })
            .expect("query");
        assert_eq!(by_actor.total, 1);
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
    }

    #[test]
    fn recently_added_orders_by_creation_date() {
        let rows = seeded().recently_added(10).expect("rows");
        assert_eq!(rows[0]["id"], "m2");
        assert_eq!(rows[1]["id"], "s1");
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

    #[test]
    fn item_detail_includes_people_and_provider_ids() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[dto(
                r#"{"Id":"m1","Name":"The Matrix","Type":"Movie","Overview":"Neo.",
                    "Genres":["Action"],"ProviderIds":{"Tmdb":"603"},
                    "People":[{"Name":"Keanu Reeves","Role":"Neo"}]}"#,
            )])
            .expect("seed");
        let detail = library.item("m1").expect("query").expect("item");
        assert_eq!(detail["overview"], "Neo.");
        assert_eq!(detail["genres"][0], "Action");
        assert_eq!(detail["people"][0]["role"], "Neo");
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
        assert_eq!(library.retain_ids(&keep).expect("retain"), 1);
        assert!(library.item("m2").expect("query").is_none());
        assert_eq!(library.retain_ids(&keep).expect("retain"), 0);
    }

    #[test]
    fn playback_progress_updates_continue_watching_then_clears_on_finish() {
        let library = seeded();
        library
            .record_playback_progress("e1", 1_200_000_000, false)
            .expect("progress");
        let resuming = library.continue_watching(10).expect("rows");
        assert!(resuming.iter().any(|row| row["id"] == "e1"));

        library
            .record_playback_progress("e1", 0, true)
            .expect("finished");
        let after = library.continue_watching(10).expect("rows");
        assert!(!after.iter().any(|row| row["id"] == "e1"));
        assert_eq!(
            library.item("e1").expect("item").expect("row")["played"],
            true
        );
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

        library.clear_session(true).expect("logout");
        let after = library.credentials();
        assert!(!after.is_authenticated());
        assert_eq!(after.device_id, device_id);
        assert_eq!(after.server_url.as_deref(), Some("http://server:8096"));
        assert_eq!(after.token, None);
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
