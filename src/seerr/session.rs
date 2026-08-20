mod auth;
mod catalog;
mod requests;

#[cfg(test)]
mod tests;

pub use requests::RequestProfileSelection;

use std::sync::{Arc, RwLock};

use serde_json::{Value, json};

use crate::library::{Library, SeerrConfig};
use crate::preferences::normalize_server_url;

use super::api::client::{SeerrClient, SessionCookies};
use super::api::error::SeerrError;
use super::api::model::{
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
        let settings = probe_public(&client)?;
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
}

impl SeerrSession {
    fn call<T>(
        &self,
        call: impl FnOnce(&SeerrClient) -> Result<T, SeerrError>,
    ) -> Result<T, SeerrError> {
        let client = self.client()?;
        let revision = self.read().revision;
        let result = call(&client);
        if !matches!(result, Err(SeerrError::Unauthorized)) {
            self.absorb_probe(&client, revision);
            return result;
        }
        match client.get_json::<SeerrUser>("auth/me", &[]) {
            Err(SeerrError::Unauthorized) => {
                if self.probe_is_current(revision) {
                    self.mark_expired();
                }
                Err(SeerrError::Unauthorized)
            }
            // The session answered for itself, so the refusal was about the
            // action, not the cookie.
            _ => {
                self.absorb_probe(&client, revision);
                Err(SeerrError::PermissionDenied)
            }
        }
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

/// Reads `/settings/public` and refuses an instance that must not be logged
/// into.
///
/// `/settings/public` rather than `/status`, because an *uninitialized* Seerr
/// treats `POST /auth/jellyfin` as its setup wizard — it would make whoever
/// logs in the instance owner, with full administrator permissions. This gate
/// is what keeps a client from silently becoming that wizard.
///
/// It is also the GET that every write is preceded by: a CSRF-protected
/// instance hands out its `_csrf` / `XSRF-TOKEN` pair here, and the very next
/// POST already needs it.
pub(super) fn probe_public(client: &SeerrClient) -> Result<PublicSettings, SeerrError> {
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
    Ok(settings)
}
