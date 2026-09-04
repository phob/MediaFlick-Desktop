use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::jellyfin::api::model::BaseItemDto;

use super::model::is_synced_kind;
use super::{ItemRecord, Library, LibraryChangeBatch, UserDataRecord, normalize_ids, now_unix};

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
}

impl Library {
    /// Ingests server DTOs into the thin index (plus their user data) in one
    /// transaction, reporting which rows and contexts actually moved.
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
            normalize_ids(&mut changes.item_ids);
            normalize_ids(&mut changes.context_ids);
            Ok(changes)
        })
    }

    /// Upserts a page of items (and their user data) in one transaction.
    pub fn upsert_page(&self, items: &[BaseItemDto]) -> rusqlite::Result<usize> {
        self.ingest_page(items)
            .map(|changes| changes.item_ids.len())
    }

    /// Applies pushed watch-state changes for items already in the catalog.
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
            normalize_ids(&mut changes.item_ids);
            normalize_ids(&mut changes.context_ids);
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

    /// Atomically replaces one live container's direct child snapshot.
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
            normalize_ids(&mut changes.item_ids);
            normalize_ids(&mut changes.context_ids);
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
            normalize_ids(&mut changes.item_ids);
            normalize_ids(&mut changes.context_ids);
            Ok(changes)
        })
    }

    /// Maps each requested card id to the item whose media streams answer its
    /// technical badge.
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

    /// The item type of a cached item.
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

    pub fn ids_by_tmdb(
        &self,
        kind: &str,
        tmdb_ids: &[String],
    ) -> rusqlite::Result<HashMap<String, String>> {
        self.ids_by_provider(kind, "tmdb_id", tmdb_ids)
    }

    pub fn ids_by_tvdb(
        &self,
        kind: &str,
        tvdb_ids: &[String],
    ) -> rusqlite::Result<HashMap<String, String>> {
        self.ids_by_provider(kind, "tvdb_id", tvdb_ids)
    }

    /// Watched flags for library items matched by provider id. Absent ids are
    /// unowned; the join reads their absence as unwatched.
    pub fn played_by_tmdb(
        &self,
        kind: &str,
        tmdb_ids: &[String],
    ) -> rusqlite::Result<HashMap<String, bool>> {
        if tmdb_ids.is_empty() {
            return Ok(HashMap::new());
        }
        self.db.with_connection(|connection| {
            let placeholders = vec!["?"; tmdb_ids.len()].join(", ");
            let mut statement = connection.prepare(&format!(
                "SELECT i.tmdb_id, COALESCE(u.played, 0) FROM items i
                 LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
                 WHERE i.kind = ?1 AND i.tmdb_id IN ({placeholders})"
            ))?;
            let arguments = std::iter::once(kind)
                .chain(tmdb_ids.iter().map(String::as_str))
                .collect::<Vec<_>>();
            let rows = statement
                .query_map(params_from_iter(arguments), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0))
                })?
                .collect::<rusqlite::Result<HashMap<_, _>>>()?;
            Ok(rows)
        })
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
    pub fn forget(&self, item_id: &str) -> rusqlite::Result<LibraryChangeBatch> {
        self.db.with_transaction(|transaction| {
            let mut changes = LibraryChangeBatch::default();
            delete_items_from_transaction(transaction, &[item_id.to_string()], &mut changes)?;
            normalize_ids(&mut changes.item_ids);
            normalize_ids(&mut changes.context_ids);
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

fn upsert_item(connection: &Connection, record: &ItemRecord) -> rusqlite::Result<bool> {
    let written = connection
        .prepare_cached(
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
        )?
        .execute(params![
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
        ])?;
    Ok(written > 0)
}

fn upsert_user_data(connection: &Connection, record: &UserDataRecord) -> rusqlite::Result<bool> {
    let written = connection
        .prepare_cached(
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
        )?
        .execute(params![
            record.jellyfin_id,
            record.played,
            record.play_count,
            record.playback_position_ticks,
            record.is_favorite,
            record.played_percentage,
            record.last_played_date,
            now_unix(),
        ])?;
    Ok(written > 0)
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::Library;
    use crate::library::test_support::{dto, seeded};

    #[test]
    fn played_flags_follow_tmdb_ids_and_absence_means_unowned() {
        let library = Library::open_in_memory().expect("library");
        let page = [
            dto(
                r#"{"Id":"m1","Name":"The Matrix","Type":"Movie","ProviderIds":{"Tmdb":"603"},
                    "UserData":{"Played":false}}"#,
            ),
            dto(
                r#"{"Id":"m2","Name":"Alien","Type":"Movie","ProviderIds":{"Tmdb":"348"},
                    "UserData":{"Played":true}}"#,
            ),
        ];
        library.ingest_page(&page).expect("ingest");

        assert_eq!(
            library
                .played_by_tmdb(
                    "Movie",
                    &["603".to_string(), "348".to_string(), "42".to_string()]
                )
                .expect("played flags"),
            HashMap::from([("603".to_string(), false), ("348".to_string(), true)])
        );
    }

    #[test]
    fn a_user_data_only_write_updates_the_played_join() {
        let library = Library::open_in_memory().expect("library");
        let page = [dto(
            r#"{"Id":"m1","Name":"The Matrix","Type":"Movie","ProviderIds":{"Tmdb":"603"}}"#,
        )];
        library.ingest_page(&page).expect("ingest");
        library
            .upsert_user_data(&[super::UserDataRecord {
                jellyfin_id: "m1".to_string(),
                played: true,
                ..Default::default()
            }])
            .expect("user data");

        assert_eq!(
            library
                .played_by_tmdb("Movie", &["603".to_string()])
                .expect("played flags"),
            HashMap::from([("603".to_string(), true)])
        );
    }

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

        let watched = [dto(
            r#"{"Id":"m1","Name":"The Matrix","Type":"Movie","ProductionYear":1999,
                "Genres":["Action"],"UserData":{"Played":true}}"#,
        )];
        let third = library.ingest_page(&watched).expect("watched ingest");
        assert_eq!(third.item_ids, vec!["m1".to_string()]);

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
                ("m1".to_string(), "m1".to_string()),
                ("s1".to_string(), "e1".to_string()),
                ("season1".to_string(), "e1".to_string()),
                ("deep-link".to_string(), "deep-link".to_string()),
            ]
        );
    }

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
    fn upserts_are_idempotent() {
        let library = seeded();
        let before = library.stats();
        library
            .upsert_page(&[dto(r#"{"Id":"m1","Name":"The Matrix","Type":"Movie"}"#)])
            .expect("re-upsert");
        assert_eq!(library.stats(), before);
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
        assert!(library.item("m1").expect("query").is_some());
        assert!(
            library
                .reconcile_children("season1", &live)
                .expect("reconcile")
                .is_empty()
        );
    }

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
}
