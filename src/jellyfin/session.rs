use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::app::services::ShellBridge;
use crate::library::{Library, StoredCredentials};
use crate::preferences::{AccountKey, normalize_server_url};

use super::api::auth::{self, Credentials};
use super::api::model::UserAccess;
use super::api::{ApiError, JellyfinClient};
use super::version::{ServerTooOld, ServerVersion};

const POLICY_RETRY_COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum PolicyState {
    #[default]
    Unverified,
    PendingRetry,
    Refreshing,
    Verified,
}

#[derive(Debug, Clone, Default)]
struct SessionState {
    server_url: Option<String>,
    server_id: Option<String>,
    user_id: Option<String>,
    user_name: Option<String>,
    device_id: String,
    token: Option<String>,
    access: UserAccess,
    policy_state: PolicyState,
    next_policy_retry_at: Option<Instant>,
    expired: bool,
    generation: u64,
    deleting: bool,
}

struct AuthenticatedParts<'a> {
    server_url: &'a str,
    token: &'a str,
    user_id: &'a str,
    account: AccountKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub authenticated: bool,
    pub expired: bool,
    pub server_url: Option<String>,
    pub server_name: Option<String>,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub device_id: String,
    pub administrator: bool,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub server_url: String,
    pub server_name: String,
    pub version: ServerVersion,
    pub quick_connect: bool,
    pub too_old: Option<ServerTooOld>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct QuickConnectStart {
    pub server_url: String,
    pub code: String,
    pub secret: String,
}

#[derive(Serialize, ts_rs::TS)]
pub struct QuickConnectPoll {
    pub authenticated: bool,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionStatus>,
}

struct SessionCore {
    library: Arc<Library>,
    shell: Arc<ShellBridge>,
    state: RwLock<SessionState>,
    operation_gate: Mutex<()>,
}

pub struct Session {
    core: Arc<SessionCore>,
}

#[derive(Clone)]
pub struct SessionScope {
    generation: u64,
    account: AccountKey,
    client: JellyfinClient,
    user_id: String,
}

pub struct LogoutTarget {
    generation: u64,
    scope: Option<SessionScope>,
}

impl SessionScope {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn account(&self) -> &AccountKey {
        &self.account
    }

    pub(in crate::jellyfin) fn client(&self) -> &JellyfinClient {
        &self.client
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn fetch<T>(
        &self,
        call: impl FnOnce(&JellyfinClient, &str) -> Result<T, ApiError>,
    ) -> Result<Scoped<T>, ApiError> {
        let value = call(&self.client, &self.user_id)?;
        Ok(Scoped {
            generation: self.generation,
            account: self.account.clone(),
            value,
        })
    }
}

#[must_use = "a fetched answer must be opened with Session::confirm or Live::open"]
pub struct Scoped<T> {
    generation: u64,
    account: AccountKey,
    value: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stale;

pub struct Live {
    generation: u64,
    account: AccountKey,
}

impl Live {
    pub fn account(&self) -> &AccountKey {
        &self.account
    }

    pub fn open<T>(&self, scoped: Scoped<T>) -> Result<T, Stale> {
        if scoped.generation == self.generation && scoped.account == self.account {
            Ok(scoped.value)
        } else {
            Err(Stale)
        }
    }

