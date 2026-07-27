//! Seerr — the project formerly called Jellyseerr — as a request backend.
//!
//! MediaFlick is a *client* to Seerr, not a second \*arr orchestrator: Seerr owns
//! the quality profiles, root folders, approval rules and quotas. The credential
//! held here is the user's own session cookie, never an instance-wide API key,
//! which by Seerr's own documentation grants administrator access.
//!
//! The session is bound to one Jellyfin account. Every acquisition of a client
//! re-checks that binding, so signing out — or signing in as somebody else —
//! cannot leave user A's Seerr cookie serving user B.

pub mod api;
pub mod headless;

use std::sync::{Arc, RwLock};

use serde_json::{Value, json};

use crate::library::{Library, SeerrConfig};
use crate::preferences::normalize_server_url;

use api::client::{SeerrClient, SessionCookies};
use api::error::SeerrError;
use api::model::{
    Capabilities, MEDIA_SERVER_JELLYFIN, PublicSettings, SeerrUser, StatusInfo, UserQuota,
};

#[derive(Debug, Clone, Default)]
struct SeerrState {
    /// Opaque `seerr_config.updated_at` revision used for optimistic writes.
    revision: i64,
    base_url: Option<String>,
    cookies: SessionCookies,
    user_id: Option<i64>,
    user_name: Option<String>,
    jellyfin_server_id: Option<String>,
    jellyfin_user_id: Option<String>,
    movie_4k_enabled: bool,
    series_4k_enabled: bool,
    partial_requests_enabled: bool,
    /// Set once `/auth/me` itself has answered 401; the UI must re-link.
    expired: bool,
}

impl SeerrState {
    fn from_config(config: SeerrConfig, revision: i64) -> Self {
        Self {
            revision,
            base_url: config.base_url,
            cookies: config
                .cookies
                .as_deref()
                .map(SessionCookies::from_json)
                .unwrap_or_default(),
            user_id: config.user_id,
            user_name: config.user_name,
            jellyfin_server_id: config.jellyfin_server_id,
            jellyfin_user_id: config.jellyfin_user_id,
            movie_4k_enabled: config.movie_4k_enabled,
            series_4k_enabled: config.series_4k_enabled,
            partial_requests_enabled: config.partial_requests_enabled,
            expired: false,
        }
    }

    fn to_config(&self) -> SeerrConfig {
        SeerrConfig {
            base_url: self.base_url.clone(),
            cookies: (!self.cookies.is_empty()).then(|| self.cookies.to_json()),
            user_id: self.user_id,
            user_name: self.user_name.clone(),
            jellyfin_server_id: self.jellyfin_server_id.clone(),
            jellyfin_user_id: self.jellyfin_user_id.clone(),
            movie_4k_enabled: self.movie_4k_enabled,
            series_4k_enabled: self.series_4k_enabled,
            partial_requests_enabled: self.partial_requests_enabled,
        }
    }

    fn is_linked(&self) -> bool {
        !self.expired && self.cookies.has_session() && self.base_url.is_some()
    }
}

pub struct SeerrSession {
    library: Arc<Library>,
    state: RwLock<SeerrState>,
}

impl SeerrSession {
    pub fn restore(library: Arc<Library>) -> Self {
        let (config, revision) = library.seerr_config_snapshot();
        let state = SeerrState::from_config(config, revision);
        Self {
            library,
            state: RwLock::new(state),
        }
    }

