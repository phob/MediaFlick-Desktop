use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, Row, params, params_from_iter};
use serde_json::{Value, json};

use super::{
    ItemDetail, ItemPage, ItemQuery, ItemSummary, Library, LibraryStats, ProviderIds, TmdbCandidate,
};

/// Bound on one `tmdb_id IN (...)` clause so even a full person filmography
/// stays well under SQLite's host-parameter limit.
const TMDB_CANDIDATE_CHUNK: usize = 400;
const ITEM_ID_CHUNK: usize = 400;

impl Library {
    pub fn has_items(&self) -> rusqlite::Result<bool> {
        self.db.with_connection(|connection| {
            connection.query_row("SELECT EXISTS(SELECT 1 FROM items)", [], |row| row.get(0))
        })
    }

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
            // Counts are only displayed; zero is the honest fallback.
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

    /// One page without its total, for surfaces such as Home rows that never
    /// show a count.
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

    /// Card rows for a bounded set of Jellyfin ids. Collection pages already
    /// know their exact local members, so one batched read avoids a separate
    /// app-scheme request and SQLite connection for every mounted card.
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

    /// One stable recommendation seed can be selected from this process's
    /// account cache. The detail row carries the leading genre needed by Home.
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

    /// The newest added episode of each series, newest first. Rows stream from
    /// `items_kind_added`, so the walk stops once `limit` series are found
    /// instead of grouping every cached episode.
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

    /// A fresh set of movies and series for the home billboard.
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

    /// The earliest episode of a series in broadcast order.
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

    /// The episode immediately before `item_id` inside its series.
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

    /// The episode that follows `item_id` inside its series.
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

fn cached_image_tag<'a>(image_tags: &'a Value, image_type: &str) -> Option<&'a str> {
    image_tags.as_object()?.iter().find_map(|(key, value)| {
        key.eq_ignore_ascii_case(image_type)
            .then(|| value.as_str())
            .flatten()
    })
}

/// Reads [`SUMMARY_COLUMNS`], which [`DETAIL_COLUMNS`] begins with.
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
        overview: None,
    })
}

fn detail_row(row: &Row<'_>) -> rusqlite::Result<ItemDetail> {
    Ok(ItemDetail {
        summary: summary_row(row)?,
        genres: serde_json::from_str(&row.get::<_, String>(21)?).unwrap_or_default(),
        original_title: row.get(22)?,
        provider_ids: ProviderIds {
            tmdb: row.get(23)?,
            imdb: row.get(24)?,
            tvdb: row.get(25)?,
        },
        parent_id: row.get(26)?,
        date_created: row.get(27)?,
    })
}

fn parsed_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| json!([]))
}

fn count_items(connection: &Connection, query: &ItemQuery) -> rusqlite::Result<i64> {
    // `user_data` holds at most one row per item, so the join only matters to
    // the count when a condition reads it.
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
    // Relevance beats alphabetical order while the user is typing.
    let order = if query.search.is_some() {
        "bm25(items_fts), i.id ASC"
    } else {
        query.sort.order_clause()
    };
    arguments.push(SqlValue::Integer(query.limit.clamp(1, 500)));
    arguments.push(SqlValue::Integer(query.offset.max(0)));
    (
        format!(
            "SELECT {SUMMARY_COLUMNS} {from_clause}{where_clause} ORDER BY {order} LIMIT ? OFFSET ?"
        ),
        arguments,
    )
}

/// Builds the FROM clause, WHERE clause, and bound arguments for a query.
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
    }

    if !query.kinds.is_empty() {
        let placeholders = vec!["?"; query.kinds.len()].join(", ");
        conditions.push(format!("i.kind IN ({placeholders})"));
        for kind in &query.kinds {
            arguments.push(SqlValue::Text(kind.clone()));
        }
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

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    (from_clause, where_clause, arguments)
}

fn fts_match_expression(input: &str) -> Option<String> {
    let tokens = input
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\"*"))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" AND "))
}

