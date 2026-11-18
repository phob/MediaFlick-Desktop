//! Opening `library.db`. Released schemas keep every row, development schemas
//! are recreated with the session, and a newer schema is refused.
//!
//! Expected values come from the frozen fixtures in
//! `fixtures/library-db/released/`, which hold what the released MediaFlick
//! 0.2.0 AppImage wrote (sanitized by `scripts/fixtures/freeze_release.py`), or
//! from the invariant a test names. Failure modes covered here, written down
//! before the code:
//!
//! - a released schema goes down the development drop-and-recreate path;
//! - a migration leaves a different structure than a fresh install;
//! - a frozen released schema is edited;
//! - a migration loses or changes a row of a released table: the token,
//!   device id, cache owner, staged imports, catalog or provider caches;
//! - a migration that rebuilds a parent table cascades its children away;
//! - a failing step leaves a half-migrated file or a raised `user_version`;
//! - a migration commits a broken foreign key;
//! - a newer schema is opened, or written to;
//! - the development recreation fails after its drop and loses the session;
//! - an unreadable development session is treated as no session.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::types::Value;
use rusqlite::{Connection, Transaction};
use sha2::{Digest, Sha256};

use super::OpenError;
use super::configure;
use super::migrate::{
    FIRST_RELEASED_SCHEMA, MIGRATIONS, Migration, SCHEMA_VERSION, migrate, migrate_with,
    user_version,
};

/// The SHA-256 of each released schema file, added when the schema's first
/// prerelease was published. A released schema never changes; a new release
/// adds a row, and `scripts/prepare-release.py` refuses to publish a schema
/// that has none.
const RELEASED_SCHEMAS: &[(i32, &str)] = &[(
    23,
    "c163dffd97f343bc1e6ec01611f8d1b15f04f15bf569da9988e997ab7cf82853",
)];

/// The synthetic sign-in token `rows-23.sql` holds.
const FIXTURE_TOKEN: &str = "fixture-token-0f1e2d3c4b5a69788796a5b4c3d2e1f0";

