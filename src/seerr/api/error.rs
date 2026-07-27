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
            Self::Unauthorized | Self::Unusable(_) | Self::Decode(_) | Self::NotConfigured => false,
        }
    }

    /// HTTP status to surface to our own UI for this failure.
    pub fn client_status(&self) -> u16 {
        match self {
            Self::Unauthorized => 401,
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
        ] {
            assert!(
                !error.to_string().to_ascii_lowercase().contains("jellyfin"),
                "{error}"
            );
        }
    }
}
