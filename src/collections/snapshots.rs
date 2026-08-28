use std::collections::{HashMap, HashSet};
use std::num::TryFromIntError;

use rusqlite::types::Type;
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::library::Library;
use crate::preferences::AccountKey;

use super::franchises::{FranchiseMembership, FranchiseSnapshot};
use super::{CanonicalIdentity, CollectionSnapshot, MediaType, NormalizedTitle, valid_opaque_id};

// Profile IDs are hexadecimal, so this non-hex key cannot collide with a
// configured profile while sharing the same account-scoped refresh table.
const FRANCHISE_REFRESH_KEY: &str = "movie-franchises";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshState {
    pub last_attempt: Option<i64>,
    pub last_success: Option<i64>,
    pub latest_failure: Option<String>,
    pub next_due: Option<i64>,
    pub initialized: bool,
}

pub struct SnapshotRepository<'a> {
    library: &'a Library,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct FranchiseResolutionPlan {
    pub stale_movie_ids: Vec<u64>,
    pub known_collection_ids: Vec<u64>,
}

impl<'a> SnapshotRepository<'a> {
    pub fn new(library: &'a Library) -> Self {
        Self { library }
    }

    /// Replaces one complete profile revision in one transaction. Provider
    /// rows marked adult never enter the cache, even if the gateway regresses.
    pub fn commit_profile(
        &self,
        account: &AccountKey,
        snapshot: &CollectionSnapshot,
    ) -> rusqlite::Result<()> {
        validate_snapshot(snapshot)?;
        let items = normalized_items(&snapshot.items);
        self.library.with_transaction(|transaction| {
            transaction.execute(
                "DELETE FROM collection_snapshots
                 WHERE server_id = ?1 AND user_id = ?2
                   AND profile_id = ?3 AND revision = ?4",
                params![
                    account.server_id(),
                    account.user_id(),
                    snapshot.profile_id,
                    snapshot.revision
                ],
            )?;
            transaction.execute(
                "INSERT INTO collection_snapshots (
                    server_id, user_id, profile_id, revision, committed_at, item_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    account.server_id(),
                    account.user_id(),
                    snapshot.profile_id,
                    snapshot.revision,
                    snapshot.committed_at,
                    items.len() as i64
                ],
            )?;
            for item in &items {
                insert_profile_item(transaction, account, snapshot, item)?;
            }
            Ok(())
        })
    }

    pub fn profile(
        &self,
        account: &AccountKey,
        profile_id: &str,
        revision: &str,
    ) -> rusqlite::Result<Option<CollectionSnapshot>> {
        self.library.with_connection(|connection| {
            let committed_at = connection
                .query_row(
                    "SELECT committed_at FROM collection_snapshots
                     WHERE server_id = ?1 AND user_id = ?2
                       AND profile_id = ?3 AND revision = ?4",
                    params![account.server_id(), account.user_id(), profile_id, revision],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(committed_at) = committed_at else {
                return Ok(None);
            };
            let mut statement = connection.prepare(
                "SELECT media_type, tmdb_id, title, original_title, year, overview,
                        release_date, source_order, poster_path, backdrop_path, adult
                 FROM collection_snapshot_items
                 WHERE server_id = ?1 AND user_id = ?2
                   AND profile_id = ?3 AND revision = ?4
                 ORDER BY source_order ASC",
            )?;
            let items = statement
                .query_map(
                    params![account.server_id(), account.user_id(), profile_id, revision],
                    title_from_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(Some(CollectionSnapshot {
                profile_id: profile_id.to_string(),
                revision: revision.to_string(),
                committed_at,
                items,
            }))
        })
    }

    pub fn has_account_results(&self, account: &AccountKey) -> rusqlite::Result<bool> {
        self.library.with_connection(|connection| {
            connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM collection_snapshots
                    WHERE server_id = ?1 AND user_id = ?2
                    UNION ALL
                    SELECT 1 FROM franchise_snapshots
                    WHERE server_id = ?1 AND user_id = ?2
                 )",
                params![account.server_id(), account.user_id()],
                |row| row.get(0),
            )
        })
    }

    pub fn title(
        &self,
        account: &AccountKey,
        media_type: MediaType,
        tmdb_id: u64,
    ) -> rusqlite::Result<Option<NormalizedTitle>> {
        let tmdb_id = i64_from_id(tmdb_id)?;
        self.library.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT media_type, tmdb_id, title, original_title, year, overview,
                            release_date, source_order, poster_path, backdrop_path, adult
                     FROM (
                         SELECT media_type, tmdb_id, title, original_title, year, overview,
                                release_date, source_order, poster_path, backdrop_path, adult,
                                0 AS source_kind
                         FROM collection_snapshot_items
                         WHERE server_id = ?1 AND user_id = ?2
                           AND media_type = ?3 AND tmdb_id = ?4
                         UNION ALL
                         SELECT media_type, tmdb_id, title, original_title, year, overview,
                                release_date, source_order, poster_path, backdrop_path, adult,
                                1 AS source_kind
                         FROM franchise_snapshot_items
                         WHERE server_id = ?1 AND user_id = ?2
                           AND media_type = ?3 AND tmdb_id = ?4
                     )
                     ORDER BY source_kind, source_order
                     LIMIT 1",
                    params![
                        account.server_id(),
                        account.user_id(),
                        media_type.identity_name(),
                        tmdb_id
                    ],
                    title_from_row,
                )
                .optional()
        })
    }

    pub fn remove_profile(&self, account: &AccountKey, profile_id: &str) -> rusqlite::Result<()> {
        self.library.with_transaction(|transaction| {
            transaction.execute(
                "DELETE FROM collection_snapshots
                 WHERE server_id = ?1 AND user_id = ?2 AND profile_id = ?3",
                params![account.server_id(), account.user_id(), profile_id],
            )?;
            transaction.execute(
                "DELETE FROM collection_refresh_state
                 WHERE server_id = ?1 AND user_id = ?2 AND profile_id = ?3",
                params![account.server_id(), account.user_id(), profile_id],
            )?;
            Ok(())
        })
    }

    pub fn remove_revision(
        &self,
        account: &AccountKey,
        profile_id: &str,
        revision: &str,
    ) -> rusqlite::Result<()> {
        self.library.with_connection(|connection| {
            connection.execute(
                "DELETE FROM collection_snapshots
                 WHERE server_id = ?1 AND user_id = ?2
                   AND profile_id = ?3 AND revision = ?4",
                params![account.server_id(), account.user_id(), profile_id, revision],
            )?;
            Ok(())
        })
    }

    pub fn remove_unreferenced_revisions(
        &self,
        account: &AccountKey,
        active: &HashMap<String, String>,
    ) -> rusqlite::Result<usize> {
        self.library.with_transaction(|transaction| {
            let mut statement = transaction.prepare(
                "SELECT profile_id, revision FROM collection_snapshots
                 WHERE server_id = ?1 AND user_id = ?2",
            )?;
            let rows = statement
                .query_map(params![account.server_id(), account.user_id()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(statement);
            let mut removed = 0;
            for (profile_id, revision) in rows {
                if active.get(&profile_id) == Some(&revision) {
                    continue;
                }
                removed += transaction.execute(
                    "DELETE FROM collection_snapshots
                     WHERE server_id = ?1 AND user_id = ?2
                       AND profile_id = ?3 AND revision = ?4",
                    params![account.server_id(), account.user_id(), profile_id, revision],
                )?;
            }
            Ok(removed)
        })
    }

    pub fn refresh_state(
        &self,
        account: &AccountKey,
        profile_id: &str,
    ) -> rusqlite::Result<RefreshState> {
        self.library.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT last_attempt, last_success, latest_failure, next_due, initialized
                     FROM collection_refresh_state
                     WHERE server_id = ?1 AND user_id = ?2 AND profile_id = ?3",
                    params![account.server_id(), account.user_id(), profile_id],
                    |row| {
                        Ok(RefreshState {
                            last_attempt: row.get(0)?,
                            last_success: row.get(1)?,
                            latest_failure: row.get(2)?,
                            next_due: row.get(3)?,
                            initialized: row.get(4)?,
                        })
                    },
                )
                .optional()
                .map(Option::unwrap_or_default)
        })
    }

    pub fn save_refresh_state(
        &self,
        account: &AccountKey,
        profile_id: &str,
        state: &RefreshState,
    ) -> rusqlite::Result<()> {
        self.library.with_connection(|connection| {
            upsert_refresh_state(connection, account, profile_id, state)
        })
    }

    pub fn franchise_refresh_state(&self, account: &AccountKey) -> rusqlite::Result<RefreshState> {
        self.refresh_state(account, FRANCHISE_REFRESH_KEY)
    }

    pub fn save_franchise_refresh_failure(
        &self,
        account: &AccountKey,
        attempted_at: i64,
    ) -> rusqlite::Result<()> {
        let mut state = self.franchise_refresh_state(account)?;
        state.last_attempt = Some(attempted_at);
        state.latest_failure = Some("Results unavailable".to_string());
        self.save_refresh_state(account, FRANCHISE_REFRESH_KEY, &state)
    }

    pub fn franchise_resolution_plan(
        &self,
        account: &AccountKey,
        owned_tmdb_ids: &[u64],
        stale_before: i64,
    ) -> rusqlite::Result<FranchiseResolutionPlan> {
        self.library.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT tmdb_id, collection_id, resolved_at
                 FROM franchise_movie_membership
                 WHERE server_id = ?1 AND user_id = ?2",
            )?;
            let mapped =
                statement.query_map(params![account.server_id(), account.user_id()], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })?;
            let mut rows = HashMap::new();
            for row in mapped {
                let (tmdb_id, collection_id, resolved_at) = row?;
                rows.insert(tmdb_id, (collection_id, resolved_at));
            }
            let mut plan = FranchiseResolutionPlan::default();
            let mut known = HashSet::new();
            for tmdb_id in owned_tmdb_ids {
                let sql_id = i64_from_id(*tmdb_id)?;
                match rows.get(&sql_id) {
                    Some((collection_id, resolved_at)) if *resolved_at > stale_before => {
                        if let Some(collection_id) = collection_id {
                            known.insert(u64_from_sql(*collection_id)?);
                        }
                    }
                    _ => plan.stale_movie_ids.push(*tmdb_id),
                }
            }
            plan.known_collection_ids = known.into_iter().collect();
            plan.known_collection_ids.sort_unstable();
            Ok(plan)
        })
    }

    pub fn commit_franchise_resolution(
        &self,
        account: &AccountKey,
        snapshots: &[FranchiseSnapshot],
        memberships: &[FranchiseMembership],
        refreshed_at: i64,
    ) -> rusqlite::Result<()> {
        self.library.with_transaction(|transaction| {
            for membership in memberships {
                transaction.execute(
                    "INSERT INTO franchise_movie_membership (
                         server_id, user_id, tmdb_id, collection_id, resolved_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(server_id, user_id, tmdb_id) DO UPDATE SET
                         collection_id = excluded.collection_id,
                         resolved_at = excluded.resolved_at",
                    params![
                        account.server_id(),
                        account.user_id(),
                        i64_from_id(membership.tmdb_id)?,
                        membership.collection_id.map(i64_from_id).transpose()?,
                        refreshed_at,
                    ],
                )?;
            }
            transaction.execute(
                "DELETE FROM franchise_snapshots WHERE server_id = ?1 AND user_id = ?2",
                params![account.server_id(), account.user_id()],
            )?;
            for snapshot in snapshots {
                insert_franchise(transaction, account, snapshot)?;
            }
            upsert_refresh_state(
                transaction,
                account,
                FRANCHISE_REFRESH_KEY,
                &RefreshState {
                    last_attempt: Some(refreshed_at),
                    last_success: Some(refreshed_at),
                    latest_failure: None,
                    next_due: None,
                    initialized: true,
                },
            )?;
            Ok(())
        })
    }

    pub fn franchises(&self, account: &AccountKey) -> rusqlite::Result<Vec<FranchiseSnapshot>> {
        self.library.with_connection(|connection| {
            let mut headers = connection.prepare(
                "SELECT tmdb_collection_id, name, poster_path, backdrop_path, committed_at
                 FROM franchise_snapshots
                 WHERE server_id = ?1 AND user_id = ?2 ORDER BY name COLLATE NOCASE",
            )?;
            let rows = headers
                .query_map(params![account.server_id(), account.user_id()], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(headers);

            let mut statement = connection.prepare(
                "SELECT tmdb_collection_id, media_type, tmdb_id, title, original_title,
                        year, overview, release_date, source_order, poster_path,
                        backdrop_path, adult
                 FROM franchise_snapshot_items
                 WHERE server_id = ?1 AND user_id = ?2
                 ORDER BY tmdb_collection_id, source_order ASC",
            )?;
            let mut items_by_collection = HashMap::<i64, Vec<NormalizedTitle>>::new();
            let items = statement
                .query_map(params![account.server_id(), account.user_id()], |row| {
                    Ok((row.get::<_, i64>(0)?, title_from_row_at(row, 1)?))
                })?;
            for item in items {
                let (collection_id, item) = item?;
                items_by_collection
                    .entry(collection_id)
                    .or_default()
                    .push(item);
            }
            drop(statement);

            rows.into_iter()
                .map(|(id, name, poster_path, backdrop_path, committed_at)| {
                    Ok(FranchiseSnapshot {
                        collection_id: u64_from_sql(id)?,
                        name,
                        poster_path,
                        backdrop_path,
                        committed_at,
                        items: items_by_collection.remove(&id).unwrap_or_default(),
                    })
                })
                .collect()
        })
    }

    pub fn franchise(
        &self,
        account: &AccountKey,
        collection_id: u64,
    ) -> rusqlite::Result<Option<FranchiseSnapshot>> {
        let collection_id = i64_from_id(collection_id)?;
        self.library.with_connection(|connection| {
            let header = connection
                .query_row(
                    "SELECT name, poster_path, backdrop_path, committed_at
                     FROM franchise_snapshots
                     WHERE server_id = ?1 AND user_id = ?2 AND tmdb_collection_id = ?3",
                    params![account.server_id(), account.user_id(), collection_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )
                .optional()?;
            let Some((name, poster_path, backdrop_path, committed_at)) = header else {
                return Ok(None);
            };
            let mut statement = connection.prepare(
                "SELECT media_type, tmdb_id, title, original_title, year, overview,
                        release_date, source_order, poster_path, backdrop_path, adult
                 FROM franchise_snapshot_items
                 WHERE server_id = ?1 AND user_id = ?2 AND tmdb_collection_id = ?3
                 ORDER BY source_order ASC",
            )?;
            let items = statement
                .query_map(
                    params![account.server_id(), account.user_id(), collection_id],
                    title_from_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(Some(FranchiseSnapshot {
                collection_id: u64_from_sql(collection_id)?,
                name,
                poster_path,
                backdrop_path,
                committed_at,
                items,
            }))
        })
    }

    pub fn remove_account(&self, account: &AccountKey) -> rusqlite::Result<()> {
        self.library.with_transaction(|transaction| {
            for table in [
                "collection_snapshots",
                "collection_refresh_state",
                "franchise_snapshots",
                "franchise_movie_membership",
            ] {
                transaction.execute(
                    &format!("DELETE FROM {table} WHERE server_id = ?1 AND user_id = ?2"),
                    params![account.server_id(), account.user_id()],
                )?;
            }
            Ok(())
        })
    }
}

fn validate_snapshot(snapshot: &CollectionSnapshot) -> rusqlite::Result<()> {
    if !valid_opaque_id(&snapshot.profile_id) || !valid_opaque_id(&snapshot.revision) {
        return Err(rusqlite::Error::InvalidParameterName(
            "invalid collection profile or revision id".to_string(),
        ));
    }
    Ok(())
}

fn upsert_refresh_state(
    connection: &rusqlite::Connection,
    account: &AccountKey,
    profile_id: &str,
    state: &RefreshState,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO collection_refresh_state (
            server_id, user_id, profile_id, last_attempt, last_success,
            latest_failure, next_due, initialized
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(server_id, user_id, profile_id) DO UPDATE SET
            last_attempt = excluded.last_attempt,
            last_success = excluded.last_success,
            latest_failure = excluded.latest_failure,
            next_due = excluded.next_due,
            initialized = excluded.initialized",
        params![
            account.server_id(),
            account.user_id(),
            profile_id,
            state.last_attempt,
            state.last_success,
            state.latest_failure,
            state.next_due,
            state.initialized
        ],
    )?;
    Ok(())
}

fn normalized_items(items: &[NormalizedTitle]) -> Vec<NormalizedTitle> {
    let mut identities = HashSet::new();
    let mut items = items
        .iter()
        .filter(|item| !item.adult && identities.insert(item.identity.clone()))
        .cloned()
        .collect::<Vec<_>>();
    items.sort_by_key(|item| item.source_order);
    items
}

fn insert_profile_item(
    transaction: &rusqlite::Transaction<'_>,
    account: &AccountKey,
    snapshot: &CollectionSnapshot,
    item: &NormalizedTitle,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO collection_snapshot_items (
            server_id, user_id, profile_id, revision, media_type, tmdb_id, title,
            original_title, year, overview, release_date, source_order, poster_path,
            backdrop_path, adult
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 0)",
        params![
            account.server_id(),
            account.user_id(),
            snapshot.profile_id,
            snapshot.revision,
            item.identity.media_type.identity_name(),
            i64_from_id(item.identity.tmdb_id)?,
            item.title,
            item.original_title,
            item.year,
            item.overview,
            item.release_date,
            item.source_order,
            item.poster_path,
            item.backdrop_path
        ],
    )?;
    Ok(())
}

fn insert_franchise(
    transaction: &rusqlite::Transaction<'_>,
    account: &AccountKey,
    snapshot: &FranchiseSnapshot,
) -> rusqlite::Result<()> {
    let collection_id = i64_from_id(snapshot.collection_id)?;
    transaction.execute(
        "INSERT INTO franchise_snapshots (
            server_id, user_id, tmdb_collection_id, name, poster_path, backdrop_path, committed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            account.server_id(),
            account.user_id(),
            collection_id,
            snapshot.name,
            snapshot.poster_path,
            snapshot.backdrop_path,
            snapshot.committed_at
        ],
    )?;
    let profile = CollectionSnapshot {
        profile_id: "0".repeat(16),
        revision: "0".repeat(16),
        committed_at: snapshot.committed_at,
        items: Vec::new(),
    };
    for item in normalized_items(&snapshot.items) {
        transaction.execute(
            "INSERT INTO franchise_snapshot_items (
                server_id, user_id, tmdb_collection_id, media_type, tmdb_id, title,
                original_title, year, overview, release_date, source_order, poster_path,
                backdrop_path, adult
             ) VALUES (?1, ?2, ?3, 'movie', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0)",
            params![
                account.server_id(),
                account.user_id(),
                collection_id,
                i64_from_id(item.identity.tmdb_id)?,
                item.title,
                item.original_title,
                item.year,
                item.overview,
                item.release_date,
                item.source_order,
                item.poster_path,
                item.backdrop_path
            ],
        )?;
    }
    drop(profile);
    Ok(())
}