fn released_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/library-db/released")
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A released schema's DDL, checked against its frozen hash.
fn released_schema(version: i32) -> String {
    let text = std::fs::read_to_string(released_dir().join(format!("schema-{version}.sql")))
        .expect("released schema file");
    let frozen = RELEASED_SCHEMAS
        .iter()
        .find(|(released, _)| *released == version)
        .map(|(_, hash)| *hash)
        .expect("released schema hash");
    assert_eq!(
        sha256(text.as_bytes()),
        frozen,
        "fixtures/library-db/released/schema-{version}.sql changed; a released schema is frozen"
    );
    text
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mediaflick-db-test-{}",
            crate::app::ids::random_hex(8)
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn database(&self) -> PathBuf {
        self.0.join("library.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A connection configured as the app configures its own.
fn open(path: &Path) -> Connection {
    let connection = Connection::open(path).expect("open library.db");
    configure(&connection).expect("configure");
    connection
}

/// The database a released version left: its frozen schema and, when the
/// release has them, its frozen rows, stamped with its `user_version`.
fn released_database(path: &Path, version: i32) -> Connection {
    let connection = open(path);
    connection
        .execute_batch(&released_schema(version))
        .expect("released schema runs");
    let rows = released_dir().join(format!("rows-{version}.sql"));
    if rows.exists() {
        connection
            .execute_batch(&std::fs::read_to_string(rows).expect("released rows"))
            .expect("released rows load");
    }
    connection
        .pragma_update(None, "user_version", version)
        .expect("stamp the released version");
    connection
}

fn fresh_database() -> Connection {
    let connection = Connection::open_in_memory().expect("open");
    configure(&connection).expect("configure");
    migrate(&connection).expect("fresh schema");
    connection
}

fn strings(connection: &Connection, sql: &str) -> Vec<String> {
    let mut statement = connection.prepare(sql).expect("prepare");
    let count = statement.column_count();
    statement
        .query_map([], |row| {
            (0..count)
                .map(|index| row.get::<_, Value>(index).map(|value| format!("{value:?}")))
                .collect::<rusqlite::Result<Vec<_>>>()
                .map(|values| values.join(" | "))
        })
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("rows")
}

/// Tables SQLite owns: its statistics and the shadow tables of each virtual
/// table, which it creates and fills itself.
fn sqlite_owned(connection: &Connection) -> impl Fn(&str) -> bool + use<> {
    let virtual_tables = names(
        connection,
        "SELECT name FROM sqlite_master WHERE sql LIKE 'CREATE VIRTUAL TABLE%'",
    );
    move |name: &str| {
        name.starts_with("sqlite_")
            || virtual_tables.iter().any(|table| {
                name.strip_prefix(table.as_str())
                    .is_some_and(|rest| rest.starts_with('_'))
            })
    }
}

fn names(connection: &Connection, sql: &str) -> Vec<String> {
    let mut statement = connection.prepare(sql).expect("prepare");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("names")
}

/// The structure a schema defines: every object's normalized SQL, and each
/// table's columns and foreign keys as SQLite reports them.
fn shape(connection: &Connection) -> BTreeMap<String, Vec<String>> {
    let owned = sqlite_owned(connection);
    let normalize = |sql: &str| {
        let collapsed = sql.split_whitespace().collect::<Vec<_>>().join(" ");
        ["(", ")", ","].iter().fold(collapsed, |text, mark| {
            text.replace(&format!(" {mark}"), mark)
                .replace(&format!("{mark} "), mark)
        })
    };
    let mut shape = BTreeMap::new();
    let mut statement = connection
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_master WHERE sql IS NOT NULL")
        .expect("prepare");
    let objects = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("objects");
    for (kind, name, table, sql) in objects {
        if owned(&name) {
            continue;
        }
        shape.insert(
            format!("{kind} {name}"),
            vec![format!("on {table}"), normalize(&sql)],
        );
        if kind == "table" {
            shape.insert(
                format!("columns {name}"),
                strings(
                    connection,
                    &format!(
                        "SELECT cid, name, type, \"notnull\", dflt_value, pk, hidden \
                         FROM pragma_table_xinfo('{name}')"
                    ),
                ),
            );
            shape.insert(
                format!("foreign keys {name}"),
                strings(
                    connection,
                    &format!("SELECT * FROM pragma_foreign_key_list('{name}')"),
                ),
            );
        }
    }
    shape
}

/// The columns each table had in the released schema.
fn columns(connection: &Connection) -> BTreeMap<String, Vec<String>> {
    let owned = sqlite_owned(connection);
    names(
        connection,
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )
    .into_iter()
    .filter(|table| !owned(table))
    .map(|table| {
        let columns = names(
            connection,
            &format!("SELECT name FROM pragma_table_xinfo('{table}') WHERE hidden = 0"),
        );
        (table, columns)
    })
    .collect()
}

/// Every row of every table, projected onto the given columns, in a stable
/// order. A table or column that no longer exists fails the query.
fn rows(
    connection: &Connection,
    columns: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    columns
        .iter()
        .map(|(table, columns)| {
            let list = columns
                .iter()
                .map(|column| format!("\"{column}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let mut found = strings(connection, &format!("SELECT {list} FROM \"{table}\""));
            found.sort();
            (table.clone(), found)
        })
        .collect()
}

fn count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |row| {
            row.get(0)
        })
        .expect("count")
}

fn foreign_keys_on(connection: &Connection) -> bool {
    connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
        .expect("foreign_keys")
        == 1
}

#[test]
fn every_released_schema_has_a_frozen_hash() {
    let mut files = std::fs::read_dir(released_dir())
        .expect("released fixtures")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter_map(|name| {
            name.strip_prefix("schema-")?
                .strip_suffix(".sql")?
                .parse::<i32>()
                .ok()
        })
        .collect::<Vec<_>>();
    files.sort_unstable();
    let frozen = RELEASED_SCHEMAS
        .iter()
        .map(|(version, _)| *version)
        .collect::<Vec<_>>();
    assert_eq!(
        files, frozen,
        "each fixtures/library-db/released/schema-<N>.sql needs a RELEASED_SCHEMAS hash, and each hash a file"
    );
    for (version, _) in RELEASED_SCHEMAS {
        released_schema(*version);
    }
    assert_eq!(frozen.first(), Some(&FIRST_RELEASED_SCHEMA));
    assert!(
        frozen
            .last()
            .is_some_and(|newest| *newest <= SCHEMA_VERSION),
        "a released schema is never newer than the one this build creates"
    );
}

#[test]
fn every_released_schema_opens_at_the_structure_of_a_fresh_install() {
    let fresh = shape(&fresh_database());
    for (version, _) in RELEASED_SCHEMAS {
        let scratch = Scratch::new();
        let connection = released_database(&scratch.database(), *version);
        migrate(&connection).expect("open the released database");
        assert_eq!(user_version(&connection).expect("version"), SCHEMA_VERSION);
        let migrated = shape(&connection);
        for (object, expected) in &fresh {
            assert_eq!(
                migrated.get(object),
                Some(expected),
                "schema {version} opened at {SCHEMA_VERSION} differs from a fresh install in {object}; \
                 a schema change bumps SCHEMA_VERSION and adds a migration to MIGRATIONS"
            );
        }
        assert_eq!(
            migrated.keys().collect::<Vec<_>>(),
            fresh.keys().collect::<Vec<_>>(),
            "schema {version} opened at {SCHEMA_VERSION} has other objects than a fresh install"
        );
    }
}

/// While `MIGRATIONS` is empty this opens schema 23 as it is; the next bump
/// makes it the real upgrade. `released_rows_survive_a_migration_to_the_next_schema`
/// covers the upgrade path until then.
#[test]
fn released_rows_survive_opening_value_for_value() {
    let scratch = Scratch::new();
    let connection = released_database(&scratch.database(), FIRST_RELEASED_SCHEMA);
    let released_columns = columns(&connection);
    let before = rows(&connection, &released_columns);
    // The comparison below is only as strong as the fixture: every table the
    // released schema has must hold a row, the token included.
    for (table, found) in &before {
        assert!(!found.is_empty(), "rows-23.sql leaves {table} empty");
    }
    let token: String = connection
        .query_row("SELECT token FROM credentials WHERE id = 1", [], |row| {
            row.get(0)
        })
        .expect("token");
    assert_eq!(token, FIXTURE_TOKEN);

    migrate(&connection).expect("open the released database");

    assert_eq!(user_version(&connection).expect("version"), SCHEMA_VERSION);
    let after = rows(&connection, &released_columns);
    for (table, expected) in &before {
        assert_eq!(
            after.get(table),
            Some(expected),
            "{table} lost or changed rows opening schema {FIRST_RELEASED_SCHEMA} at {SCHEMA_VERSION}"
        );
    }
    drop(connection);
    let reopened = open(&scratch.database());
    assert_eq!(rows(&reopened, &released_columns), before);
}

#[test]
fn released_rows_survive_a_migration_to_the_next_schema() {
    // With MIGRATIONS still empty, this is the path the next schema bump takes.
    let (_scratch, result, reopened) = upgrade_released(&[add_a_column]);
    result.expect("the migration commits");
    assert_eq!(
        user_version(&reopened).expect("version"),
        FIRST_RELEASED_SCHEMA + 1
    );
    let expected = Scratch::new();
    let original = released_database(&expected.database(), FIRST_RELEASED_SCHEMA);
    let released_columns = columns(&original);
    let before = rows(&original, &released_columns);
    let after = rows(&reopened, &released_columns);
    for (table, expected) in &before {
        assert_eq!(
            after.get(table),
            Some(expected),
            "{table} lost or changed rows in a migration from schema {FIRST_RELEASED_SCHEMA}"
        );
    }
    assert!(
        names(&reopened, "SELECT name FROM pragma_table_xinfo('meta')")
            .contains(&"note".to_owned()),
        "the migration's own change is there"
    );
}

fn add_a_column(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch("ALTER TABLE meta ADD COLUMN note TEXT;")
}

fn delete_the_session(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch("DELETE FROM credentials;")
}

fn fail(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch("SELECT * FROM a_table_no_schema_has;")
}

/// SQLite's documented way to change a table's definition: create the new
/// table, copy, drop the old one, rename.
fn rebuild_collection_snapshots(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE collection_snapshots_new (
             server_id TEXT NOT NULL, user_id TEXT NOT NULL, profile_id TEXT NOT NULL,
             revision TEXT NOT NULL, committed_at INTEGER NOT NULL, item_count INTEGER NOT NULL,
             PRIMARY KEY (server_id, user_id, profile_id, revision)
         );
         INSERT INTO collection_snapshots_new SELECT * FROM collection_snapshots;
         DROP TABLE collection_snapshots;
         ALTER TABLE collection_snapshots_new RENAME TO collection_snapshots;",
    )
}

