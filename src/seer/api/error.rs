//! Seer failures.
//!
//! Deliberately not [`ApiError`]: that type's `Display` names Jellyfin, so a
//! Seer outage reported through it would read as "could not reach Jellyfin",
//! and a Seer session expiry would look like a Jellyfin one to the UI.
//!
//! [`ApiError`]: crate::jellyfin::api::ApiError

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeerError {
    /// No Seer instance is configured, or the address given is not usable as
    /// an HTTP(S) one.
    NotConfigured,
    /// Seer rejected the session cookie. Note that 403 is *not* folded in
    /// here: Seer answers 403 to a user it has never imported, which needs its
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

impl SeerError {
    /// Whether retrying the same request could plausibly succeed. Only ever
    /// consulted for GETs; see [`SeerClient`](super::client::SeerClient).
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

impl fmt::Display for SeerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => write!(formatter, "the Seer server rejected the session"),
            Self::Unusable(message) => write!(formatter, "{message}"),
            Self::Status { status } => write!(formatter, "Seer returned HTTP {status}"),
            Self::Transport(message) => write!(formatter, "could not reach Seer: {message}"),
            Self::Decode(message) => write!(formatter, "unexpected Seer response: {message}"),
            Self::NotConfigured => write!(formatter, "no Seer server is configured"),
        }
    }
}

impl std::error::Error for SeerError {}

#[cfg(test)]
mod tests {
    use super::SeerError;

    #[test]
    fn only_transport_and_server_errors_are_retryable() {
        assert!(SeerError::Transport("reset".to_string()).is_retryable());
        assert!(SeerError::Status { status: 503 }.is_retryable());
        assert!(SeerError::Status { status: 429 }.is_retryable());
        assert!(!SeerError::Status { status: 404 }.is_retryable());
        assert!(!SeerError::Unauthorized.is_retryable());
        assert!(!SeerError::Unusable("setup".to_string()).is_retryable());
    }

    #[test]
    fn failures_map_to_our_own_api_statuses() {
        assert_eq!(SeerError::Unauthorized.client_status(), 401);
        assert_eq!(SeerError::NotConfigured.client_status(), 409);
        assert_eq!(
            SeerError::Unusable("setup".to_string()).client_status(),
            409
        );
        assert_eq!(SeerError::Status { status: 404 }.client_status(), 404);
        assert_eq!(SeerError::Transport("x".to_string()).client_status(), 502);
    }

    #[test]
    fn messages_never_name_jellyfin() {
        for error in [
            SeerError::Unauthorized,
            SeerError::Status { status: 500 },
            SeerError::Transport("reset".to_string()),
            SeerError::Decode("bad".to_string()),
            SeerError::NotConfigured,
        ] {
            assert!(
                !error.to_string().to_ascii_lowercase().contains("jellyfin"),
                "{error}"
            );
        }
    }
}
