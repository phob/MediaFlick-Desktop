use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};

use super::libraries::{LibraryId, LibraryRegistry, MediaLibrary};
use super::session_store::save_meta;
use super::{Library, now_unix};

/// Set once `/UserViews` has answered for this cache, so an empty `libraries`
/// table can tell "never read" from "the user sees no library".
const LIBRARIES_READ_AT: &str = "sync.libraries_at";
/// The identity sweep a removed library leaves for a later sync step. It is
/// set in the same transaction as the removal and cleared only once the
/// sweep's deletion pass ran, so a cycle that fails in between leaves it to
/// the next one. A library's owed membership walk is its `membership_owed`.
pub(super) const META_SWEEP_OWED: &str = "sync.sweep_owed";

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
    /// removed library owes an identity sweep and a new catalog library its
    /// membership walk, recorded in the same transaction. Repeating it with
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
                    Some(title) if !catalog_before.contains(&(&library.id, title)) => {
                        mark_owed(transaction, &library.id)?;
                    }
                    Some(_) => {}
                }
            }
            if removed {
                save_meta(transaction, META_SWEEP_OWED, "1")?;
            }
            save_meta(transaction, LIBRARIES_READ_AT, &now_unix().to_string())
        })
    }

    /// Marks every catalog library as owing its membership walk until
    /// `replace_membership` lands one.
    pub(crate) fn owe_every_membership(&self) -> rusqlite::Result<()> {
        self.db.with_transaction(|transaction| {
            stored_libraries(transaction)?
                .iter()
                .filter(|library| library.catalog_title().is_some())
                .try_for_each(|library| mark_owed(transaction, &library.id))
        })
    }

    /// Makes `item_ids` the membership of one library and settles the walk
    /// it owed. An empty `item_ids` keeps the stored membership, because an
    /// empty answer is far likelier a server hiccup than an emptied library.
    /// Returns whether the membership changed.
    pub(crate) fn replace_membership(
        &self,
        library: &LibraryId,
        item_ids: &HashSet<String>,
    ) -> rusqlite::Result<bool> {
        self.db.with_transaction(|transaction| {
            transaction.execute(
                "UPDATE libraries SET membership_owed = 0 WHERE id = ?1",
                params![library.as_str()],
            )?;
            if item_ids.is_empty() {
                return Ok(false);
            }
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
}

fn stored_libraries(connection: &Connection) -> rusqlite::Result<Vec<MediaLibrary>> {
    let rows = connection
        .prepare(
            "SELECT id, name, collection_type, primary_image_tag, membership_owed
             FROM libraries ORDER BY position",
        )?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(
            |(id, name, collection_type, primary_image_tag, membership_pending)| {
                Some(MediaLibrary {
                    id: LibraryId::parse(&id)?,
                    name,
                    collection_type,
                    primary_image_tag,
                    membership_pending,
                })
            },
        )
        .collect())
}

fn mark_owed(connection: &Connection, library: &LibraryId) -> rusqlite::Result<()> {
    connection
        .prepare_cached("UPDATE libraries SET membership_owed = 1 WHERE id = ?1")?
        .execute(params![library.as_str()])?;
    Ok(())
}

/// Drops one library's membership, and with it any walk it owed.
fn delete_membership(connection: &Connection, library: &LibraryId) -> rusqlite::Result<()> {
    connection.execute(
        "DELETE FROM item_libraries WHERE library_id = ?1",
        params![library.as_str()],
    )?;
    connection.execute(
        "UPDATE libraries SET membership_owed = 0 WHERE id = ?1",
        params![library.as_str()],
    )?;
    Ok(())
}