fn orphan_a_snapshot_item(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "INSERT INTO collection_snapshot_items
             (server_id, user_id, profile_id, revision, media_type, tmdb_id, title, overview,
              source_order)
         VALUES ('no-server', 'no-user', 'no-profile', 'no-revision', 'movie', 1, 'Orphan', '', 0);",
    )
}

/// Opens the released schema 23 with `steps` as the migrations, then reopens
/// the file the way the next start would.
fn upgrade_released(steps: &[Migration]) -> (Scratch, Result<(), OpenError>, Connection) {
    let scratch = Scratch::new();
    let connection = released_database(&scratch.database(), FIRST_RELEASED_SCHEMA);
    let result = migrate_with(&connection, steps);
    assert!(
        foreign_keys_on(&connection),
        "the connection gets its foreign keys back after a migration"
    );
    drop(connection);
    let reopened = open(&scratch.database());
    (scratch, result, reopened)
}

#[test]
fn a_failed_migration_leaves_the_released_database_as_it_was() {
    let scratch = Scratch::new();
    let original = released_database(&scratch.database(), FIRST_RELEASED_SCHEMA);
    let released_columns = columns(&original);
    let before = rows(&original, &released_columns);
    let released_shape = shape(&original);
    drop(original);

    for steps in [
        &[add_a_column, fail][..],
        &[delete_the_session, add_a_column, fail][..],
    ] {
        let (_scratch, result, reopened) = upgrade_released(steps);
        assert!(
            matches!(
                result,
                Err(OpenError::Migration {
                    from: FIRST_RELEASED_SCHEMA,
                    ..
                })
            ),
            "a failing step fails the migration: {result:?}"
        );
        assert_eq!(
            user_version(&reopened).expect("version"),
            FIRST_RELEASED_SCHEMA
        );
        assert_eq!(
            shape(&reopened),
            released_shape,
            "no step of a failed migration stays"
        );
        assert_eq!(rows(&reopened, &released_columns), before);
    }
}

