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
pub const SCHEMA_VERSION: i32 = 16;

/// Connections kept alive between queries. The UI issues a handful of parallel
/// reads at most; the sync thread holds one for the length of a page.
const MAX_IDLE_CONNECTIONS: usize = 4;

pub struct Database {
    path: PathBuf,
    open_flags: OpenFlags,
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
            open_flags: read_write_flags(),
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
        migrate(&connection)?;
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

fn read_write_flags() -> OpenFlags {
    OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
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
    let mut legacy_account_data = None;
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
        if version == 13 {
            legacy_account_data = Some(saved_legacy_account_data(connection)?);
        }
        drop_everything(connection)?;
    }
    connection.execute_batch(SCHEMA)?;
    if let Some(session) = session {
        restore_session(connection, &session)?;
    }
    if let Some(legacy_account_data) = legacy_account_data {
        restore_legacy_account_data(connection, &legacy_account_data)?;
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

#[derive(Default)]
struct SavedLegacyAccountData {
    profiles: Vec<SavedExternalProfile>,
    playback: Vec<SavedPlaybackPreference>,
}

struct SavedExternalProfile {
    id: String,
    provider: String,
    profile_key: String,
    display_name: String,
    canonical_url: String,
    server_id: String,
    user_id: String,
    enabled: bool,
    verification_status: String,
    created_at: i64,
    last_checked_at: Option<i64>,
}

struct SavedPlaybackPreference {
    item_id: String,
    server_key: String,
    user_id: String,
    media_source: String,
    audio_track: Option<String>,
    subtitle_track: Option<String>,
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

fn saved_legacy_account_data(connection: &Connection) -> rusqlite::Result<SavedLegacyAccountData> {
    let profiles = {
        let mut statement = connection.prepare(
            "SELECT id, provider, profile_key, display_name, canonical_url,
                    jellyfin_server_id, jellyfin_user_id, enabled,
                    verification_status, created_at, last_checked_at
             FROM external_profiles",
        )?;
        statement
            .query_map([], |row| {
                Ok(SavedExternalProfile {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    profile_key: row.get(2)?,
                    display_name: row.get(3)?,
                    canonical_url: row.get(4)?,
                    server_id: row.get(5)?,
                    user_id: row.get(6)?,
                    enabled: row.get(7)?,
                    verification_status: row.get(8)?,
                    created_at: row.get(9)?,
                    last_checked_at: row.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let playback = {
        let mut statement = connection.prepare(
            "SELECT jellyfin_id, jellyfin_server_key, jellyfin_user_id,
                    media_source, audio_track, subtitle_track, updated_at
             FROM item_playback_preferences",
        )?;
        statement
            .query_map([], |row| {
                Ok(SavedPlaybackPreference {
                    item_id: row.get(0)?,
                    server_key: row.get(1)?,
                    user_id: row.get(2)?,
                    media_source: row.get(3)?,
                    audio_track: row.get(4)?,
                    subtitle_track: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(SavedLegacyAccountData { profiles, playback })
}

fn restore_legacy_account_data(
    connection: &Connection,
    data: &SavedLegacyAccountData,
) -> rusqlite::Result<()> {
    for profile in &data.profiles {
        connection.execute(
            "INSERT INTO external_profiles (
                 id, provider, profile_key, display_name, canonical_url,
                 jellyfin_server_id, jellyfin_user_id, enabled,
                 verification_status, created_at, last_checked_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                profile.id,
                profile.provider,
                profile.profile_key,
                profile.display_name,
                profile.canonical_url,
                profile.server_id,
                profile.user_id,
                profile.enabled,
                profile.verification_status,
                profile.created_at,
                profile.last_checked_at,
            ],
        )?;
    }
    for preference in &data.playback {
        connection.execute(
            "INSERT INTO legacy_item_playback_preferences (
                 jellyfin_id, jellyfin_server_key, jellyfin_user_id,
                 media_source, audio_track, subtitle_track, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                preference.item_id,
                preference.server_key,
                preference.user_id,
                preference.media_source,
                preference.audio_track,
                preference.subtitle_track,
                preference.updated_at,
            ],
        )?;
    }
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

-- Schema-13 playback rows cannot be copied into the account JSON files until
-- those stores open later in startup. This durable handoff survives a crash
-- between database recreation and the file imports.
CREATE TABLE legacy_item_playback_preferences (
    jellyfin_id         TEXT NOT NULL,
    jellyfin_server_key TEXT NOT NULL,
    jellyfin_user_id    TEXT NOT NULL,
    media_source        TEXT NOT NULL CHECK (json_valid(media_source)),
    audio_track         TEXT CHECK (audio_track IS NULL OR json_valid(audio_track)),
    subtitle_track      TEXT CHECK (subtitle_track IS NULL OR json_valid(subtitle_track)),
    updated_at          INTEGER NOT NULL,
    PRIMARY KEY (jellyfin_id, jellyfin_server_key, jellyfin_user_id)
);

-- Stable-provider rating cache.
--
-- Cache identity deliberately excludes Jellyfin IDs: when Jellyfin recreates
-- an item, its TMDB/IMDb identity can reuse the same durable result. Provider
-- credentials stay in Companion and never enter this database.
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
    PRIMARY KEY (provider, provider_id, media_type)
);
CREATE INDEX rating_cache_expiry ON rating_cache (expires_at);

-- Collection configuration lives in collections.json. These tables contain
-- provider and ownership results that the app may discard and rebuild.
CREATE TABLE collection_snapshots (
    server_id       TEXT NOT NULL,
    user_id         TEXT NOT NULL,
    profile_id      TEXT NOT NULL,
    revision        TEXT NOT NULL,
    committed_at    INTEGER NOT NULL,
    item_count      INTEGER NOT NULL,
    PRIMARY KEY (server_id, user_id, profile_id, revision)
);

CREATE TABLE collection_snapshot_items (
    server_id       TEXT NOT NULL,
    user_id         TEXT NOT NULL,
    profile_id      TEXT NOT NULL,
    revision        TEXT NOT NULL,
    media_type      TEXT NOT NULL CHECK (media_type IN ('movie', 'series')),
    tmdb_id         INTEGER NOT NULL CHECK (tmdb_id > 0),
    title           TEXT NOT NULL,
    original_title  TEXT,
    year            INTEGER,
    overview        TEXT NOT NULL,
    release_date    TEXT,
    source_order    INTEGER NOT NULL,
    poster_path     TEXT,
    backdrop_path   TEXT,
    adult           INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, user_id, profile_id, revision, media_type, tmdb_id),
    FOREIGN KEY (server_id, user_id, profile_id, revision)
        REFERENCES collection_snapshots(server_id, user_id, profile_id, revision)
        ON DELETE CASCADE
);
CREATE INDEX collection_snapshot_order
    ON collection_snapshot_items (server_id, user_id, profile_id, revision, source_order);
CREATE INDEX collection_snapshot_identity
    ON collection_snapshot_items (server_id, user_id, media_type, tmdb_id);

CREATE TABLE collection_refresh_state (
    server_id       TEXT NOT NULL,
    user_id         TEXT NOT NULL,
    profile_id      TEXT NOT NULL,
    last_attempt    INTEGER,
    last_success    INTEGER,
    latest_failure  TEXT,
    next_due        INTEGER,
    initialized     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, user_id, profile_id)
);
CREATE INDEX collection_refresh_due
    ON collection_refresh_state (server_id, user_id, next_due);

CREATE TABLE franchise_snapshots (
    server_id          TEXT NOT NULL,
    user_id            TEXT NOT NULL,
    tmdb_collection_id INTEGER NOT NULL CHECK (tmdb_collection_id > 0),
    name               TEXT NOT NULL,
    poster_path        TEXT,
    backdrop_path      TEXT,
    committed_at       INTEGER NOT NULL,
    PRIMARY KEY (server_id, user_id, tmdb_collection_id)
);

CREATE TABLE franchise_snapshot_items (
    server_id          TEXT NOT NULL,
    user_id            TEXT NOT NULL,
    tmdb_collection_id INTEGER NOT NULL,
    media_type         TEXT NOT NULL CHECK (media_type = 'movie'),
    tmdb_id            INTEGER NOT NULL CHECK (tmdb_id > 0),
    title              TEXT NOT NULL,
    original_title     TEXT,
    year               INTEGER,
    overview           TEXT NOT NULL,
    release_date       TEXT,
    source_order       INTEGER NOT NULL,
    poster_path        TEXT,
    backdrop_path      TEXT,
    adult              INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, user_id, tmdb_collection_id, tmdb_id),
    FOREIGN KEY (server_id, user_id, tmdb_collection_id)
        REFERENCES franchise_snapshots(server_id, user_id, tmdb_collection_id)
        ON DELETE CASCADE
);

-- TMDB movie membership changes rarely. Keeping negative lookups as NULL is
-- just as important as positive ones: otherwise every non-franchise movie is
-- fetched again after each library sync.
CREATE TABLE franchise_movie_membership (
    server_id      TEXT NOT NULL,
    user_id        TEXT NOT NULL,
    tmdb_id        INTEGER NOT NULL CHECK (tmdb_id > 0),
    collection_id  INTEGER CHECK (collection_id > 0),
    resolved_at    INTEGER NOT NULL,
    PRIMARY KEY (server_id, user_id, tmdb_id)
);
CREATE INDEX franchise_movie_membership_age
    ON franchise_movie_membership (server_id, user_id, resolved_at);
CREATE INDEX franchise_snapshot_order
    ON franchise_snapshot_items (server_id, user_id, tmdb_collection_id, source_order);
CREATE INDEX franchise_snapshot_identity
    ON franchise_snapshot_items (server_id, user_id, media_type, tmdb_id);

CREATE TABLE provider_identity_map (
    media_type      TEXT NOT NULL CHECK (media_type IN ('movie', 'series')),
    provider        TEXT NOT NULL CHECK (provider IN ('imdb', 'tvdb')),
    provider_id     TEXT NOT NULL,
    tmdb_id         INTEGER NOT NULL CHECK (tmdb_id > 0),
    resolved_at     INTEGER NOT NULL,
    PRIMARY KEY (media_type, provider, provider_id)
);
CREATE INDEX provider_identity_tmdb
    ON provider_identity_map (media_type, tmdb_id);

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
            "external_profiles",
            "legacy_item_playback_preferences",
            "rating_cache",
            "collection_snapshots",
            "collection_snapshot_items",
            "collection_refresh_state",
            "franchise_snapshots",
            "franchise_snapshot_items",
            "franchise_movie_membership",
            "provider_identity_map",
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
    fn pooled_in_memory_connections_share_the_schema() {
        let database = Database::open_in_memory().expect("open");
        database
            .with_connection(|_| {
                let version = database.with_connection(user_version)?;
                assert_eq!(version, SCHEMA_VERSION);
                Ok(())
            })
            .expect("nested connection");
    }

    #[test]
    fn an_older_database_is_recreated_rather_than_migrated() {
        let connection = Connection::open_in_memory().expect("open");
        // A minimal stand-in for a pre-14 database: an items table with a rich
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
    fn schema_13_account_data_survives_recreation_for_the_file_import() {
        let connection = Connection::open_in_memory().expect("open");
        connection
            .execute_batch(
                r#"CREATE TABLE credentials (
                     id INTEGER PRIMARY KEY CHECK (id = 1),
                     server_url TEXT, user_id TEXT, user_name TEXT, server_id TEXT,
                     device_id TEXT NOT NULL, token TEXT, updated_at INTEGER NOT NULL
                 );
                 INSERT INTO credentials
                     (id, server_url, user_id, server_id, device_id, token, updated_at)
                 VALUES
                     (1, 'http://server:8096', NULL, 'server-a', 'device', NULL, 7);
                 CREATE TABLE items (
                     id INTEGER PRIMARY KEY,
                     jellyfin_id TEXT NOT NULL UNIQUE
                 );
                 INSERT INTO items (jellyfin_id) VALUES ('movie-1');
                 CREATE TABLE external_profiles (
                     id TEXT PRIMARY KEY, provider TEXT NOT NULL, profile_key TEXT NOT NULL,
                     display_name TEXT NOT NULL, canonical_url TEXT NOT NULL,
                     jellyfin_server_id TEXT NOT NULL, jellyfin_user_id TEXT NOT NULL,
                     enabled INTEGER NOT NULL, verification_status TEXT NOT NULL,
                     created_at INTEGER NOT NULL, last_checked_at INTEGER
                 );
                 INSERT INTO external_profiles VALUES (
                     '0123456789abcdef', 'letterboxd', 'alice', 'Alice',
                     'https://letterboxd.com/alice/', 'server-a', 'user-a',
                     1, 'verified', 10, 11
                 );
                 CREATE TABLE item_playback_preferences (
                     jellyfin_id TEXT NOT NULL REFERENCES items(jellyfin_id) ON DELETE CASCADE,
                     jellyfin_server_key TEXT NOT NULL, jellyfin_user_id TEXT NOT NULL,
                     media_source TEXT NOT NULL, audio_track TEXT, subtitle_track TEXT,
                     updated_at INTEGER NOT NULL,
                     PRIMARY KEY (jellyfin_id, jellyfin_server_key, jellyfin_user_id)
                 );
                 INSERT INTO item_playback_preferences VALUES (
                     'movie-1', 'server-a', 'user-a',
                     '{"index":0,"name":"Main"}',
                     '{"index":1,"language":"jpn"}', NULL, 12
                 );"#,
            )
            .expect("schema 13 fixture");
        connection
            .pragma_update(None, "user_version", 13)
            .expect("stamp schema 13");

        migrate(&connection).expect("migrate schema 13");
        migrate(&connection).expect("idempotent retry");

        let profile: (String, String, String) = connection
            .query_row(
                "SELECT profile_key, jellyfin_server_id, jellyfin_user_id
                 FROM external_profiles",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("staged profile");
        assert_eq!(
            profile,
            ("alice".into(), "server-a".into(), "user-a".into())
        );
        let playback: (String, String, Option<String>) = connection
            .query_row(
                "SELECT jellyfin_id, media_source, subtitle_track
                 FROM legacy_item_playback_preferences",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("staged playback preference");
        assert_eq!(playback.0, "movie-1");
        assert!(playback.1.contains("Main"));
        assert_eq!(playback.2, None);
    }

    #[test]
    fn migrating_an_already_current_database_changes_nothing() {
        let connection = Connection::open_in_memory().expect("open");
        migrate(&connection).expect("first migrate");
        connection
            .execute("INSERT INTO meta (key, value) VALUES ('kept', 'yes')", [])
            .expect("seed");
        migrate(&connection).expect("second migrate");
        let value: String = connection
            .query_row("SELECT value FROM meta WHERE key = 'kept'", [], |row| {
                row.get(0)
            })
            .expect("row survived");
        assert_eq!(value, "yes");
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
