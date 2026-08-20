use rusqlite::params;

use super::{Library, SeerrConfig, StoredCredentials, now_unix};

impl Library {
    pub fn credentials(&self) -> StoredCredentials {
        self.db
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT server_url, user_id, user_name, server_id, device_id, token
                     FROM credentials WHERE id = 1",
                    [],
                    |row| {
                        Ok(StoredCredentials {
                            server_url: row.get(0)?,
                            user_id: row.get(1)?,
                            user_name: row.get(2)?,
                            server_id: row.get(3)?,
                            device_id: row.get(4)?,
                            token: row.get(5)?,
                        })
                    },
                )
            })
            .unwrap_or_default()
    }

    pub fn save_credentials(&self, credentials: &StoredCredentials) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "UPDATE credentials SET server_url = ?1, user_id = ?2, user_name = ?3,
                     server_id = ?4, token = ?5, updated_at = ?6 WHERE id = 1",
                params![
                    credentials.server_url,
                    credentials.user_id,
                    credentials.user_name,
                    credentials.server_id,
                    credentials.token,
                    now_unix(),
                ],
            )?;
            Ok(())
        })
    }

    /// Drops the token but keeps the server URL so the login screen stays
    /// prefilled, and wipes cached metadata that belonged to that account.
    pub fn clear_session(&self, forget_library: bool) -> rusqlite::Result<()> {
        self.db.with_transaction(|transaction| {
            transaction.execute(
                "UPDATE credentials SET user_id = NULL, user_name = NULL, token = NULL,
                     updated_at = ?1 WHERE id = 1",
                params![now_unix()],
            )?;
            if forget_library {
                transaction.execute("DELETE FROM items", [])?;
                transaction.execute("DELETE FROM user_data", [])?;
                transaction.execute("DELETE FROM meta", [])?;
            }
            Ok(())
        })
    }

    #[cfg(test)]
    pub fn seerr_config(&self) -> SeerrConfig {
        self.seerr_config_snapshot().0
    }

    /// Reads the link together with the opaque revision used to prevent a
    /// long-running status probe from overwriting a newer link.
    pub(crate) fn seerr_config_snapshot(&self) -> (SeerrConfig, i64) {
        self.try_seerr_config_snapshot()
            .unwrap_or_else(|error| {
                tracing::warn!(
                    target: "library.db",
                    "could not read the Seerr config: {error}"
                );
                None
            })
            .unwrap_or_default()
    }

    /// Unlike [`Self::seerr_config_snapshot`], preserves storage failures so a
    /// live session does not mistake a transient read error for an empty row.
    pub(crate) fn try_seerr_config_snapshot(&self) -> rusqlite::Result<Option<(SeerrConfig, i64)>> {
        match self.db.with_connection(|connection| {
            connection.query_row(
                "SELECT base_url, cookies, user_id, user_name, jellyfin_server_id,
                     jellyfin_user_id, movie_4k_enabled, series_4k_enabled,
                     partial_requests_enabled, updated_at
                 FROM seerr_config WHERE id = 1",
                [],
                |row| {
                    Ok((
                        SeerrConfig {
                            base_url: row.get(0)?,
                            cookies: row.get(1)?,
                            user_id: row.get(2)?,
                            user_name: row.get(3)?,
                            jellyfin_server_id: row.get(4)?,
                            jellyfin_user_id: row.get(5)?,
                            movie_4k_enabled: row.get(6)?,
                            series_4k_enabled: row.get(7)?,
                            partial_requests_enabled: row.get(8)?,
                        },
                        row.get(9)?,
                    ))
                },
            )
        }) {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Upserts rather than updates: unlike `credentials`, this row is only
    /// created when a Seerr instance is first configured.
    #[cfg(test)]
    pub fn save_seerr_config(&self, config: &SeerrConfig) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "INSERT INTO seerr_config (id, base_url, cookies, user_id, user_name,
                     jellyfin_server_id, jellyfin_user_id, movie_4k_enabled,
                     series_4k_enabled, partial_requests_enabled, updated_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                     base_url = excluded.base_url,
                     cookies = excluded.cookies,
                     user_id = excluded.user_id,
                     user_name = excluded.user_name,
                     jellyfin_server_id = excluded.jellyfin_server_id,
                     jellyfin_user_id = excluded.jellyfin_user_id,
                     movie_4k_enabled = excluded.movie_4k_enabled,
                     series_4k_enabled = excluded.series_4k_enabled,
                     partial_requests_enabled = excluded.partial_requests_enabled,
                     updated_at = seerr_config.updated_at + 1",
                params![
                    config.base_url,
                    config.cookies,
                    config.user_id,
                    config.user_name,
                    config.jellyfin_server_id,
                    config.jellyfin_user_id,
                    config.movie_4k_enabled,
                    config.series_4k_enabled,
                    config.partial_requests_enabled,
                    1,
                ],
            )?;
            Ok(())
        })
    }

    /// Saves only if nobody has changed the link since `expected_revision` was
    /// read. `None` means the caller's snapshot is stale and was not written.
    pub(crate) fn save_seerr_config_if_revision(
        &self,
        config: &SeerrConfig,
        expected_revision: i64,
    ) -> rusqlite::Result<Option<i64>> {
        self.db.with_connection(|connection| {
            let changed = connection.execute(
                "INSERT INTO seerr_config (id, base_url, cookies, user_id, user_name,
                     jellyfin_server_id, jellyfin_user_id, movie_4k_enabled,
                     series_4k_enabled, partial_requests_enabled, updated_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)
                 ON CONFLICT(id) DO UPDATE SET
                     base_url = excluded.base_url,
                     cookies = excluded.cookies,
                     user_id = excluded.user_id,
                     user_name = excluded.user_name,
                     jellyfin_server_id = excluded.jellyfin_server_id,
                     jellyfin_user_id = excluded.jellyfin_user_id,
                     movie_4k_enabled = excluded.movie_4k_enabled,
                     series_4k_enabled = excluded.series_4k_enabled,
                     partial_requests_enabled = excluded.partial_requests_enabled,
                     updated_at = seerr_config.updated_at + 1
                 WHERE seerr_config.updated_at = ?10",
                params![
                    config.base_url,
                    config.cookies,
                    config.user_id,
                    config.user_name,
                    config.jellyfin_server_id,
                    config.jellyfin_user_id,
                    config.movie_4k_enabled,
                    config.series_4k_enabled,
                    config.partial_requests_enabled,
                    expected_revision,
                ],
            )?;
            if changed == 0 {
                return Ok(None);
            }
            connection
                .query_row(
                    "SELECT updated_at FROM seerr_config WHERE id = 1",
                    [],
                    |row| row.get(0),
                )
                .map(Some)
        })
    }

    /// Drops the Seerr session and the account binding, keeping the instance
    /// address so re-linking does not mean retyping it.
    #[cfg(test)]
    pub fn clear_seerr_link(&self) -> rusqlite::Result<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "UPDATE seerr_config SET cookies = NULL, user_id = NULL, user_name = NULL,
                     jellyfin_server_id = NULL, jellyfin_user_id = NULL,
                     updated_at = updated_at + 1
                 WHERE id = 1",
                [],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Library, SeerrConfig};
    use crate::library::test_support::dto;

    #[test]
    fn an_unconfigured_library_has_an_empty_seerr_config() {
        let library = Library::open_in_memory().expect("library");
        assert_eq!(library.seerr_config(), SeerrConfig::default());
    }

    #[test]
    fn the_seerr_config_round_trips_through_its_single_row() {
        let library = Library::open_in_memory().expect("library");
        let config = SeerrConfig {
            base_url: Some("https://seerr.test".to_string()),
            cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
            user_id: Some(7),
            user_name: Some("pho".to_string()),
            jellyfin_server_id: Some("srv".to_string()),
            jellyfin_user_id: Some("uid".to_string()),
            movie_4k_enabled: true,
            series_4k_enabled: false,
            partial_requests_enabled: true,
        };
        library.save_seerr_config(&config).expect("save");
        assert_eq!(library.seerr_config(), config);

        let moved = SeerrConfig {
            base_url: Some("https://other.test".to_string()),
            ..config
        };
        library.save_seerr_config(&moved).expect("re-save");
        assert_eq!(library.seerr_config(), moved);
    }

    #[test]
    fn a_stale_seerr_revision_cannot_replace_a_newer_config() {
        let library = Library::open_in_memory().expect("library");
        let original = SeerrConfig {
            base_url: Some("https://old.test".to_string()),
            ..SeerrConfig::default()
        };
        library.save_seerr_config(&original).expect("original");
        let (_, original_revision) = library.seerr_config_snapshot();

        let newer = SeerrConfig {
            base_url: Some("https://new.test".to_string()),
            ..SeerrConfig::default()
        };
        library.save_seerr_config(&newer).expect("newer");

        let stale = SeerrConfig {
            base_url: Some("https://stale.test".to_string()),
            ..SeerrConfig::default()
        };
        assert_eq!(
            library
                .save_seerr_config_if_revision(&stale, original_revision)
                .expect("conditional save"),
            None
        );
        assert_eq!(library.seerr_config(), newer);
    }

    #[test]
    fn clearing_the_seerr_link_keeps_the_instance_address() {
        let library = Library::open_in_memory().expect("library");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some("https://seerr.test".to_string()),
                cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                user_name: Some("pho".to_string()),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                partial_requests_enabled: true,
                ..SeerrConfig::default()
            })
            .expect("save");
        library.clear_seerr_link().expect("clear");

        let config = library.seerr_config();
        assert_eq!(config.base_url.as_deref(), Some("https://seerr.test"));
        assert!(config.partial_requests_enabled);
        assert_eq!(config.cookies, None);
        assert_eq!(config.user_id, None);
        assert_eq!(config.jellyfin_user_id, None);
    }

    #[test]
    fn credentials_round_trip_and_keep_the_device_id_across_logout() {
        let library = Library::open_in_memory().expect("library");
        let mut credentials = library.credentials();
        let device_id = credentials.device_id.clone();
        assert!(!device_id.is_empty());
        assert!(!credentials.is_authenticated());

        credentials.server_url = Some("http://server:8096".to_string());
        credentials.user_id = Some("uid".to_string());
        credentials.user_name = Some("pho".to_string());
        credentials.token = Some("tok".to_string());
        library.save_credentials(&credentials).expect("save");

        let loaded = library.credentials();
        assert!(loaded.is_authenticated());
        assert_eq!(loaded.device_id, device_id);

        library
            .ingest_page(&[dto(
                r#"{"Id":"sparse","Name":"Sparse","Type":"Movie","ProviderIds":{"Tmdb":"1"}}"#,
            )])
            .expect("cache a row");
        assert_eq!(library.stats().total, 1);

        library.clear_session(true).expect("logout");
        let after = library.credentials();
        assert!(!after.is_authenticated());
        assert_eq!(after.device_id, device_id);
        assert_eq!(after.server_url.as_deref(), Some("http://server:8096"));
        assert_eq!(after.token, None);
        assert_eq!(library.stats().total, 0);
    }
}
