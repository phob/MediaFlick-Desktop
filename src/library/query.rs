use rusqlite::functions::FunctionFlags;
use std::cmp::Ordering;

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, Row, params, params_from_iter};
use serde_json::{Value, json};

use super::libraries::LibraryId;
use super::{
    CatalogKind, ItemDetail, ItemPage, ItemQuery, ItemSummary, Library, LibraryStats, ProviderIds,
    TmdbCandidate, WatchFilter,
};

/// Bound on one `tmdb_id IN (...)` clause so even a full person filmography
/// stays well under SQLite's host-parameter limit.
const TMDB_CANDIDATE_CHUNK: usize = 400;
const ITEM_ID_CHUNK: usize = 400;

impl Library {
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
            .unwrap_or_else(|error| {
                tracing::warn!(target: "library.db", "could not count cached items: {error}");
                LibraryStats::default()
            })
    }

    pub fn query(&self, query: &ItemQuery) -> rusqlite::Result<ItemPage> {
        self.db.with_connection(|connection| {
            let total = count_items(connection, query)?;
            let items = page_items(connection, query)?;
            Ok(ItemPage { items, total })
        })
    }

    pub fn letter_index(&self, query: &ItemQuery) -> rusqlite::Result<Vec<(String, i64)>> {
        let ItemSql {
            from_clause,
            where_clause,
            order,
            arguments,
        } = query_base(query, true);
        let sql = format!(
            "SELECT letter, min(position) FROM (
                 SELECT CASE WHEN initial BETWEEN 'A' AND 'Z' THEN initial ELSE '#' END AS letter,
                        position
                 FROM (
                     SELECT upper(substr(COALESCE(i.sort_name, i.name), 1, 1)) AS initial,
                            row_number() OVER (ORDER BY {order}) - 1 AS position
                     {from_clause}{where_clause}
                 )
             )
             GROUP BY letter ORDER BY letter"
        );
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare_cached(&sql)?;
            statement
                .query_map(params_from_iter(arguments.iter()), |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?
                .collect()
        })
    }

    pub fn query_page(&self, query: &ItemQuery) -> rusqlite::Result<Vec<ItemSummary>> {
        self.db
            .with_connection(|connection| page_items(connection, query))
    }

    /// Cached Jellyfin ids for these TMDB credits. The cache is only a
    /// pre-filter for live ownership checks because it can hold rows the server
    /// has since deleted. Callers must fetch the ids from Jellyfin before
    /// treating them as owned.
    pub(crate) fn tmdb_candidates(&self, tmdb_ids: &[i64]) -> rusqlite::Result<Vec<TmdbCandidate>> {
        let mut candidates = Vec::new();
        if tmdb_ids.is_empty() {
            return Ok(candidates);
        }
        self.db.with_connection(|connection| {
            for chunk in tmdb_ids.chunks(TMDB_CANDIDATE_CHUNK) {
                let placeholders = vec!["?"; chunk.len()].join(",");
                let sql = format!(
                    "SELECT DISTINCT i.tmdb_id, i.kind, i.jellyfin_id FROM items i \
                     WHERE i.kind IN ('Movie', 'Series') \
                     AND i.tmdb_id IN ({placeholders})"
                );
                let mut statement = connection.prepare(&sql)?;
                let rows = statement.query_map(params_from_iter(chunk.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?;
                for row in rows {
                    let (tmdb_id, kind, item_id) = row?;
                    if let Ok(tmdb_id) = tmdb_id.parse::<i64>() {
                        candidates.push(TmdbCandidate {
                            tmdb_id,
                            kind,
                            item_id,
                        });
                    }
                }
            }
            Ok(())
        })?;
        Ok(candidates)
    }

    pub fn item(&self, item_id: &str) -> rusqlite::Result<Option<ItemDetail>> {
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

    pub fn items_by_ids(&self, item_ids: &[String]) -> rusqlite::Result<Vec<ItemSummary>> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut seen = std::collections::HashSet::new();
        let ids = item_ids
            .iter()
            .filter(|id| seen.insert(id.as_str()))
            .collect::<Vec<_>>();
        self.db.with_connection(|connection| {
            let mut items = Vec::with_capacity(ids.len());
            for chunk in ids.chunks(ITEM_ID_CHUNK) {
                let placeholders = vec!["?"; chunk.len()].join(",");
                let sql = format!(
                    "SELECT {SUMMARY_COLUMNS} FROM items i
                     LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                     WHERE i.jellyfin_id IN ({placeholders})"
                );
                let mut statement = connection.prepare(&sql)?;
                let rows = statement.query_map(params_from_iter(chunk.iter()), summary_row)?;
                items.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
            }
            Ok(items)
        })
    }

    /// Seasons of a series, or episodes of a season, in broadcast order.
    ///
    /// Summary rows only: episode synopses are not cached. The children API
    /// handler overlays them from its live server reconcile when online.
    pub fn children(&self, parent_id: &str) -> rusqlite::Result<Vec<ItemSummary>> {
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

    pub fn continue_watching(&self, limit: i64) -> rusqlite::Result<Vec<ItemSummary>> {
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

    pub fn random_played_movie_with_genre(&self) -> rusqlite::Result<Option<ItemDetail>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {DETAIL_COLUMNS} FROM items i
                 JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 WHERE i.kind = 'Movie' AND u.played = 1
                   AND EXISTS (SELECT 1 FROM item_genres g WHERE g.item_id = i.id)
                 ORDER BY random()
                 LIMIT 1"
            ))?;
            let mut rows = statement.query([])?;
            match rows.next()? {
                Some(row) => Ok(Some(detail_row(row)?)),
                None => Ok(None),
            }
        })
    }

    pub fn recently_added_episodes(&self, limit: usize) -> rusqlite::Result<Vec<ItemSummary>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM items i
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 WHERE i.kind = 'Episode'
                 ORDER BY i.date_created DESC NULLS LAST, i.id DESC"
            ))?;
            let mut rows = statement.query([])?;
            let mut series = std::collections::HashSet::new();
            let mut episodes = Vec::new();
            while episodes.len() < limit
                && let Some(row) = rows.next()?
            {
                let series_id = row.get::<_, Option<String>>(7)?;
                if series_id.is_none_or(|id| series.insert(id)) {
                    episodes.push(summary_row(row)?);
                }
            }
            Ok(episodes)
        })
    }

    pub fn random_billboard_titles(&self, limit: i64) -> rusqlite::Result<Vec<ItemSummary>> {
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

    pub fn genres(&self, library: Option<&LibraryId>) -> rusqlite::Result<Vec<String>> {
        self.db.with_connection(|connection| {
            let mut statement = match library {
                None => connection.prepare_cached(DISTINCT_GENRES_SKIP_SCAN)?,
                Some(_) => connection.prepare_cached(
                    "SELECT DISTINCT g.genre FROM item_libraries l
                     JOIN items i ON i.jellyfin_id = l.item_id
                     JOIN item_genres g ON g.item_id = i.id
                     WHERE l.library_id = ?1
                     ORDER BY g.genre COLLATE NOCASE ASC",
                )?,
            };
            let library = library.map(LibraryId::as_str);
            let rows = statement
                .query_map(params_from_iter(library), |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn official_ratings(
        &self,
        kinds: &[CatalogKind],
        library: Option<&LibraryId>,
    ) -> rusqlite::Result<Vec<String>> {
        let mut ratings = self.db.with_connection(|connection| {
            let placeholders = vec!["?"; kinds.len()].join(", ");
            let in_library = if library.is_some() {
                " AND EXISTS (SELECT 1 FROM item_libraries l
                     WHERE l.library_id = ? AND l.item_id = items.jellyfin_id)"
            } else {
                ""
            };
            let mut statement = connection.prepare(&format!(
                "SELECT DISTINCT official_rating FROM items
                 WHERE official_rating <> '' AND kind IN ({placeholders}){in_library}"
            ))?;
            let arguments = kinds
                .iter()
                .map(|kind| kind.as_str())
                .chain(library.map(LibraryId::as_str));
            let rows = statement
                .query_map(params_from_iter(arguments), |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;
        ratings.sort_by(|left, right| natural_order(left, right));
        Ok(ratings)
    }

    pub fn first_episode(&self, series_id: &str) -> rusqlite::Result<Option<ItemSummary>> {
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

    pub fn previous_episode(&self, item_id: &str) -> rusqlite::Result<Option<ItemSummary>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM items i
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 JOIN items current ON current.jellyfin_id = ?1
                 WHERE i.kind = 'Episode'
                   AND i.series_id = current.series_id
                   AND (COALESCE(i.parent_index_number, 0), COALESCE(i.index_number, 0))
                       < (COALESCE(current.parent_index_number, 0), COALESCE(current.index_number, 0))
                 ORDER BY i.parent_index_number DESC, i.index_number DESC
                 LIMIT 1"
            ))?;
            let mut rows = statement.query(params![item_id])?;
            match rows.next()? {
                Some(row) => Ok(Some(summary_row(row)?)),
                None => Ok(None),
            }
        })
    }

    pub fn next_episode(&self, item_id: &str) -> rusqlite::Result<Option<ItemSummary>> {
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

/// Counted locally rather than cached from the server's series
/// `UnplayedItemCount`: Jellyfin's user-data push climbs only from an episode
/// to its season, so a cached series count would go stale on every pushed
/// episode change.
macro_rules! summary_columns {
    () => {
        "i.jellyfin_id, i.kind, i.name, i.year, i.runtime_ticks, \
i.community_rating, i.official_rating, i.series_id, i.series_name, i.index_number, \
i.parent_index_number, i.primary_image_tag, i.child_count, i.premiere_date, i.season_id, \
COALESCE(u.played, 0), COALESCE(u.play_count, 0), COALESCE(u.playback_position_ticks, 0), \
COALESCE(u.is_favorite, 0), i.image_tags, i.backdrop_image_tag, \
CASE WHEN i.kind = 'Series' THEN (
    SELECT count(*) FROM items e
    LEFT JOIN user_data ue ON ue.jellyfin_id = e.jellyfin_id
    WHERE e.kind = 'Episode' AND e.series_id = i.jellyfin_id AND e.is_virtual = 0
      AND COALESCE(ue.played, 0) = 0
) END"
    };
}

const SUMMARY_COLUMNS: &str = summary_columns!();

const DETAIL_COLUMNS: &str = concat!(
    summary_columns!(),
    ", i.genres, i.original_title, i.tmdb_id, i.imdb_id, i.tvdb_id, i.parent_id, i.date_created"
);

fn cached_image_tag<'a>(image_tags: &'a Value, image_type: &str) -> Option<&'a str> {
    image_tags.as_object()?.iter().find_map(|(key, value)| {
        key.eq_ignore_ascii_case(image_type)
            .then(|| value.as_str())
            .flatten()
    })
}

fn summary_row(row: &Row<'_>) -> rusqlite::Result<ItemSummary> {
    let image_tags = parsed_json(&row.get::<_, String>(19)?);
    Ok(ItemSummary {
        id: row.get(0)?,
        kind: row.get(1)?,
        name: row.get(2)?,
        year: row.get(3)?,
        runtime_ticks: row.get(4)?,
        community_rating: row.get(5)?,
        official_rating: row.get(6)?,
        series_id: row.get(7)?,
        series_name: row.get(8)?,
        index_number: row.get(9)?,
        parent_index_number: row.get(10)?,
        primary_image_tag: row.get(11)?,
        child_count: row.get(12)?,
        premiere_date: row.get(13)?,
        season_id: row.get(14)?,
        played: row.get::<_, i64>(15)? != 0,
        play_count: row.get(16)?,
        position_ticks: row.get(17)?,
        favorite: row.get::<_, i64>(18)? != 0,
        thumb_image_tag: cached_image_tag(&image_tags, "Thumb").map(str::to_string),
        logo_image_tag: cached_image_tag(&image_tags, "Logo").map(str::to_string),
        backdrop_image_tag: row.get(20)?,
        unplayed_count: row.get(21)?,
        primary_aspect: None,
        overview: None,
        playlist_index: None,
    })
}

fn detail_row(row: &Row<'_>) -> rusqlite::Result<ItemDetail> {
    Ok(ItemDetail {
        summary: summary_row(row)?,
        genres: serde_json::from_str(&row.get::<_, String>(22)?).unwrap_or_default(),
        original_title: row.get(23)?,
        provider_ids: ProviderIds {
            tmdb: row.get(24)?,
            imdb: row.get(25)?,
            tvdb: row.get(26)?,
        },
        parent_id: row.get(27)?,
        date_created: row.get(28)?,
    })
}

fn parsed_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| json!([]))
}

