use serde_json::{Value, json};

use crate::jellyfin::api::JellyfinClient;
use crate::jellyfin::api::auth as jellyfin_auth;
use crate::library::StoredCredentials;

use super::{SeerrSession, probe_public};
use crate::seerr::api::client::SeerrClient;
use crate::seerr::api::error::SeerrError;
use crate::seerr::api::model::{PublicSettings, QuickConnectHandshake, SeerrUser};

/// Seerr's own login, present in every release.
const LOGIN_PATH: &str = "auth/jellyfin";
/// The Quick Connect login pair, present only on builds newer than v3.3.0.
const QUICK_CONNECT_INITIATE: &str = "auth/jellyfin/quickconnect/initiate";
const QUICK_CONNECT_AUTHENTICATE: &str = "auth/jellyfin/quickconnect/authenticate";

impl SeerrSession {
    /// Links with the user's media-server password — the path every released
    /// Seerr has, and therefore the one the UI must always be able to fall back
    /// to. The password is used once and never stored; what is kept is the
    /// session cookie Seerr answers with.
    pub fn link_with_password(&self, username: &str, password: &str) -> Result<Value, SeerrError> {
        let username = username.trim();
        if username.is_empty() || password.is_empty() {
            return Err(SeerrError::Unusable(
                "enter the username and password of your media-server account".to_string(),
            ));
        }
        let client = self.link_client()?;
        // One GET before the write: it re-checks the instance is still usable
        // and refreshes a CSRF pair that may have rotated since the last run.
        let settings = probe_public(&client)?;
        let _: Value = client
            .post_json(
                LOGIN_PATH,
                &json!({ "username": username, "password": password }),
            )
            .map_err(map_login_error)?;
        let status = self.finish_link(&client, Some(&settings))?;
        Ok(json!({ "method": "password", "linked": true, "status": status }))
    }

    /// Links without a password, when both halves of the flow support it.
    ///
    /// Seerr starts a Quick Connect handshake against the same Jellyfin server,
    /// we approve its code as an already-authenticated client of that server,
    /// and Seerr redeems it for a session. This needs a Seerr new enough to
    /// carry the routes — they are absent from v3.3.0, the latest stable
    /// release — *and* Quick Connect enabled on the Jellyfin server, which is
    /// off by default. Every step therefore probes and falls back to the
    /// password path instead of surfacing an error the user cannot act on.
    pub fn link_start(&self) -> Result<Value, SeerrError> {
        let client = self.link_client()?;
        let settings = probe_public(&client)?;
        let Some(jellyfin) = self.jellyfin_client() else {
            return Ok(password_required(
                "there is no Jellyfin session to approve with",
            ));
        };
        // Asked before Seerr is involved, so a server with Quick Connect off
        // never leaves an orphaned handshake behind.
        if !jellyfin_auth::quick_connect_enabled(&jellyfin) {
            return Ok(password_required(
                "Quick Connect is turned off on the Jellyfin server",
            ));
        }
        let handshake: QuickConnectHandshake =
            match client.post_json(QUICK_CONNECT_INITIATE, &json!({})) {
                Ok(handshake) => handshake,
                // An unreachable instance is not a missing feature: the password
                // path would fail the same way, so it is reported rather than hidden.
                Err(error @ SeerrError::Transport(_)) => return Err(error),
                Err(error) => {
                    return Ok(password_required(&format!(
                        "this Seerr has no Quick Connect login ({error})"
                    )));
                }
            };
        if !handshake.is_usable() {
            return Ok(password_required(
                "Seerr answered without a usable handshake",
            ));
        }
        // The one call that makes this password-less, and the one that proves
        // the handshake belongs to *our* server: a code minted elsewhere is not
        // one this server will approve.
        match jellyfin_auth::quick_connect_authorize(&jellyfin, &handshake.code) {
            Ok(true) => {}
            Ok(false) => {
                return Ok(password_required(
                    "the Jellyfin server did not recognize the Seerr handshake",
                ));
            }
            Err(error) => {
                return Ok(password_required(&format!(
                    "the Jellyfin server would not approve the handshake ({error})"
                )));
            }
        }
        // The initiate may have rotated the CSRF pair, and the poll that can
        // follow is a separate request built from what is stored.
        self.persist_cookies(&client);
        self.complete_quick_connect(&client, &handshake.secret, Some(&settings))
    }

