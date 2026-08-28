//! The one authenticated Jellyfin session shared by the UI API, the sync
//! thread, the play path, and the playback reporter.

use std::sync::{Arc, Mutex, RwLock};

use serde_json::{Value, json};

use crate::library::{Library, StoredCredentials};
use crate::preferences::{AccountKey, AppSettings, normalize_server_url};

use super::api::auth::{self, Credentials};
use super::api::{ApiError, JellyfinClient};

#[derive(Debug, Clone, Default)]
struct SessionState {
    server_url: Option<String>,
    server_name: Option<String>,
    server_id: Option<String>,
    user_id: Option<String>,
    user_name: Option<String>,
    device_id: String,
    token: Option<String>,
    restricted: bool,
    /// Set when the server rejected our token; the UI must re-authenticate.
    expired: bool,
    generation: u64,
    deleting: bool,
}

pub struct Session {
    library: Arc<Library>,
    state: RwLock<SessionState>,
    operation_gate: Mutex<()>,
}

#[derive(Clone)]
pub struct SessionScope {
    generation: u64,
    account: AccountKey,
    client: JellyfinClient,
    user_id: String,
}

impl SessionScope {
    pub fn account(&self) -> &AccountKey {
        &self.account
    }

    pub fn client(&self) -> &JellyfinClient {
        &self.client
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }
}

impl Session {
    /// Restores the persisted session, falling back to the server URL that the
    /// pre-own-UI releases kept in the legacy `config.json`.
    pub fn restore(library: Arc<Library>) -> Self {
        let stored = library.credentials();
        let authenticated = stored.is_authenticated();
        let server_url = stored
            .server_url
            .clone()
            .or_else(|| AppSettings::load().jellyfin_url);
        let state = SessionState {
            server_url,
            server_name: None,
            server_id: stored.server_id,
            user_id: stored.user_id.clone(),
            user_name: stored.user_name.clone(),
            device_id: stored.device_id.clone(),
            token: stored.token,
            // Restored sessions hide Missing until a live user-policy read
            // proves that the account has no catalog restrictions.
            restricted: true,
            expired: false,
            generation: u64::from(authenticated),
            deleting: false,
        };
        Self {
            library,
            state: RwLock::new(state),
            operation_gate: Mutex::new(()),
        }
    }

    fn read(&self) -> SessionState {
        self.state
            .read()
            .map(|state| state.clone())
            .unwrap_or_default()
    }

    pub fn server_url(&self) -> Option<String> {
        self.read().server_url
    }

    pub fn user_id(&self) -> Option<String> {
        self.read().user_id
    }

    pub fn account_key(&self) -> Option<AccountKey> {
        let state = self.read();
        AccountKey::new(state.server_id?, state.user_id?)
    }

    pub fn is_authenticated(&self) -> bool {
        let state = self.read();
        !state.expired
            && !state.deleting
            && state.token.is_some()
            && state.user_id.is_some()
            && state.server_id.is_some()
            && state.server_url.is_some()
    }

    pub fn user_restricted(&self) -> bool {
        self.read().restricted
    }

    pub fn refresh_user_policy(&self) {
        let Ok(scope) = self.scope() else {
            return;
        };
        let path = format!(
            "/Users/{}",
            crate::app::urls::encode_path_segment(scope.user_id())
        );
        let Ok(user) = scope
            .client()
            .get_json::<super::api::model::UserDto>(&path, &[])
        else {
            return;
        };
        let restricted = user
            .policy
            .as_ref()
            .is_none_or(super::api::model::UserPolicy::restricts_catalog);
        let _ = self.commit_if_current(
            &scope,
            || (),
            || {
                if let Ok(mut state) = self.state.write() {
                    state.restricted = restricted;
                }
                Ok(())
            },
        );
    }