    #[cfg(test)]
    pub fn for_test() -> Self {
        Self {
            generation: 0,
            account: AccountKey::new("server", "user").expect("a non-empty test account key"),
        }
    }
}

impl SessionCore {
    fn read(&self) -> SessionState {
        self.state
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn commit_if_current<T, E>(
        &self,
        scope: &SessionScope,
        stale: impl FnOnce() -> E,
        commit: impl FnOnce(&Live) -> Result<T, E>,
    ) -> Result<T, E> {
        let _gate = self
            .operation_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !self.read().matches(scope) {
            return Err(stale());
        }
        commit(&Live {
            generation: scope.generation,
            account: scope.account.clone(),
        })
    }

    fn run_policy_refresh(&self, scope: &SessionScope, apply_cooldown_on_failure: bool) {
        let path = format!(
            "/Users/{}",
            crate::app::urls::encode_path_segment(scope.user_id())
        );
        let Ok(user) =
            scope.fetch(|client, _| client.get_json::<super::api::model::UserDto>(&path, &[]))
        else {
            let _gate = self
                .operation_gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
            if state.matches(scope) && state.policy_state == PolicyState::Refreshing {
                state.policy_state = PolicyState::PendingRetry;
                state.next_policy_retry_at =
                    apply_cooldown_on_failure.then(|| Instant::now() + POLICY_RETRY_COOLDOWN);
            }
            drop(state);
            return;
        };
        let access_changed = self
            .commit_if_current(
                scope,
                || (),
                |live| {
                    let user = live.open(user).map_err(drop)?;
                    let access = UserAccess::of(user.policy.as_ref());
                    let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
                    let changed = state.access != access;
                    state.access = access;
                    state.policy_state = PolicyState::Verified;
                    state.next_policy_retry_at = None;
                    drop(state);
                    Ok(changed)
                },
            )
            .unwrap_or(false);
        if access_changed {
            self.shell.collections_changed();
        }
    }

    fn clear_local_locked(&self, forget_library: bool) -> rusqlite::Result<()> {
        self.library.clear_session(forget_library)?;
        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        state.user_id = None;
        state.user_name = None;
        state.server_id = None;
        state.token = None;
        state.access = UserAccess::UNVERIFIED;
        state.policy_state = PolicyState::Unverified;
        state.next_policy_retry_at = None;
        state.expired = false;
        state.deleting = false;
        state.generation = next_generation(state.generation);
        drop(state);
        Ok(())
    }
}

impl Session {
    pub fn restore(library: Arc<Library>, shell: Arc<ShellBridge>) -> Self {
        let stored = library.credentials();
        let mut state = SessionState {
            server_url: stored.server_url,
            server_id: stored.server_id,
            user_id: stored.user_id,
            user_name: stored.user_name,
            device_id: stored.device_id,
            token: stored.token,
            access: UserAccess::UNVERIFIED,
            policy_state: PolicyState::Unverified,
            next_policy_retry_at: None,
            expired: false,
            generation: 0,
            deleting: false,
        };
        state.generation = u64::from(state.is_authenticated());
        Self {
            core: Arc::new(SessionCore {
                library,
                shell,
                state: RwLock::new(state),
                operation_gate: Mutex::new(()),
            }),
        }
    }

    fn read(&self) -> SessionState {
        self.core.read()
    }

    pub fn server_url(&self) -> Option<String> {
        self.read().server_url
    }

    pub fn account_key(&self) -> Option<AccountKey> {
        let state = self.read();
        AccountKey::new(state.server_id?, state.user_id?)
    }

    pub fn generation(&self) -> u64 {
        self.read().generation
    }

    pub fn is_authenticated(&self) -> bool {
        self.read().is_authenticated()
    }

    pub fn user_restricted(&self) -> bool {
        self.read().access.restricted
    }

    pub fn refresh_user_policy(&self) {
        let scope = {
            let _gate = self
                .core
                .operation_gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut state = self
                .core
                .state
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            if matches!(
                state.policy_state,
                PolicyState::Verified | PolicyState::Refreshing
            ) {
                return;
            }
            let Ok(scope) = state.to_scope() else {
                return;
            };
            state.policy_state = PolicyState::Refreshing;
            drop(state);
            scope
        };
        self.core.run_policy_refresh(&scope, false);
    }

    pub fn scope(&self) -> Result<SessionScope, ApiError> {
        let _gate = self
            .core
            .operation_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.read().to_scope()
    }

    pub fn scope_is_current(&self, scope: &SessionScope) -> bool {
        let _gate = self
            .core
            .operation_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.read().matches(scope)
    }

    pub fn commit_if_current<T, E>(
        &self,
        scope: &SessionScope,
        stale: impl FnOnce() -> E,
        commit: impl FnOnce(&Live) -> Result<T, E>,
    ) -> Result<T, E> {
        self.core.commit_if_current(scope, stale, commit)
    }