    /// Retries the redemption half of [`Self::link_start`]. The code is already
    /// approved by the time that call returns, so this exists for a Seerr that
    /// has not caught up yet rather than for a user who has still to act.
    pub fn link_poll(&self, secret: &str) -> Result<Value, SeerrError> {
        let secret = secret.trim();
        if secret.is_empty() {
            return Err(SeerrError::Unusable(
                "there is no Quick Connect attempt to finish".to_string(),
            ));
        }
        let client = self.link_client()?;
        let settings = probe_public(&client)?;
        self.complete_quick_connect(&client, secret, Some(&settings))
    }

    fn complete_quick_connect(
        &self,
        client: &SeerrClient,
        secret: &str,
        settings: Option<&PublicSettings>,
    ) -> Result<Value, SeerrError> {
        match client.post_json::<_, Value>(QUICK_CONNECT_AUTHENTICATE, &json!({ "secret": secret }))
        {
            Ok(_) => {}
            // Seerr has not observed the approval yet. The caller polls; it is
            // not told to type a password, because none is needed.
            Err(SeerrError::Status { status }) if is_pending(status) => {
                return Ok(json!({
                    "method": "quickconnect",
                    "linked": false,
                    "secret": secret,
                }));
            }
            Err(error) => return Err(map_login_error(error)),
        }
        let status = self.finish_link(client, settings)?;
        Ok(json!({ "method": "quickconnect", "linked": true, "status": status }))
    }

    /// Ends the session at the instance, then forgets it locally. The address
    /// stays, so re-linking does not mean retyping it.
    pub fn unlink(&self) -> Value {
        self.revalidate();
        self.discard_remote_session(&self.read());
        self.unlink_locally();
        tracing::info!(target: "seerr.session", "unlinked from Seerr");
        self.status()
    }

    /// Turns a session Seerr has just handed us into a stored link, or refuses
    /// it outright.
    ///
    /// The guard is the point of this function. `/api/v1/status` exposes no
    /// media-server identity and every `/settings` route is admin-only, so an
    /// unauthenticated probe cannot tell which Jellyfin server an instance is
    /// wired to. What *can* be checked is the account behind the session that
    /// was just established — and if it is not the one this app is signed in
    /// as, nothing about it is worth keeping.
    fn finish_link(
        &self,
        client: &SeerrClient,
        settings: Option<&PublicSettings>,
    ) -> Result<Value, SeerrError> {
        let user: SeerrUser = client.get_json("auth/me", &[]).map_err(map_login_error)?;
        let credentials = self.library.credentials();
        let Some(signed_in_as) = credentials.user_id.clone() else {
            return Err(self.refuse(client, "this app is not signed in to Jellyfin"));
        };
        let Some(linked_to) = user.jellyfin_user_id.clone() else {
            return Err(self.refuse(
                client,
                "Seerr did not say which media-server account that login belongs to",
            ));
        };
        if !same_media_server_user(&linked_to, &signed_in_as) {
            return Err(self.refuse(
                client,
                "that Seerr account belongs to a different media-server user \
                 than the one signed in here",
            ));
        }

        let name = user.preferred_name().to_string();
        if !self.store_link(client, &user, &credentials, settings) {
            return Err(self.refuse(
                client,
                "the stored Seerr link changed while signing in; try again",
            ));
        }
        tracing::info!(target: "seerr.session", user = %name, "linked to Seerr");
        Ok(self.status())
    }

    /// Fails closed: a session that will not be kept is ended at the instance
    /// too, and nothing about it reaches the database.
    fn refuse(&self, client: &SeerrClient, message: &str) -> SeerrError {
        if let Err(error) = client.post_empty("auth/logout") {
            tracing::debug!(
                target: "seerr.session",
                "could not discard the refused Seerr session: {error}"
            );
        }
        tracing::warn!(target: "seerr.session", "refused a Seerr link: {message}");
        SeerrError::Unusable(message.to_string())
    }