fn count_items(connection: &Connection, query: &ItemQuery) -> rusqlite::Result<i64> {
    let needs_user_data = query.watched.is_some() || query.favorite.is_some();
    let ItemSql {
        from_clause,
        where_clause,
        arguments,
        ..
    } = query_base(query, needs_user_data);
    connection
        .prepare_cached(&format!("SELECT count(*) {from_clause}{where_clause}"))?
        .query_row(params_from_iter(arguments.iter()), |row| row.get(0))
}

fn page_items(connection: &Connection, query: &ItemQuery) -> rusqlite::Result<Vec<ItemSummary>> {
    let (sql, arguments) = page_sql(query);
    let mut statement = connection.prepare_cached(&sql)?;
    statement
        .query_map(params_from_iter(arguments.iter()), summary_row)?
        .collect()
}

fn page_sql(query: &ItemQuery) -> (String, Vec<SqlValue>) {
    let ItemSql {
        from_clause,
        where_clause,
        order,
        mut arguments,
    } = query_base(query, true);
    arguments.push(SqlValue::Integer(query.limit.clamp(1, 500)));
    arguments.push(SqlValue::Integer(query.offset.max(0)));
    (
        format!(
            "SELECT {SUMMARY_COLUMNS} {from_clause}{where_clause} ORDER BY {order} LIMIT ? OFFSET ?"
        ),
        arguments,
    )
}