    pub fn confirm<T>(&self, scoped: Scoped<T>) -> Result<T, ApiError> {
        let (result, retry_scope) = {
            let _gate = self
                .core
                .operation_gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut state = self
                .core
                .state
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            let outcome = if state.is_current(scoped.generation, &scoped.account) {
                let retry_scope = if state.policy_state == PolicyState::PendingRetry
                    && state
                        .next_policy_retry_at
                        .is_none_or(|at| Instant::now() >= at)
                {
                    match state.to_scope() {
                        Ok(scope) => {
                            state.policy_state = PolicyState::Refreshing;
                            Some(scope)
                        }
                        Err(_) => None,
                    }
                } else {
                    None
                };
                (Ok(scoped.value), retry_scope)
            } else if state.expired_itself(scoped.generation, &scoped.account) {
                (Err(ApiError::Unauthorized), None)
            } else {
                (Err(ApiError::AccountChanged), None)
            };
            drop(state);
            outcome
        };
        if let Some(scope) = retry_scope {
            let core = Arc::clone(&self.core);
            crate::app::threads::spawn_named("session-policy-retry", move || {
                core.run_policy_refresh(&scope, true);
            });
        }
        result
    }

    pub fn fetch_current<T>(
        &self,
        scope: &SessionScope,
        call: impl FnOnce(&JellyfinClient, &str) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let fetched = scope
            .fetch(call)
            .map_err(|error| self.scoped_error(scope, error))?;
        self.confirm(fetched)
    }

    pub fn anonymous_client(&self, server_url: &str) -> JellyfinClient {
        JellyfinClient::new(server_url, &self.read().device_id, None)
    }

    pub fn connect(
        &self,
        server_url: &str,
        cancelled: Option<Arc<AtomicBool>>,
    ) -> Result<ServerInfo, ApiError> {
        let normalized = normalize_server_url(server_url).ok_or(ApiError::NotConfigured)?;
        let mut client = self.anonymous_client(&normalized);
        if let Some(cancelled) = cancelled {
            client = client.with_cancellation(cancelled);
        }
        let (server_name, version) = identify(&client)?;
        if client.is_cancelled() {
            return Err(ApiError::Cancelled);
        }
        let too_old = version.short_of(ServerVersion::DESKTOP_MINIMUM);
        let quick_connect = too_old.is_none() && auth::quick_connect_enabled(&client);
        if client.is_cancelled() {
            return Err(ApiError::Cancelled);
        }
        Ok(ServerInfo {
            server_url: normalized,
            server_name,
            version,
            quick_connect,
            too_old,
        })
    }

    pub fn login(
        &self,
        server_url: &str,
        username: &str,
        password: &str,
    ) -> Result<SessionStatus, ApiError> {
        let normalized = normalize_server_url(server_url).ok_or(ApiError::NotConfigured)?;
        let client = self.anonymous_client(&normalized);
        require_supported(&client)?;
        let credentials = auth::authenticate_by_name(&client, username, password)?;
        self.accept(&normalized, credentials, None)
    }

    pub fn quick_connect_start(&self, server_url: &str) -> Result<QuickConnectStart, ApiError> {
        let normalized = normalize_server_url(server_url).ok_or(ApiError::NotConfigured)?;
        let client = self.anonymous_client(&normalized);
        require_supported(&client)?;
        if !auth::quick_connect_enabled(&client) {
            return Err(ApiError::Status { status: 501 });
        }
        let result = auth::quick_connect_initiate(&client)?;
        Ok(QuickConnectStart {
            server_url: normalized,
            code: result.code,
            secret: result.secret,
        })
    }

    pub fn quick_connect_poll(
        &self,
        server_url: &str,
        secret: &str,
        cancelled: Option<Arc<AtomicBool>>,
    ) -> Result<QuickConnectPoll, ApiError> {
        let normalized = normalize_server_url(server_url).ok_or(ApiError::NotConfigured)?;
        let mut client = self.anonymous_client(&normalized);
        if let Some(cancelled) = cancelled {
            client = client.with_cancellation(cancelled);
        }
        if client.is_cancelled() {
            return Err(ApiError::Cancelled);
        }
        let state = auth::quick_connect_state(&client, secret)?;
        if client.is_cancelled() {
            return Err(ApiError::Cancelled);
        }
        if !state.authenticated {
            return Ok(QuickConnectPoll {
                authenticated: false,
                session: None,
            });
        }
        let credentials = auth::authenticate_with_quick_connect(&client, secret)?;
        if client.is_cancelled() {
            return Err(ApiError::Cancelled);
        }
        let session = self.accept(&normalized, credentials, Some(&client))?;
        Ok(QuickConnectPoll {
            authenticated: true,
            session: Some(session),
        })
    }