fn title_from_row(row: &Row<'_>) -> rusqlite::Result<NormalizedTitle> {
    title_from_row_at(row, 0)
}

fn title_from_row_at(row: &Row<'_>, offset: usize) -> rusqlite::Result<NormalizedTitle> {
    let media_type = match row.get::<_, String>(offset)?.as_str() {
        "movie" => MediaType::Movie,
        "series" => MediaType::Series,
        value => return Err(text_conversion_error(value)),
    };
    Ok(NormalizedTitle {
        identity: CanonicalIdentity::new(media_type, u64_from_sql(row.get(offset + 1)?)?)
            .ok_or_else(|| text_conversion_error("mixed"))?,
        title: row.get(offset + 2)?,
        original_title: row.get(offset + 3)?,
        year: row.get(offset + 4)?,
        overview: row.get(offset + 5)?,
        release_date: row.get(offset + 6)?,
        source_order: row.get(offset + 7)?,
        poster_path: row.get(offset + 8)?,
        backdrop_path: row.get(offset + 9)?,
        adult: row.get(offset + 10)?,
    })
}

fn i64_from_id(id: u64) -> rusqlite::Result<i64> {
    i64::try_from(id).map_err(integer_conversion_error)
}

fn u64_from_sql(id: i64) -> rusqlite::Result<u64> {
    u64::try_from(id).map_err(integer_conversion_error)
}