    fn read(&self) -> SeerrState {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// A client carrying the stored session, or the reason one cannot be built.
    pub fn client(&self) -> Result<SeerrClient, SeerrError> {
        self.revalidate();
        let state = self.read();
        if state.expired {
            return Err(SeerrError::Unauthorized);
        }
        let base_url = state.base_url.ok_or(SeerrError::NotConfigured)?;
        Ok(SeerrClient::new(&base_url, state.cookies))
    }

    /// Probes an instance and remembers it as the link target.
    ///
    /// `/settings/public` rather than `/status`, because an *uninitialized*
    /// Seerr treats `POST /auth/jellyfin` as its setup wizard — it would make
    /// whoever logs in the instance owner. Requiring `initialized: true` here
    /// is what keeps a client from silently becoming that wizard. The probe is
    /// also where a CSRF-protected instance hands out the cookie pair the
    /// first POST will need.
    pub fn connect(&self, server_url: &str) -> Result<Value, SeerrError> {
        let normalized = normalize_server_url(server_url).ok_or(SeerrError::NotConfigured)?;
        let client = SeerrClient::new(&normalized, SessionCookies::default());
        let settings: PublicSettings = client.get_json("settings/public", &[])?;
        if !settings.initialized {
            return Err(SeerrError::Unusable(
                "this Seerr instance has not finished its setup wizard yet".to_string(),
            ));
        }
        // Only present on an initialized instance, so it is checked, not required.
        if settings
            .media_server_type
            .is_some_and(|kind| kind != MEDIA_SERVER_JELLYFIN)
        {
            return Err(SeerrError::Unusable(
                "this Seerr instance is not connected to a Jellyfin server".to_string(),
            ));
        }
        // The version is informational and feature-detects the Quick Connect
        // login routes later, so a failure here must not fail the connect.
        let version = client
            .get_json::<StatusInfo>("status", &[])
            .map(|status| status.version)
            .unwrap_or_default();

        // The probes above can take minutes. Re-read the stored link and check
        // its Jellyfin binding only after they finish, before preserving any
        // part of the previous session.
        self.revalidate();
        let previous = self.read();
        let same_instance = previous.base_url.as_deref() == Some(normalized.as_str());
        if !same_instance {
            self.discard_remote_session(&previous);
        }

        let mut next = if same_instance {
            // Re-probing the instance we are already linked to: keep the
            // session and only take the (possibly rotated) CSRF pair.
            let mut next = previous;
            next.cookies.merge(client.cookies());
            next
        } else {
            SeerrState {
                revision: previous.revision,
                cookies: client.cookies(),
                ..SeerrState::default()
            }
        };
        next.base_url = Some(normalized.clone());
        next.movie_4k_enabled = settings.movie_4k_enabled;
        next.series_4k_enabled = settings.series_4k_enabled;
        next.partial_requests_enabled = settings.partial_requests_enabled;
        let linked = next.is_linked();
        let linked = if self.commit(next) {
            linked
        } else {
            tracing::debug!(
                target: "seerr.session",
                "discarded a stale Seerr connect result because the stored link changed"
            );
            false
        };

        tracing::info!(target: "seerr.session", url = %normalized, "connected to Seerr");
        Ok(json!({
            "serverUrl": normalized,
            "version": version,
            "applicationTitle": settings.application_title,
            "localLogin": settings.local_login,
            "mediaServerLogin": settings.media_server_login,
            "newSignInAllowed": settings.new_plex_login,
            "movie4kEnabled": settings.movie_4k_enabled,
            "series4kEnabled": settings.series_4k_enabled,
            "partialRequestsEnabled": settings.partial_requests_enabled,
            "linked": linked,
        }))
    }

    /// What the UI needs to decide whether to show the Seerr views at all.
    ///
    /// A linked session is confirmed against the instance rather than assumed:
    /// Express sessions lapse without warning, and the answer carries the
    /// permission mask and quota, neither of which is worth caching.
    pub fn status(&self) -> Value {
        self.revalidate();
        let state = self.read();
        let mut status = Self::status_from_state(&state);
        if !state.is_linked() {
            return status;
        }

        let client = match self.client() {
            Ok(client) => client,
            Err(_) => return Self::status_from_state(&self.read()),
        };
        let user: SeerrUser = match client.get_json("auth/me", &[]) {
            Ok(user) => user,
            Err(SeerrError::Unauthorized) => {
                // This *is* `/auth/me`, so a 401 here is unambiguous, provided
                // the link probed is still the one stored now.
                if !self.probe_is_current(state.revision) {
                    return Self::status_from_state(&self.read());
                }
                self.mark_expired();
                status["expired"] = json!(true);
                return status;
            }
            Err(error) => {
                tracing::debug!(target: "seerr.session", "could not read the Seerr user: {error}");
                if !self.probe_is_current(state.revision) {
                    return Self::status_from_state(&self.read());
                }
                return status;
            }
        };

        let capabilities = Capabilities::derive(
            user.permissions,
            state.movie_4k_enabled,
            state.series_4k_enabled,
        );
        // Quota needs its own call — `/auth/me` carries the mask but not usage
        // — and is a nicety, so a failure leaves it null rather than failing.
        let quota = client
            .get_json::<UserQuota>(&format!("user/{}/quota", user.id), &[])
            .ok();

        if !self.absorb_probe(&client, state.revision) {
            return Self::status_from_state(&self.read());
        }

        status["linked"] = json!(true);
        status["user"] = json!({
            "id": user.id,
            "name": user.preferred_name(),
            "avatar": user.avatar,
            "jellyfinUserId": user.jellyfin_user_id,
        });
        status["capabilities"] = serde_json::to_value(capabilities).unwrap_or(Value::Null);
        status["quota"] = quota
            .and_then(|quota| serde_json::to_value(quota).ok())
            .unwrap_or(Value::Null);
        status
    }

    fn status_from_state(state: &SeerrState) -> Value {
        json!({
            "configured": state.base_url.is_some(),
            "linked": false,
            "expired": state.expired,
            "serverUrl": state.base_url.clone(),
            "instance": {
                "movie4kEnabled": state.movie_4k_enabled,
                "series4kEnabled": state.series_4k_enabled,
                "partialRequestsEnabled": state.partial_requests_enabled,
            },
            "user": Value::Null,
            "capabilities": Value::Null,
            "quota": Value::Null,
        })
    }

    /// Drops the link when the Jellyfin account it was made under is no longer
    /// the one signed in. Cheap enough to run on every acquisition, and run
    /// eagerly by the sign-in and sign-out paths so a signed-out machine keeps
    /// no Seerr cookie on disk.
    pub fn revalidate(&self) {
        self.refresh_from_storage();
        let state = self.read();
        let Some(bound_user) = state.jellyfin_user_id.clone() else {
            // Nothing linked yet: only an instance address, which is not secret.
            return;
        };
        let credentials = self.library.credentials();
        let same_user = credentials.user_id.as_deref() == Some(bound_user.as_str());
        // Older rows may predate the server id; only a genuine mismatch counts.
        let same_server = match (&credentials.server_id, &state.jellyfin_server_id) {
            (Some(current), Some(bound)) => current == bound,
            _ => true,
        };
        if same_user && same_server {
            return;
        }
        tracing::info!(
            target: "seerr.session",
            "dropping the Seerr link: the Jellyfin account it was made under is no longer signed in"
        );
        self.unlink_locally();
    }

    /// Forgets the session without touching the instance address, so re-linking
    /// does not mean retyping it.
    fn unlink_locally(&self) {
        // A lost revision race must not leave the previous account's cookie
        // behind, so the clear is retried against the refreshed revision.
        for _ in 0..3 {
            let mut next = self.read();
            if next.cookies.is_empty() && next.jellyfin_user_id.is_none() {
                return;
            }
            next.cookies = SessionCookies::default();
            next.user_id = None;
            next.user_name = None;
            next.jellyfin_server_id = None;
            next.jellyfin_user_id = None;
            next.expired = false;
            if self.commit(next) {
                return;
            }
        }
        tracing::warn!(
            target: "seerr.session",
            "could not drop the Seerr link: the stored link kept changing"
        );
    }

    /// Best-effort server-side teardown before a stored session is abandoned.
    fn discard_remote_session(&self, state: &SeerrState) {
        let (Some(base_url), false) = (&state.base_url, state.cookies.is_empty()) else {
            return;
        };
        let client = SeerrClient::new(base_url, state.cookies.clone());
        if let Err(error) = client.post_empty("auth/logout") {
            tracing::debug!(target: "seerr.session", "server-side Seerr logout failed: {error}");
        }
    }

    fn mark_expired(&self) {
        if self.read().expired {
            return;
        }
        self.state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .expired = true;
        tracing::warn!(
            target: "seerr.session",
            "the Seerr session has lapsed; re-linking is required"
        );
    }

    /// Takes back whatever cookies a call rotated, so a refreshed CSRF pair
    /// survives the restart. The probe result is discarded if another process
    /// changed the stored link while the request was in flight.
    fn absorb_probe(&self, client: &SeerrClient, expected_revision: i64) -> bool {
        if !self.probe_is_current(expected_revision) {
            return false;
        }
        let cookies = client.cookies();
        let mut next = self.read();
        if next.revision != expected_revision {
            return false;
        }
        if next.cookies == cookies {
            return true;
        }
        next.cookies = cookies;
        self.commit(next)
    }

    fn probe_is_current(&self, expected_revision: i64) -> bool {
        let (config, revision) = match self.library.try_seerr_config_snapshot() {
            Ok(snapshot) => snapshot.unwrap_or_default(),
            Err(error) => {
                tracing::warn!(
                    target: "seerr.session",
                    "failed to verify the stored Seerr link revision: {error}"
                );
                return false;
            }
        };
        if revision == expected_revision {
            return true;
        }
        self.replace_state(SeerrState::from_config(config, revision));
        tracing::debug!(
            target: "seerr.session",
            "discarded a stale Seerr probe because the stored link changed"
        );
        false
    }

    fn refresh_from_storage(&self) {
        let revision = self.read().revision;
        let (config, stored_revision) = match self.library.try_seerr_config_snapshot() {
            Ok(snapshot) => snapshot.unwrap_or_default(),
            Err(error) => {
                tracing::warn!(
                    target: "seerr.session",
                    "failed to refresh the stored Seerr link: {error}"
                );
                return;
            }
        };
        if stored_revision != revision {
            self.replace_state(SeerrState::from_config(config, stored_revision));
        }
    }

    fn replace_state(&self, next: SeerrState) {
        *self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
    }

    fn commit(&self, mut next: SeerrState) -> bool {
        match self
            .library
            .save_seerr_config_if_revision(&next.to_config(), next.revision)
        {
            Ok(Some(revision)) => {
                next.revision = revision;
                self.replace_state(next);
                true
            }
            Ok(None) => {
                self.refresh_from_storage();
                false
            }
            Err(error) => {
                tracing::warn!(target: "seerr.session", "failed to persist the Seerr link: {error}");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SeerrClient, SeerrSession, SeerrState, SessionCookies};
    use crate::library::{Library, SeerrConfig};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    fn library() -> Arc<Library> {
        Arc::new(Library::open_in_memory().expect("library"))
    }

    /// A throwaway HTTP server answering one canned response per request and
    /// recording the request heads it saw. The cookie plumbing is the part of
    /// this milestone that only a real socket can prove.
    fn fake_seerr(responses: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base_url = format!("http://{}", listener.local_addr().expect("address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for (stream, response) in listener.incoming().zip(responses) {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) if line == "\r\n" => break,
                        Ok(_) => head.push_str(&line),
                    }
                }
                seen.lock().expect("lock").push(head);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (base_url, requests)
    }

    /// `Connection: close` keeps one request to one connection, so the canned
    /// responses line up with the requests one for one.
    fn response(status: &str, body: &str, cookies: &[&str]) -> String {
        let mut head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for cookie in cookies {
            head.push_str(&format!("Set-Cookie: {cookie}\r\n"));
        }
        format!("{head}\r\n{body}")
    }

    const INITIALIZED: &str = r#"{"initialized":true,"applicationTitle":"Seerr","mediaServerType":2,
        "localLogin":true,"mediaServerLogin":true,"newPlexLogin":true,"movie4kEnabled":false,
        "series4kEnabled":false,"partialRequestsEnabled":true}"#;
    const VERSION: &str = r#"{"version":"3.3.0","commitTag":"local"}"#;

    fn signed_in(library: &Library, user_id: &str) {
        let mut credentials = library.credentials();
        credentials.server_url = Some("http://server:8096".to_string());
        credentials.user_id = Some(user_id.to_string());
        credentials.server_id = Some("srv".to_string());
        credentials.token = Some("tok".to_string());
        library.save_credentials(&credentials).expect("credentials");
    }

    /// The link as the session itself sees it, once the account guard has run.
    /// Deliberately not `status()`: that confirms a live link against the
    /// instance, which these tests have no server for.
    fn is_linked(session: &SeerrSession) -> bool {
        session.revalidate();
        session.read().is_linked()
    }

    fn linked(library: &Library, jellyfin_user_id: &str) {
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some("https://seerr.test".to_string()),
                cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                user_name: Some("pho".to_string()),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some(jellyfin_user_id.to_string()),
                partial_requests_enabled: true,
                ..SeerrConfig::default()
            })
            .expect("seerr config");
    }