    fn accept(
        &self,
        server_url: &str,
        credentials: Credentials,
        cancel_guard: Option<&JellyfinClient>,
    ) -> Result<SessionStatus, ApiError> {
        let user_name = credentials.user_name.clone();
        let cleared_cache = self.commit_credentials(server_url, credentials, cancel_guard)?;
        if cleared_cache {
            tracing::info!(
                target: "jellyfin.session",
                "cleared the metadata cache for a new Jellyfin account"
            );
        }
        tracing::info!(
            target: "jellyfin.session",
            user = %user_name,
            "authenticated against Jellyfin"
        );
        Ok(self.status())
    }

    fn commit_credentials(
        &self,
        server_url: &str,
        credentials: Credentials,
        cancel_guard: Option<&JellyfinClient>,
    ) -> Result<bool, ApiError> {
        let _gate = self
            .core
            .operation_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if cancel_guard.is_some_and(JellyfinClient::is_cancelled) {
            return Err(ApiError::Cancelled);
        }
        let previous = self.core.library.credentials();
        let cache_owner = self
            .core
            .library
            .cache_owner()
            .or_else(|| previous.server_id.clone().zip(previous.user_id.clone()));
        let next_owner = (credentials.server_id.clone(), credentials.user_id.clone());
        let switched_account = cache_owner
            .as_ref()
            .is_some_and(|owner| owner != &next_owner)
            || cache_owner.is_none() && self.core.library.stats().total > 0;
        if switched_account {
            self.core
                .library
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
        self.core
            .library
            .save_credentials(&stored)
            .map_err(|error| storage_error(&error))?;

        if let Ok(mut state) = self.core.state.write() {
            state.server_url = Some(server_url.to_string());
            state.server_id = Some(credentials.server_id);
            state.user_id = Some(credentials.user_id);
            state.user_name = Some(credentials.user_name);
            state.token = Some(credentials.token);
            state.access = credentials.access;
            state.policy_state = PolicyState::Verified;
            state.next_policy_retry_at = None;
            state.expired = false;
            state.deleting = false;
            state.generation = next_generation(state.generation);
        }
        Ok(switched_account)
    }

    pub fn capture_logout(&self) -> LogoutTarget {
        let _gate = self
            .core
            .operation_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let state = self.read();
        let scope = state.to_scope().ok();
        LogoutTarget {
            generation: state.generation,
            scope,
        }
    }

    pub fn logout_target_is_current(&self, target: &LogoutTarget) -> bool {
        let _gate = self
            .core
            .operation_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.read().matches_logout(target)
    }

    pub fn logout(&self, target: &LogoutTarget, forget_library: bool) -> Result<(), ApiError> {
        if !self.logout_target_is_current(target) {
            return Err(ApiError::AccountChanged);
        }
        if let Some(scope) = &target.scope
            && let Err(error) = auth::logout(scope.client())
        {
            tracing::debug!(target: "jellyfin.session", "server-side logout failed: {error}");
        }
        {
            let _gate = self
                .core
                .operation_gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !self.read().matches_logout(target) {
                return Err(ApiError::AccountChanged);
            }
            self.core
                .clear_local_locked(forget_library)
                .map_err(|error| storage_error(&error))?;
        }
        tracing::info!(target: "jellyfin.session", "signed out of Jellyfin");
        Ok(())
    }

    pub fn clear_local(&self, forget_library: bool) -> rusqlite::Result<()> {
        let _gate = self
            .core
            .operation_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.core.clear_local_locked(forget_library)
    }

    pub fn begin_account_deletion(&self, account: &AccountKey) -> bool {
        let _gate = self
            .core
            .operation_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Ok(mut state) = self.core.state.write() else {
            return false;
        };
        if state.account_key().as_ref() != Some(account) {
            return false;
        }
        state.generation = next_generation(state.generation);
        state.deleting = true;
        true
    }

    pub fn note_scoped_error(&self, scope: &SessionScope, error: &ApiError) {
        drop(self.scoped_error(scope, error.clone()));
    }

    pub fn scoped_error(&self, scope: &SessionScope, error: ApiError) -> ApiError {
        let expired = {
            let _gate = self
                .core
                .operation_gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let Ok(mut state) = self.core.state.write() else {
                return error;
            };
            if !state.owns_failure(scope) {
                return ApiError::AccountChanged;
            }
            if error != ApiError::Unauthorized || state.expired {
                return error;
            }
            state.expired = true;
            state.generation = next_generation(state.generation);
            true
        };
        if expired {
            tracing::warn!(
                target: "jellyfin.session",
                "the Jellyfin server rejected the stored token; re-authentication required"
            );
            self.core.shell.session_expired();
        }
        error
    }

    pub fn status(&self) -> SessionStatus {
        let state = self.read();
        SessionStatus {
            authenticated: state.is_authenticated(),
            expired: state.expired,
            server_url: state.server_url,
            server_name: None,
            user_id: state.user_id,
            user_name: state.user_name,
            device_id: state.device_id,
            administrator: state.access.administrator,
        }
    }
}

impl SessionState {
    fn authenticated_parts(&self) -> Result<AuthenticatedParts<'_>, ApiError> {
        if self.expired || self.deleting {
            return Err(ApiError::Unauthorized);
        }
        let server_url = self
            .server_url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(ApiError::NotConfigured)?;
        let token = self
            .token
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(ApiError::NotConfigured)?;
        let user_id = self.user_id.as_deref().ok_or(ApiError::NotConfigured)?;
        let server_id = self.server_id.as_deref().ok_or(ApiError::NotConfigured)?;
        let account = AccountKey::new(server_id, user_id).ok_or(ApiError::NotConfigured)?;
        Ok(AuthenticatedParts {
            server_url,
            token,
            user_id,
            account,
        })
    }

