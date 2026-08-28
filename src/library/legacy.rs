use rusqlite::types::Type;

use crate::integrations::letterboxd::ExternalProfile;

use super::{ItemPlaybackPreference, Library};

pub(crate) struct LegacyPlaybackPreference {
    pub server_key: String,
    pub user_id: String,
    pub item_id: String,
    pub preference: ItemPlaybackPreference,
}

impl Library {
    pub(crate) fn legacy_external_profiles(&self) -> rusqlite::Result<Vec<ExternalProfile>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, provider, profile_key, display_name, canonical_url,
                        enabled, verification_status, created_at, last_checked_at,
                        jellyfin_server_id, jellyfin_user_id
                 FROM external_profiles",
            )?;
            statement
                .query_map([], |row| {
                    Ok(ExternalProfile {
                        id: row.get(0)?,
                        provider: row.get(1)?,
                        profile_key: row.get(2)?,
                        display_name: row.get(3)?,
                        canonical_url: row.get(4)?,
                        enabled: row.get(5)?,
                        verification_status: row.get(6)?,
                        created_at: row.get(7)?,
                        last_checked_at: row.get(8)?,
                        jellyfin_server_id: row.get(9)?,
                        jellyfin_user_id: row.get(10)?,
                    })
                })?
                .collect()
        })
    }

    pub(crate) fn legacy_playback_preferences(
        &self,
    ) -> rusqlite::Result<Vec<LegacyPlaybackPreference>> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT jellyfin_id, jellyfin_server_key, jellyfin_user_id,
                        media_source, audio_track, subtitle_track
                 FROM legacy_item_playback_preferences",
            )?;
            statement
                .query_map([], |row| {
                    let source: String = row.get(3)?;
                    let audio: Option<String> = row.get(4)?;
                    let subtitle: Option<String> = row.get(5)?;
                    let media_source = decode_json(&source)?;
                    let audio_track = audio.as_deref().map(decode_json).transpose()?;
                    let subtitle_track = subtitle.as_deref().map(decode_json).transpose()?;
                    Ok(LegacyPlaybackPreference {
                        item_id: row.get(0)?,
                        server_key: row.get(1)?,
                        user_id: row.get(2)?,
                        preference: ItemPlaybackPreference {
                            media_source,
                            audio_track,
                            subtitle_track,
                        },
                    })
                })?
                .collect()
        })
    }

    pub(crate) fn finish_legacy_account_import(&self) -> rusqlite::Result<()> {
        self.db.with_transaction(|transaction| {
            transaction.execute("DELETE FROM external_profiles", [])?;
            transaction.execute("DELETE FROM legacy_item_playback_preferences", [])?;
            Ok(())
        })
    }
}

fn decode_json<T: serde::de::DeserializeOwned>(raw: &str) -> rusqlite::Result<T> {
    serde_json::from_str(raw)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error)))
}
