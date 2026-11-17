//! Development schemas, below the first released one, are recreated rather
//! than migrated: they exist only on developer machines and in the verify
//! harness, which seeds a schema-1 database holding only a session.

use rusqlite::{Connection, OptionalExtension, Transaction};

/// Recreates a development database at the current schema, inside the
/// caller's transaction, keeping the signed-in session (it performs the
/// resync) and, from schema 13, the account data the JSON stores import
/// later in the same start. Every historical schema stored the session under
/// the same single-row shape.
pub(super) fn recreate(transaction: &Transaction<'_>, version: i32) -> rusqlite::Result<()> {
    let session = saved_session(transaction)?;
    let legacy_account_data = if version == 13 {
        Some(saved_legacy_account_data(transaction)?)
    } else {
        None
    };
    drop_everything(transaction)?;
    super::migrate::create(transaction)?;
    if let Some(session) = session {
        restore_session(transaction, &session)?;
    }
    if let Some(legacy_account_data) = legacy_account_data {
        restore_legacy_account_data(transaction, &legacy_account_data)?;
    }
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

/// The stored session, or `None` when there is no row. Any other error stops
/// the recreation: a session that cannot be read is not an absent one.
fn saved_session(connection: &Connection) -> rusqlite::Result<Option<SavedSession>> {
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
        .optional()
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
/// schema starts from nothing. The virtual table goes first so its FTS shadow
/// tables disappear with it instead of erroring on a direct drop. The caller
/// turned foreign keys off before its transaction began, so no drop cascades.
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
    for table in &tables {
        connection.execute_batch(&format!("DROP TABLE IF EXISTS \"{table}\";"))?;
    }
    Ok(())
}