    pub fn scope(&self) -> Result<SessionScope, ApiError> {
        let _gate = self
            .operation_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = self.read();
        if state.expired || state.deleting {
            return Err(ApiError::Unauthorized);
        }
        let server_url = state.server_url.as_deref().ok_or(ApiError::NotConfigured)?;
        let token = state.token.as_deref().ok_or(ApiError::NotConfigured)?;
        let user_id = state.user_id.clone().ok_or(ApiError::NotConfigured)?;
        let account = AccountKey::new(
            state.server_id.clone().ok_or(ApiError::NotConfigured)?,
            user_id.clone(),
        )
        .ok_or(ApiError::NotConfigured)?;
        Ok(SessionScope {
            generation: state.generation,
            account,
            client: JellyfinClient::new(server_url, &state.device_id, Some(token)),
            user_id,
        })
    }

    pub fn scope_is_current(&self, scope: &SessionScope) -> bool {
        let _gate = self
            .operation_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.read().matches(scope)
    }

    pub fn commit_if_current<T, E>(
        &self,
        scope: &SessionScope,
        stale: impl FnOnce() -> E,
        commit: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let _gate = self
            .operation_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !self.read().matches(scope) {
            return Err(stale());
        }
        commit()
    }

    /// An authenticated client, or the reason one cannot be built.
    pub fn client(&self) -> Result<JellyfinClient, ApiError> {
        let state = self.read();
        if state.expired || state.deleting {
            return Err(ApiError::Unauthorized);
        }
        let (Some(server_url), Some(token)) = (state.server_url, state.token) else {
            return Err(ApiError::NotConfigured);
        };
        Ok(JellyfinClient::new(
            &server_url,
            &state.device_id,
            Some(&token),
        ))
    }

    /// An authenticated client together with the user id every item query needs.
    pub fn client_and_user(&self) -> Result<(JellyfinClient, String), ApiError> {
        let client = self.client()?;
        let user_id = self.user_id().ok_or(ApiError::NotConfigured)?;
        Ok((client, user_id))
    }

    /// A client for pre-login calls against an arbitrary server.
    pub fn anonymous_client(&self, server_url: &str) -> JellyfinClient {
        JellyfinClient::new(server_url, &self.read().device_id, None)
    }

    /// Probes a server and remembers it as the login target.
    pub fn connect(&self, server_url: &str) -> Result<Value, ApiError> {
        let normalized = normalize_server_url(server_url).ok_or(ApiError::NotConfigured)?;
        let client = self.anonymous_client(&normalized);
        let info = auth::public_system_info(&client)?;
        let quick_connect = auth::quick_connect_enabled(&client);
        if let Ok(mut state) = self.state.write() {
            state.server_url = Some(normalized.clone());
            state.server_name = Some(info.server_name.clone());
        }
        Ok(json!({
            "serverUrl": normalized,
            "serverName": info.server_name,
            "version": info.version,
            "quickConnect": quick_connect,
        }))
    }

    pub fn login(
        &self,
        server_url: &str,
        username: &str,
        password: &str,
    ) -> Result<Value, ApiError> {
        let normalized = normalize_server_url(server_url).ok_or(ApiError::NotConfigured)?;
        let client = self.anonymous_client(&normalized);
        let credentials = auth::authenticate_by_name(&client, username, password)?;
        self.accept(&normalized, credentials)
    }

    pub fn quick_connect_start(&self, server_url: &str) -> Result<Value, ApiError> {
        let normalized = normalize_server_url(server_url).ok_or(ApiError::NotConfigured)?;
        let client = self.anonymous_client(&normalized);
        if !auth::quick_connect_enabled(&client) {
            return Err(ApiError::Status { status: 501 });
        }
        let result = auth::quick_connect_initiate(&client)?;
        Ok(json!({
            "serverUrl": normalized,
            "code": result.code,
            "secret": result.secret,
        }))
    }

    /// One poll step: reports whether the code has been approved yet, and
    /// completes the login when it has.
    pub fn quick_connect_poll(&self, server_url: &str, secret: &str) -> Result<Value, ApiError> {
        let normalized = normalize_server_url(server_url).ok_or(ApiError::NotConfigured)?;
        let client = self.anonymous_client(&normalized);
        let state = auth::quick_connect_state(&client, secret)?;
        if !state.authenticated {
            return Ok(json!({ "authenticated": false }));
        }
        let credentials = auth::authenticate_with_quick_connect(&client, secret)?;
        let session = self.accept(&normalized, credentials)?;
        Ok(json!({ "authenticated": true, "session": session }))
    }

