use rusqlite::functions::FunctionFlags;
use std::cmp::Ordering;

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, Row, params, params_from_iter};
use serde_json::{Value, json};

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
        let (from_clause, where_clause, arguments) = query_base(query, true);
        let order = order_by(query);
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

    pub fn genres(&self) -> rusqlite::Result<Vec<String>> {
        // Skip-scan: each step seeks the next distinct genre in the primary key
        // instead of walking every tagged item.
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare_cached(
                "WITH RECURSIVE distinct_genres(genre) AS (
                     SELECT min(genre) FROM item_genres
                     UNION ALL
                     SELECT (SELECT min(genre) FROM item_genres WHERE genre > previous.genre)
                     FROM distinct_genres previous WHERE previous.genre IS NOT NULL
                 )
                 SELECT genre FROM distinct_genres WHERE genre IS NOT NULL
                 ORDER BY genre COLLATE NOCASE ASC",
            )?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn official_ratings(&self, kinds: &[CatalogKind]) -> rusqlite::Result<Vec<String>> {
        let mut ratings = self.db.with_connection(|connection| {
            let placeholders = vec!["?"; kinds.len()].join(", ");
            let mut statement = connection.prepare(&format!(
                "SELECT DISTINCT official_rating FROM items
                 WHERE official_rating <> '' AND kind IN ({placeholders})"
            ))?;
            let rows = statement
                .query_map(
                    params_from_iter(kinds.iter().map(|kind| kind.as_str())),
                    |row| row.get::<_, String>(0),
                )?
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
        overview: None,
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
    let (from_clause, where_clause, arguments) = query_base(query, needs_user_data);
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
    let (from_clause, where_clause, mut arguments) = query_base(query, true);
    let order = order_by(query);
    arguments.push(SqlValue::Integer(query.limit.clamp(1, 500)));
    arguments.push(SqlValue::Integer(query.offset.max(0)));
    (
        format!(
            "SELECT {SUMMARY_COLUMNS} {from_clause}{where_clause} ORDER BY {order} LIMIT ? OFFSET ?"
        ),
        arguments,
    )
}

fn order_by(query: &ItemQuery) -> String {
    if query
        .search
        .as_deref()
        .and_then(fts_match_expression)
        .is_some()
    {
        return "bm25(items_fts), i.id ASC".to_string();
    }
    query.sort.order_clause(
        query
            .direction
            .unwrap_or_else(|| query.sort.natural_direction()),
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

fn query_base(query: &ItemQuery, with_user_data: bool) -> (String, String, Vec<SqlValue>) {
    let mut conditions = Vec::new();
    let mut arguments = Vec::new();

    let search = query.search.as_deref().and_then(fts_match_expression);
    let mut from_clause = if search.is_some() {
        "FROM items_fts JOIN items i ON i.id = items_fts.rowid".to_string()
    } else {
        "FROM items i".to_string()
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
    (from_clause, where_clause, arguments)
}

pub(super) fn natural_order(left: &str, right: &str) -> Ordering {
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

pub(super) fn civil_from_days(days: i64) -> (i64, u32, u32) {
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
    use std::collections::HashSet;

    use serde_json::json;

    use super::{ItemQuery, Library, WatchFilter, civil_from_days};
    use crate::library::test_support::{dto, seeded};
    use crate::library::{
        CatalogKind, ItemSort, KindFilter, current_release_decade, release_decade_from_id,
    };

    #[test]
    fn search_matches_genres_by_prefix() {
        let library = seeded();
        let by_genre = library
            .query(&ItemQuery {
                search: Some("acti".to_string()),
                limit: 10,
                ..Default::default()
            })
            .expect("query");
        assert_eq!(by_genre.total, 1);
        assert_eq!(by_genre.items[0].id, "m1");
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
                kinds: KindFilter::Only(vec![CatalogKind::Movie]),
                genre: Some("Action".to_string()),
                release_decade: Some(1990),
                watched: Some(WatchFilter::Unwatched),
                favorite: Some(true),
                limit: 1,
                ..Default::default()
            })
            .expect("combined filters");
        assert_eq!(page.total, 2);
        assert_eq!(page.items.len(), 1);

        let all = library
            .query(&ItemQuery {
                kinds: KindFilter::Only(vec![CatalogKind::Movie]),
                genre: Some("Action".to_string()),
                release_decade: Some(1990),
                watched: Some(WatchFilter::Unwatched),
                favorite: Some(true),
                limit: 10,
                ..Default::default()
            })
            .expect("all combined filters");
        assert_eq!(
            all.items
                .iter()
                .map(|item| item.id.as_str())
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
                dto(r#"{"Id":"series","Name":"Show","Type":"Series",
                    "PremiereDate":"2017-02-15T00:00:00.0000000Z"}"#),
            ])
            .expect("seed");

        let movies = library
            .query(&ItemQuery {
                kinds: KindFilter::Only(vec![CatalogKind::Movie]),
                release_decade: Some(1990),
                limit: 10,
                ..Default::default()
            })
            .expect("movies");
        let series = library
            .query(&ItemQuery {
                kinds: KindFilter::Only(vec![CatalogKind::Series]),
                release_decade: Some(2010),
                limit: 10,
                ..Default::default()
            })
            .expect("series");

        assert_eq!(movies.items[0].id, "movie");
        assert_eq!(series.items[0].id, "series");
        assert_eq!(series.items[0].year, Some(2017));
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
    fn titles_without_a_server_sort_name_sort_by_their_name() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(r#"{"Id":"gamma","Name":"gamma","Type":"Movie"}"#),
                dto(r#"{"Id":"beta","Name":"Beta","Type":"Movie"}"#),
                dto(r#"{"Id":"alpha","Name":"Alpha","Type":"Movie","SortName":"alpha"}"#),
            ])
            .expect("seed");

        let ids = library
            .query(&ItemQuery {
                kinds: KindFilter::Only(vec![CatalogKind::Movie]),
                sort: ItemSort::Name,
                limit: 10,
                ..Default::default()
            })
            .expect("query")
            .items
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, ["alpha", "beta", "gamma"]);
    }

    #[test]
    fn continue_watching_lists_partially_played_items() {
        let rows = seeded().continue_watching(10).expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "m1");
        assert_eq!(rows[0].position_ticks, 600_000_000i64);
    }

    #[test]
    fn recently_added_episodes_keep_the_newest_episode_of_each_series() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(
                    r#"{"Id":"sev-1","Name":"One","Type":"Episode","SeriesId":"sev",
                    "IndexNumber":1,"ParentIndexNumber":1,"DateCreated":"2024-01-01"}"#,
                ),
                dto(
                    r#"{"Id":"silo-1","Name":"One","Type":"Episode","SeriesId":"silo",
                    "IndexNumber":1,"ParentIndexNumber":1,"DateCreated":"2024-02-01"}"#,
                ),
                dto(
                    r#"{"Id":"sev-2","Name":"Two","Type":"Episode","SeriesId":"sev",
                    "IndexNumber":2,"ParentIndexNumber":1,"DateCreated":"2024-03-01"}"#,
                ),
                dto(
                    r#"{"Id":"andor-1","Name":"One","Type":"Episode","SeriesId":"andor",
                    "IndexNumber":1,"ParentIndexNumber":1,"DateCreated":"2023-12-01"}"#,
                ),
                dto(r#"{"Id":"movie","Name":"Film","Type":"Movie","DateCreated":"2024-04-01"}"#),
            ])
            .expect("seed");

        let ids = |limit| {
            library
                .recently_added_episodes(limit)
                .expect("rows")
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(10), ["sev-2", "silo-1", "andor-1"]);
        assert_eq!(ids(2), ["sev-2", "silo-1"]);
    }

    #[test]
    fn children_returns_episodes_of_a_season_in_order() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(
                    r#"{"Id":"e2","Name":"A Second","Type":"Episode","SeriesId":"s1",
                        "ParentId":"season1","SeasonId":"season1",
                        "IndexNumber":2,"ParentIndexNumber":1}"#,
                ),
                dto(
                    r#"{"Id":"e1","Name":"Z First","Type":"Episode","SeriesId":"s1",
                        "ParentId":"season1","SeasonId":"season1",
                        "IndexNumber":1,"ParentIndexNumber":1}"#,
                ),
            ])
            .expect("seed");

        let ids = library
            .children("season1")
            .expect("children")
            .into_iter()
            .map(|row| row.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, ["e1", "e2"]);
    }

    #[test]
    fn next_episode_follows_broadcast_order() {
        let library = seeded();
        let next = library.next_episode("e1").expect("next").expect("episode");
        assert_eq!(next.id, "e2");
        assert!(library.next_episode("e2").expect("next").is_none());
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
        assert_eq!(detail.genres[0], "Action");
        assert_eq!(detail.original_title.as_deref(), Some("Matrix"));
        assert_eq!(detail.provider_ids.tmdb.as_deref(), Some("603"));
        assert!(library.item("missing").expect("query").is_none());

        let wire = serde_json::to_value(&detail).expect("detail json");
        assert_eq!(wire["id"], "m1");
        assert_eq!(wire["kind"], "Movie");
        assert_eq!(wire["played"], false);
        assert_eq!(wire["genres"], json!(["Action"]));
        assert_eq!(
            wire["providerIds"],
            json!({ "tmdb": "603", "imdb": null, "tvdb": null })
        );
        assert!(wire.get("summary").is_none());
    }

    #[test]
    fn genre_lookups_follow_items_through_updates() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(r#"{"Id":"a","Name":"A","Type":"Movie","Genres":["drama","Action",""]}"#),
                dto(r#"{"Id":"b","Name":"B","Type":"Series","Genres":["Action"]}"#),
            ])
            .expect("seed");
        assert_eq!(
            library.genres().expect("genres"),
            vec!["Action", "drama"],
            "ordered case-insensitively, without empty names"
        );

        library
            .upsert_page(&[dto(
                r#"{"Id":"a","Name":"A","Type":"Movie","Genres":["Comedy"]}"#,
            )])
            .expect("retag");
        assert_eq!(library.genres().expect("genres"), vec!["Action", "Comedy"]);
        let action = library
            .query(&ItemQuery {
                genre: Some("Action".to_string()),
                limit: 10,
                ..Default::default()
            })
            .expect("action");
        assert_eq!(action.total, 1);
        assert_eq!(action.items[0].id, "b");

        library.forget("b").expect("forget");
        assert_eq!(library.genres().expect("genres"), vec!["Comedy"]);
    }

    #[test]
    fn typed_search_is_an_all_words_prefix_match_that_ignores_fts_syntax() {
        let library = seeded();
        let search = |text: &str| {
            library.query(&ItemQuery {
                search: Some(text.to_string()),
                limit: 10,
                ..Default::default()
            })
        };

        let matrix = search(r#"matr" -"#).expect("punctuation is not a syntax error");
        assert_eq!(matrix.total, 1);
        assert_eq!(matrix.items[0].id, "m1");
        assert_eq!(search("matrix OR arrival").expect("operator word").total, 0);
        assert_eq!(search("the arriv").expect("two words").total, 0);
    }

    #[test]
    fn search_follows_renamed_and_removed_items() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[dto(r#"{"Id":"a","Name":"The Matrix","Type":"Movie"}"#)])
            .expect("seed");
        library
            .upsert_page(&[dto(r#"{"Id":"a","Name":"Reloaded","Type":"Movie"}"#)])
            .expect("rename");
        let total = |text: &str| {
            library
                .query(&ItemQuery {
                    search: Some(text.to_string()),
                    limit: 10,
                    ..Default::default()
                })
                .expect("search")
                .total
        };
        assert_eq!(total("matr"), 0, "the old name still matches");
        assert_eq!(total("reloa"), 1);

        library.forget("a").expect("forget");
        library
            .upsert_page(&[dto(r#"{"Id":"b","Name":"Arrival","Type":"Movie"}"#)])
            .expect("reuse");
        assert_eq!(total("reloa"), 0, "a removed name still matches");
        assert_eq!(total("arriv"), 1);
    }

    #[test]
    fn civil_dates_convert_at_known_epochs() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }
}