/// SQL functions the catalog queries call. Registered on every pooled
/// connection, because SQLite functions belong to one connection.
pub(super) fn register_functions(connection: &Connection) -> rusqlite::Result<()> {
    connection.create_scalar_function(
        "mediaflick_shuffle",
        2,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |context| Ok(shuffle_key(context.get(0)?, context.get(1)?)),
    )
}

fn shuffle_key(id: i64, seed: i64) -> i64 {
    let combined = id.cast_unsigned() ^ seed.cast_unsigned().wrapping_mul(0x9E37_79B9_7F4A_7C15);
    splitmix64_finalize(combined).cast_signed()
}

fn splitmix64_finalize(value: u64) -> u64 {
    let mixed = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^ (mixed >> 31)
}

/// Each step seeks the next distinct genre in the primary key instead of
/// walking every tagged item.
const DISTINCT_GENRES_SKIP_SCAN: &str = "WITH RECURSIVE distinct_genres(genre) AS (
        SELECT min(genre) FROM item_genres
        UNION ALL
        SELECT (SELECT min(genre) FROM item_genres WHERE genre > previous.genre)
        FROM distinct_genres previous WHERE previous.genre IS NOT NULL
    )
    SELECT genre FROM distinct_genres WHERE genre IS NOT NULL
    ORDER BY genre COLLATE NOCASE ASC";