    fn accept(&self, server_url: &str, credentials: Credentials) -> Result<Value, ApiError> {
        let _gate = self
            .operation_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = self.library.credentials();
        let cache_owner = self
            .library
            .cache_owner()
            .or_else(|| previous.server_id.clone().zip(previous.user_id.clone()));
        let next_owner = (credentials.server_id.clone(), credentials.user_id.clone());
        let switched_account = cache_owner
            .as_ref()
            .is_some_and(|owner| owner != &next_owner)
            || cache_owner.is_none() && self.library.stats().total > 0;
        if switched_account {
            tracing::info!(
                target: "jellyfin.session",
                "clearing the metadata cache for a new Jellyfin account"
            );
            self.library
                .clear_session(true)
                .map_err(|error| storage_error(&error))?;
        }

        let stored = StoredCredentials {
            server_url: Some(server_url.to_string()),
            user_id: Some(credentials.user_id.clone()),
            user_name: Some(credentials.user_name.clone()),
            server_id: Some(credentials.server_id.clone()),
            device_id: previous.device_id,
            token: Some(credentials.token.clone()),
        };
        self.library
            .save_credentials(&stored)
            .map_err(|error| storage_error(&error))?;
        self.remember_server_url(server_url);

        if let Ok(mut state) = self.state.write() {
            state.server_url = Some(server_url.to_string());
            state.server_id = Some(credentials.server_id.clone());
            state.user_id = Some(credentials.user_id.clone());
            state.user_name = Some(credentials.user_name.clone());
            state.token = Some(credentials.token);
            state.restricted = credentials.restricted;
            state.expired = false;
            state.deleting = false;
            state.generation = next_generation(state.generation);
        }
        tracing::info!(
            target: "jellyfin.session",
            user = %credentials.user_name,
            "authenticated against Jellyfin"
        );
        Ok(self.status())
    }

    /// Keeps `settings.json` in step so the dashboard action and upgrades from
    /// older releases keep working.
    fn remember_server_url(&self, server_url: &str) {
        let Some(services) = crate::app::services::services() else {
            tracing::warn!(target: "jellyfin.session", "preferences service was unavailable while saving the server URL");
            return;
        };
        if services.preferences.snapshot().jellyfin_url.as_deref() == Some(server_url) {
            return;
        }
        if let Err(error) = services.preferences.set_server_url(server_url.to_string()) {
            tracing::warn!(target: "jellyfin.session", "failed to save the server URL: {error}");
        }
    }

    pub fn logout(&self, forget_library: bool) -> rusqlite::Result<()> {
        if let Ok(client) = self.client()
            && let Err(error) = auth::logout(&client)
        {
            tracing::debug!(target: "jellyfin.session", "server-side logout failed: {error}");
        }
        self.clear_local(forget_library)?;
        tracing::info!(target: "jellyfin.session", "signed out of Jellyfin");
        Ok(())
    }

    pub fn clear_local(&self, forget_library: bool) -> rusqlite::Result<()> {
        let _gate = self
            .operation_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.library.clear_session(forget_library)?;
        if let Ok(mut state) = self.state.write() {
            state.user_id = None;
            state.user_name = None;
            state.server_id = None;
            state.token = None;
            state.restricted = true;
            state.expired = false;
            state.deleting = false;
            state.generation = next_generation(state.generation);
        }
        Ok(())
    }

    pub fn begin_account_deletion(&self, account: &AccountKey) -> bool {
        let _gate = self
            .operation_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Ok(mut state) = self.state.write() else {
            return false;
        };
        if state.account_key().as_ref() != Some(account) {
            return false;
        }
        state.generation = next_generation(state.generation);
        state.deleting = true;
        true
    }