    /// Writes the new session against the link as it now stands, retrying a
    /// revision that moved underneath — but only while it is still the same
    /// instance, since a link that moved elsewhere must not be overwritten.
    fn store_link(
        &self,
        client: &SeerrClient,
        user: &SeerrUser,
        credentials: &StoredCredentials,
        settings: Option<&PublicSettings>,
    ) -> bool {
        for _ in 0..3 {
            let mut next = self.read();
            if next.base_url.as_deref() != Some(client.base_url()) {
                return false;
            }
            next.cookies = client.cookies();
            next.user_id = Some(user.id);
            next.user_name = Some(user.preferred_name().to_string());
            next.jellyfin_server_id.clone_from(&credentials.server_id);
            next.jellyfin_user_id.clone_from(&credentials.user_id);
            if let Some(settings) = settings {
                next.movie_4k_enabled = settings.movie_4k_enabled;
                next.series_4k_enabled = settings.series_4k_enabled;
                next.partial_requests_enabled = settings.partial_requests_enabled;
            }
            next.expired = false;
            if self.commit(next) {
                return true;
            }
        }
        false
    }

    /// A client for the configured instance whether or not a session is held —
    /// unlike [`Self::client`], which refuses once one has lapsed. Linking is
    /// exactly what a lapsed session needs.
    fn link_client(&self) -> Result<SeerrClient, SeerrError> {
        self.revalidate();
        let state = self.read();
        let base_url = state.base_url.ok_or(SeerrError::NotConfigured)?;
        Ok(SeerrClient::new(&base_url, state.cookies))
    }

    /// The Jellyfin client used to approve a Quick Connect code. Built from the
    /// stored credentials rather than from [`Session`](crate::jellyfin::session::Session),
    /// which keeps this module free of a dependency on it — the same row
    /// [`Self::revalidate`] already reads.
    fn jellyfin_client(&self) -> Option<JellyfinClient> {
        let credentials = self.library.credentials();
        let (Some(server_url), Some(token)) = (credentials.server_url, credentials.token) else {
            return None;
        };
        Some(JellyfinClient::new(
            &server_url,
            &credentials.device_id,
            Some(&token),
        ))
    }

    /// Keeps a rotated CSRF pair across a call boundary, so a request that
    /// arrives later — and rebuilds its client from storage — still has it.
    fn persist_cookies(&self, client: &SeerrClient) {
        let revision = self.read().revision;
        self.absorb_probe(client, revision);
    }
}

/// The answer that sends the UI to the password form. The reason is logged
/// rather than shown: a Quick Connect that is merely unavailable is not a
/// failure the user can act on, and every release supports the password path.
fn password_required(reason: &str) -> Value {
    tracing::debug!(target: "seerr.session", "falling back to Seerr password login: {reason}");
    json!({ "method": "password", "linked": false })
}

/// Failures of a *login*, which are not failures of a session.
///
/// A 401 here is a rejected credential, not a lapsed cookie, and must not put
/// the UI into the re-link prompt it is already in. A 403 is Seerr's answer for
/// a user it has never imported while "enable new sign-ins" is off — something
/// only an administrator can fix, so it says so.
fn map_login_error(error: SeerrError) -> SeerrError {
    match error {
        SeerrError::Unauthorized => SeerrError::LoginRejected,
        SeerrError::Status { status: 403 } => SeerrError::UnknownUser,
        other => other,
    }
}

/// Whether a refused Quick Connect redemption is worth polling again. Seerr
/// answers 404/405 when the route is absent and 401/403 when the login itself
/// was refused; those are answers, not delays.
fn is_pending(status: u16) -> bool {
    matches!(status, 400 | 409 | 425)
}

/// Compares two media-server user ids. Jellyfin hands its GUIDs out both with
/// and without dashes depending on the endpoint and version, so a plain string
/// comparison would reject a match — and, worse, look like the account switch
/// this guard exists to catch.
pub(super) fn same_media_server_user(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|character| *character != '-')
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let (left, right) = (normalize(left), normalize(right));
    !left.is_empty() && left == right
}
