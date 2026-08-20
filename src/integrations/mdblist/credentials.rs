use serde_json::{Value, json};

use crate::library::{IntegrationState, now_unix};

use super::schema::known_source_definitions;
use super::transport::{MdbError, Quota};
use super::{Origin, RatingsError, RatingsService};

pub(super) const MDBLIST_CREDENTIAL: &str = "mdblist-api-key";
const TMDB_CREDENTIAL: &str = "tmdb-api-key";
pub(super) const MAX_QUOTA: i64 = 1_000_000_000_000;

impl RatingsService {
    pub fn status(&self, selected_sources: &[String]) -> Value {
        let _ = self.companion.probe(false);
        let mdblist = self.credential_status(MDBLIST_CREDENTIAL);
        let tmdb = self.credential_status(TMDB_CREDENTIAL);
        let plugin = self.companion.supports("ratings-v1");
        let origin = effective_origin(
            mdblist["configured"].as_bool().unwrap_or(false),
            mdblist["valid"].as_bool().unwrap_or(false),
            plugin,
        );
        // Source labels are part of the security boundary. Do not derive a
        // public source definition from server/cache text: a credential-shaped
        // upstream label would otherwise become desktop-visible metadata.
        let sources = known_source_definitions();
        json!({
            "boundaryVersion": 1,
            "auth": {
                "currentMode": "api_key",
                "supportedModes": ["api_key"],
                "futureModes": ["public_pkce"],
            },
            "credentialPrecedence": ["local", "plugin", "none"],
            "effectiveOrigin": origin.map(Origin::as_str).unwrap_or("none"),
            "available": origin.is_some(),
            "selectionEnabled": origin.is_some(),
            "local": { "mdblist": mdblist, "tmdb": tmdb },
            "plugin": {
                "available": plugin,
                "capability": "ratings-v1",
                "boundaryVersion": 1,
                "detail": if plugin {
                    "Compatible server rating capability detected. No administrator credential is exposed."
                } else {
                    "The optional ratings-v1 server capability is not available; local MDBList remains independent."
                },
            },
            "sources": sources,
            "selectedSources": selected_sources,
        })
    }