    /// Called when the server answers 401/403: pauses sync and sends the UI
    /// back to the login view without discarding the cache.
    ///
    /// The UI is told through the shell event here, on the first rejection,
    /// because many callers swallow their errors by design (card badges, the
    /// about panel); without the push the app could sit on authenticated
    /// screens with dead controls until something re-read `/api/status`.
    pub fn mark_expired(&self) {
        if !self.mark_expired_inner(None) {
            return;
        }
        tracing::warn!(
            target: "jellyfin.session",
            "the Jellyfin server rejected the stored token; re-authentication required"
        );
        crate::app::services::notify_session_expired();
    }

    /// Routes an API failure so a rejected token is only ever handled once.
    pub fn note_error(&self, error: &ApiError) {
        if *error == ApiError::Unauthorized {
            self.mark_expired();
        }
    }

    pub fn note_scoped_error(&self, scope: &SessionScope, error: &ApiError) {
        if *error == ApiError::Unauthorized && self.mark_expired_inner(Some(scope)) {
            tracing::warn!(
                target: "jellyfin.session",
                "the Jellyfin server rejected the stored token; re-authentication required"
            );
            crate::app::services::notify_session_expired();
        }
    }

    fn mark_expired_inner(&self, scope: Option<&SessionScope>) -> bool {
        let _gate = self
            .operation_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Ok(mut state) = self.state.write() else {
            return false;
        };
        if state.expired || state.deleting || scope.is_some_and(|scope| !state.matches(scope)) {
            return false;
        }
        state.expired = true;
        state.generation = next_generation(state.generation);
        true
    }

    pub fn status(&self) -> Value {
        let state = self.read();
        json!({
            "authenticated": !state.expired && !state.deleting
                && state.token.is_some() && state.user_id.is_some(),
            "expired": state.expired,
            "serverUrl": state.server_url,
            "serverName": state.server_name,
            "userId": state.user_id,
            "userName": state.user_name,
            "deviceId": state.device_id,
        })
    }
}

impl SessionState {
    fn account_key(&self) -> Option<AccountKey> {
        AccountKey::new(self.server_id.clone()?, self.user_id.clone()?)
    }

    fn matches(&self, scope: &SessionScope) -> bool {
        !self.expired
            && !self.deleting
            && self.generation == scope.generation
            && self.account_key().as_ref() == Some(scope.account())
    }
}

fn next_generation(generation: u64) -> u64 {
    generation.wrapping_add(1).max(1)
}

