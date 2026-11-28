use rusqlite::functions::FunctionFlags;

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, Row, params, params_from_iter};
use serde_json::{Value, json};

use super::libraries::LibraryId;
use super::{
    CatalogKind, ItemDetail, ItemPage, ItemQuery, ItemSort, ItemSummary, Library, LibraryStats,
    ProviderIds, TmdbCandidate, WatchFilter,
};

mod fast;
mod sql;

use sql::{ItemSql, QueryPlan, fts_match_expression, natural_order, query_base};

const TMDB_CANDIDATE_CHUNK: usize = 400;
const ITEM_ID_CHUNK: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facet {
    Genre,
    Studio,
    Tag,
}

impl Facet {
    pub(super) const fn table(self) -> (&'static str, &'static str) {
        match self {
            Self::Genre => ("item_genres", "genre"),
            Self::Studio => ("item_studios", "studio"),
            Self::Tag => ("item_tags", "tag"),
        }
    }
}

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
            if fast::uses_single_pass_query(query) {
                return fast::query_and_count_single_pass(connection, query);
            }
            let total = count_items(connection, query)?;
            let items = page_items(connection, query)?;
            Ok(ItemPage { items, total })
        })
    }

    pub fn letter_index(&self, query: &ItemQuery) -> rusqlite::Result<Vec<(String, i64)>> {
        if matches!(query.sort, ItemSort::Name) && query.search.is_none() {
            return self.letter_index_grouped(query);
        }
        let plan = QueryPlan {
            user_data: query.watched.is_some()
                || query.favorite.is_some()
                || matches!(query.sort, ItemSort::DatePlayed | ItemSort::PlayCount),
            series_aggregates: matches!(query.watched, Some(WatchFilter::InProgress))
                || matches!(query.sort, ItemSort::EpisodeAdded | ItemSort::DatePlayed),
            drive_by_library: true,
            letter_cover: false,
        };
        let ItemSql {
            from_clause,
            where_clause,
            order,
            arguments,
            ..
        } = query_base(query, plan);
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

    fn letter_index_grouped(&self, query: &ItemQuery) -> rusqlite::Result<Vec<(String, i64)>> {
        let kinds = query.kinds.kinds();
        let unfiltered_single_top_kind = kinds.len() == 1
            && matches!(kinds[0], CatalogKind::Movie | CatalogKind::Series)
            && query.watched.is_none()
            && query.favorite.is_none()
            && query.genre.is_none()
            && query.studio.is_none()
            && query.tag.is_none()
            && query.official_rating.is_none()
            && query.release_decade.is_none()
            && query.series_status.is_none();
        let plan = QueryPlan {
            user_data: query.watched.is_some() || query.favorite.is_some(),
            series_aggregates: matches!(query.watched, Some(WatchFilter::InProgress)),
            drive_by_library: !unfiltered_single_top_kind,
            letter_cover: unfiltered_single_top_kind,
        };
        let ItemSql {
            from_clause,
            where_clause,
            arguments,
            ..
        } = query_base(query, plan);
        let sql = format!(
            "SELECT upper(substr(COALESCE(i.sort_name, i.name), 1, 1)) AS initial, count(*)
             {from_clause}{where_clause}
             GROUP BY initial"
        );
        let mut counts = self.db.with_connection(|connection| {
            let mut statement = connection.prepare_cached(&sql)?;
            statement
                .query_map(params_from_iter(arguments.iter()), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
        })?;
        let descending = matches!(
            query
                .direction
                .unwrap_or_else(|| query.sort.natural_direction()),
            super::SortDirection::Descending
        );
        counts.sort_by(|(left, _), (right, _)| {
            let order = left.cmp(right);
            if descending { order.reverse() } else { order }
        });
        let mut letters: std::collections::BTreeMap<String, i64> =
            std::collections::BTreeMap::new();
        let mut offset = 0_i64;
        for (initial, count) in counts {
            let bucket = match initial.as_bytes() {
                [ch @ b'A'..=b'Z'] => String::from(*ch as char),
                _ => "#".to_string(),
            };
            letters.entry(bucket).or_insert(offset);
            offset += count;
        }
        Ok(letters.into_iter().collect())
    }

    pub fn query_page(&self, query: &ItemQuery) -> rusqlite::Result<Vec<ItemSummary>> {
        self.db
            .with_connection(|connection| page_items(connection, query))
    }

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
                 LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
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
                     LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
                     WHERE i.jellyfin_id IN ({placeholders})"
                );
                let mut statement = connection.prepare(&sql)?;
                let rows = statement.query_map(params_from_iter(chunk.iter()), summary_row)?;
                items.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
            }
            Ok(items)
        })
    }

    pub fn children(&self, parent_id: &str) -> rusqlite::Result<Vec<ItemSummary>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM items i
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
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
                 LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
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
            let (min_id, max_id): (Option<i64>, Option<i64>) = connection.query_row(
                "SELECT
                     (SELECT id FROM items WHERE kind = 'Movie' ORDER BY id ASC LIMIT 1),
                     (SELECT id FROM items WHERE kind = 'Movie' ORDER BY id DESC LIMIT 1)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let (Some(min_id), Some(max_id)) = (min_id, max_id) else {
                return Ok(None);
            };
            let mut seed = [0u8; 8];
            let _ = getrandom::fill(&mut seed);
            let span = (max_id - min_id + 1).max(1).cast_unsigned();
            let cursor = min_id + (u64::from_le_bytes(seed) % span).cast_signed();
            let mut upper = connection.prepare_cached(&format!(
                "SELECT {DETAIL_COLUMNS} FROM items i
                 CROSS JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
                 WHERE i.kind = 'Movie' AND i.id >= ?1 AND u.played = 1
                   AND EXISTS (SELECT 1 FROM item_genres g WHERE g.item_id = i.id)
                 ORDER BY i.id ASC
                 LIMIT 1"
            ))?;
            if let Some(row) = upper.query(params![cursor])?.next()? {
                return Ok(Some(detail_row(row)?));
            }
            let mut lower = connection.prepare_cached(&format!(
                "SELECT {DETAIL_COLUMNS} FROM items i
                 CROSS JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
                 WHERE i.kind = 'Movie' AND i.id < ?1 AND u.played = 1
                   AND EXISTS (SELECT 1 FROM item_genres g WHERE g.item_id = i.id)
                 ORDER BY i.id ASC
                 LIMIT 1"
            ))?;
            match lower.query(params![cursor])?.next()? {
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
                 LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
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
        let limit = limit.clamp(1, 50);
        self.db.with_connection(|connection| {
            let (m_min, m_max, s_min, s_max): (
                Option<i64>,
                Option<i64>,
                Option<i64>,
                Option<i64>,
            ) = connection.query_row(
                "SELECT
                     (SELECT id FROM items WHERE kind = 'Movie' AND backdrop_image_tag IS NOT NULL AND backdrop_image_tag <> '' ORDER BY id ASC LIMIT 1),
                     (SELECT id FROM items WHERE kind = 'Movie' AND backdrop_image_tag IS NOT NULL AND backdrop_image_tag <> '' ORDER BY id DESC LIMIT 1),
                     (SELECT id FROM items WHERE kind = 'Series' AND backdrop_image_tag IS NOT NULL AND backdrop_image_tag <> '' ORDER BY id ASC LIMIT 1),
                     (SELECT id FROM items WHERE kind = 'Series' AND backdrop_image_tag IS NOT NULL AND backdrop_image_tag <> '' ORDER BY id DESC LIMIT 1)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            let min_id = [m_min, s_min].into_iter().flatten().min();
            let max_id = [m_max, s_max].into_iter().flatten().max();
            let mut items = Vec::new();
            if let (Some(min_id), Some(max_id)) = (min_id, max_id) {
                let mut seed = [0u8; 8];
                let _ = getrandom::fill(&mut seed);
                let span = (max_id - min_id + 1).max(1).cast_unsigned();
                let start_id = min_id + (u64::from_le_bytes(seed) % span).cast_signed();
                let mut upper = connection.prepare_cached(&format!(
                    "SELECT {SUMMARY_COLUMNS} FROM items i
                     LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                     LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
                     WHERE i.kind IN ('Movie', 'Series')
                       AND i.backdrop_image_tag IS NOT NULL AND i.backdrop_image_tag <> ''
                       AND i.id >= ?1
                     ORDER BY i.id ASC
                     LIMIT ?2"
                ))?;
                items.extend(
                    upper
                        .query_map(params![start_id, limit], summary_row)?
                        .collect::<rusqlite::Result<Vec<_>>>()?,
                );
                let remaining = limit - i64::try_from(items.len()).unwrap_or(limit);
                if remaining > 0 {
                    let mut lower = connection.prepare_cached(&format!(
                        "SELECT {SUMMARY_COLUMNS} FROM items i
                         LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                         LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
                         WHERE i.kind IN ('Movie', 'Series')
                           AND i.backdrop_image_tag IS NOT NULL AND i.backdrop_image_tag <> ''
                           AND i.id < ?1
                         ORDER BY i.id ASC
                         LIMIT ?2"
                    ))?;
                    items.extend(
                        lower
                            .query_map(params![start_id, remaining], summary_row)?
                            .collect::<rusqlite::Result<Vec<_>>>()?,
                    );
                }
            }
            if i64::try_from(items.len()).unwrap_or(limit) < limit {
                let remaining = limit - i64::try_from(items.len()).unwrap_or(limit);
                let mut seen: std::collections::HashSet<String> =
                    items.iter().map(|entry| entry.id.clone()).collect();
                let mut thumb_stmt = connection.prepare_cached(&format!(
                    "SELECT {SUMMARY_COLUMNS} FROM items i
                     LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                     LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
                     WHERE i.kind IN ('Movie', 'Series')
                       AND COALESCE(i.backdrop_image_tag, '') = ''
                       AND EXISTS (
                           SELECT 1 FROM json_each(i.image_tags) image
                           WHERE lower(image.key) = 'thumb'
                             AND COALESCE(image.value, '') <> ''
                       )
                     LIMIT ?1"
                ))?;
                for row in thumb_stmt.query_map(params![remaining], summary_row)? {
                    let item = row?;
                    if seen.insert(item.id.clone()) {
                        items.push(item);
                    }
                }
            }
            Ok(items)
        })
    }

    pub fn facet_values(
        &self,
        facet: Facet,
        library: Option<&LibraryId>,
    ) -> rusqlite::Result<Vec<String>> {
        let (table, column) = facet.table();
        let sql = match library {
            None => format!(
                "WITH RECURSIVE distinct_values(value) AS (
                     SELECT min({column}) FROM {table}
                     UNION ALL
                     SELECT (SELECT min({column}) FROM {table} WHERE {column} > previous.value)
                     FROM distinct_values previous WHERE previous.value IS NOT NULL
                 )
                 SELECT value FROM distinct_values WHERE value IS NOT NULL
                 ORDER BY value COLLATE NOCASE ASC"
            ),
            Some(_) => format!(
                "WITH RECURSIVE distinct_values(value) AS (
                     SELECT min({column}) FROM {table}
                     UNION ALL
                     SELECT (SELECT min({column}) FROM {table} WHERE {column} > previous.value)
                     FROM distinct_values previous WHERE previous.value IS NOT NULL
                 )
                 SELECT value FROM distinct_values
                 WHERE value IS NOT NULL
                   AND EXISTS (
                       SELECT 1 FROM {table} f
                       JOIN items i ON i.id = f.item_id
                       JOIN item_libraries l ON l.item_id = i.jellyfin_id
                       WHERE f.{column} = distinct_values.value AND l.library_id = ?1
                   )
                 ORDER BY value COLLATE NOCASE ASC"
            ),
        };
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare_cached(&sql)?;
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
            let kind_list = kinds
                .iter()
                .map(|kind| format!("'{}'", kind.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            let (from_clause, lib_filter) = if library.is_some() {
                (
                    "FROM item_libraries l JOIN items i ON i.jellyfin_id = l.item_id",
                    " AND l.library_id = ?1",
                )
            } else {
                ("FROM items i", "")
            };
            let mut statement = connection.prepare_cached(&format!(
                "SELECT DISTINCT i.official_rating {from_clause}
                 WHERE i.kind IN ({kind_list})
                   AND i.official_rating IS NOT NULL
                   AND i.official_rating <> ''{lib_filter}"
            ))?;
            let rows = statement
                .query_map(params_from_iter(library.map(LibraryId::as_str)), |row| {
                    row.get::<_, String>(0)
                })?
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
                 LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
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
                 LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
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
                 LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
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

macro_rules! summary_columns {
    () => {
        "i.jellyfin_id, i.kind, i.name, i.year, i.runtime_ticks, \
i.community_rating, i.official_rating, i.series_id, i.series_name, i.index_number, \
i.parent_index_number, i.primary_image_tag, i.child_count, i.premiere_date, i.season_id, \
COALESCE(u.played, 0), COALESCE(u.play_count, 0), COALESCE(u.playback_position_ticks, 0), \
COALESCE(u.is_favorite, 0), i.image_tags, i.backdrop_image_tag, \
CASE WHEN i.kind = 'Series' THEN COALESCE(sa.unplayed_present_count, 0) END"
    };
}

pub(super) const SUMMARY_COLUMNS: &str = summary_columns!();

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
    summary_row_at(row, 0)
}

pub(super) fn summary_row_at(row: &Row<'_>, offset: usize) -> rusqlite::Result<ItemSummary> {
    let image_tags = parsed_json(&row.get::<_, String>(offset + 19)?);
    Ok(ItemSummary {
        id: row.get(offset)?,
        kind: row.get(offset + 1)?,
        name: row.get(offset + 2)?,
        year: row.get(offset + 3)?,
        runtime_ticks: row.get(offset + 4)?,
        community_rating: row.get(offset + 5)?,
        official_rating: row.get(offset + 6)?,
        series_id: row.get(offset + 7)?,
        series_name: row.get(offset + 8)?,
        index_number: row.get(offset + 9)?,
        parent_index_number: row.get(offset + 10)?,
        primary_image_tag: row.get(offset + 11)?,
        child_count: row.get(offset + 12)?,
        premiere_date: row.get(offset + 13)?,
        season_id: row.get(offset + 14)?,
        played: row.get::<_, i64>(offset + 15)? != 0,
        play_count: row.get(offset + 16)?,
        position_ticks: row.get(offset + 17)?,
        favorite: row.get::<_, i64>(offset + 18)? != 0,
        thumb_image_tag: cached_image_tag(&image_tags, "Thumb").map(str::to_string),
        logo_image_tag: cached_image_tag(&image_tags, "Logo").map(str::to_string),
        backdrop_image_tag: row.get(offset + 20)?,
        unplayed_count: row.get(offset + 21)?,
        primary_aspect: None,
        overview: None,
        playlist_index: None,
    })
}

fn genres_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str(&row.get::<_, String>(index)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn detail_row(row: &Row<'_>) -> rusqlite::Result<ItemDetail> {
    Ok(ItemDetail {
        summary: summary_row(row)?,
        genres: genres_column(row, 22)?,
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

pub(super) fn page_query_plan(query: &ItemQuery) -> QueryPlan {
    QueryPlan {
        user_data: query.watched.is_some()
            || query.favorite.is_some()
            || matches!(query.sort, ItemSort::DatePlayed | ItemSort::PlayCount),
        series_aggregates: matches!(query.watched, Some(WatchFilter::InProgress))
            || matches!(query.sort, ItemSort::EpisodeAdded | ItemSort::DatePlayed),
        drive_by_library: !matches!(
            query.sort,
            ItemSort::Name
                | ItemSort::DateAdded
                | ItemSort::ReleaseDate
                | ItemSort::CommunityRating
                | ItemSort::Runtime
        ) || query.genre.is_some()
            || query.studio.is_some()
            || query.tag.is_some()
            || query.official_rating.is_some()
            || query.watched.is_some()
            || query.favorite.is_some()
            || query.release_decade.is_some()
            || query.series_status.is_some(),
        letter_cover: false,
    }
}

fn count_items(connection: &Connection, query: &ItemQuery) -> rusqlite::Result<i64> {
    let plan = QueryPlan {
        user_data: query.watched.is_some() || query.favorite.is_some(),
        series_aggregates: matches!(query.watched, Some(WatchFilter::InProgress)),
        drive_by_library: true,
        letter_cover: false,
    };
    let ItemSql {
        from_clause,
        where_clause,
        arguments,
        ..
    } = query_base(query, plan);
    connection
        .prepare_cached(&format!("SELECT count(*) {from_clause}{where_clause}"))?
        .query_row(params_from_iter(arguments.iter()), |row| row.get(0))
}

fn page_items(connection: &Connection, query: &ItemQuery) -> rusqlite::Result<Vec<ItemSummary>> {
    if let Some(items) = fast::try_play_count_desc_fast(connection, query)? {
        return Ok(items);
    }
    let (sql, arguments) = page_sql(query);
    let mut statement = connection.prepare_cached(&sql)?;
    statement
        .query_map(params_from_iter(arguments.iter()), summary_row)?
        .collect()
}

pub(super) fn paged_query_base(query: &ItemQuery, limit: i64, offset: i64) -> (QueryPlan, ItemSql) {
    let plan = page_query_plan(query);
    let mut base = query_base(query, plan);
    base.arguments.push(SqlValue::Integer(limit));
    base.arguments.push(SqlValue::Integer(offset));
    (plan, base)
}

fn page_sql(query: &ItemQuery) -> (String, Vec<SqlValue>) {
    let limit = query.limit.clamp(1, 500);
    let offset = query.offset.max(0);
    if let Some(search) = query.search.as_deref().and_then(fts_match_expression)
        && fast::is_unfiltered_top_level_all(query)
    {
        return (
            format!(
                "WITH page AS (
                     SELECT rowid AS page_id, bm25(catalog_search_fts) AS score
                     FROM catalog_search_fts
                     WHERE catalog_search_fts MATCH ?1
                     ORDER BY score ASC, page_id ASC
                     LIMIT ?2 OFFSET ?3
                 )
                 SELECT {SUMMARY_COLUMNS}
                 FROM page
                 JOIN items i ON i.id = page.page_id
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
                 ORDER BY page.score ASC, page.page_id ASC"
            ),
            vec![
                SqlValue::Text(search),
                SqlValue::Integer(limit),
                SqlValue::Integer(offset),
            ],
        );
    }
    let (
        _,
        ItemSql {
            from_clause,
            where_clause,
            order,
            has_search,
            arguments,
        },
    ) = paged_query_base(query, limit, offset);
    if has_search {
        let fts = fast::fts_table_for(query);
        return (
            format!(
                "WITH page AS (
                     SELECT i.id AS page_id, bm25({fts}) AS score
                     {from_clause}{where_clause}
                     ORDER BY score ASC, page_id ASC
                     LIMIT ? OFFSET ?
                 )
                 SELECT {SUMMARY_COLUMNS}
                 FROM page
                 JOIN items i ON i.id = page.page_id
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
                 ORDER BY page.score ASC, page.page_id ASC"
            ),
            arguments,
        );
    }
    (
        format!(
            "WITH page AS (
                 SELECT i.id AS page_id
                 {from_clause}{where_clause}
                 ORDER BY {order}
                 LIMIT ? OFFSET ?
             )
             SELECT {SUMMARY_COLUMNS}
             FROM page
             JOIN items i ON i.id = page.page_id
             LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
             LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
             ORDER BY {order}"
        ),
        arguments,
    )
}

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

#[cfg(test)]
mod scale_tests;
#[cfg(test)]
mod tests;
