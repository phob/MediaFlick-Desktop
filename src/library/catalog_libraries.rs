use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::jellyfin::api::{ApiError, items};
use crate::jellyfin::session::{Session, SessionScope};

use super::libraries::{LibraryId, LibraryRegistry, MediaLibrary};
use super::query::natural_order;
use super::session_store::save_meta;
use super::sync::{META_MEMBERSHIP_OWED, META_SWEEP_OWED, commit, storage_error};
use super::{Library, now_unix};

/// Set once `/UserViews` has answered for this cache, so an empty `libraries`
/// table can tell "never read" from "the user sees no library".
const LIBRARIES_READ_AT: &str = "sync.libraries_at";

/// The genre and parental rating choices of one library's titles.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptions {
    pub genres: Vec<String>,
    pub official_ratings: Vec<String>,
}

/// Stores the server's library list. A successful empty answer is the
/// server's and is stored as such.
pub(super) fn refresh_libraries(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
) -> Result<(), ApiError> {
    let views = items::fetch_user_views(scope.client(), scope.user_id())?;
    let mut seen = HashSet::new();
    let libraries = views
        .iter()
        .filter_map(|view| {
            let parsed = MediaLibrary::from_view(view);
            if parsed.is_none() {
                tracing::warn!(target: "library.sync", id = view.id, "skipped a library with an unusable id");
            }
            parsed
        })
        .filter(|parsed| seen.insert(parsed.id.clone()))
        .collect::<Vec<_>>();
    commit(session, scope, || {
        library
            .replace_libraries(&libraries)
            .map_err(|error| storage_error(&error))
    })
}

impl Library {
    pub fn libraries(&self) -> rusqlite::Result<LibraryRegistry> {
        // One read transaction, so a sign-out clearing both between the two
        // reads cannot turn "never read" into "no library".
        self.db.with_transaction(|transaction| {
            let read = transaction
                .query_row(
                    "SELECT 1 FROM meta WHERE key = ?1",
                    params![LIBRARIES_READ_AT],
                    |_| Ok(()),
                )
                .optional()?;
            match read {
                None => Ok(LibraryRegistry::Unknown),
                Some(()) => stored_libraries(transaction).map(LibraryRegistry::Known),
            }
        })
    }

    /// Makes `libraries`, in server order, the stored library set. Membership
    /// of a library that is gone or not a catalog library goes with it. A
    /// removed library owes an identity sweep and a new catalog library a
    /// membership pass, recorded in the same transaction. Repeating it with
    /// the same list changes nothing.
    pub(crate) fn replace_libraries(&self, libraries: &[MediaLibrary]) -> rusqlite::Result<()> {
        self.db.with_transaction(|transaction| {
            let before = stored_libraries(transaction)?;
            let listed = libraries
                .iter()
                .map(|library| &library.id)
                .collect::<HashSet<_>>();
            let mut removed = false;
            for gone in before
                .iter()
                .filter(|library| !listed.contains(&library.id))
            {
                transaction.execute(
                    "DELETE FROM libraries WHERE id = ?1",
                    params![gone.id.as_str()],
                )?;
                delete_membership(transaction, &gone.id)?;
                removed = true;
            }
            let catalog_before = before
                .iter()
                .filter_map(|library| Some((&library.id, library.catalog_title()?)))
                .collect::<HashSet<_>>();
            let mut new_catalog = false;
            for (position, library) in libraries.iter().enumerate() {
                transaction
                    .prepare_cached(
                        "INSERT INTO libraries
                             (id, name, collection_type, position, primary_image_tag)
                         VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT(id) DO UPDATE SET
                             name = excluded.name,
                             collection_type = excluded.collection_type,
                             position = excluded.position,
                             primary_image_tag = excluded.primary_image_tag",
                    )?
                    .execute(params![
                        library.id.as_str(),
                        library.name,
                        library.collection_type,
                        position as i64,
                        library.primary_image_tag,
                    ])?;
                match library.catalog_title() {
                    None => delete_membership(transaction, &library.id)?,
                    Some(title) => new_catalog |= !catalog_before.contains(&(&library.id, title)),
                }
            }
            if removed {
                save_meta(transaction, META_SWEEP_OWED, "1")?;
            }
            if new_catalog {
                save_meta(transaction, META_MEMBERSHIP_OWED, "1")?;
            }
            save_meta(transaction, LIBRARIES_READ_AT, &now_unix().to_string())
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
