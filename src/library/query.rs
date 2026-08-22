use rusqlite::types::Value as SqlValue;
use rusqlite::{Row, params, params_from_iter};
use serde_json::{Value, json};

use super::{ItemPage, ItemQuery, Library, LibraryStats, TmdbCandidate};

/// Bound on one `tmdb_id IN (...)` clause so even a full person filmography
/// stays well under SQLite's host-parameter limit.
const TMDB_CANDIDATE_CHUNK: usize = 400;

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

    /// The episode that follows `item_id` inside its series.
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

    use super::{ItemQuery, Library, cached_image_tag, civil_from_days, fts_match_expression};
    use crate::library::test_support::{dto, seeded};
    use crate::library::{ItemSort, TmdbCandidate, current_release_decade, release_decade_from_id};

    #[test]
    fn cached_image_tags_are_found_whatever_the_server_capitalised() {
        let tags = json!({ "primary": "p", "THUMB": "t", "Logo": "l" });
        assert_eq!(cached_image_tag(&tags, "Primary"), Some("p"));
        assert_eq!(cached_image_tag(&tags, "Thumb"), Some("t"));
        assert_eq!(cached_image_tag(&tags, "Logo"), Some("l"));
        assert_eq!(cached_image_tag(&tags, "Banner"), None);
        assert_eq!(cached_image_tag(&json!(null), "Logo"), None);
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
        assert!(library.tmdb_candidates(&[]).expect("empty").is_empty());
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