#[test]
fn a_migration_that_rebuilds_a_parent_table_keeps_its_children() {
    let scratch = Scratch::new();
    let original = released_database(&scratch.database(), FIRST_RELEASED_SCHEMA);
    let released_columns = columns(&original);
    let before = rows(&original, &released_columns);
    drop(original);
    assert!(!before["collection_snapshot_items"].is_empty());

    let (_scratch, result, reopened) = upgrade_released(&[rebuild_collection_snapshots]);

    result.expect("the rebuild commits");
    assert_eq!(
        user_version(&reopened).expect("version"),
        FIRST_RELEASED_SCHEMA + 1
    );
    let after = rows(&reopened, &released_columns);
    for table in ["collection_snapshots", "collection_snapshot_items"] {
        assert_eq!(after[table], before[table], "{table} kept its rows");
    }
}

#[test]
fn a_migration_that_breaks_a_foreign_key_is_rolled_back() {
    let scratch = Scratch::new();
    let original = released_database(&scratch.database(), FIRST_RELEASED_SCHEMA);
    let released_columns = columns(&original);
    let before = rows(&original, &released_columns);
    drop(original);

    let (_scratch, result, reopened) = upgrade_released(&[orphan_a_snapshot_item]);

    assert!(
        matches!(
            result,
            Err(OpenError::Migration {
                from: FIRST_RELEASED_SCHEMA,
                ..
            })
        ),
        "a broken reference fails the migration: {result:?}"
    );
    assert!(
        format!("{result:?}").contains("broken foreign keys: collection_snapshot_items"),
        "the integrity check, not another statement, failed it: {result:?}"
    );
    assert_eq!(
        user_version(&reopened).expect("version"),
        FIRST_RELEASED_SCHEMA
    );
    assert_eq!(rows(&reopened, &released_columns), before);
    assert_eq!(
        count(&reopened, "collection_snapshot_items"),
        i64::try_from(before["collection_snapshot_items"].len()).expect("count")
    );
}

