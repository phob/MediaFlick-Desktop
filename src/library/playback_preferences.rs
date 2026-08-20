use rusqlite::{OptionalExtension, params};

use super::{ItemPlaybackPreference, Library, now_unix};

impl Library {
    /// The source and tracks explicitly saved for one exact Jellyfin item and
    /// the currently signed-in Jellyfin account.
    pub fn playback_preference(
        &self,
        item_id: &str,
    ) -> rusqlite::Result<Option<ItemPlaybackPreference>> {
        let row = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT preference.media_source, preference.audio_track,
                            preference.subtitle_track
                     FROM item_playback_preferences preference
                     JOIN credentials account ON account.id = 1
                     WHERE preference.jellyfin_id = ?1
                       AND preference.jellyfin_server_key =
                           COALESCE(NULLIF(account.server_id, ''), NULLIF(account.server_url, ''))
                       AND preference.jellyfin_user_id = account.user_id",
                    params![item_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()
        })?;
        let Some((source, audio, subtitle)) = row else {
            return Ok(None);
        };
        let Ok(media_source) = serde_json::from_str(&source) else {
            return Ok(None);
        };
        let audio_track = match audio {
            Some(raw) => match serde_json::from_str(&raw) {
                Ok(track) => Some(track),
                Err(_) => return Ok(None),
            },
            None => None,
        };
        let subtitle_track = match subtitle {
            Some(raw) => match serde_json::from_str(&raw) {
                Ok(track) => Some(track),
                Err(_) => return Ok(None),
            },
            None => None,
        };
        Ok(Some(ItemPlaybackPreference {
            media_source,
            audio_track,
            subtitle_track,
        }))
    }

    /// Atomically replaces both selections for an item.
    pub fn save_playback_preference(
        &self,
        item_id: &str,
        preference: &ItemPlaybackPreference,
    ) -> rusqlite::Result<()> {
        let source = serde_json::to_string(&preference.media_source)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let audio = preference
            .audio_track
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let subtitle = preference
            .subtitle_track
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        self.db.with_connection(|connection| {
            let (server_key, user_id) = connection.query_row(
                "SELECT COALESCE(NULLIF(server_id, ''), NULLIF(server_url, '')), user_id
                 FROM credentials
                 WHERE id = 1 AND user_id IS NOT NULL",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?;
            connection.execute(
                "INSERT INTO item_playback_preferences (
                     jellyfin_id, jellyfin_server_key, jellyfin_user_id,
                     media_source, audio_track, subtitle_track, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(jellyfin_id, jellyfin_server_key, jellyfin_user_id) DO UPDATE SET
                     media_source = excluded.media_source,
                     audio_track = excluded.audio_track,
                     subtitle_track = excluded.subtitle_track,
                     updated_at = excluded.updated_at",
                params![
                    item_id,
                    server_key,
                    user_id,
                    source,
                    audio,
                    subtitle,
                    now_unix()
                ],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::jellyfin::api::model::MediaSourceInfo;

    use super::{ItemPlaybackPreference, Library};
    use crate::library::test_support::dto;

    #[test]
    fn item_playback_preferences_survive_reopen_and_are_keyed_to_one_item() {
        let path = std::env::temp_dir().join(format!(
            "mediaflick-track-preference-{}.sqlite",
            crate::app::ids::random_hex(8)
        ));
        let source: MediaSourceInfo = serde_json::from_str(
            r#"{"Id":"source-a","Name":"Main file","Container":"mkv","MediaStreams":[
                    {"Index":1,"Type":"Audio","Language":"jpn","Codec":"dts","Channels":6},
                    {"Index":2,"Type":"Subtitle","Language":"eng","Title":"English SDH",
                     "IsHearingImpaired":true,"IsForced":false}] }"#,
        )
        .expect("media source");
        let preference = ItemPlaybackPreference::capture(
            &source,
            0,
            source.streams_of_type("Audio").next(),
            source.streams_of_type("Subtitle").next(),
        );

        {
            let library = Library::open(&path).expect("create library");
            let mut credentials = library.credentials();
            credentials.server_url = Some("http://server.test".to_string());
            credentials.server_id = Some("server-a".to_string());
            credentials.user_id = Some("user-a".to_string());
            credentials.token = Some("token".to_string());
            library
                .save_credentials(&credentials)
                .expect("save account identity");
            library
                .upsert_page(&[
                    dto(r#"{"Id":"m1","Name":"One","Type":"Movie"}"#),
                    dto(r#"{"Id":"m2","Name":"Two","Type":"Movie"}"#),
                ])
                .expect("seed items");
            library
                .save_playback_preference("m1", &preference)
                .expect("save preference");
            assert_eq!(
                library.playback_preference("m1").expect("load"),
                Some(preference.clone())
            );
            assert_eq!(library.playback_preference("m2").expect("other item"), None);

            credentials.user_id = Some("user-b".to_string());
            library
                .save_credentials(&credentials)
                .expect("switch account");
            assert_eq!(
                library
                    .playback_preference("m1")
                    .expect("other account preference"),
                None
            );
            credentials.user_id = Some("user-a".to_string());
            library
                .save_credentials(&credentials)
                .expect("restore account");
        }

        {
            let library = Library::open(&path).expect("reopen library");
            assert_eq!(
                library.playback_preference("m1").expect("restored"),
                Some(preference)
            );
            assert!(!library.forget("m1").expect("forget item").is_empty());
            assert_eq!(
                library
                    .playback_preference("m1")
                    .expect("preference removed with item"),
                None
            );
        }

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