const SERIES_HAS_STARTED_EPISODE: &str = "EXISTS (SELECT 1 FROM items e
    JOIN user_data ue ON ue.jellyfin_id = e.jellyfin_id
    WHERE e.kind = 'Episode' AND e.series_id = i.jellyfin_id
      AND (ue.played = 1 OR ue.playback_position_ticks > 0))";

const SERIES_HAS_UNWATCHED_PRESENT_EPISODE: &str = "EXISTS (SELECT 1 FROM items e
    LEFT JOIN user_data ue ON ue.jellyfin_id = e.jellyfin_id
    WHERE e.kind = 'Episode' AND e.series_id = i.jellyfin_id AND e.is_virtual = 0
      AND COALESCE(ue.played, 0) = 0)";

const MOVIE_IN_PROGRESS: &str =
    "COALESCE(u.playback_position_ticks, 0) > 0 AND COALESCE(u.played, 0) = 0";

fn in_progress() -> String {
    format!(
        "CASE WHEN i.kind = 'Series' THEN {SERIES_HAS_STARTED_EPISODE} \
        AND {SERIES_HAS_UNWATCHED_PRESENT_EPISODE} ELSE {MOVIE_IN_PROGRESS} END"
    )
}

/// The parts of one catalog item query. The full-text decision is made once
/// here: a matchable search both joins `items_fts` and orders by its rank, and
/// the order clause must never name `items_fts` when the FROM clause lacks it.
struct ItemSql {
    from_clause: String,
    where_clause: String,
    order: String,
    arguments: Vec<SqlValue>,
}