fn storage_error(error: &rusqlite::Error) -> ApiError {
    ApiError::Decode(format!("session storage failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::Session;
    use crate::jellyfin::api::ApiError;
    use crate::jellyfin::api::auth::Credentials;
    use crate::library::Library;
    use crate::library::test_support::dto;
    use std::sync::Arc;

    fn session() -> Session {
        Session::restore(Arc::new(Library::open_in_memory().expect("library")))
    }

    fn credentials(user: &str) -> Credentials {
        Credentials {
            token: format!("token-{user}"),
            user_id: user.to_string(),
            user_name: user.to_string(),
            server_id: "server".to_string(),
            restricted: false,
        }
    }

    #[test]
    fn a_fresh_session_is_not_authenticated_and_has_no_client() {
        let session = session();
        assert!(!session.is_authenticated());
        assert!(matches!(session.client(), Err(ApiError::NotConfigured)));
        assert_eq!(session.status()["authenticated"], false);
        assert!(
            !session.status()["deviceId"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
        );
    }

    #[test]
    fn restoring_reads_persisted_credentials() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let mut credentials = library.credentials();
        credentials.server_url = Some("http://server:8096".to_string());
        credentials.user_id = Some("uid".to_string());
        credentials.server_id = Some("server".to_string());
        credentials.token = Some("tok".to_string());
        library.save_credentials(&credentials).expect("save");

        let session = Session::restore(library);
        assert!(session.is_authenticated());
        let client = session.client().expect("client");
        assert_eq!(client.base_url(), "http://server:8096");
        assert_eq!(client.token(), Some("tok"));
        assert_eq!(session.client_and_user().expect("client and user").1, "uid");
    }

    #[test]
    fn an_expired_token_blocks_client_creation_until_re_login() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let mut credentials = library.credentials();
        credentials.server_url = Some("http://server:8096".to_string());
        credentials.user_id = Some("uid".to_string());
        credentials.server_id = Some("server".to_string());
        credentials.token = Some("tok".to_string());
        library.save_credentials(&credentials).expect("save");

        let session = Session::restore(library);
        session.note_error(&ApiError::Unauthorized);
        assert!(!session.is_authenticated());
        assert!(matches!(session.client(), Err(ApiError::Unauthorized)));
        assert_eq!(session.status()["expired"], true);
    }

    #[test]
    fn other_errors_do_not_expire_the_session() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let mut credentials = library.credentials();
        credentials.server_url = Some("http://server:8096".to_string());
        credentials.user_id = Some("uid".to_string());
        credentials.server_id = Some("server".to_string());
        credentials.token = Some("tok".to_string());
        library.save_credentials(&credentials).expect("save");

        let session = Session::restore(library);
        session.note_error(&ApiError::Status { status: 500 });
        session.note_error(&ApiError::Transport("reset".to_string()));
        assert!(session.is_authenticated());
    }

    #[test]
    fn connecting_to_an_unusable_url_is_rejected_before_any_request() {
        let session = session();
        assert!(matches!(
            session.connect("file:///etc/passwd"),
            Err(ApiError::NotConfigured)
        ));
        assert!(matches!(
            session.login("javascript:alert(1)", "u", "p"),
            Err(ApiError::NotConfigured)
        ));
    }

    #[test]
    fn anonymous_clients_reuse_the_persisted_device_id() {
        let session = session();
        let device_id = session.status()["deviceId"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let client = session.anonymous_client("http://server:8096");
        assert_eq!(client.device_id(), device_id);
        assert_eq!(client.token(), None);
    }

    #[test]
    fn old_successes_and_unauthorized_errors_are_discarded_after_account_switch() {
        let session = session();
        session
            .accept("http://server:8096", credentials("alice"))
            .expect("sign in Alice");
        let alice = session.scope().expect("Alice scope");
        session
            .accept("http://server:8096", credentials("bob"))
            .expect("sign in Bob");

        let result = session.commit_if_current(
            &alice,
            || "stale",
            || -> Result<(), &str> { panic!("an obsolete success committed") },
        );
        assert_eq!(result, Err("stale"));
        session.note_scoped_error(&alice, &ApiError::Unauthorized);
        assert!(session.is_authenticated());
        assert_eq!(session.user_id().as_deref(), Some("bob"));
        assert_eq!(session.status()["expired"], false);
    }

    #[test]
    fn a_restricted_user_gets_an_empty_cache_after_a_normal_logout_and_switch() {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let session = Session::restore(library.clone());
        session
            .accept("http://server:8096", credentials("alice"))
            .expect("sign in Alice");
        library
            .upsert_page(&[dto(
                r#"{"Id":"alice-movie","Name":"Alice Only","Type":"Movie"}"#,
            )])
            .expect("cache Alice's library");

        session.clear_local(false).expect("normal logout");
        assert_eq!(library.stats().total, 1);
        assert_eq!(
            library.cache_owner(),
            Some(("server".to_string(), "alice".to_string()))
        );

        let mut bob = credentials("bob");
        bob.restricted = true;
        session
            .accept("http://server:8096", bob)
            .expect("sign in restricted Bob");

        assert_eq!(library.stats().total, 0);
        assert!(session.user_restricted());
        assert_eq!(
            library.cache_owner(),
            Some(("server".to_string(), "bob".to_string()))
        );
    }

    #[test]
    fn deletion_tombstone_blocks_new_and_in_flight_account_work() {
        let session = session();
        session
            .accept("http://server:8096", credentials("alice"))
            .expect("sign in Alice");
        let scope = session.scope().expect("Alice scope");
        assert!(session.begin_account_deletion(scope.account()));

        assert!(!session.is_authenticated());
        assert!(session.scope().is_err());
        assert!(session.client().is_err());
        assert_eq!(
            session.commit_if_current(&scope, || "stale", || Ok::<_, &str>(())),
            Err("stale")
        );
    }
}
