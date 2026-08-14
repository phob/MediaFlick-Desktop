//! SQLite storage for credentials, the catalog index, and sync state.
//!
//! `rusqlite::Connection` is not `Sync`, so callers borrow one from a small
//! pool for the duration of a query instead of sharing a single handle.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags};

/// Bump whenever the schema changes. Pre-1.0 databases are not migrated: an
/// older version is dropped wholesale and recreated, and the app resyncs the
/// catalog from the server.
pub const SCHEMA_VERSION: i32 = 13;

/// Connections kept alive between queries. The UI issues a handful of parallel
/// reads at most; the sync thread holds one for the length of a page.
const MAX_IDLE_CONNECTIONS: usize = 4;

pub struct Database {
    path: PathBuf,
    idle: Mutex<Vec<Connection>>,
}

impl Database {
    /// Opens (creating if needed) the database and ensures the current schema.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
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
            idle: Mutex::new(Vec::new()),
        };
        let connection = database.new_connection()?;
        migrate(&connection)?;
        database.release(connection);
        Ok(database)
    }

    /// An in-memory database for tests.
    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let connection = Connection::open_in_memory()?;
        configure(&connection)?;
        migrate(&connection)?;
        Ok(Self {
            path: PathBuf::from(":memory:"),
            idle: Mutex::new(vec![connection]),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn new_connection(&self) -> rusqlite::Result<Connection> {
        let connection = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
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

    /// Runs `work` against a pooled connection.
    pub fn with_connection<T>(
        &self,
        work: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let connection = self.acquire()?;
        let result = work(&connection);
        self.release(connection);
        result
    }

    /// Runs `work` inside a transaction, committing only on success.
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

fn configure(connection: &Connection) -> rusqlite::Result<()> {
    // WAL keeps the sync thread's writes from blocking UI reads.
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(std::time::Duration::from_secs(10))?;
    Ok(())
}

fn user_version(connection: &Connection) -> rusqlite::Result<i32> {
    connection.query_row("PRAGMA user_version", [], |row| row.get(0))
}

fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    let version = user_version(connection)?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if version > SCHEMA_VERSION {
        tracing::warn!(
            target: "library.db",
            version,
            expected = SCHEMA_VERSION,
            "library database was written by a newer app version"
        );
        return Ok(());
    }
    let mut session = None;
    if version > 0 {
        // Pre-1.0 rule: no migrations. An older database is dropped wholesale
        // and the catalog resyncs from scratch. Only the signed-in session
        // survives — it is what performs that resync, and every historical
        // schema stored it under the same single-row shape.
        tracing::info!(
            target: "library.db",
            version,
            "recreating the pre-1.0 library database at schema {SCHEMA_VERSION}"
        );
        session = saved_session(connection);
        drop_everything(connection)?;
    }
    connection.execute_batch(SCHEMA)?;
    if let Some(session) = session {
        restore_session(connection, &session)?;
    }
    connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tracing::debug!(target: "library.db", version = SCHEMA_VERSION, "library schema ready");
    Ok(())
}

struct SavedSession {
    server_url: Option<String>,
    user_id: Option<String>,
    user_name: Option<String>,
    server_id: Option<String>,
    device_id: String,
    token: Option<String>,
    updated_at: i64,
}

fn saved_session(connection: &Connection) -> Option<SavedSession> {
    connection
        .query_row(
            "SELECT server_url, user_id, user_name, server_id, device_id, token, updated_at
             FROM credentials WHERE id = 1",
            [],
            |row| {
                Ok(SavedSession {
                    server_url: row.get(0)?,
                    user_id: row.get(1)?,
                    user_name: row.get(2)?,
                    server_id: row.get(3)?,
                    device_id: row.get(4)?,
                    token: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )
        .ok()
}

fn restore_session(connection: &Connection, session: &SavedSession) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO credentials (id, server_url, user_id, user_name, server_id,
             device_id, token, updated_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            session.server_url,
            session.user_id,
            session.user_name,
            session.server_id,
            session.device_id,
            session.token,
            session.updated_at,
        ],
    )?;
    Ok(())
}

/// Drops every table (and, with them, indexes and triggers) so the fresh
/// schema starts from nothing. Views and virtual tables are dropped first so
/// FTS shadow tables disappear with their owner instead of erroring on a
/// direct drop.
fn drop_everything(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch("DROP TABLE IF EXISTS items_fts;")?;
    let tables: Vec<String> = {
        let mut statement = connection.prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )?;
        statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    // Referential actions between the historical tables all cascade towards
    // `items`; dropping with foreign keys off sidesteps ordering entirely.
    connection.pragma_update(None, "foreign_keys", "OFF")?;
    let result = (|| {
        for table in &tables {
            connection.execute_batch(&format!("DROP TABLE IF EXISTS \"{table}\";"))?;
        }
        Ok(())
    })();
    connection.pragma_update(None, "foreign_keys", "ON")?;
    result
}

/// The complete current schema.
///
/// `items` is a thin catalog index: one row per Movie/Series/Season/Episode
/// with only what browsing, sorting, filtering, joining, and drawing cards
/// needs. Rich metadata — synopsis, cast, technical streams, tags, studios,
/// critic ratings — is fetched live from Jellyfin when a surface asks for it.
const SCHEMA: &str = r#"
CREATE TABLE credentials (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    server_url   TEXT,
    user_id      TEXT,
    user_name    TEXT,
    server_id    TEXT,
    device_id    TEXT NOT NULL,
    token        TEXT,
    updated_at   INTEGER NOT NULL
);

CREATE TABLE items (
    id                  INTEGER PRIMARY KEY,
    jellyfin_id         TEXT NOT NULL UNIQUE,
    kind                TEXT NOT NULL,
    name                TEXT NOT NULL,
    original_title      TEXT,
    sort_name           TEXT,
    year                INTEGER,
    premiere_date       TEXT,
    runtime_ticks       INTEGER,
    community_rating    REAL,
    official_rating     TEXT,
    parent_id           TEXT,
    series_id           TEXT,
    series_name         TEXT,
    season_id           TEXT,
    index_number        INTEGER,
    parent_index_number INTEGER,
    child_count         INTEGER,
    tmdb_id             TEXT,
    imdb_id             TEXT,
    tvdb_id             TEXT,
    genres              TEXT NOT NULL DEFAULT '[]',
    image_tags          TEXT NOT NULL DEFAULT '{}',
    primary_image_tag   TEXT,
    backdrop_image_tag  TEXT,
    search_genres       TEXT NOT NULL DEFAULT '',
    date_created        TEXT,
    date_last_saved     TEXT,
    synced_at           INTEGER NOT NULL
);

CREATE INDEX items_kind_sort ON items (kind, sort_name);
CREATE INDEX items_series ON items (series_id, parent_index_number, index_number);
CREATE INDEX items_parent ON items (parent_id);
CREATE INDEX items_date_created ON items (date_created DESC);
CREATE INDEX items_date_last_saved ON items (date_last_saved DESC);
CREATE INDEX items_tmdb ON items (tmdb_id) WHERE tmdb_id IS NOT NULL;
CREATE INDEX items_imdb ON items (imdb_id) WHERE imdb_id IS NOT NULL;
CREATE INDEX items_tvdb ON items (tvdb_id) WHERE tvdb_id IS NOT NULL;
-- Release-decade filtering starts with kind and then applies a bounded year
-- range, so this composite index keeps both the count and each page efficient.
CREATE INDEX items_kind_year ON items (kind, year);

CREATE TABLE user_data (
    jellyfin_id             TEXT PRIMARY KEY,
    played                  INTEGER NOT NULL DEFAULT 0,
    play_count              INTEGER NOT NULL DEFAULT 0,
    playback_position_ticks INTEGER NOT NULL DEFAULT 0,
    is_favorite             INTEGER NOT NULL DEFAULT 0,
    played_percentage       REAL,
    last_played_date        TEXT,
    updated_at              INTEGER NOT NULL
);

CREATE INDEX user_data_resume ON user_data (last_played_date DESC)
    WHERE playback_position_ticks > 0;
CREATE INDEX user_data_favorite ON user_data (is_favorite) WHERE is_favorite = 1;

CREATE VIRTUAL TABLE items_fts USING fts5(
    name,
    original_title,
    search_genres,
    content='items',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER items_fts_insert AFTER INSERT ON items BEGIN
    INSERT INTO items_fts(rowid, name, original_title, search_genres)
    VALUES (new.id, new.name, new.original_title, new.search_genres);
END;

CREATE TRIGGER items_fts_delete AFTER DELETE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, name, original_title, search_genres)
    VALUES ('delete', old.id, old.name, old.original_title, old.search_genres);
END;

CREATE TRIGGER items_fts_update AFTER UPDATE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, name, original_title, search_genres)
    VALUES ('delete', old.id, old.name, old.original_title, old.search_genres);
    INSERT INTO items_fts(rowid, name, original_title, search_genres)
    VALUES (new.id, new.name, new.original_title, new.search_genres);
END;

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- The Seerr link, in the same single-row style as `credentials` and with the
-- same posture: plaintext, no OS keychain, exactly like the Jellyfin token
-- next to it.
--
-- `jellyfin_server_id` / `jellyfin_user_id` record the account the link was
-- made under. Without them an in-process account switch would leave user A's
-- Seerr cookie serving user B.
--
-- The Sonarr/Radarr pairs share the row: they are the same kind of optional,
-- instance-wide configuration.
CREATE TABLE seerr_config (
    id                       INTEGER PRIMARY KEY CHECK (id = 1),
    base_url                 TEXT,
    cookies                  TEXT,
    user_id                  INTEGER,
    user_name                TEXT,
    jellyfin_server_id       TEXT,
    jellyfin_user_id         TEXT,
    movie_4k_enabled         INTEGER NOT NULL DEFAULT 0,
    series_4k_enabled        INTEGER NOT NULL DEFAULT 0,
    partial_requests_enabled INTEGER NOT NULL DEFAULT 0,
    sonarr_url               TEXT,
    sonarr_api_key           TEXT,
    radarr_url               TEXT,
    radarr_api_key           TEXT,
    updated_at               INTEGER NOT NULL
);

-- Public integrations are user-associated application data, not process
-- configuration. A household sharing one desktop must never inherit another
-- Jellyfin user's connected profile.
CREATE TABLE external_profiles (
    id                  TEXT PRIMARY KEY,
    provider            TEXT NOT NULL,
    profile_key         TEXT NOT NULL,
    display_name        TEXT NOT NULL,
    canonical_url       TEXT NOT NULL,
    jellyfin_server_id  TEXT NOT NULL,
    jellyfin_user_id    TEXT NOT NULL,
    enabled             INTEGER NOT NULL DEFAULT 1,
    verification_status TEXT NOT NULL,
    created_at          INTEGER NOT NULL,
    last_checked_at     INTEGER,
    UNIQUE(provider, profile_key, jellyfin_server_id, jellyfin_user_id)
);
CREATE INDEX external_profiles_account
    ON external_profiles (jellyfin_server_id, jellyfin_user_id, provider);

-- Item-scoped source and track choices.
--
-- JSON snapshots retain both Jellyfin's current stream index and the language,
-- title, codec, channel, forced, external, and accessibility descriptors used
-- to identify what the user meant. A nullable subtitle snapshot in an existing
-- row is the explicit subtitles-off choice. The account identity follows the
-- same Jellyfin server/user scoping as other user-associated data, while the
-- item foreign key makes cache eviction remove every account's orphaned row.
CREATE TABLE item_playback_preferences (
    jellyfin_id         TEXT NOT NULL
                         REFERENCES items(jellyfin_id) ON DELETE CASCADE,
    jellyfin_server_key TEXT NOT NULL,
    jellyfin_user_id    TEXT NOT NULL,
    media_source        TEXT NOT NULL CHECK (json_valid(media_source)),
    audio_track         TEXT CHECK (audio_track IS NULL OR json_valid(audio_track)),
    subtitle_track      TEXT CHECK (subtitle_track IS NULL OR json_valid(subtitle_track)),
    updated_at          INTEGER NOT NULL,
    PRIMARY KEY (jellyfin_id, jellyfin_server_key, jellyfin_user_id)
);

-- Stable-provider rating cache and non-secret integration health.
--
-- Cache identity deliberately excludes Jellyfin IDs: when Jellyfin recreates
-- an item, its TMDB/IMDb identity can reuse the same durable result. Secrets
-- are never stored here; `integration_state` contains validation/quota facts
-- only and the API key stays in the operating-system credential vault.
CREATE TABLE rating_cache (
    provider           TEXT NOT NULL,
    provider_id        TEXT NOT NULL,
    media_type         TEXT NOT NULL CHECK (media_type IN ('movie', 'show')),
    ratings            TEXT NOT NULL CHECK (json_valid(ratings)),
    source_updated_at  TEXT,
    fetched_at         INTEGER NOT NULL,
    stale_at           INTEGER NOT NULL,
    expires_at         INTEGER NOT NULL,
    schema_version     INTEGER NOT NULL,
    origin             TEXT NOT NULL CHECK (origin IN ('local_mdblist', 'plugin')),
    PRIMARY KEY (provider, provider_id, media_type, origin)
);
CREATE INDEX rating_cache_expiry ON rating_cache (expires_at);

CREATE TABLE integration_state (
    service          TEXT PRIMARY KEY,
    validation       TEXT NOT NULL,
    valid            INTEGER NOT NULL DEFAULT 0,
    detail           TEXT,
    quota_limit      INTEGER,
    quota_remaining  INTEGER,
    quota_reset_at   INTEGER,
    retry_at         INTEGER,
    failure_count    INTEGER NOT NULL DEFAULT 0,
    updated_at       INTEGER NOT NULL
);
"#;

#[cfg(test)]
mod tests {
    use super::{Database, SCHEMA_VERSION, migrate, user_version};
    use rusqlite::Connection;

    #[test]
    fn a_fresh_database_has_the_current_schema() {
        let database = Database::open_in_memory().expect("open");
        let version = database.with_connection(user_version).expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        for table in [
            "credentials",
            "items",
            "user_data",
            "meta",
            "seerr_config",
            "external_profiles",
            "item_playback_preferences",
            "rating_cache",
            "integration_state",
        ] {
            let count: i64 = database
                .with_connection(|connection| {
                    connection.query_row(
                        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        [table],
                        |row| row.get(0),
                    )
                })
                .expect("table lookup");
            assert_eq!(count, 1, "missing table {table}");
        }
        let dropped_column: i64 = database
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM pragma_table_info('items')
                     WHERE name IN ('overview', 'people', 'media_streams', 'tags',
                                    'studios', 'critic_rating', 'search_people')",
                    [],
                    |row| row.get(0),
                )
            })
            .expect("column lookup");
        assert_eq!(dropped_column, 0, "a rich-metadata column survived");
    }

    #[test]
    fn an_older_database_is_recreated_rather_than_migrated() {
        let connection = Connection::open_in_memory().expect("open");
        // A minimal stand-in for a pre-13 database: an items table with a rich
        // column this schema no longer has, plus the queue table v13 removed.
        connection
            .execute_batch(
                "CREATE TABLE items (
                     id INTEGER PRIMARY KEY,
                     jellyfin_id TEXT NOT NULL UNIQUE,
                     overview TEXT
                 );
                 CREATE TABLE catalog_enrichment (jellyfin_id TEXT PRIMARY KEY);
                 INSERT INTO items (jellyfin_id, overview) VALUES ('m1', 'Old synopsis');
                 CREATE TABLE credentials (
                     id INTEGER PRIMARY KEY CHECK (id = 1),
                     server_url TEXT, user_id TEXT, user_name TEXT, server_id TEXT,
                     device_id TEXT NOT NULL, token TEXT, updated_at INTEGER NOT NULL
                 );
                 INSERT INTO credentials (id, server_url, user_id, device_id, token, updated_at)
                 VALUES (1, 'http://server:8096', 'uid', 'dev', 'tok', 7);",
            )
            .expect("old schema");
        connection
            .pragma_update(None, "user_version", 12)
            .expect("stamp v12");

        migrate(&connection).expect("migrate");

        assert_eq!(user_version(&connection).expect("version"), SCHEMA_VERSION);
        let items: i64 = connection
            .query_row("SELECT count(*) FROM items", [], |row| row.get(0))
            .expect("items");
        assert_eq!(items, 0, "old rows must not survive the recreate");
        let overview_column: i64 = connection
            .query_row(
                "SELECT count(*) FROM pragma_table_info('items') WHERE name = 'overview'",
                [],
                |row| row.get(0),
            )
            .expect("column lookup");
        assert_eq!(overview_column, 0);
        let queue_table: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'catalog_enrichment'",
                [],
                |row| row.get(0),
            )
            .expect("queue lookup");
        assert_eq!(queue_table, 0);
        // The signed-in session is the one thing that survives: it is what
        // performs the from-scratch resync after the recreate.
        let (token, device_id): (String, String) = connection
            .query_row(
                "SELECT token, device_id FROM credentials WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("session survived");
        assert_eq!(token, "tok");
        assert_eq!(device_id, "dev");
    }

    #[test]
    fn migrating_an_already_current_database_changes_nothing() {
        let connection = Connection::open_in_memory().expect("open");
        migrate(&connection).expect("first migrate");
        connection
            .execute(
                "INSERT INTO seerr_config (id, base_url, updated_at)
                 VALUES (1, 'https://seerr.test', 0)",
                [],
            )
            .expect("seed");
        migrate(&connection).expect("second migrate");
        let url: String = connection
            .query_row(
                "SELECT base_url FROM seerr_config WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .expect("row survived");
        assert_eq!(url, "https://seerr.test");
    }

    #[test]
    fn a_newer_database_is_left_untouched() {
        let connection = Connection::open_in_memory().expect("open");
        connection
            .execute_batch("CREATE TABLE future_table (id INTEGER PRIMARY KEY);")
            .expect("future schema");
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .expect("stamp future");

        migrate(&connection).expect("migrate");

        assert_eq!(
            user_version(&connection).expect("version"),
            SCHEMA_VERSION + 1
        );
        let survived: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'future_table'",
                [],
                |row| row.get(0),
            )
            .expect("future table");
        assert_eq!(survived, 1);
    }

    #[test]
    fn fts_rows_track_item_inserts_updates_and_deletes() {
        let database = Database::open_in_memory().expect("open");
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO items (jellyfin_id, kind, name, search_genres, synced_at)
                     VALUES ('a', 'Movie', 'The Matrix', 'Action, Sci-Fi', 0)",
                    [],
                )?;
                let hits: i64 = connection.query_row(
                    "SELECT count(*) FROM items_fts WHERE items_fts MATCH 'matrix'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(hits, 1);
                let by_genre: i64 = connection.query_row(
                    "SELECT count(*) FROM items_fts WHERE items_fts MATCH 'sci'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(by_genre, 1);

                connection.execute(
                    "UPDATE items SET name = 'Reloaded' WHERE jellyfin_id = 'a'",
                    [],
                )?;
                let stale: i64 = connection.query_row(
                    "SELECT count(*) FROM items_fts WHERE items_fts MATCH 'matrix'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(stale, 0);
                let renamed: i64 = connection.query_row(
                    "SELECT count(*) FROM items_fts WHERE items_fts MATCH 'reloaded'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(renamed, 1);

                connection.execute("DELETE FROM items WHERE jellyfin_id = 'a'", [])?;
                let removed: i64 = connection.query_row(
                    "SELECT count(*) FROM items_fts WHERE items_fts MATCH 'reloaded'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(removed, 0);
                Ok(())
            })
            .expect("fts consistency");
    }

    #[test]
    fn transactions_roll_back_on_error() {
        let database = Database::open_in_memory().expect("open");
        let result: rusqlite::Result<()> = database.with_transaction(|transaction| {
            transaction.execute(
                "INSERT INTO items (jellyfin_id, kind, name, synced_at) VALUES ('a','Movie','A',0)",
                [],
            )?;
            Err(rusqlite::Error::QueryReturnedNoRows)
        });
        assert!(result.is_err());
        let count: i64 = database
            .with_connection(|connection| {
                connection.query_row("SELECT count(*) FROM items", [], |row| row.get(0))
            })
            .expect("count");
        assert_eq!(count, 0);
    }
}
