//! SQLite storage for credentials, the metadata cache, and sync state.
//!
//! `rusqlite::Connection` is not `Sync`, so callers borrow one from a small
//! pool for the duration of a query instead of sharing a single handle.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags};

/// Bump together with a new `migrate` arm whenever the schema changes.
pub const SCHEMA_VERSION: i32 = 7;

/// Connections kept alive between queries. The UI issues a handful of parallel
/// reads at most; the sync thread holds one for the length of a page.
const MAX_IDLE_CONNECTIONS: usize = 4;

pub struct Database {
    path: PathBuf,
    idle: Mutex<Vec<Connection>>,
}

impl Database {
    /// Opens (creating if needed) the database and applies pending migrations.
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
    let mut version = user_version(connection)?;
    if version > SCHEMA_VERSION {
        tracing::warn!(
            target: "library.db",
            version,
            expected = SCHEMA_VERSION,
            "library database was written by a newer app version"
        );
        return Ok(());
    }
    if version < 1 {
        connection.execute_batch(SCHEMA_V1)?;
        connection.pragma_update(None, "user_version", 1)?;
        version = 1;
    }
    if version < 2 {
        connection.execute_batch(SCHEMA_V2)?;
        connection.pragma_update(None, "user_version", 2)?;
        version = 2;
    }
    if version < 3 {
        // Pre-release builds created this table under the project's old
        // spelling. Those databases are already at version 2, so the arm above
        // never runs for them and the renamed table would never appear —
        // leaving Seerr permanently unconfigurable on that machine. Nothing
        // was ever released under the old name, so the stale table is dropped
        // rather than migrated.
        connection.execute_batch("DROP TABLE IF EXISTS seer_config;")?;
        connection.execute_batch(SCHEMA_V2)?;
        connection.pragma_update(None, "user_version", 3)?;
        version = 3;
    }
    if version < 4 {
        connection.execute_batch(SCHEMA_V4)?;
        connection.pragma_update(None, "user_version", 4)?;
        version = 4;
    }
    if version < 5 {
        connection.execute_batch(SCHEMA_V5)?;
        connection.pragma_update(None, "user_version", 5)?;
        version = 5;
    }
    if version < 6 {
        connection.execute_batch(SCHEMA_V6)?;
        connection.pragma_update(None, "user_version", 6)?;
        version = 6;
    }
    if version < 7 {
        connection.execute_batch(SCHEMA_V7)?;
        connection.pragma_update(None, "user_version", 7)?;
        version = 7;
    }
    tracing::debug!(target: "library.db", version, "library schema ready");
    Ok(())
}

const SCHEMA_V1: &str = r#"
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
    overview            TEXT,
    community_rating    REAL,
    critic_rating       REAL,
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
    tags                TEXT NOT NULL DEFAULT '[]',
    studios             TEXT NOT NULL DEFAULT '[]',
    people              TEXT NOT NULL DEFAULT '[]',
    image_tags          TEXT NOT NULL DEFAULT '{}',
    primary_image_tag   TEXT,
    backdrop_image_tag  TEXT,
    search_genres       TEXT NOT NULL DEFAULT '',
    search_people       TEXT NOT NULL DEFAULT '',
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
    overview,
    search_genres,
    search_people,
    content='items',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER items_fts_insert AFTER INSERT ON items BEGIN
    INSERT INTO items_fts(rowid, name, original_title, overview, search_genres, search_people)
    VALUES (new.id, new.name, new.original_title, new.overview, new.search_genres, new.search_people);
END;

CREATE TRIGGER items_fts_delete AFTER DELETE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, name, original_title, overview, search_genres, search_people)
    VALUES ('delete', old.id, old.name, old.original_title, old.overview, old.search_genres, old.search_people);
END;

CREATE TRIGGER items_fts_update AFTER UPDATE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, name, original_title, overview, search_genres, search_people)
    VALUES ('delete', old.id, old.name, old.original_title, old.overview, old.search_genres, old.search_people);
    INSERT INTO items_fts(rowid, name, original_title, overview, search_genres, search_people)
    VALUES (new.id, new.name, new.original_title, new.overview, new.search_genres, new.search_people);