#[test]
fn the_migration_list_covers_every_released_schema() {
    // Also a compile-time assertion in migrate.rs; this names the rule.
    assert_eq!(
        i32::try_from(MIGRATIONS.len()).expect("count"),
        SCHEMA_VERSION - FIRST_RELEASED_SCHEMA
    );
}

#[test]
fn a_newer_database_is_refused_and_left_untouched() {
    let scratch = Scratch::new();
    let connection = released_database(&scratch.database(), FIRST_RELEASED_SCHEMA);
    connection
        .execute_batch("CREATE TABLE future_table (id INTEGER PRIMARY KEY);")
        .expect("future schema");
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .expect("stamp future");
    let released_columns = columns(&connection);
    let before = rows(&connection, &released_columns);
    let future_shape = shape(&connection);

    let result = migrate(&connection);

    assert!(
        matches!(result, Err(OpenError::NewerSchema { found }) if found == SCHEMA_VERSION + 1),
        "a newer schema is refused: {result:?}"
    );
    drop(connection);
    let reopened = open(&scratch.database());
    assert_eq!(
        user_version(&reopened).expect("version"),
        SCHEMA_VERSION + 1
    );
    assert_eq!(shape(&reopened), future_shape);
    assert_eq!(rows(&reopened, &released_columns), before);
}

#[test]
fn a_newer_database_in_rollback_mode_keeps_its_bytes() {
    // The app's own files are in WAL mode, but a refused file is not this
    // build's to change in any way, its journal mode included.
    let scratch = Scratch::new();
    {
        let connection = Connection::open(scratch.database()).expect("open");
        connection
            .execute_batch("PRAGMA journal_mode = DELETE; CREATE TABLE future_table (id INTEGER);")
            .expect("future schema");
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .expect("stamp future");
    }
    let before = std::fs::read(scratch.database()).expect("bytes");

    let result = super::Database::open(&scratch.database());

    assert!(
        matches!(result, Err(OpenError::NewerSchema { found }) if found == SCHEMA_VERSION + 1),
        "a newer schema is refused: {:?}",
        result.err()
    );
    assert_eq!(
        std::fs::read(scratch.database()).expect("bytes"),
        before,
        "refusing the file changed it"
    );
}