    pub fn save_credential(
        &self,
        provider: &str,
        secret: &str,
        selected_sources: &[String],
    ) -> Result<Value, RatingsError> {
        let provider = credential_name(provider)?;
        let secret = secret.trim();
        if secret.is_empty()
            || secret.len() > 2048
            || secret.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(RatingsError::new("enter a valid API key"));
        }
        if provider == TMDB_CREDENTIAL && !valid_tmdb_key_shape(secret) {
            self.save_state(&IntegrationState {
                service: provider.to_string(),
                validation: "invalid".to_string(),
                detail: Some(
                    "TMDB keys are normally a 32-character v3 key or a v4 JWT token.".to_string(),
                ),
                updated_at: now_unix(),
                ..IntegrationState::default()
            })?;
            return Ok(self.status(selected_sources));
        }
        self.credentials
            .set(provider, secret)
            .map_err(|error| RatingsError::new(error.to_string()))?;
        if provider == MDBLIST_CREDENTIAL {
            self.validate_key(secret, false)?;
        } else {
            self.save_state(&IntegrationState {
                service: provider.to_string(),
                validation: "saved".to_string(),
                valid: true,
                detail: Some(
                    "Saved securely for future TMDB features. Ratings do not use this key."
                        .to_string(),
                ),
                updated_at: now_unix(),
                ..IntegrationState::default()
            })?;
        }
        Ok(self.status(selected_sources))
    }

    pub fn validate_credential(
        &self,
        provider: &str,
        selected_sources: &[String],
    ) -> Result<Value, RatingsError> {
        let provider = credential_name(provider)?;
        let key = self
            .credentials
            .get(provider)
            .map_err(|error| RatingsError::new(error.to_string()))?
            .ok_or_else(|| RatingsError::new("no credential is saved"))?;
        if provider == MDBLIST_CREDENTIAL {
            self.validate_key(&key, true)?;
        } else {
            self.save_state(&IntegrationState {
                service: provider.to_string(),
                validation: "saved".to_string(),
                valid: valid_tmdb_key_shape(&key),
                detail: Some(
                    "Saved securely for future TMDB features. It is not sent by rating retrieval."
                        .to_string(),
                ),
                updated_at: now_unix(),
                ..IntegrationState::default()
            })?;
        }
        Ok(self.status(selected_sources))
    }

    pub fn reveal_credential(&self, provider: &str) -> Result<String, RatingsError> {
        let provider = credential_name(provider)?;
        self.credentials
            .get(provider)
            .map_err(|error| RatingsError::new(error.to_string()))?
            .ok_or_else(|| RatingsError::new("no credential is saved"))
    }

    pub fn remove_credential(
        &self,
        provider: &str,
        selected_sources: &[String],
    ) -> Result<Value, RatingsError> {
        let provider = credential_name(provider)?;
        self.credentials
            .remove(provider)
            .map_err(|error| RatingsError::new(error.to_string()))?;
        self.library
            .clear_integration_state(provider)
            .map_err(storage_error)?;
        Ok(self.status(selected_sources))
    }

    fn credential_status(&self, name: &str) -> Value {
        let read = self.credentials.get(name);
        let (configured, store_error) = match read {
            Ok(value) => (value.is_some(), false),
            Err(_) => (false, true),
        };
        let state = self
            .library
            .integration_state(name)
            .ok()
            .flatten()
            .unwrap_or_else(|| IntegrationState {
                service: name.to_string(),
                validation: "unchecked".to_string(),
                ..IntegrationState::default()
            });
        let validation = normalized_validation(&state.validation);
        json!({
            "configured": configured,
            "valid": configured
                && state.valid
                && matches!(validation, "valid" | "offline" | "rate_limited" | "unavailable" | "saved"),
            "validation": if configured { validation } else { "absent" },
            "detail": store_error.then_some("The operating-system credential vault could not read this credential.")
                .or_else(|| status_detail(validation, name)),
            "quota": {
                "limit": bounded_nonnegative(state.quota_limit, MAX_QUOTA),
                "remaining": bounded_nonnegative(state.quota_remaining, MAX_QUOTA),
                "resetAt": bounded_timestamp(state.quota_reset_at),
            },
            "retryAt": bounded_timestamp(state.retry_at),
            "lastCheckedAt": (state.updated_at > 0).then_some(state.updated_at).and_then(|value| bounded_timestamp(Some(value))),
            "storage": "os_credential_vault",
            "usedForRatings": name == MDBLIST_CREDENTIAL,
        })
    }

    fn validate_key(&self, key: &str, preserve_valid_on_offline: bool) -> Result<(), RatingsError> {
        let previous = self
            .library
            .integration_state(MDBLIST_CREDENTIAL)
            .map_err(storage_error)?
            .unwrap_or_default();
        let now = now_unix();
        let state = match self.transport.validate(key) {
            Ok(response) => state_with_quota(
                "valid",
                true,
                "Valid MDBList credential.",
                &response.quota,
                0,
                now,
            ),
            Err(MdbError::Unauthorized(quota)) => state_with_quota(
                "invalid",
                false,
                "MDBList rejected this API key (401).",
                &quota,
                0,
                now,
            ),
            Err(MdbError::RateLimited(quota)) => state_with_quota(
                "rate_limited",
                true,
                "MDBList accepted the credential but its request quota is exhausted.",
                &quota,
                previous.failure_count,
                now,
            ),
            Err(MdbError::Transport | MdbError::Decode) => offline_state(
                &previous,
                preserve_valid_on_offline,
                "MDBList could not be reached. The key is saved; retry when online.",
                now,
            ),
            Err(MdbError::Remote { status, quota }) => state_with_quota(
                "unavailable",
                preserve_valid_on_offline && previous.valid,
                &format!("MDBList returned HTTP {status}; retry later."),
                &quota,
                previous.failure_count.saturating_add(1),
                now,
            ),
        };
        self.save_state(&state)
    }

    fn save_state(&self, state: &IntegrationState) -> Result<(), RatingsError> {
        self.library
            .save_integration_state(state)
            .map_err(storage_error)
    }

    pub(super) fn note_quota(&self, quota: &Quota) -> Result<(), RatingsError> {
        let previous = self
            .library
            .integration_state(MDBLIST_CREDENTIAL)
            .map_err(storage_error)?
            .unwrap_or_default();
        let now = now_unix();
        self.save_state(&IntegrationState {
            service: MDBLIST_CREDENTIAL.to_string(),
            validation: "valid".to_string(),
            valid: true,
            detail: Some("Valid MDBList credential.".to_string()),
            quota_limit: quota.limit.or(previous.quota_limit),
            quota_remaining: quota.remaining.or(previous.quota_remaining),
            quota_reset_at: quota.reset_at.or(previous.quota_reset_at),
            retry_at: quota.retry_at(now),
            failure_count: 0,
            updated_at: now,
        })
    }

    pub(super) fn note_fetch_error(&self, error: &MdbError) -> Result<(), RatingsError> {
        let previous = self
            .library
            .integration_state(MDBLIST_CREDENTIAL)
            .map_err(storage_error)?
            .unwrap_or_default();
        let now = now_unix();
        let state = match error {
            MdbError::Unauthorized(quota) => state_with_quota(
                "invalid",
                false,
                "MDBList rejected the saved key (401). Validate or replace it.",
                quota,
                0,
                now,
            ),
            MdbError::RateLimited(quota) => state_with_quota(
                "rate_limited",
                true,
                "MDBList quota exhausted; cached ratings remain visible.",
                quota,
                previous.failure_count,
                now,
            ),
            MdbError::Remote { status, quota } => state_with_quota(
                "unavailable",
                previous.valid,
                &format!("MDBList returned HTTP {status}; cached ratings remain visible."),
                quota,
                previous.failure_count.saturating_add(1),
                now,
            ),
            MdbError::Transport | MdbError::Decode => offline_state(
                &previous,
                true,
                "MDBList is offline or returned an unreadable response; cached ratings remain visible.",
                now,
            ),
        };
        self.save_state(&state)
    }
}