/// Howard Hinnant's `civil_from_days`, the standard days-to-date conversion.
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

    use super::{ITEM_ID_CHUNK, ItemQuery, Library, civil_from_days};
    use crate::library::test_support::{dto, seeded};
    use crate::library::{ItemSort, TmdbCandidate, current_release_decade, release_decade_from_id};

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
        assert_eq!(page.items[0].name, "The Matrix");

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
        assert_eq!(page.items[0].id, "m1");

        let watched = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                watched: Some(true),
                limit: 10,
                ..Default::default()
            })
            .expect("query");
        assert_eq!(watched.total, 1);
        assert_eq!(watched.items[0].id, "m2");
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
        assert_eq!(favorites.items[0].id, "m1");

        let not_favorites = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                favorite: Some(false),
                limit: 10,
                ..Default::default()
            })
            .expect("not favorites");
        assert_eq!(not_favorites.total, 1);
        assert_eq!(not_favorites.items[0].id, "m2");
    }

    #[test]
    fn tmdb_candidates_list_only_browsable_kinds_with_parsed_ids() {
        let library = Library::open_in_memory().expect("library");
        library
            .upsert_page(&[
                dto(r#"{"Id":"movie","Name":"Film","Type":"Movie","ProviderIds":{"Tmdb":"603"}}"#),
                dto(r#"{"Id":"series","Name":"Show","Type":"Series","ProviderIds":{"tmdb":"769"}}"#),
                dto(r#"{"Id":"episode","Name":"Episode","Type":"Episode","ProviderIds":{"Tmdb":"603"}}"#),
                dto(r#"{"Id":"unparsed","Name":"Odd","Type":"Movie","ProviderIds":{"Tmdb":"not-a-number"}}"#),
            ])
            .expect("seed");

        let mut candidates = library
            .tmdb_candidates(&[603, 769, 404])
            .expect("candidates");
        candidates.sort();
        assert_eq!(
            candidates,
            vec![
                TmdbCandidate {
                    tmdb_id: 603,
                    kind: "Movie".to_string(),
                    item_id: "movie".to_string(),
                },
                TmdbCandidate {
                    tmdb_id: 769,
                    kind: "Series".to_string(),
                    item_id: "series".to_string(),
                },
            ]
        );
    }

    #[test]
    fn items_by_ids_batches_large_sets_and_deduplicates_requested_ids() {
        let library = Library::open_in_memory().expect("library");
        let source = (0..=ITEM_ID_CHUNK)
            .map(|index| {
                dto(&format!(
                    r#"{{"Id":"item-{index}","Name":"Movie {index}","Type":"Movie", "Overview":"Rich metadata"}}"#
                ))
            })
            .collect::<Vec<_>>();
        library.upsert_page(&source).expect("seed items");
        let mut ids = (0..=ITEM_ID_CHUNK)
            .rev()
            .map(|index| format!("item-{index}"))
            .collect::<Vec<_>>();
        ids.push("item-0".to_string());
        ids.push("missing".to_string());

        let rows = library.items_by_ids(&ids).expect("batch lookup");
        assert_eq!(rows.len(), ITEM_ID_CHUNK + 1);
        assert_eq!(
            rows.iter()
                .map(|row| row.id.as_str())
                .collect::<HashSet<_>>()
                .len(),
            ITEM_ID_CHUNK + 1
        );
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
        assert_eq!(by_year.items[0].id, "m2");

        let second_page = library
            .query(&ItemQuery {
                kinds: vec!["Movie".to_string()],
                sort: ItemSort::Year,
                limit: 1,
                offset: 1,
                ..Default::default()
            })
            .expect("query");
        assert_eq!(second_page.items[0].id, "m1");
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
                kinds: vec!["Movie".to_string()],
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
        assert_eq!(rows[0].thumb_image_tag.as_deref(), Some("thumb-tag"));
        assert_eq!(rows[0].backdrop_image_tag.as_deref(), Some("backdrop-tag"));
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
    fn recommendation_seed_is_a_played_movie_with_genres() {
        let seed = seeded()
            .random_played_movie_with_genre()
            .expect("query")
            .expect("seed");
        assert_eq!(seed.summary.id, "m2");
        assert_eq!(seed.genres[0], "Drama");
    }

    #[test]
    fn billboard_titles_include_movies_and_series_with_landscape_artwork() {
        let rows = seeded().random_billboard_titles(5).expect("rows");
        let mut kinds = rows
            .iter()
            .map(|row| (row.id.as_str(), row.kind.as_str()))
            .collect::<Vec<_>>();
        kinds.sort_unstable();
        assert_eq!(kinds, [("m1", "Movie"), ("s1", "Series")]);
    }

    #[test]
    fn children_returns_episodes_of_a_season_in_order() {
        // Inserted against broadcast order, and with names that sort the
        // other way, so neither row order nor a name sort can pass.
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
    fn previous_episode_reverses_broadcast_order() {
        let library = seeded();
        let previous = library
            .previous_episode("e2")
            .expect("previous")
            .expect("episode");
        assert_eq!(previous.id, "e1");
        assert!(library.previous_episode("e1").expect("previous").is_none());
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

        // The UI's ItemDetail extends ItemSummary: one flat object.
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
    fn a_page_without_its_total_matches_the_counted_page() {
        let library = seeded();
        let query = ItemQuery {
            kinds: vec!["Movie".to_string(), "Series".to_string()],
            sort: ItemSort::DateAdded,
            limit: 2,
            ..Default::default()
        };
        assert_eq!(
            library.query_page(&query).expect("page"),
            library.query(&query).expect("counted").items
        );
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

        // Quotes, dashes, and operator words are text, not FTS syntax.
        let matrix = search(r#"matr" -"#).expect("punctuation is not a syntax error");
        assert_eq!(matrix.total, 1);
        assert_eq!(matrix.items[0].id, "m1");
        assert_eq!(search("matrix OR arrival").expect("operator word").total, 0);
        // Every typed word must match, not any of them.
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

        // The next insert reuses the removed row id, so a leftover index
        // entry would make the old name find the new title.
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
