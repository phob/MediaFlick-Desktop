use rusqlite::params;

use super::{Library, StoredCredentials, now_unix};

const CACHE_OWNER_SERVER: &str = "cache.owner_server_id";
const CACHE_OWNER_USER: &str = "cache.owner_user_id";

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
        self.db.with_transaction(|transaction| {
            transaction.execute(
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
            if let (Some(server_id), Some(user_id)) = (
                credentials.server_id.as_deref(),
                credentials.user_id.as_deref(),
            ) {
                save_meta(transaction, CACHE_OWNER_SERVER, server_id)?;
                save_meta(transaction, CACHE_OWNER_USER, user_id)?;
            }
            Ok(())
        })
    }

    pub fn cache_owner(&self) -> Option<(String, String)> {
        self.db
            .with_connection(|connection| {
                let server_id = meta_value(connection, CACHE_OWNER_SERVER)?;
                let user_id = meta_value(connection, CACHE_OWNER_USER)?;
                Ok(server_id.zip(user_id))
            })
            .ok()
            .flatten()
    }

    /// Drops the token but keeps the server URL so the login screen stays
    /// prefilled, and wipes cached metadata that belonged to that account.
    pub fn clear_session(&self, forget_library: bool) -> rusqlite::Result<()> {
        self.db.with_transaction(|transaction| {
            if !forget_library {
                let owner = transaction.query_row(
                    "SELECT server_id, user_id FROM credentials WHERE id = 1",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, Option<String>>(1)?,
                        ))
                    },
                )?;
                if let (Some(server_id), Some(user_id)) = owner {
                    save_meta(transaction, CACHE_OWNER_SERVER, &server_id)?;
                    save_meta(transaction, CACHE_OWNER_USER, &user_id)?;
                }
            }
            transaction.execute(
                "UPDATE credentials SET user_id = NULL, user_name = NULL, token = NULL,
                     updated_at = ?1 WHERE id = 1",
                params![now_unix()],
            )?;
            if forget_library {
                transaction.execute("DELETE FROM items", [])?;
                transaction.execute("DELETE FROM user_data", [])?;
                transaction.execute("DELETE FROM external_profiles", [])?;
                transaction.execute("DELETE FROM legacy_item_playback_preferences", [])?;
                transaction.execute("DELETE FROM franchise_movie_membership", [])?;
                transaction.execute("DELETE FROM meta", [])?;
            }
            Ok(())
        })
    }
}

fn save_meta(connection: &rusqlite::Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn meta_value(connection: &rusqlite::Connection, key: &str) -> rusqlite::Result<Option<String>> {
    use rusqlite::OptionalExtension;

    connection
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use rusqlite::Connection;

    use super::Library;
    use crate::library::test_support::dto;

    static DATABASE_ID: AtomicU64 = AtomicU64::new(1);

    struct TemporaryDatabase(PathBuf);

    impl TemporaryDatabase {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "mediaflick-session-cleanup-{}-{}.db",
                std::process::id(),
                DATABASE_ID.fetch_add(1, Ordering::Relaxed)
            )))
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TemporaryDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let _ = std::fs::remove_file(format!("{}-wal", self.0.display()));
            let _ = std::fs::remove_file(format!("{}-shm", self.0.display()));
        }
    }

    fn authenticated_library(library: &Library) {
        let mut credentials = library.credentials();
        credentials.server_url = Some("http://server:8096".to_string());
        credentials.server_id = Some("server".to_string());
        credentials.user_id = Some("user".to_string());
        credentials.token = Some("token".to_string());
        library
            .save_credentials(&credentials)
            .expect("save credentials");
        library
            .upsert_page(&[dto(r#"{"Id":"m1","Name":"One","Type":"Movie"}"#)])
            .expect("seed cache");
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
        credentials.server_id = Some("server".to_string());
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

    #[test]
    fn cache_ownership_survives_normal_logout_but_not_a_library_forget() {
        let library = Library::open_in_memory().expect("library");
        let mut credentials = library.credentials();
        credentials.server_url = Some("http://server:8096".to_string());
        credentials.server_id = Some("server".to_string());
        credentials.user_id = Some("alice".to_string());
        credentials.token = Some("token".to_string());
        library.save_credentials(&credentials).expect("save");

        library.clear_session(false).expect("normal logout");
        assert_eq!(
            library.cache_owner(),
            Some(("server".to_string(), "alice".to_string()))
        );

        library.clear_session(true).expect("forget library");
        assert_eq!(library.cache_owner(), None);
    }

    #[test]
    fn a_persistent_cleanup_failure_rolls_back_credentials_and_cached_rows() {
        let library = Library::open_in_memory().expect("library");
        authenticated_library(&library);
        library
            .db
            .with_connection(|connection| {
                connection.execute_batch(
                    "CREATE TRIGGER reject_session_cleanup
                     BEFORE UPDATE OF token ON credentials
                     BEGIN SELECT RAISE(ABORT, 'injected cleanup failure'); END;",
                )
            })
            .expect("install failure injection");

        assert!(library.clear_session(true).is_err());
        assert_eq!(library.credentials().token.as_deref(), Some("token"));
        assert_eq!(library.stats().total, 1);
    }

    #[test]
    fn a_busy_database_does_not_claim_the_session_was_cleared() {
        let database = TemporaryDatabase::new();
        let library = Library::open(database.path()).expect("library");
        authenticated_library(&library);
        library
            .db
            .with_connection(|connection| connection.busy_timeout(std::time::Duration::ZERO))
            .expect("disable the busy wait for the failure injection");
        let lock = Connection::open(database.path()).expect("locking connection");
        lock.execute_batch("BEGIN IMMEDIATE")
            .expect("hold the SQLite writer lock");

        assert!(library.clear_session(true).is_err());
        assert_eq!(library.credentials().token.as_deref(), Some("token"));
        assert_eq!(library.stats().total, 1);

        lock.execute_batch("ROLLBACK").expect("release writer lock");
    }

    #[test]
    fn a_read_only_database_does_not_claim_the_session_was_cleared() {
        let library = Library::open_in_memory().expect("library");
        authenticated_library(&library);
        library
            .db
            .with_connection(|connection| connection.pragma_update(None, "query_only", "ON"))
            .expect("make the database read-only");

        assert!(library.clear_session(true).is_err());
        assert_eq!(library.credentials().token.as_deref(), Some("token"));
        assert_eq!(library.stats().total, 1);

        library
            .db
            .with_connection(|connection| connection.pragma_update(None, "query_only", "OFF"))
            .expect("restore database writes");
    }
}