    #[test]
    fn a_fresh_session_is_neither_configured_nor_linked() {
        let session = SeerrSession::restore(library());
        let status = session.status();
        assert_eq!(status["configured"], false);
        assert_eq!(status["linked"], false);
        assert_eq!(status["user"], serde_json::Value::Null);
        assert!(matches!(
            session.client(),
            Err(super::SeerrError::NotConfigured)
        ));
    }

    #[test]
    fn restoring_reads_the_persisted_link() {
        let library = library();
        signed_in(&library, "uid");
        linked(&library, "uid");

        let session = SeerrSession::restore(library);
        let client = session.client().expect("client");
        assert_eq!(
            client.url("auth/me", &[]),
            "https://seerr.test/api/v1/auth/me"
        );
        assert!(client.cookies().has_session());
    }

    #[test]
    fn an_unusable_address_is_rejected_before_any_request() {
        let session = SeerrSession::restore(library());
        assert!(matches!(
            session.connect("file:///etc/passwd"),
            Err(super::SeerrError::NotConfigured)
        ));
        assert!(matches!(
            session.connect("javascript:alert(1)"),
            Err(super::SeerrError::NotConfigured)
        ));
        assert!(matches!(
            session.connect("  "),
            Err(super::SeerrError::NotConfigured)
        ));
    }