fn integer_conversion_error(error: TryFromIntError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

fn text_conversion_error(value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        Type::Text,
        format!("unsupported media type {value}").into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> AccountKey {
        AccountKey::new("server", "user").expect("account")
    }

    fn title(id: u64, order: u32, adult: bool) -> NormalizedTitle {
        NormalizedTitle {
            identity: CanonicalIdentity::new(MediaType::Movie, id).expect("identity"),
            title: format!("Movie {id}"),
            original_title: None,
            year: Some(2020),
            overview: String::new(),
            release_date: Some("2020-01-01".to_string()),
            source_order: order,
            poster_path: None,
            backdrop_path: None,
            adult,
        }
    }

    #[test]
    fn complete_snapshots_commit_atomically_and_drop_duplicates_and_adult_rows() {
        let library = Library::open_in_memory().expect("library");
        let repository = SnapshotRepository::new(&library);
        let snapshot = CollectionSnapshot {
            profile_id: "a".repeat(16),
            revision: "b".repeat(16),
            committed_at: 10,
            items: vec![
                title(2, 2, false),
                title(1, 1, false),
                title(1, 3, false),
                title(3, 4, true),
            ],
        };
        repository
            .commit_profile(&account(), &snapshot)
            .expect("commit");
        let loaded = repository
            .profile(&account(), &snapshot.profile_id, &snapshot.revision)
            .expect("load")
            .expect("snapshot");
        assert_eq!(
            loaded
                .items
                .iter()
                .map(|item| item.identity.tmdb_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            repository
                .title(&account(), MediaType::Movie, 1)
                .expect("title lookup")
                .expect("matching title")
                .title,
            "Movie 1"
        );
        assert!(
            repository
                .title(
                    &AccountKey::new("server", "other").expect("other account"),
                    MediaType::Movie,
                    1,
                )
                .expect("isolated lookup")
                .is_none()
        );
    }

    #[test]
    fn an_empty_franchise_result_is_initialized_and_reused() {
        let library = Library::open_in_memory().expect("library");
        let repository = SnapshotRepository::new(&library);
        repository
            .commit_franchise_resolution(&account(), &[], &[], 123)
            .expect("commit empty result");

        let state = repository
            .franchise_refresh_state(&account())
            .expect("refresh state");
        assert!(state.initialized);
        assert_eq!(state.last_success, Some(123));
        assert!(
            repository
                .franchises(&account())
                .expect("franchises")
                .is_empty()
        );

        repository
            .save_franchise_refresh_failure(&account(), 456)
            .expect("save failed attempt");
        let failed = repository
            .franchise_refresh_state(&account())
            .expect("failed state");
        assert!(failed.initialized);
        assert_eq!(failed.last_success, Some(123));
        assert_eq!(failed.last_attempt, Some(456));
    }

    #[test]
    fn franchise_membership_plan_scales_and_reuses_positive_and_negative_lookups() {
        let library = Library::open_in_memory().expect("library");
        let repository = SnapshotRepository::new(&library);
        let owned = (1..=10_000).collect::<Vec<_>>();
        let memberships = owned
            .iter()
            .map(|tmdb_id| FranchiseMembership {
                tmdb_id: *tmdb_id,
                collection_id: (tmdb_id % 2 == 0).then_some(42),
            })
            .collect::<Vec<_>>();
        repository
            .commit_franchise_resolution(&account(), &[], &memberships, 1_000)
            .expect("commit memberships");

        let fresh = repository
            .franchise_resolution_plan(&account(), &owned, 999)
            .expect("fresh plan");
        assert!(fresh.stale_movie_ids.is_empty());
        assert_eq!(fresh.known_collection_ids, vec![42]);

        let mut changed_library = owned;
        changed_library.push(10_001);
        let incremental = repository
            .franchise_resolution_plan(&account(), &changed_library, 999)
            .expect("incremental plan");
        assert_eq!(incremental.stale_movie_ids, vec![10_001]);

        let stale = repository
            .franchise_resolution_plan(&account(), &changed_library, 1_000)
            .expect("stale plan");
        assert_eq!(stale.stale_movie_ids.len(), 10_001);
    }

    #[test]
    fn one_franchise_is_loaded_without_materializing_the_full_cache() {
        let library = Library::open_in_memory().expect("library");
        let repository = SnapshotRepository::new(&library);
        let snapshots = vec![
            FranchiseSnapshot {
                collection_id: 10,
                name: "First".to_string(),
                poster_path: None,
                backdrop_path: None,
                committed_at: 50,
                items: vec![title(1, 1, false)],
            },
            FranchiseSnapshot {
                collection_id: 20,
                name: "Second".to_string(),
                poster_path: Some("/poster.jpg".to_string()),
                backdrop_path: None,
                committed_at: 50,
                items: vec![title(2, 2, false), title(3, 1, false)],
            },
        ];
        repository
            .commit_franchise_resolution(&account(), &snapshots, &[], 50)
            .expect("commit franchises");

        let loaded = repository
            .franchise(&account(), 20)
            .expect("load franchise")
            .expect("matching franchise");
        assert_eq!(loaded.collection_id, 20);
        assert_eq!(loaded.name, "Second");
        assert_eq!(loaded.poster_path.as_deref(), Some("/poster.jpg"));
        assert_eq!(
            loaded
                .items
                .iter()
                .map(|item| item.identity.tmdb_id)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
        assert!(
            repository
                .franchise(&account(), 99)
                .expect("missing")
                .is_none()
        );
    }
}