    fn is_authenticated(&self) -> bool {
        self.authenticated_parts().is_ok()
    }

    fn to_scope(&self) -> Result<SessionScope, ApiError> {
        let parts = self.authenticated_parts()?;
        Ok(SessionScope {
            generation: self.generation,
            account: parts.account,
            client: JellyfinClient::new(parts.server_url, &self.device_id, Some(parts.token)),
            user_id: parts.user_id.to_string(),
        })
    }

    fn account_key(&self) -> Option<AccountKey> {
        AccountKey::new(self.server_id.clone()?, self.user_id.clone()?)
    }

    fn matches(&self, scope: &SessionScope) -> bool {
        self.is_current(scope.generation, scope.account())
    }

    fn is_current(&self, generation: u64, account: &AccountKey) -> bool {
        !self.expired
            && !self.deleting
            && self.generation == generation
            && self.account_key().as_ref() == Some(account)
    }

    fn owns_failure(&self, scope: &SessionScope) -> bool {
        self.matches(scope) || self.expired_itself(scope.generation, scope.account())
    }

    fn expired_itself(&self, generation: u64, account: &AccountKey) -> bool {
        self.expired
            && !self.deleting
            && self.generation == next_generation(generation)
            && self.account_key().as_ref() == Some(account)
    }

    fn matches_logout(&self, target: &LogoutTarget) -> bool {
        self.generation == target.generation
            || target
                .scope
                .as_ref()
                .is_some_and(|scope| self.expired_itself(scope.generation, scope.account()))
    }
}

fn identify(client: &JellyfinClient) -> Result<(String, ServerVersion), ApiError> {
    let info = auth::public_system_info(client)?;
    let version = info.version.ok_or_else(|| {
        ApiError::Decode(
            "the server did not report its Jellyfin version; if it is starting up, try again in a moment"
                .to_string(),
        )
    })?;
    Ok((info.server_name, version))
}

fn require_supported(client: &JellyfinClient) -> Result<(), ApiError> {
    let (_, version) = identify(client)?;
    match version.short_of(ServerVersion::DESKTOP_MINIMUM) {
        Some(too_old) => Err(ApiError::ServerTooOld(too_old)),
        None => Ok(()),
    }
}

fn next_generation(generation: u64) -> u64 {
    generation.wrapping_add(1).max(1)
}

fn storage_error(error: &rusqlite::Error) -> ApiError {
    ApiError::Storage(format!("session storage: {error}"))
}

#[cfg(test)]
mod tests;
