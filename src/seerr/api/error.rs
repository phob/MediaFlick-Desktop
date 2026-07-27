//! Seerr failures.
//!
//! Deliberately not [`ApiError`]: that type's `Display` names Jellyfin, so a
//! Seerr outage reported through it would read as "could not reach Jellyfin",
//! and a Seerr session expiry would look like a Jellyfin one to the UI.
//!
//! [`ApiError`]: crate::jellyfin::api::ApiError

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeerrError {
    /// No Seerr instance is configured, or the address given is not usable as
    /// an HTTP(S) one.
    NotConfigured,
    /// Seerr rejected the session cookie. Note that 403 is *not* folded in
    /// here: Seerr answers 403 to a user it has never imported, which needs its
    /// own message, and it answers 401 to a valid session that merely lacks a
    /// permission — so a 401 alone does not prove the session is gone.
    Unauthorized,
    /// The instance answered, but cannot be used: its setup wizard is
    /// unfinished, or it is wired to a media server that is not Jellyfin.
    Unusable(String),
    /// Seerr refused the credentials offered while linking. Kept apart from
    /// [`Self::Unauthorized`] so a mistyped password is not reported to the UI
    /// as a lapsed session — the user is *establishing* one.
    LoginRejected,
    /// Seerr has no account for this user and will not create one on the fly,
    /// which is the 403 its login answers when "enable new sign-ins" is off.
    /// A generic login failure would send the user hunting for a typo that is
    /// not there; only an administrator can resolve this one.
    UnknownUser,
    /// The session is fine but the account may not do this. Seerr answers 401
    /// here as well as for a lapsed cookie, so this is what a 401 becomes once
    /// `/auth/me` has confirmed the session is still good — the disambiguation
    /// that keeps a refused cancellation from logging the user out.
    PermissionDenied,
    /// Any other non-success HTTP status.
    Status { status: u16 },
    /// Connection, DNS, TLS, or timeout failure.
    Transport(String),
    /// The response body did not match the expected shape.
    Decode(String),
}

impl SeerrError {
    /// Whether retrying the same request could plausibly succeed. Only ever
    /// consulted for GETs; see [`SeerrClient`](super::client::SeerrClient).
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Status { status } => *status >= 500 || *status == 429,
            Self::Unauthorized
            | Self::Unusable(_)
            | Self::LoginRejected
            | Self::UnknownUser
            | Self::PermissionDenied
            | Self::Decode(_)
            | Self::NotConfigured => false,
        }
    }

    /// HTTP status to surface to our own UI for this failure.
    pub fn client_status(&self) -> u16 {
        match self {
            Self::Unauthorized | Self::LoginRejected => 401,
            Self::UnknownUser | Self::PermissionDenied => 403,
            Self::Status { status } => *status,
            Self::NotConfigured | Self::Unusable(_) => 409,
            Self::Transport(_) | Self::Decode(_) => 502,
        }
    }
}

impl fmt::Display for SeerrError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => write!(formatter, "the Seerr server rejected the session"),
            Self::Unusable(message) => write!(formatter, "{message}"),
            Self::LoginRejected => write!(formatter, "Seerr rejected that username or password"),
            Self::UnknownUser => write!(
                formatter,
                "Seerr has no account for this user. Ask your Seerr administrator to import it, \
                 or to allow new sign-ins."
            ),
            Self::PermissionDenied => {
                write!(formatter, "your Seerr account is not allowed to do that")
            }
            Self::Status { status } => write!(formatter, "Seerr returned HTTP {status}"),
            Self::Transport(message) => write!(formatter, "could not reach Seerr: {message}"),
            Self::Decode(message) => write!(formatter, "unexpected Seerr response: {message}"),
            Self::NotConfigured => write!(formatter, "no Seerr server is configured"),
        }
    }
}

impl std::error::Error for SeerrError {}

#[cfg(test)]
mod tests {
    use super::SeerrError;

    #[test]
    fn only_transport_and_server_errors_are_retryable() {
        assert!(SeerrError::Transport("reset".to_string()).is_retryable());
        assert!(SeerrError::Status { status: 503 }.is_retryable());
        assert!(SeerrError::Status { status: 429 }.is_retryable());
        assert!(!SeerrError::Status { status: 404 }.is_retryable());
        assert!(!SeerrError::Unauthorized.is_retryable());
        assert!(!SeerrError::Unusable("setup".to_string()).is_retryable());
        assert!(!SeerrError::LoginRejected.is_retryable());
        assert!(!SeerrError::UnknownUser.is_retryable());
        assert!(!SeerrError::PermissionDenied.is_retryable());
    }

    /// Seerr answers 401 both to a lapsed session and to a permission it will
    /// not grant, so the two must stay distinguishable all the way to the UI:
    /// only one of them means "sign in again".
    #[test]
    fn a_refused_permission_is_not_a_lapsed_session() {
        assert_ne!(SeerrError::PermissionDenied, SeerrError::Unauthorized);
        assert_eq!(SeerrError::PermissionDenied.client_status(), 403);
        assert!(
            SeerrError::PermissionDenied
                .to_string()
                .contains("not allowed")
        );
    }

    /// A failed *link* is not a lapsed session: only `Unauthorized` sets the
    /// re-link flag, so the two must stay distinct values as well as messages.
    #[test]
    fn a_refused_login_is_not_the_same_value_as_a_lapsed_session() {
        assert_ne!(SeerrError::LoginRejected, SeerrError::Unauthorized);
        assert_eq!(SeerrError::LoginRejected.client_status(), 401);
        assert_eq!(SeerrError::UnknownUser.client_status(), 403);
        assert!(
            SeerrError::UnknownUser
                .to_string()
                .contains("administrator")
        );
    }

    #[test]
    fn failures_map_to_our_own_api_statuses() {
        assert_eq!(SeerrError::Unauthorized.client_status(), 401);
        assert_eq!(SeerrError::NotConfigured.client_status(), 409);
        assert_eq!(
            SeerrError::Unusable("setup".to_string()).client_status(),
            409
        );
        assert_eq!(SeerrError::Status { status: 404 }.client_status(), 404);
        assert_eq!(SeerrError::Transport("x".to_string()).client_status(), 502);
    }

    #[test]
    fn messages_never_name_jellyfin() {
        for error in [
            SeerrError::Unauthorized,
            SeerrError::Status { status: 500 },
            SeerrError::Transport("reset".to_string()),
            SeerrError::Decode("bad".to_string()),
            SeerrError::NotConfigured,
            SeerrError::LoginRejected,
            SeerrError::UnknownUser,
            SeerrError::PermissionDenied,
        ] {
            assert!(
                !error.to_string().to_ascii_lowercase().contains("jellyfin"),
                "{error}"
            );
        }
    }
}
