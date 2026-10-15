use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};

use super::libraries::{LibraryId, LibraryRegistry, MediaLibrary};
use super::session_store::save_meta;
use super::{Library, now_unix};

const LIBRARIES_READ_AT: &str = "sync.libraries_at";
pub(super) const META_SWEEP_OWED: &str = "sync.sweep_owed";

impl Library {
    pub fn libraries(&self) -> rusqlite::Result<LibraryRegistry> {
        self.db.with_transaction(|transaction| {
            if libraries_read(transaction)? {
                stored_libraries(transaction).map(LibraryRegistry::Known)
            } else {
                Ok(LibraryRegistry::Unknown)
            }
        })
    }

    pub(crate) fn replace_libraries(&self, libraries: &[MediaLibrary]) -> rusqlite::Result<bool> {
        self.db.with_transaction(|transaction| {
            let first_read = !libraries_read(transaction)?;
            let before = stored_libraries(transaction)?;
            let listing = |entries: &[MediaLibrary]| {
                entries
                    .iter()
                    .map(|library| (library.id.clone(), library.name.clone(), library.kind()))
                    .collect::<Vec<_>>()
            };
            let changed = first_read || listing(&before) != listing(libraries);
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
            save_meta(transaction, LIBRARIES_READ_AT, &now_unix().to_string())?;
            Ok(changed)
        })
    }

    pub fn library_of(&self, item_id: &str) -> rusqlite::Result<Option<LibraryId>> {
        let library = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT m.library_id FROM items i
                     JOIN item_libraries m ON m.item_id = CASE
                         WHEN i.kind IN ('Season', 'Episode') THEN i.series_id
                         ELSE i.jellyfin_id
                     END
                     JOIN libraries l ON l.id = m.library_id
                     WHERE i.jellyfin_id = ?1
                     ORDER BY l.position
                     LIMIT 1",
                    params![item_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
        })?;
        Ok(library.as_deref().and_then(LibraryId::parse))
    }

    pub(crate) fn owe_every_membership(&self) -> rusqlite::Result<()> {
        self.db.with_transaction(|transaction| {
            stored_libraries(transaction)?
                .iter()
                .filter(|library| library.catalog_title().is_some())
                .try_for_each(|library| mark_owed(transaction, &library.id))
        })
    }

    pub(crate) fn replace_membership(
        &self,
        library: &LibraryId,
        item_ids: &HashSet<String>,
    ) -> rusqlite::Result<MembershipReplaced> {
        self.db.with_transaction(|transaction| {
            let settled = transaction.execute(
                "UPDATE libraries SET membership_owed = 0 WHERE id = ?1 AND membership_owed = 1",
                params![library.as_str()],
            )? > 0;
            if item_ids.is_empty() {
                return Ok(MembershipReplaced {
                    settled,
                    changed: false,
                });
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
            Ok(MembershipReplaced { settled, changed })
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct MembershipReplaced {
    pub(crate) settled: bool,
    pub(crate) changed: bool,
}

impl MembershipReplaced {
    pub(crate) fn merge(&mut self, other: Self) {
        self.settled |= other.settled;
        self.changed |= other.changed;
    }
}

fn libraries_read(connection: &Connection) -> rusqlite::Result<bool> {
    connection
        .query_row(
            "SELECT 1 FROM meta WHERE key = ?1",
            params![LIBRARIES_READ_AT],
            |_| Ok(()),
        )
        .optional()
        .map(|read| read.is_some())
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
