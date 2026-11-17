//! SQLite storage for credentials, the catalog index, and sync state.
//!
//! `rusqlite::Connection` is not `Sync`, so callers borrow one from a small
//! pool for the duration of a query instead of sharing a single handle.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use rusqlite::{Connection, OpenFlags};

mod development;
mod migrate;
mod schema;
#[cfg(test)]
mod tests;

const MAX_IDLE_CONNECTIONS: usize = 4;

/// Why `library.db` could not be opened. Every variant leaves the file as it
/// was: a schema change commits whole or not at all.
#[derive(Debug)]
pub enum OpenError {
    Sqlite(rusqlite::Error),
    /// A newer MediaFlick wrote the file, at a schema this build does not know.
    NewerSchema {
        found: i32,
    },
    /// A schema change failed and was rolled back.
    Migration {
        from: i32,
        error: rusqlite::Error,
    },
}

impl fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "{error}"),
            Self::NewerSchema { found } => write!(
                formatter,
                "library.db is at schema {found}, which a newer MediaFlick wrote; this version reads \
                 schema {} and older. The file was left as it was: install that newer MediaFlick \
                 again to keep using it",
                migrate::SCHEMA_VERSION
            ),
            Self::Migration { from, error } => write!(
                formatter,
                "could not update library.db from schema {from} to {}: {error}. The file was left \
                 at schema {from}",
                migrate::SCHEMA_VERSION
            ),
        }
    }
}

impl From<rusqlite::Error> for OpenError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

pub struct Database {
    path: PathBuf,
    open_flags: OpenFlags,
    idle: Mutex<Vec<Connection>>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, OpenError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                    Some(format!("could not create {}: {error}", parent.display())),
                )
            })?;
        }
        let database = Self {
            path: path.to_path_buf(),
            open_flags: read_write_flags(),
            idle: Mutex::new(Vec::new()),
        };
        let connection = database.new_connection()?;
        migrate::migrate(&connection)?;
        database.release(connection);
        Ok(database)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, OpenError> {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_DATABASE_ID: AtomicU64 = AtomicU64::new(0);

        let id = NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "file:mediaflick-desktop-test-{}-{id}?mode=memory&cache=shared",
            std::process::id()
        ));
        let open_flags = read_write_flags() | OpenFlags::SQLITE_OPEN_URI;
        let connection = Connection::open_with_flags(&path, open_flags)?;
        configure(&connection)?;
        migrate::migrate(&connection)?;
        Ok(Self {
            path,
            open_flags,
            idle: Mutex::new(vec![connection]),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn new_connection(&self) -> rusqlite::Result<Connection> {
        let connection = Connection::open_with_flags(&self.path, self.open_flags)?;
        configure(&connection)?;
        Ok(connection)
    }

    fn acquire(&self) -> rusqlite::Result<Connection> {
        let pooled = self.idle.lock().ok().and_then(|mut idle| idle.pop());
        match pooled {
            Some(connection) => Ok(connection),
            None => self.new_connection(),
        }
    }

    fn release(&self, connection: Connection) {
        if let Ok(mut idle) = self.idle.lock()
            && idle.len() < MAX_IDLE_CONNECTIONS
        {
            idle.push(connection);
        }
    }

    pub fn with_connection<T>(
        &self,
        work: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let connection = self.acquire()?;
        let result = work(&connection);
        self.release(connection);
        result
    }

    /// Refreshes the query planner's statistics. SQLite only reads them when a
    /// connection opens, so idle pooled connections are retired and reopen
    /// with the new statistics on their next use.
    pub fn optimize(&self) -> rusqlite::Result<()> {
        let connection = self.acquire()?;
        let result = (|| {
            connection.pragma_update(None, "analysis_limit", 1000)?;
            connection.pragma_update(None, "optimize", 0x10002)
        })();
        if result.is_ok() {
            let retired =
                std::mem::take(&mut *self.idle.lock().unwrap_or_else(PoisonError::into_inner));
            drop(retired);
        }
        self.release(connection);
        result
    }

    pub fn with_transaction<T>(
        &self,
        work: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let mut connection = self.acquire()?;
        let result = (|| {
            let transaction = connection.transaction()?;
            let value = work(&transaction)?;
            transaction.commit()?;
            Ok(value)
        })();
        self.release(connection);
        result
    }
}

fn read_write_flags() -> OpenFlags {
    OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
}

fn configure(connection: &Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(std::time::Duration::from_secs(10))?;
    super::query::register_functions(connection)
}

pub const CATALOG_TABLES: [&str; 8] = [
    "items",
    "user_data",
    "libraries",
    "item_libraries",
    "external_profiles",
    "legacy_item_playback_preferences",
    "franchise_movie_membership",
    "meta",
];
