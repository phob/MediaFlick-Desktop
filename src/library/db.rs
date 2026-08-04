//! SQLite storage for credentials, the metadata cache, and sync state.
//!
//! `rusqlite::Connection` is not `Sync`, so callers borrow one from a small
//! pool for the duration of a query instead of sharing a single handle.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags};

use super::model::{ItemRecord, metadata_convergence_initial_delay, persisted_metadata_quality};
use super::now_unix;

/// Bump together with a new `migrate` arm whenever the schema changes.
pub const SCHEMA_VERSION: i32 = 12;

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
    if version < 8 {
        migrate_v8(connection)?;
        connection.pragma_update(None, "user_version", 8)?;
        version = 8;
    }
    if version < 9 {
        connection.execute_batch(SCHEMA_V9)?;
        connection.pragma_update(None, "user_version", 9)?;
        version = 9;
    }
    if version < 10 {
        migrate_v10(connection)?;
        connection.pragma_update(None, "user_version", 10)?;
        version = 10;
    }
    if version < 11 {
        connection.execute_batch(SCHEMA_V11)?;
        connection.pragma_update(None, "user_version", 11)?;
        version = 11;
    }
    if version < 12 {
        connection.execute_batch(SCHEMA_V12)?;
        connection.pragma_update(None, "user_version", 12)?;
        version = 12;
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

/// Re-evaluates rows that v7 missed because its one-time backfill only trusted
/// the obsolete v6 repair flag. This deliberately reads the current cache and
/// applies the same quality dimensions as live DTO ingestion; see
/// `persisted_metadata_quality` for the one conservative provider-id caveat.
/// The production defect is confined to top-level Movie/Series rows. Seasons
/// can already complete from hierarchy plus one provider signal, while a broad
/// historical Episode backfill could enqueue an unbounded child library, so
/// neither child kind is migrated without evidence of a similarly stranded set.
///
/// No table is rebuilt. Existing queue rows are excluded before insertion, so
/// retry, progress, dormancy, and completion state survive byte-for-byte. If a
/// migration is interrupted before `user_version` advances, `INSERT OR IGNORE`
/// makes replay safe.
fn migrate_v8(connection: &Connection) -> rusqlite::Result<()> {
    let mut statement = connection.prepare(
        "SELECT jellyfin_id, kind, name, original_title, sort_name, year, premiere_date,
                runtime_ticks, overview, community_rating, critic_rating, official_rating,
                parent_id, series_id, series_name, season_id, index_number,
                parent_index_number, child_count, tmdb_id, imdb_id, tvdb_id, genres, tags,
                studios, people, image_tags, primary_image_tag, backdrop_image_tag,
                search_genres, search_people, date_created, date_last_saved
         FROM items
         WHERE kind IN ('Movie', 'Series')
           AND NOT EXISTS (
               SELECT 1 FROM metadata_convergence convergence
               WHERE convergence.jellyfin_id = items.jellyfin_id
           )",
    )?;
    let records = statement
        .query_map([], |row| {
            Ok(ItemRecord {
                jellyfin_id: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                original_title: row.get(3)?,
                sort_name: row.get(4)?,
                year: row.get(5)?,
                premiere_date: row.get(6)?,
                runtime_ticks: row.get(7)?,
                overview: row.get(8)?,
                community_rating: row.get(9)?,
                critic_rating: row.get(10)?,
                official_rating: row.get(11)?,
                parent_id: row.get(12)?,
                series_id: row.get(13)?,
                series_name: row.get(14)?,
                season_id: row.get(15)?,
                index_number: row.get(16)?,
                parent_index_number: row.get(17)?,
                child_count: row.get(18)?,
                tmdb_id: row.get(19)?,
                imdb_id: row.get(20)?,
                tvdb_id: row.get(21)?,
                genres: row.get(22)?,
                tags: row.get(23)?,
                studios: row.get(24)?,
                people: row.get(25)?,
                image_tags: row.get(26)?,
                primary_image_tag: row.get(27)?,
                backdrop_image_tag: row.get(28)?,
                // v8 runs before the v10 column exists. The later migration
                // starts historical rows empty and schedules one full metadata
                // refresh to populate them.
                media_streams: "[]".to_string(),
                search_genres: row.get(29)?,
                search_people: row.get(30)?,
                date_created: row.get(31)?,
                date_last_saved: row.get(32)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);

    let now = now_unix();
    for record in records {
        let quality = persisted_metadata_quality(&record);
        if quality.complete {
            continue;
        }
        connection.execute(
            "INSERT OR IGNORE INTO metadata_convergence (
                 jellyfin_id, state, missing_quality, quality_score,
                 first_observed_at, last_observed_at, last_progress_at, next_attempt_at,
                 attempt_count, unchanged_count, error_count, successful_unchanged, signature
             ) VALUES (?1, 'pending', ?2, ?3, ?4, ?4, ?4, ?5, 0, 0, 0, 0, ?6)",
            rusqlite::params![
                record.jellyfin_id,
                i64::from(quality.missing),
                i64::from(quality.score),
                now,
                now + metadata_convergence_initial_delay(&record.jellyfin_id),
                quality.signature,
            ],
        )?;
    }
    Ok(())
}

/// Item-scoped source and track choices.
///
/// JSON snapshots retain both Jellyfin's current stream index and the language,
/// title, codec, channel, forced, external, and accessibility descriptors used
/// to identify what the user meant. A nullable subtitle snapshot in an existing
/// row is the explicit subtitles-off choice. The account identity follows the
/// same Jellyfin server/user scoping as other user-associated data, while the
/// item foreign key makes cache eviction remove every account's orphaned row.
const SCHEMA_V9: &str = r#"
CREATE TABLE IF NOT EXISTS item_playback_preferences (
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
"#;

/// Lightweight technical metadata for cards. v10 originally forced a weekly
/// catalog refresh to populate historical rows; v11 replaces that broad fetch
/// with its dedicated finite enrichment queue.
const SCHEMA_V10: &str = r#"
ALTER TABLE items ADD COLUMN media_streams TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid(media_streams));
"#;

fn migrate_v10(connection: &Connection) -> rusqlite::Result<()> {
    let already_added: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('items') WHERE name = 'media_streams')",
        [],
        |row| row.get(0),
    )?;
    if !already_added {
        connection.execute_batch(SCHEMA_V10)?;
    }
    // Replays are harmless. This was v10's historical backfill trigger; the
    // following v11 migration restores completed-catalog state and queues the
    // finite enrichment pass instead.
    connection.execute("DELETE FROM meta WHERE key = 'sync.bootstrap_at'", [])?;
    Ok(())
}

/// Every catalog row gets an explicit durable rich-metadata job. This queue is
/// separate from the older convergence queue: enrichment is a finite pass over
/// all rows, while convergence revisits the genuinely sparse subset during
/// Jellyfin's asynchronous metadata-provider window.
///
/// Existing rows are queued without touching catalog/bootstrap markers, user
/// data, preferences, or item metadata. A migration therefore behaves as a
/// background backfill and never turns a usable cache into first-time setup.
const SCHEMA_V11: &str = r#"
-- v10 used a forced full catalog refresh to acquire MediaStreams. The finite
-- queue below supersedes that behavior: restore a missing timestamp only for a
-- cache explicitly known to have completed, so this migration is a backfill
-- and never a first-time/bootstrap transition.
INSERT INTO meta (key, value)
SELECT 'sync.bootstrap_at', CAST(strftime('%s', 'now') AS TEXT)
WHERE EXISTS (SELECT 1 FROM items)
  AND EXISTS (SELECT 1 FROM meta WHERE key = 'sync.bootstrap_done' AND value = '1')
  AND NOT EXISTS (SELECT 1 FROM meta WHERE key = 'sync.bootstrap_at');

CREATE TABLE IF NOT EXISTS catalog_enrichment (
    jellyfin_id     TEXT PRIMARY KEY
                    REFERENCES items(jellyfin_id) ON DELETE CASCADE,
    state           TEXT NOT NULL CHECK (state IN ('pending', 'complete')),
    next_attempt_at INTEGER,
    attempt_count   INTEGER NOT NULL DEFAULT 0,
    error_count     INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT,
    updated_at      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS catalog_enrichment_due
    ON catalog_enrichment (next_attempt_at, jellyfin_id)
    WHERE state = 'pending' AND next_attempt_at IS NOT NULL;

INSERT OR IGNORE INTO catalog_enrichment (
    jellyfin_id, state, next_attempt_at, attempt_count, error_count, last_error, updated_at
)
SELECT jellyfin_id, 'pending', CAST(strftime('%s', 'now') AS INTEGER), 0, 0, NULL,
       CAST(strftime('%s', 'now') AS INTEGER)
FROM items;
"#;

/// Stable-provider rating cache and non-secret integration health.
///
/// Cache identity deliberately excludes Jellyfin IDs: when Jellyfin recreates
/// an item, its TMDB/IMDb identity can reuse the same durable result. Secrets
/// are never stored here; `integration_state` contains validation/quota facts
/// only and the API key stays in the operating-system credential vault.
const SCHEMA_V12: &str = r#"
CREATE TABLE IF NOT EXISTS rating_cache (
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
CREATE INDEX IF NOT EXISTS rating_cache_expiry ON rating_cache (expires_at);

CREATE TABLE IF NOT EXISTS integration_state (
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
    use super::{
        Database, SCHEMA_V1, SCHEMA_V2, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6, SCHEMA_V7, SCHEMA_VERSION,
        migrate, migrate_v8, user_version,
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
        let preference_table: i64 = database
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'item_playback_preferences'",
                    [],
                    |row| row.get(0),
                )
            })
            .expect("item playback preference table");
        assert_eq!(preference_table, 1);
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
        let technical_stream_column: i64 = database
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM pragma_table_info('items') WHERE name = 'media_streams'",
                    [],
                    |row| row.get(0),
                )
            })
            .expect("media_streams column");
        assert_eq!(technical_stream_column, 1);
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
        let enrichment_table: i64 = database
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'catalog_enrichment'",
                    [],
                    |row| row.get(0),
                )
            })
            .expect("catalog enrichment table");
        assert_eq!(enrichment_table, 1);
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
                "INSERT INTO items (
                     jellyfin_id, kind, name, year, overview, tmdb_id, image_tags, synced_at,
                     metadata_repair_pending, metadata_repair_attempts
                 ) VALUES
                   ('pending', 'Movie', 'Sparse', NULL, NULL, NULL, '{}', 10, 1, 3),
                   ('complete', 'Movie', 'Rich', 2000, 'Overview', '1',
                    '{\"Primary\":\"poster\"}', 11, 0, 0)",
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

    #[test]
    fn a_v7_database_requeues_sparse_cached_items_without_overwriting_queue_state() {
        let connection = Connection::open_in_memory().expect("open");
        connection.execute_batch(SCHEMA_V1).expect("v1 schema");
        connection.execute_batch(SCHEMA_V2).expect("v2 schema");
        connection.execute_batch(SCHEMA_V4).expect("v4 schema");
        connection.execute_batch(SCHEMA_V5).expect("v5 schema");
        connection.execute_batch(SCHEMA_V6).expect("v6 schema");
        connection.execute_batch(SCHEMA_V7).expect("v7 schema");
        connection
            .execute_batch(
                "INSERT INTO items (
                     jellyfin_id, kind, name, year, tmdb_id, tvdb_id, overview,
                     image_tags, metadata_repair_pending, synced_at
                 ) VALUES
                   ('blob', 'Movie', 'The Blob', 1988, '9599', NULL, NULL, '{}', 0, 10),
                   ('series', 'Series', 'Sparse Series', 2020, NULL, '42', NULL, '{}', 0, 11),
                   ('rich', 'Movie', 'Rich', 2000, '1', NULL, 'Description',
                    '{\"Primary\":\"poster\"}', 0, 12),
                   ('pending', 'Movie', 'Existing Pending', NULL, NULL, NULL, NULL, '{}', 0, 13),
                   ('dormant', 'Movie', 'Existing Dormant', NULL, NULL, NULL, NULL, '{}', 0, 14),
                   ('complete', 'Movie', 'Existing Complete', 2001, '2', NULL, 'Overview',
                    '{\"Primary\":\"art\"}', 0, 15);

                 INSERT INTO metadata_convergence (
                     jellyfin_id, state, missing_quality, quality_score,
                     first_observed_at, last_observed_at, last_progress_at, next_attempt_at,
                     attempt_count, unchanged_count, error_count, successful_unchanged, signature
                 ) VALUES
                   ('pending', 'pending', 125, 2, 101, 102, 103, 104, 5, 6, 7, 8, 'pending-sig'),
                   ('dormant', 'dormant', 99, 3, 201, 202, 203, NULL, 9, 10, 11, 12, 'dormant-sig'),
                   ('complete', 'complete', 0, 7, 301, 302, 303, NULL, 13, 14, 15, 16,
                    'complete-sig');",
            )
            .expect("seed v7 data");
        connection
            .pragma_update(None, "user_version", 7)
            .expect("stamp v7");
        let retained_before = convergence_rows(&connection, "pending,dormant,complete");

        migrate(&connection).expect("migrate");

        assert_eq!(user_version(&connection).expect("version"), SCHEMA_VERSION);
        assert_eq!(
            convergence_rows(&connection, "pending,dormant,complete"),
            retained_before,
            "v8 changed existing retry/progress/dormancy state"
        );
        let backfilled: Vec<(String, i64, i64, i64, i64, i64, String)> = {
            let mut statement = connection
                .prepare(
                    "SELECT jellyfin_id, missing_quality, quality_score,
                            first_observed_at, last_observed_at, last_progress_at, signature
                     FROM metadata_convergence
                     WHERE jellyfin_id IN ('blob', 'series')
                     ORDER BY jellyfin_id",
                )
                .expect("prepare");
            statement
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                })
                .expect("query")
                .collect::<rusqlite::Result<_>>()
                .expect("rows")
        };
        assert_eq!(backfilled.len(), 2);
        for (id, missing, score, first, observed, progress, signature) in &backfilled {
            assert_eq!((*missing, *score), (6, 5), "wrong quality for {id}");
            assert_eq!((*observed, *progress), (*first, *first));
            assert!(signature.starts_with("v8:"));
            let next: i64 = connection
                .query_row(
                    "SELECT next_attempt_at FROM metadata_convergence WHERE jellyfin_id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .expect("next attempt");
            assert!((120..=149).contains(&(next - first)));
        }
        let rich_rows: i64 = connection
            .query_row(
                "SELECT count(*) FROM metadata_convergence WHERE jellyfin_id = 'rich'",
                [],
                |row| row.get(0),
            )
            .expect("rich queue count");
        assert_eq!(rich_rows, 0);

        let all_before_replay = convergence_rows(&connection, "*");
        migrate_v8(&connection).expect("replay v8 backfill");
        assert_eq!(convergence_rows(&connection, "*"), all_before_replay);

        connection
            .pragma_update(None, "foreign_keys", "ON")
            .expect("foreign keys");
        connection
            .execute("DELETE FROM items WHERE jellyfin_id = 'blob'", [])
            .expect("evict item");
        let blob_queue_rows: i64 = connection
            .query_row(
                "SELECT count(*) FROM metadata_convergence WHERE jellyfin_id = 'blob'",
                [],
                |row| row.get(0),
            )
            .expect("blob queue count");
        assert_eq!(blob_queue_rows, 0);
    }

    fn convergence_rows(connection: &Connection, ids: &str) -> Vec<String> {
        let condition = if ids == "*" {
            String::new()
        } else {
            format!("WHERE instr(',{ids},', ',' || jellyfin_id || ',') > 0")
        };
        let sql = format!(
            "SELECT json_array(jellyfin_id, state, missing_quality, quality_score,
                               first_observed_at, last_observed_at, last_progress_at,
                               next_attempt_at, attempt_count, unchanged_count, error_count,
                               successful_unchanged, signature)
             FROM metadata_convergence {condition} ORDER BY jellyfin_id"
        );
        let mut statement = connection.prepare(&sql).expect("prepare queue snapshot");
        statement
            .query_map([], |row| row.get(0))
            .expect("query queue snapshot")
            .collect::<rusqlite::Result<_>>()
            .expect("queue snapshot")
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
    fn v9_through_v11_migrations_preserve_items_and_queue_stream_enrichment() {
        let connection = Connection::open_in_memory().expect("open");
        migrate(&connection).expect("initial schema");
        connection
            .execute(
                "INSERT INTO items (jellyfin_id, kind, name, synced_at)
                 VALUES ('existing', 'Movie', 'Existing film', 0)",
                [],
            )
            .expect("seed existing item");
        connection
            .execute_batch(
                "INSERT INTO meta (key, value) VALUES ('sync.bootstrap_at', '123');
                 INSERT INTO meta (key, value) VALUES ('sync.bootstrap_done', '1');",
            )
            .expect("seed completed bootstrap markers");
        // Recreate the exact v8-to-v9 boundary without replaying unrelated,
        // much older migrations in this focused preservation test.
        connection
            .execute_batch("DROP TABLE item_playback_preferences;")
            .expect("remove v9 table");
        connection
            .pragma_update(None, "user_version", 8)
            .expect("stamp v8");

        migrate(&connection).expect("migrate to v9");

        let name: String = connection
            .query_row(
                "SELECT name FROM items WHERE jellyfin_id = 'existing'",
                [],
                |row| row.get(0),
            )
            .expect("existing item survived");
        assert_eq!(name, "Existing film");
        let preference_tables: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'item_playback_preferences'",
                [],
                |row| row.get(0),
            )
            .expect("preference table");
        assert_eq!(preference_tables, 1);
        let stale_bootstrap_timestamp: i64 = connection
            .query_row(
                "SELECT count(*) FROM meta WHERE key = 'sync.bootstrap_at'",
                [],
                |row| row.get(0),
            )
            .expect("bootstrap timestamp");
        assert_eq!(stale_bootstrap_timestamp, 1);
        let enrichment: (String, i64) = connection
            .query_row(
                "SELECT state, attempt_count FROM catalog_enrichment
                 WHERE jellyfin_id = 'existing'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("existing row queued without replacement");
        assert_eq!(enrichment, ("pending".to_string(), 0));
        assert_eq!(user_version(&connection).expect("version"), SCHEMA_VERSION);
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
