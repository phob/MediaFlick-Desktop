//! Opening `library.db` at the schema this build reads.
//!
//! Schema 23, which MediaFlick 0.2.0 shipped, is the first one real users
//! have. From it on, a schema change bumps `SCHEMA_VERSION` and appends one
//! step to `MIGRATIONS`, and a database is never recreated. Released builds
//! run their own SQL against a newer file, so a step only adds: tables,
//! nullable or defaulted columns, indexes and triggers (AGENTS.md,
//! "Persistence and compatibility"). Development schemas below 23 are
//! recreated with their session.

use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::OpenError;
use super::development;
use super::schema::SCHEMA;

pub const SCHEMA_VERSION: i32 = 23;

/// The schema MediaFlick 0.2.0 shipped, the first that real users have.
pub(super) const FIRST_RELEASED_SCHEMA: i32 = 23;

/// One forward step: `MIGRATIONS[i]` moves schema `23 + i` to `24 + i`. A
/// step that has shipped is never edited, reordered or removed; the next step
/// fixes it. A step only changes the database: no file, network or other side
/// effect, since a failure rolls back the database alone.
pub(super) type Migration = fn(&Transaction<'_>) -> rusqlite::Result<()>;

pub(super) const MIGRATIONS: &[Migration] = &[];

const _: () = assert!(
    MIGRATIONS.len() as i32 == SCHEMA_VERSION - FIRST_RELEASED_SCHEMA,
    "a SCHEMA_VERSION bump adds exactly one step to MIGRATIONS"
);

pub(super) fn user_version(connection: &Connection) -> rusqlite::Result<i32> {
    connection.query_row("PRAGMA user_version", [], |row| row.get(0))
}

/// Refuses a database a newer MediaFlick wrote. Reads only.
pub(super) fn refuse_newer(connection: &Connection) -> Result<(), OpenError> {
    let version = user_version(connection)?;
    if version > SCHEMA_VERSION {
        return Err(OpenError::NewerSchema { found: version });
    }
    Ok(())
}

pub(super) fn migrate(connection: &Connection) -> Result<(), OpenError> {
    migrate_with(connection, MIGRATIONS)
}

/// Opens at the schema `migrations` lead to from schema 23: `MIGRATIONS` in
/// the app, a list a test chooses in the tests.
pub(super) fn migrate_with(
    connection: &Connection,
    migrations: &[Migration],
) -> Result<(), OpenError> {
    let version = user_version(connection)?;
    let target = FIRST_RELEASED_SCHEMA
        + i32::try_from(migrations.len()).map_err(|_| OpenError::NewerSchema { found: version })?;
    if version == target {
        return Ok(());
    }
    if version > target {
        return Err(OpenError::NewerSchema { found: version });
    }
    if version >= FIRST_RELEASED_SCHEMA {
        return upgrade(connection, version, migrations, target);
    }
    if version != 0 {
        tracing::info!(
            target: "library.db",
            version,
            "recreating the development library database at schema {target}"
        );
    }
    change_schema(connection, target, |transaction| {
        if version == 0 {
            create(transaction)
        } else {
            development::recreate(transaction, version)
        }
    })
    .map_err(|error| OpenError::Migration {
        from: version,
        to: target,
        error,
    })?;
    tracing::debug!(target: "library.db", version = target, "library schema ready");
    Ok(())
}

/// Runs `migrations` from schema `from` on, all in one transaction, and
/// stamps `to`, the version after the last one.
fn upgrade(
    connection: &Connection,
    from: i32,
    migrations: &[Migration],
    to: i32,
) -> Result<(), OpenError> {
    let steps = usize::try_from(from - FIRST_RELEASED_SCHEMA)
        .ok()
        .and_then(|first| migrations.get(first..))
        .ok_or(OpenError::NewerSchema { found: from })?;
    change_schema(connection, to, |transaction| {
        steps.iter().try_for_each(|step| step(transaction))
    })
    .map_err(|error| OpenError::Migration { from, to, error })?;
    tracing::info!(target: "library.db", from, to, "library schema migrated {from} → {to}");
    Ok(())
}

/// The current schema, on an empty database.
pub(super) fn create(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(SCHEMA)?;
    transaction.execute_batch(&super::super::sort_indexes())
}

/// Runs `change` and stamps `to` in one `BEGIN IMMEDIATE` transaction, with
/// foreign keys off for its duration: SQLite ignores `PRAGMA foreign_keys`
/// inside a transaction, and with them on, dropping a parent table to rebuild
/// it deletes its children. The result must pass `foreign_key_check` and
/// `quick_check` before it commits; otherwise nothing of it stays.
fn change_schema(
    connection: &Connection,
    to: i32,
    change: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<()>,
) -> rusqlite::Result<()> {
    connection.pragma_update(None, "foreign_keys", "OFF")?;
    let result = (|| {
        let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
        change(&transaction)?;
        check_integrity(&transaction)?;
        transaction.pragma_update(None, "user_version", to)?;
        transaction.commit()
    })();
    let restored = connection.pragma_update(None, "foreign_keys", "ON");
    result.and(restored)
}

fn check_integrity(connection: &Connection) -> rusqlite::Result<()> {
    let broken = {
        let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
        statement
            .query_map([], |row| {
                Ok(format!(
                    "{} row {} has no {}",
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?
                        .map_or_else(|| "?".to_owned(), |rowid| rowid.to_string()),
                    row.get::<_, String>(2)?
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    if !broken.is_empty() {
        return Err(integrity_error(
            rusqlite::ffi::SQLITE_CONSTRAINT,
            format!("broken foreign keys: {}", broken.join("; ")),
        ));
    }
    let report = {
        let mut statement = connection.prepare("PRAGMA quick_check")?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    if report != ["ok"] {
        return Err(integrity_error(
            rusqlite::ffi::SQLITE_CORRUPT,
            format!("quick_check: {}", report.join("; ")),
        ));
    }
    Ok(())
}

fn integrity_error(code: std::ffi::c_int, message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), Some(message))
}