/// A schema-13 development database whose staged playback choice holds a
/// media source the current schema's CHECK rejects. Restoring it fails after
/// the old tables were dropped: where a crash would leave the file.
fn development_database_failing_after_its_drop(path: &Path) -> Connection {
    let connection = open(path);
    connection
        .execute_batch(
            "CREATE TABLE credentials (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 server_url TEXT, user_id TEXT, user_name TEXT, server_id TEXT,
                 device_id TEXT NOT NULL, token TEXT, updated_at INTEGER NOT NULL
             );
             INSERT INTO credentials
                 (id, server_url, user_id, user_name, server_id, device_id, token, updated_at)
             VALUES (1, 'http://server:8096', 'user-a', 'Alice', 'server-a', 'dev', 'tok', 7);
             CREATE TABLE items (id INTEGER PRIMARY KEY, jellyfin_id TEXT NOT NULL UNIQUE);
             INSERT INTO items (jellyfin_id) VALUES ('movie-1');
             CREATE TABLE external_profiles (
                 id TEXT PRIMARY KEY, provider TEXT NOT NULL, profile_key TEXT NOT NULL,
                 display_name TEXT NOT NULL, canonical_url TEXT NOT NULL,
                 jellyfin_server_id TEXT NOT NULL, jellyfin_user_id TEXT NOT NULL,
                 enabled INTEGER NOT NULL, verification_status TEXT NOT NULL,
                 created_at INTEGER NOT NULL, last_checked_at INTEGER
             );
             CREATE TABLE item_playback_preferences (
                 jellyfin_id TEXT NOT NULL, jellyfin_server_key TEXT NOT NULL,
                 jellyfin_user_id TEXT NOT NULL, media_source TEXT NOT NULL, audio_track TEXT,
                 subtitle_track TEXT, updated_at INTEGER NOT NULL,
                 PRIMARY KEY (jellyfin_id, jellyfin_server_key, jellyfin_user_id)
             );
             INSERT INTO item_playback_preferences
             VALUES ('movie-1', 'server-a', 'user-a', 'not json', NULL, NULL, 12);",
        )
        .expect("schema 13 fixture");
    connection
        .pragma_update(None, "user_version", 13)
        .expect("stamp schema 13");
    connection
}

#[test]
fn a_failed_development_recreation_keeps_the_old_database() {
    let scratch = Scratch::new();
    let connection = development_database_failing_after_its_drop(&scratch.database());
    let old_columns = columns(&connection);
    let before = rows(&connection, &old_columns);

    let result = migrate(&connection);

    assert!(
        format!("{result:?}").contains("CHECK constraint failed"),
        "restoring the staged choice after the drop failed it: {result:?}"
    );
    assert!(foreign_keys_on(&connection));
    drop(connection);
    let reopened = open(&scratch.database());
    assert_eq!(user_version(&reopened).expect("version"), 13);
    assert_eq!(
        columns(&reopened),
        old_columns,
        "every old table is still there"
    );
    assert_eq!(
        rows(&reopened, &old_columns),
        before,
        "the session and every old table are still there"
    );
}

#[test]
fn an_unreadable_development_session_stops_the_recreation() {
    let scratch = Scratch::new();
    let connection = open(&scratch.database());
    connection
        .execute_batch(
            "CREATE TABLE credentials (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 server_url TEXT, user_id TEXT, user_name TEXT, server_id TEXT,
                 device_id TEXT NOT NULL, token TEXT, updated_at INTEGER NOT NULL
             );
             INSERT INTO credentials (id, server_url, user_id, device_id, token, updated_at)
             VALUES (1, 'http://server:8096', 'uid', 'dev', 'tok', 'not a time');",
        )
        .expect("old schema");
    connection
        .pragma_update(None, "user_version", 12)
        .expect("stamp v12");
    let old_columns = columns(&connection);
    let before = rows(&connection, &old_columns);

    let result = migrate(&connection);

    assert!(
        result.is_err(),
        "a session that cannot be read is not the same as no session: {result:?}"
    );
    drop(connection);
    let reopened = open(&scratch.database());
    assert_eq!(user_version(&reopened).expect("version"), 12);
    assert_eq!(rows(&reopened, &old_columns), before);
}

#[test]
fn an_older_database_is_recreated_rather_than_migrated() {
    let connection = Connection::open_in_memory().expect("open");
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