fn query_base(query: &ItemQuery, with_user_data: bool) -> ItemSql {
    let mut conditions = Vec::new();
    let mut arguments = Vec::new();

    let search = query.search.as_deref().and_then(fts_match_expression);
    let (mut from_clause, order) = if search.is_some() {
        (
            "FROM items_fts JOIN items i ON i.id = items_fts.rowid".to_string(),
            "bm25(items_fts), i.id ASC".to_string(),
        )
    } else {
        (
            "FROM items i".to_string(),
            query.sort.order_clause(
                query
                    .direction
                    .unwrap_or_else(|| query.sort.natural_direction()),
            ),
        )
    };
    if with_user_data {
        from_clause.push_str(" LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id");
    }
    if let Some(search) = search {
        conditions.push("items_fts MATCH ?".to_string());
        arguments.push(SqlValue::Text(search));
    } else if query.search.is_some() {
        conditions.push("0".to_string());
    }

    let kinds = query.kinds.kinds();
    let placeholders = vec!["?"; kinds.len()].join(", ");
    conditions.push(format!("i.kind IN ({placeholders})"));
    arguments.extend(
        kinds
            .iter()
            .map(|kind| SqlValue::Text(kind.as_str().to_string())),
    );
    if let Some(library) = &query.library {
        conditions.push(
            "EXISTS (SELECT 1 FROM item_libraries l
                 WHERE l.library_id = ? AND l.item_id = i.jellyfin_id)"
                .to_string(),
        );
        arguments.push(SqlValue::Text(library.as_str().to_string()));
    }
    if let Some(genre) = &query.genre {
        conditions.push("i.id IN (SELECT item_id FROM item_genres WHERE genre = ?)".to_string());
        arguments.push(SqlValue::Text(genre.clone()));
    }
    if let Some(decade) = query.release_decade {
        conditions.push("i.year >= ? AND i.year < ?".to_string());
        arguments.push(SqlValue::Integer(decade));
        arguments.push(SqlValue::Integer(decade.saturating_add(10)));
    }
    if let Some(watched) = query.watched {
        conditions.push(match watched {
            WatchFilter::Unwatched => "COALESCE(u.played, 0) = 0".to_string(),
            WatchFilter::Watched => "COALESCE(u.played, 0) = 1".to_string(),
            WatchFilter::InProgress => in_progress(),
        });
    }
    if let Some(rating) = &query.official_rating {
        conditions.push("i.official_rating = ?".to_string());
        arguments.push(SqlValue::Text(rating.clone()));
    }
    if let Some(status) = query.series_status {
        conditions.push("i.series_status = ?".to_string());
        arguments.push(SqlValue::Text(status.as_str().to_string()));
    }
    if let Some(favorite) = query.favorite {
        conditions.push(if favorite {
            "COALESCE(u.is_favorite, 0) = 1".to_string()
        } else {
            "COALESCE(u.is_favorite, 0) = 0".to_string()
        });
    }

    let where_clause = format!(" WHERE {}", conditions.join(" AND "));
    ItemSql {
        from_clause,
        where_clause,
        order,
        arguments,
    }
}

fn natural_order(left: &str, right: &str) -> Ordering {
    fn chunks(text: &str) -> impl Iterator<Item = (bool, String)> + '_ {
        let mut rest = text;
        std::iter::from_fn(move || {
            let first = rest.chars().next()?;
            let digits = first.is_ascii_digit();
            let end = rest
                .find(|ch: char| ch.is_ascii_digit() != digits)
                .unwrap_or(rest.len());
            let (chunk, tail) = rest.split_at(end);
            rest = tail;
            Some((digits, chunk.to_lowercase()))
        })
    }
    let compare = |(left_digits, left): (bool, String), (right_digits, right): (bool, String)| {
        if left_digits && right_digits {
            let left = left.trim_start_matches('0');
            let right = right.trim_start_matches('0');
            left.len().cmp(&right.len()).then_with(|| left.cmp(right))
        } else {
            left.cmp(&right)
        }
    };
    let mut left_chunks = chunks(left);
    let mut right_chunks = chunks(right);
    loop {
        match (left_chunks.next(), right_chunks.next()) {
            (None, None) => return left.cmp(right),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left_chunk), Some(right_chunk)) => {
                let order = compare(left_chunk, right_chunk);
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

fn fts_match_expression(input: &str) -> Option<String> {
    let tokens = input
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\"*"))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" AND "))
}

#[cfg(test)]
mod tests;