pub(super) fn effective_origin(
    local_configured: bool,
    local_valid: bool,
    plugin: bool,
) -> Option<Origin> {
    if local_configured && local_valid {
        Some(Origin::Local)
    } else if plugin {
        Some(Origin::Plugin)
    } else {
        None
    }
}

fn credential_name(provider: &str) -> Result<&'static str, RatingsError> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "mdblist" => Ok(MDBLIST_CREDENTIAL),
        "tmdb" => Ok(TMDB_CREDENTIAL),
        _ => Err(RatingsError::new("unsupported credential provider")),
    }
}

fn valid_tmdb_key_shape(value: &str) -> bool {
    (value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        || (value.starts_with("eyJ") && value.matches('.').count() == 2 && value.len() <= 2048)
}

fn state_with_quota(
    validation: &str,
    valid: bool,
    detail: &str,
    quota: &Quota,
    failure_count: i64,
    now: i64,
) -> IntegrationState {
    IntegrationState {
        service: MDBLIST_CREDENTIAL.to_string(),
        validation: validation.to_string(),
        valid,
        detail: Some(detail.to_string()),
        quota_limit: quota.limit,
        quota_remaining: quota.remaining,
        quota_reset_at: quota.reset_at,
        retry_at: quota.retry_at(now),
        failure_count,
        updated_at: now,
    }
}

fn offline_state(
    previous: &IntegrationState,
    preserve_valid: bool,
    detail: &str,
    now: i64,
) -> IntegrationState {
    let failures = previous.failure_count.saturating_add(1).min(10);
    let delay = 30_i64.saturating_mul(1_i64 << failures.min(8));
    IntegrationState {
        service: MDBLIST_CREDENTIAL.to_string(),
        validation: "offline".to_string(),
        valid: preserve_valid && previous.valid,
        detail: Some(detail.to_string()),
        retry_at: Some(now.saturating_add(delay.min(6 * 60 * 60))),
        failure_count: failures,
        updated_at: now,
        ..previous.clone()
    }
}

pub(super) fn storage_error(_: rusqlite::Error) -> RatingsError {
    // Database errors may name a file or a rejected payload. Keep the native
    // API diagnostic fixed so neither reaches browser-visible telemetry.
    RatingsError::new("ratings storage is unavailable")
}

fn normalized_validation(validation: &str) -> &'static str {
    match validation {
        "valid" => "valid",
        "invalid" => "invalid",
        "offline" => "offline",
        "rate_limited" => "rate_limited",
        "unavailable" => "unavailable",
        "saved" => "saved",
        _ => "unchecked",
    }
}

fn status_detail(validation: &str, credential: &str) -> Option<&'static str> {
    match validation {
        "valid" if credential == MDBLIST_CREDENTIAL => Some("Valid MDBList credential."),
        "valid" => Some("Credential is valid."),
        "invalid" if credential == MDBLIST_CREDENTIAL => {
            Some("MDBList rejected the saved API key.")
        }
        "invalid" => Some("The saved credential is invalid."),
        "offline" | "unavailable" => {
            Some("MDBList is temporarily unavailable; cached ratings remain available.")
        }
        "rate_limited" => Some("MDBList quota is exhausted; cached ratings remain available."),
        "saved" => Some("Saved for future TMDB features. Rating retrieval does not use this key."),
        _ => None,
    }
}

pub(super) fn bounded_nonnegative(value: Option<i64>, maximum: i64) -> Option<i64> {
    value.filter(|value| (0..=maximum).contains(value))
}

pub(super) fn bounded_timestamp(value: Option<i64>) -> Option<i64> {
    bounded_nonnegative(value, 4_102_444_800)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_precedence_is_local_then_plugin_then_none() {
        assert_eq!(effective_origin(true, true, true), Some(Origin::Local));
        assert_eq!(effective_origin(false, false, true), Some(Origin::Plugin));
        assert_eq!(effective_origin(true, false, true), Some(Origin::Plugin));
        assert_eq!(effective_origin(true, false, false), None);
    }

    #[test]
    fn tmdb_validation_is_format_only_and_explicit() {
        assert!(valid_tmdb_key_shape("0123456789abcdef0123456789abcdef"));
        assert!(valid_tmdb_key_shape("eyJabc.payload.signature"));
        assert!(!valid_tmdb_key_shape("not-a-tmdb-key"));
    }
}
