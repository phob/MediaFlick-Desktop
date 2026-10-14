use std::collections::HashSet;

use rusqlite::{Connection, params};
use serde::Serialize;

use super::libraries::{GridSource, LibraryId, LibraryRegistry, MediaLibrary};
use super::query::natural_order;
use super::session_store::save_meta;
use super::{Library, now_unix};

/// Set once `/UserViews` has answered for this cache, so an empty `libraries`
/// table can tell "never read" from "the user sees no library".
const LIBRARIES_READ_AT: &str = "sync.libraries_at";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LibrarySetChange {
    pub added: Vec<LibraryId>,
    pub removed: Vec<LibraryId>,
    /// A library that stayed changed its name, kind, artwork or position.
    pub renamed: bool,
}

impl LibrarySetChange {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && !self.renamed
    }
}

/// The genre and parental rating choices of one library's titles.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptions {
    pub genres: Vec<String>,
    pub official_ratings: Vec<String>,
}

impl Library {
    pub fn libraries(&self) -> rusqlite::Result<LibraryRegistry> {
        if self.meta(LIBRARIES_READ_AT)?.is_none() {
            return Ok(LibraryRegistry::Unknown);
        }
        self.db
            .with_connection(stored_libraries)
            .map(LibraryRegistry::Known)
    }

    /// Makes `libraries`, in server order, the stored library set. Membership
    /// of a library that is gone or not a catalog library goes with it.
    /// Repeating it with the same list changes nothing.
    pub(crate) fn replace_libraries(
        &self,
        libraries: &[MediaLibrary],
    ) -> rusqlite::Result<LibrarySetChange> {
        self.db.with_transaction(|transaction| {
            let change = set_change(&stored_libraries(transaction)?, libraries);
            for id in &change.removed {
                transaction.execute("DELETE FROM libraries WHERE id = ?1", params![id.as_str()])?;
                delete_membership(transaction, id)?;
            }
            let synced_at = now_unix();
            for (position, library) in libraries.iter().enumerate() {
                transaction
                    .prepare_cached(
                        "INSERT INTO libraries
                             (id, name, collection_type, position, primary_image_tag, synced_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT(id) DO UPDATE SET
                             name = excluded.name,
                             collection_type = excluded.collection_type,
                             position = excluded.position,
                             primary_image_tag = excluded.primary_image_tag,
                             synced_at = excluded.synced_at",
                    )?
                    .execute(params![
                        library.id.as_str(),
                        library.name,
                        library.collection_type,
                        position as i64,
                        library.primary_image_tag,
                        synced_at,
                    ])?;
                if !matches!(library.kind().profile().grid, GridSource::Catalog { .. }) {
                    delete_membership(transaction, &library.id)?;
                }
            }
            save_meta(transaction, LIBRARIES_READ_AT, &synced_at.to_string())?;
            Ok(change)
        })
    }

    /// Makes `item_ids` the membership of one library. Returns whether it
    /// changed.
    pub(crate) fn replace_membership(
        &self,
        library: &LibraryId,
        item_ids: &HashSet<String>,
    ) -> rusqlite::Result<bool> {
        self.db.with_transaction(|transaction| {
            let current = transaction
                .prepare("SELECT item_id FROM item_libraries WHERE library_id = ?1")?
                .query_map(params![library.as_str()], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<HashSet<_>>>()?;
            let mut changed = false;
            for item_id in current.difference(item_ids) {
                transaction
                    .prepare_cached(
                        "DELETE FROM item_libraries WHERE library_id = ?1 AND item_id = ?2",
                    )?
                    .execute(params![library.as_str(), item_id])?;
                changed = true;
            }
            for item_id in item_ids.difference(&current) {
                transaction
                    .prepare_cached(
                        "INSERT INTO item_libraries (library_id, item_id) VALUES (?1, ?2)",
                    )?
                    .execute(params![library.as_str(), item_id])?;
                changed = true;
            }
            Ok(changed)
        })
    }

    pub fn library_filter_options(&self, library: &LibraryId) -> rusqlite::Result<FilterOptions> {
        self.db.with_connection(|connection| {
            let genres = connection
                .prepare_cached(
                    "SELECT DISTINCT g.genre FROM item_libraries l
                     JOIN items i ON i.jellyfin_id = l.item_id
                     JOIN item_genres g ON g.item_id = i.id
                     WHERE l.library_id = ?1
                     ORDER BY g.genre COLLATE NOCASE ASC",
                )?
                .query_map(params![library.as_str()], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut official_ratings = connection
                .prepare_cached(
                    "SELECT DISTINCT i.official_rating FROM item_libraries l
                     JOIN items i ON i.jellyfin_id = l.item_id
                     WHERE l.library_id = ?1 AND i.official_rating <> ''",
                )?
                .query_map(params![library.as_str()], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            official_ratings.sort_by(|left, right| natural_order(left, right));
            Ok(FilterOptions {
                genres,
                official_ratings,
            })
        })
    }
}

fn stored_libraries(connection: &Connection) -> rusqlite::Result<Vec<MediaLibrary>> {
    let rows = connection
        .prepare(
            "SELECT id, name, collection_type, primary_image_tag FROM libraries
             ORDER BY position",
        )?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, name, collection_type, primary_image_tag)| {
            Some(MediaLibrary {
                id: LibraryId::parse(&id)?,
                name,
                collection_type,
                primary_image_tag,
            })
        })
        .collect())
}

fn delete_membership(connection: &Connection, library: &LibraryId) -> rusqlite::Result<()> {
    connection.execute(
        "DELETE FROM item_libraries WHERE library_id = ?1",
        params![library.as_str()],
    )?;
    Ok(())
}

fn set_change(before: &[MediaLibrary], after: &[MediaLibrary]) -> LibrarySetChange {
    let (before_ids, after_ids) = (ids(before), ids(after));
    LibrarySetChange {
        added: missing_from(after, &before_ids)
            .map(|library| library.id.clone())
            .collect(),
        removed: missing_from(before, &after_ids)
            .map(|library| library.id.clone())
            .collect(),
        renamed: kept_in(before, &after_ids).ne(kept_in(after, &before_ids)),
    }
}

fn ids(libraries: &[MediaLibrary]) -> HashSet<LibraryId> {
    libraries.iter().map(|library| library.id.clone()).collect()
}

fn missing_from<'a>(
    libraries: &'a [MediaLibrary],
    other: &'a HashSet<LibraryId>,
) -> impl Iterator<Item = &'a MediaLibrary> {
    libraries
        .iter()
        .filter(|library| !other.contains(&library.id))
}

fn kept_in<'a>(
    libraries: &'a [MediaLibrary],
    other: &'a HashSet<LibraryId>,
) -> impl Iterator<Item = &'a MediaLibrary> {
    libraries
        .iter()
        .filter(|library| other.contains(&library.id))
}