    #[test]
    fn switching_jellyfin_user_drops_the_link_from_memory_and_disk() {
        let library = library();
        signed_in(&library, "uid");
        linked(&library, "uid");
        let session = SeerrSession::restore(library.clone());
        assert!(is_linked(&session));

        // Somebody else signs in on the same machine.
        signed_in(&library, "other-uid");

        assert!(!is_linked(&session));
        assert_eq!(session.status()["linked"], false);
        let stored = library.seerr_config();
        assert_eq!(stored.cookies, None);
        assert_eq!(stored.user_id, None);
        // The instance address survives, so re-linking needs no retyping.
        assert_eq!(stored.base_url.as_deref(), Some("https://seerr.test"));
    }

    #[test]
    fn signing_out_of_jellyfin_drops_the_link() {
        let library = library();
        signed_in(&library, "uid");
        linked(&library, "uid");
        let session = SeerrSession::restore(library.clone());

        library.clear_session(false).expect("sign out");

        assert!(!is_linked(&session));
        assert_eq!(library.seerr_config().cookies, None);
    }

    #[test]
    fn a_link_made_against_another_jellyfin_server_is_dropped() {
        let library = library();
        signed_in(&library, "uid");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some("https://seerr.test".to_string()),
                cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
                jellyfin_server_id: Some("a-different-server".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");

        let session = SeerrSession::restore(library.clone());
        assert!(!is_linked(&session));
        assert_eq!(library.seerr_config().cookies, None);
    }

    #[test]
    fn connecting_captures_the_csrf_pair_from_the_very_first_probe() {
        let (base_url, requests) = fake_seerr(vec![
            response(
                "200 OK",
                INITIALIZED,
                &[
                    "_csrf=secret; Path=/; HttpOnly; SameSite=Strict",
                    "XSRF-TOKEN=token123; Path=/",
                ],
            ),
            response("200 OK", VERSION, &[]),
        ]);
        let library = library();
        let session = SeerrSession::restore(library.clone());

        let result = session.connect(&base_url).expect("connect");
        assert_eq!(result["serverUrl"], base_url);
        assert_eq!(result["version"], "3.3.0");
        assert_eq!(result["partialRequestsEnabled"], true);
        assert_eq!(result["linked"], false);

        let requests = requests.lock().expect("lock");
        assert!(requests[0].starts_with("GET /api/v1/settings/public HTTP/1.1"));
        assert!(requests[1].starts_with("GET /api/v1/status HTTP/1.1"));
        // The pair the probe handed out is already on the second request, and
        // is persisted so the first write after a restart still has it.
        assert!(requests[1].contains("cookie: XSRF-TOKEN=token123; _csrf=secret"));
        let stored = library.seerr_config();
        assert_eq!(stored.base_url.as_deref(), Some(base_url.as_str()));
        assert!(
            stored
                .cookies
                .as_deref()
                .is_some_and(|cookies| cookies.contains("XSRF-TOKEN"))
        );
        assert!(stored.partial_requests_enabled);
    }

    #[test]
    fn an_uninitialized_instance_is_refused_before_it_can_be_set_up_by_accident() {
        let (base_url, _) = fake_seerr(vec![response(
            "200 OK",
            r#"{"initialized":false,"plexClientIdentifier":"abc"}"#,
            &[],
        )]);
        let library = library();
        let session = SeerrSession::restore(library.clone());

        let error = session.connect(&base_url).expect_err("refused");
        assert!(matches!(error, super::SeerrError::Unusable(_)));
        assert!(error.to_string().contains("setup wizard"));
        assert_eq!(library.seerr_config(), SeerrConfig::default());
    }

    #[test]
    fn an_instance_wired_to_another_media_server_is_refused() {
        let (base_url, _) = fake_seerr(vec![response(
            "200 OK",
            r#"{"initialized":true,"mediaServerType":1}"#,
            &[],
        )]);
        let session = SeerrSession::restore(library());
        let error = session.connect(&base_url).expect_err("refused");
        assert!(error.to_string().contains("Jellyfin"));
    }

    #[test]
    fn a_failing_version_probe_does_not_fail_the_connect() {
        let (base_url, _) = fake_seerr(vec![
            response("200 OK", INITIALIZED, &[]),
            response("404 Not Found", "{}", &[]),
        ]);
        let session = SeerrSession::restore(library());
        let result = session.connect(&base_url).expect("connect");
        assert_eq!(result["version"], "");
    }

    #[test]
    fn moving_to_another_instance_logs_the_old_session_out_with_its_csrf_header() {
        let (old_url, old_requests) = fake_seerr(vec![response("204 No Content", "", &[])]);
        let (new_url, _) = fake_seerr(vec![
            response("200 OK", INITIALIZED, &[]),
            response("200 OK", VERSION, &[]),
        ]);
        let library = library();
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some(old_url.clone()),
                cookies: Some(r#"{"XSRF-TOKEN":"token123","connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                user_name: Some("pho".to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");
        let session = SeerrSession::restore(library.clone());

        session.connect(&new_url).expect("connect");

        let logout = old_requests.lock().expect("lock");
        assert_eq!(logout.len(), 1, "the old instance was not logged out");
        assert!(logout[0].starts_with("POST /api/v1/auth/logout HTTP/1.1"));
        assert!(logout[0].contains("cookie: XSRF-TOKEN=token123; connect.sid=abc"));
        // csurf accepts the token echoed as a header; without it a
        // CSRF-protected instance rejects every write.
        assert!(logout[0].contains("x-xsrf-token: token123"));

        // Nothing of the old link survives the move.
        let stored = library.seerr_config();
        assert_eq!(stored.base_url.as_deref(), Some(new_url.as_str()));
        assert_eq!(stored.user_id, None);
        assert!(
            stored
                .cookies
                .as_deref()
                .is_none_or(|cookies| !cookies.contains("connect.sid"))
        );
    }

    #[test]
    fn re_probing_the_same_instance_keeps_the_session_and_refreshes_the_csrf_pair() {
        let (base_url, _) = fake_seerr(vec![
            response("200 OK", INITIALIZED, &["XSRF-TOKEN=rotated; Path=/"]),
            response("200 OK", VERSION, &[]),
        ]);
        let library = library();
        signed_in(&library, "uid");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some(base_url.clone()),
                cookies: Some(r#"{"XSRF-TOKEN":"stale","connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                user_name: Some("pho".to_string()),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");
        let session = SeerrSession::restore(library.clone());

        let result = session.connect(&base_url).expect("connect");
        assert_eq!(result["linked"], true);

        let stored = library.seerr_config();
        assert_eq!(stored.user_id, Some(7));
        let cookies = stored.cookies.expect("cookies");
        assert!(cookies.contains(r#""connect.sid":"abc""#));
        assert!(cookies.contains(r#""XSRF-TOKEN":"rotated""#));
        assert!(is_linked(&session));
    }

    #[test]
    fn re_probing_after_an_account_switch_does_not_preserve_the_previous_link() {
        let (base_url, _) = fake_seerr(vec![
            response("200 OK", INITIALIZED, &[]),
            response("200 OK", VERSION, &[]),
        ]);
        let library = library();
        signed_in(&library, "uid");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some(base_url.clone()),
                cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
                user_id: Some(7),
                user_name: Some("pho".to_string()),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");
        let session = SeerrSession::restore(library.clone());

        signed_in(&library, "other-uid");
        let result = session.connect(&base_url).expect("connect");

        assert_eq!(result["linked"], false);
        let stored = library.seerr_config();
        assert_eq!(stored.base_url.as_deref(), Some(base_url.as_str()));
        assert_eq!(stored.cookies, None);
        assert_eq!(stored.user_id, None);
        assert_eq!(stored.jellyfin_user_id, None);
    }

    #[test]
    fn a_stale_probe_cannot_overwrite_a_newer_stored_link() {
        let library = library();
        signed_in(&library, "uid");
        linked(&library, "uid");
        let session = SeerrSession::restore(library.clone());
        let stale_revision = session.read().revision;
        let stale_client = SeerrClient::new(
            "https://seerr.test",
            SessionCookies::from_json(r#"{"connect.sid":"stale"}"#),
        );

        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some("https://new.test".to_string()),
                cookies: Some(r#"{"connect.sid":"new"}"#.to_string()),
                user_id: Some(99),
                user_name: Some("new user".to_string()),
                jellyfin_server_id: Some("srv".to_string()),
                jellyfin_user_id: Some("uid".to_string()),
                ..SeerrConfig::default()
            })
            .expect("new link");

        assert!(!session.absorb_probe(&stale_client, stale_revision));
        let stored = library.seerr_config();
        assert_eq!(stored.base_url.as_deref(), Some("https://new.test"));
        assert_eq!(stored.user_id, Some(99));
        assert!(
            stored
                .cookies
                .as_deref()
                .is_some_and(|cookies| cookies.contains(r#""connect.sid":"new""#))
        );
        assert_eq!(session.read().base_url.as_deref(), Some("https://new.test"));
    }

    #[test]
    fn a_configured_but_unlinked_instance_reports_its_switches() {
        let library = library();
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some("https://seerr.test".to_string()),
                movie_4k_enabled: true,
                partial_requests_enabled: true,
                ..SeerrConfig::default()
            })
            .expect("seerr config");

        let status = SeerrSession::restore(library).status();
        assert_eq!(status["configured"], true);
        assert_eq!(status["linked"], false);
        assert_eq!(status["serverUrl"], "https://seerr.test");
        assert_eq!(status["instance"]["movie4kEnabled"], true);
        assert_eq!(status["instance"]["series4kEnabled"], false);
        assert_eq!(status["instance"]["partialRequestsEnabled"], true);
    }

    #[test]
    fn the_state_round_trips_through_its_stored_form() {
        let config = SeerrConfig {
            base_url: Some("https://seerr.test".to_string()),
            cookies: Some(r#"{"XSRF-TOKEN":"t","connect.sid":"abc"}"#.to_string()),
            user_id: Some(7),
            user_name: Some("pho".to_string()),
            jellyfin_server_id: Some("srv".to_string()),
            jellyfin_user_id: Some("uid".to_string()),
            movie_4k_enabled: true,
            series_4k_enabled: true,
            partial_requests_enabled: true,
        };
        assert_eq!(
            SeerrState::from_config(config.clone(), 42).to_config(),
            config
        );
    }

    #[test]
    fn an_empty_cookie_jar_is_stored_as_no_cookies_at_all() {
        let state = SeerrState::from_config(
            SeerrConfig {
                base_url: Some("https://seerr.test".to_string()),
                ..SeerrConfig::default()
            },
            0,
        );
        assert_eq!(state.to_config().cookies, None);
    }

    #[test]
    fn a_poisoned_state_lock_keeps_the_existing_state() {
        let session = SeerrSession::restore(library());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut state = session.state.write().expect("state");
            state.base_url = Some("https://seerr.test".to_string());
            panic!("poison the state lock");
        }));

        assert_eq!(
            session.read().base_url.as_deref(),
            Some("https://seerr.test")
        );
    }

    /// Guards against the credentials row disappearing under a live session.
    #[test]
    fn a_link_with_no_jellyfin_binding_is_left_alone() {
        let library = library();
        signed_in(&library, "uid");
        library
            .save_seerr_config(&SeerrConfig {
                base_url: Some("https://seerr.test".to_string()),
                cookies: Some(r#"{"XSRF-TOKEN":"t"}"#.to_string()),
                ..SeerrConfig::default()
            })
            .expect("seerr config");

        let session = SeerrSession::restore(library.clone());
        session.revalidate();
        assert!(library.seerr_config().cookies.is_some());
    }
}