END;

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// The Seerr link, in the same single-row style as `credentials` and with the
/// same posture: plaintext, no OS keychain, exactly like the Jellyfin token
/// next to it.
///
/// `jellyfin_server_id` / `jellyfin_user_id` record the account the link was
/// made under. Without them an in-process account switch would leave user A's
/// Seerr cookie serving user B.
///
/// The Sonarr/Radarr pairs share the row: they are the same kind of optional,
/// instance-wide configuration, and one row is one migration.
///
/// `IF NOT EXISTS` because the v3 arm replays this batch to repair a database
/// that reached version 2 without it.
const SCHEMA_V2: &str = r#"
CREATE TABLE IF NOT EXISTS seerr_config (
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
"#;

/// Public integrations are user-associated application data, not process
/// configuration.  A household sharing one desktop must never inherit another
/// Jellyfin user's connected profile.
const SCHEMA_V4: &str = r#"
CREATE TABLE IF NOT EXISTS external_profiles (
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
CREATE INDEX IF NOT EXISTS external_profiles_account
    ON external_profiles (jellyfin_server_id, jellyfin_user_id, provider);
"#;

/// Release-decade filtering starts with kind and then applies a bounded year
/// range, so this composite index keeps both the count and each 60-item page
/// efficient. The update repairs older rows for which Jellyfin supplied only
/// PremiereDate; new writes perform the same fallback in `ItemRecord`.
const SCHEMA_V5: &str = r#"
UPDATE items
SET year = CAST(substr(premiere_date, 1, 4) AS INTEGER)
WHERE year IS NULL
  AND premiere_date GLOB '[0-9][0-9][0-9][0-9]-*'
  AND CAST(substr(premiere_date, 1, 4) AS INTEGER) BETWEEN 1900 AND 9999;
CREATE INDEX IF NOT EXISTS items_kind_year ON items (kind, year);
"#;

/// Durable discovery for items cached during Jellyfin's enrichment window.
///
/// The flag is updated by every subsequent full DTO upsert. Existing databases
/// need the equivalent conservative predicate once so rows stranded by the old
/// `DateCreated` watermark become eligible without title- or time-specific
/// code. Attempt ordering rotates a bounded batch across launches while leaving
/// unresolved rows eligible indefinitely.
const SCHEMA_V6: &str = r#"
ALTER TABLE items ADD COLUMN metadata_repair_pending INTEGER NOT NULL DEFAULT 0;
ALTER TABLE items ADD COLUMN metadata_repair_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE items ADD COLUMN metadata_repair_last_attempt_at INTEGER;

UPDATE items
SET metadata_repair_pending = 1
WHERE kind IN ('Movie', 'Series')
  AND COALESCE(primary_image_tag, '') = ''
  AND COALESCE(backdrop_image_tag, '') = ''
  AND NOT EXISTS (
      SELECT 1
      FROM json_each(CASE WHEN json_valid(image_tags) THEN image_tags ELSE '{}' END) image
      WHERE trim(COALESCE(CAST(image.value AS TEXT), '')) <> ''
  )
  AND COALESCE(trim(overview), '') = ''
  AND COALESCE(trim(tmdb_id), '') = ''
  AND COALESCE(trim(imdb_id), '') = ''
  AND COALESCE(trim(tvdb_id), '') = ''
  AND NOT EXISTS (
      SELECT 1
      FROM json_each(CASE WHEN json_valid(genres) THEN genres ELSE '[]' END) genre
      WHERE trim(COALESCE(CAST(genre.value AS TEXT), '')) <> ''
  )
  AND NOT EXISTS (
      SELECT 1
      FROM json_each(CASE WHEN json_valid(tags) THEN tags ELSE '[]' END) tag
      WHERE trim(COALESCE(CAST(tag.value AS TEXT), '')) <> ''
  )
  AND NOT EXISTS (
      SELECT 1
      FROM json_each(CASE WHEN json_valid(studios) THEN studios ELSE '[]' END) studio
      WHERE trim(COALESCE(CAST(studio.value AS TEXT), '')) <> ''
  )
  AND NOT EXISTS (
      SELECT 1
      FROM json_each(CASE WHEN json_valid(people) THEN people ELSE '[]' END) person
      WHERE CASE WHEN person.type = 'object'
                 THEN COALESCE(trim(json_extract(person.value, '$.name')), '')
                 ELSE '' END <> ''
  )
  AND COALESCE(trim(official_rating), '') = ''
  AND community_rating IS NULL
  AND critic_rating IS NULL;

CREATE INDEX items_metadata_repair_candidates
    ON items (metadata_repair_pending, metadata_repair_last_attempt_at, date_created DESC)
    WHERE metadata_repair_pending = 1 AND kind IN ('Movie', 'Series');
"#;

/// Per-item metadata convergence is deliberately separate from `items`.
///
/// The v6 columns remain physically present for safe forward migration, but no
/// current code reads or writes them. A foreign key makes every cache eviction
/// clean up queue state in the same transaction. `next_attempt_at` is wall-clock
/// time so restart/offline time preserves, rather than resets, backoff.
const SCHEMA_V7: &str = r#"
CREATE TABLE metadata_convergence (
    jellyfin_id          TEXT PRIMARY KEY
                          REFERENCES items(jellyfin_id) ON DELETE CASCADE,
    state                TEXT NOT NULL CHECK (state IN ('pending', 'complete', 'dormant')),
    missing_quality      INTEGER NOT NULL DEFAULT 0,
    quality_score        INTEGER NOT NULL DEFAULT 0,
    first_observed_at    INTEGER NOT NULL,
    last_observed_at     INTEGER NOT NULL,
    last_progress_at     INTEGER NOT NULL,
    next_attempt_at      INTEGER,
    attempt_count        INTEGER NOT NULL DEFAULT 0,
    unchanged_count      INTEGER NOT NULL DEFAULT 0,
    error_count          INTEGER NOT NULL DEFAULT 0,
    successful_unchanged INTEGER NOT NULL DEFAULT 0,
    signature            TEXT NOT NULL DEFAULT ''
);

CREATE INDEX metadata_convergence_due
    ON metadata_convergence (next_attempt_at, first_observed_at, jellyfin_id)
    WHERE state = 'pending' AND next_attempt_at IS NOT NULL;

DROP INDEX IF EXISTS items_metadata_repair_candidates;

-- Preserve every unresolved v6 candidate. Its first v7 observation will
-- replace the conservative score/signature with the pure quality model.
INSERT INTO metadata_convergence (
    jellyfin_id, state, missing_quality, quality_score,
    first_observed_at, last_observed_at, last_progress_at, next_attempt_at,
    attempt_count, unchanged_count, error_count, successful_unchanged, signature
)
SELECT jellyfin_id, 'pending', 127, 0,
       synced_at, synced_at, synced_at,
       max(synced_at, CAST(strftime('%s', 'now') AS INTEGER))
           + 120 + (length(jellyfin_id) * 17) % 30,
       metadata_repair_attempts, 0, 0, 0, ''
FROM items
WHERE metadata_repair_pending = 1
  AND kind IN ('Movie', 'Series');
"#;

#[cfg(test)]
mod tests {
    use super::{
        Database, SCHEMA_V1, SCHEMA_V2, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6, SCHEMA_VERSION, migrate,
        user_version,
    };
    use rusqlite::Connection;

    #[test]
    fn a_fresh_database_is_migrated_to_the_current_schema() {
        let database = Database::open_in_memory().expect("open");
        let version = database.with_connection(user_version).expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        let table: i64 = database
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'seerr_config'",
                    [],
                    |row| row.get(0),
                )
            })
            .expect("seerr_config");
        assert_eq!(table, 1);
        let profiles: i64 = database
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'external_profiles'",
                    [],
                    |row| row.get(0),
                )
            })
            .expect("external_profiles");
        assert_eq!(profiles, 1);
        let year_index: i64 = database
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'items_kind_year'",
                    [],
                    |row| row.get(0),
                )
            })
            .expect("items_kind_year");
        assert_eq!(year_index, 1);
        let convergence_table: i64 = database
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'metadata_convergence'",
                    [],
                    |row| row.get(0),
                )
            })
            .expect("metadata convergence table");
        assert_eq!(convergence_table, 1);
    }

    #[test]
    fn a_v1_database_gains_seerr_config_without_losing_its_session() {
        let connection = Connection::open_in_memory().expect("open");
        connection.execute_batch(SCHEMA_V1).expect("v1 schema");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("stamp v1");
        connection
            .execute(
                "INSERT INTO credentials (id, server_url, user_id, device_id, token, updated_at)
                 VALUES (1, 'http://server:8096', 'uid', 'dev', 'tok', 0)",
                [],
            )
            .expect("seed credentials");

        migrate(&connection).expect("migrate");

        assert_eq!(user_version(&connection).expect("version"), SCHEMA_VERSION);
        let token: String = connection
            .query_row("SELECT token FROM credentials WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("credentials survived");
        assert_eq!(token, "tok");
        // The table exists and is empty: nothing is linked until the user says so.
        let rows: i64 = connection
            .query_row("SELECT count(*) FROM seerr_config", [], |row| row.get(0))
            .expect("seerr_config");
        assert_eq!(rows, 0);
    }

    #[test]
    fn a_v4_database_backfills_release_years_and_adds_the_filter_index() {
        let connection = Connection::open_in_memory().expect("open");
        connection.execute_batch(SCHEMA_V1).expect("v1 schema");
        connection.execute_batch(SCHEMA_V2).expect("v2 schema");
        connection.execute_batch(SCHEMA_V4).expect("v4 schema");
        connection
            .execute(
                "INSERT INTO items (jellyfin_id, kind, name, premiere_date, synced_at)
                 VALUES ('series', 'Series', 'Premiere-only', '2017-02-15T00:00:00Z', 0)",
                [],
            )
            .expect("seed item");
        connection
            .pragma_update(None, "user_version", 4)
            .expect("stamp v4");

        migrate(&connection).expect("migrate");

        assert_eq!(user_version(&connection).expect("version"), SCHEMA_VERSION);
        assert_eq!(
            connection
                .query_row(
                    "SELECT year FROM items WHERE jellyfin_id = 'series'",
                    [],
                    |row| { row.get::<_, i64>(0) }
                )
                .expect("year"),
            2017
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'items_kind_year'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("index"),
            1
        );
    }

    #[test]
    fn a_v5_database_discovers_existing_sparse_movies_without_title_rules() {
        let connection = Connection::open_in_memory().expect("open");
        connection.execute_batch(SCHEMA_V1).expect("v1 schema");
        connection.execute_batch(SCHEMA_V2).expect("v2 schema");
        connection.execute_batch(SCHEMA_V4).expect("v4 schema");
        connection.execute_batch(SCHEMA_V5).expect("v5 schema");
        connection
            .execute_batch(
                "INSERT INTO items (jellyfin_id, kind, name, year, date_created, synced_at)
                 VALUES
                   ('6627a9641b7d3db806eb2f2545b6f6dc', 'Movie', 'Krull', 1983,
                    '2024-01-01T00:00:00Z', 1),
                   ('b0b16f465f948879b6932d56bd6bfec3', 'Movie', 'The Blob', 1988,
                    '2024-01-02T00:00:00Z', 2),
                   ('complete', 'Movie', 'Complete', 2000,
                    '2024-01-03T00:00:00Z', 3);
                 UPDATE items SET image_tags = '{\"Primary\":\"poster\"}'
                 WHERE jellyfin_id = 'complete';",
            )
            .expect("seed v5 items");
        connection
            .pragma_update(None, "user_version", 5)
            .expect("stamp v5");

        migrate(&connection).expect("migrate");

        let pending = |id: &str| {
            connection
                .query_row(
                    "SELECT metadata_repair_pending FROM items WHERE jellyfin_id = ?1",
                    [id],
                    |row| row.get::<_, bool>(0),
                )
                .expect("pending flag")
        };
        assert!(pending("6627a9641b7d3db806eb2f2545b6f6dc"));
        assert!(pending("b0b16f465f948879b6932d56bd6bfec3"));
        assert!(!pending("complete"));
    }

    #[test]
    fn a_v6_database_backfills_pending_candidates_and_cascades_eviction() {
        let connection = Connection::open_in_memory().expect("open");
        connection.execute_batch(SCHEMA_V1).expect("v1 schema");
        connection.execute_batch(SCHEMA_V2).expect("v2 schema");
        connection.execute_batch(SCHEMA_V4).expect("v4 schema");
        connection.execute_batch(SCHEMA_V5).expect("v5 schema");
        connection.execute_batch(SCHEMA_V6).expect("v6 schema");
        connection
            .execute(
                "INSERT INTO items (jellyfin_id, kind, name, synced_at,
                                    metadata_repair_pending, metadata_repair_attempts)
                 VALUES ('pending', 'Movie', 'Sparse', 10, 1, 3),
                        ('complete', 'Movie', 'Rich', 11, 0, 0)",
                [],
            )
            .expect("seed");
        connection
            .pragma_update(None, "user_version", 6)
            .expect("stamp v6");

        migrate(&connection).expect("migrate");

        let row: (String, i64) = connection
            .query_row(
                "SELECT state, attempt_count FROM metadata_convergence WHERE jellyfin_id = 'pending'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("backfilled row");
        assert_eq!(row, ("pending".to_string(), 3));
        let complete_rows: i64 = connection
            .query_row(
                "SELECT count(*) FROM metadata_convergence WHERE jellyfin_id = 'complete'",
                [],
                |row| row.get(0),
            )
            .expect("complete rows");
        assert_eq!(complete_rows, 0);

        connection
            .pragma_update(None, "foreign_keys", "ON")
            .expect("foreign keys");
        connection
            .execute("DELETE FROM items WHERE jellyfin_id = 'pending'", [])
            .expect("delete item");
        let queued: i64 = connection
            .query_row("SELECT count(*) FROM metadata_convergence", [], |row| {
                row.get(0)
            })
            .expect("queue rows");
        assert_eq!(queued, 0);
    }

    /// A pre-release build stamped version 2 while creating the table under the
    /// project's old spelling, which left the renamed one absent for good —
    /// every Seerr read answered "no such table" and nothing could be linked.
    #[test]
    fn a_database_stamped_v2_by_a_pre_rename_build_is_repaired() {
        let connection = Connection::open_in_memory().expect("open");
        connection.execute_batch(SCHEMA_V1).expect("v1 schema");
        connection
            .execute_batch("CREATE TABLE seer_config (id INTEGER PRIMARY KEY, base_url TEXT);")
            .expect("old table");
        connection
            .pragma_update(None, "user_version", 2)
            .expect("stamp v2");

        migrate(&connection).expect("migrate");

        assert_eq!(user_version(&connection).expect("version"), SCHEMA_VERSION);
        let rows: i64 = connection
            .query_row("SELECT count(*) FROM seerr_config", [], |row| row.get(0))
            .expect("seerr_config exists");
        assert_eq!(rows, 0);
        let stale: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'seer_config'",
                [],
                |row| row.get(0),
            )
            .expect("sqlite_master");
        assert_eq!(stale, 0, "the pre-rename table was left behind");
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
    fn fts_rows_track_item_inserts_updates_and_deletes() {
        let database = Database::open_in_memory().expect("open");
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO items (jellyfin_id, kind, name, search_people, synced_at)
                     VALUES ('a', 'Movie', 'The Matrix', 'Keanu Reeves', 0)",
                    [],
                )?;
                let hits: i64 = connection.query_row(
                    "SELECT count(*) FROM items_fts WHERE items_fts MATCH 'matrix'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(hits, 1);

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
    fn people_are_searchable_through_the_index() {
        let database = Database::open_in_memory().expect("open");
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO items (jellyfin_id, kind, name, search_people, synced_at)
                     VALUES ('a', 'Movie', 'Speed', 'Keanu Reeves, Sandra Bullock', 0)",
                    [],
                )?;
                let hits: i64 = connection.query_row(
                    "SELECT count(*) FROM items_fts WHERE items_fts MATCH 'bullock'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(hits, 1);
                Ok(())
            })
            .expect("people search");
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
