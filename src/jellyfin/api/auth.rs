//! Native Jellyfin authentication: password login, Quick Connect, and logout.

use serde_json::{Value, json};

use super::client::{ApiError, JellyfinClient};
use super::model::{AuthenticationResult, PublicSystemInfo, QuickConnectResult};

/// Credentials worth persisting after a successful authentication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub token: String,
    pub user_id: String,
    pub user_name: String,
    pub server_id: String,
}

impl Credentials {
    fn from_result(result: AuthenticationResult) -> Result<Self, ApiError> {
        let user = result
            .user
            .filter(|user| !user.id.trim().is_empty())
            .ok_or_else(|| ApiError::Decode("authentication response had no user".to_string()))?;
        if result.access_token.trim().is_empty() {
            return Err(ApiError::Decode(
                "authentication response had no access token".to_string(),
            ));
        }
        Ok(Self {
            token: result.access_token,
            user_id: user.id,
            user_name: user.name,
            server_id: result.server_id,
        })
    }
}

/// Reachability probe that also yields the name shown on the login screen.
pub fn public_system_info(client: &JellyfinClient) -> Result<PublicSystemInfo, ApiError> {
    client.get_json("/System/Info/Public", &[])
}

pub fn authenticate_by_name(
    client: &JellyfinClient,
    username: &str,
    password: &str,
) -> Result<Credentials, ApiError> {
    let body = json!({ "Username": username, "Pw": password });
    let result: AuthenticationResult = client.post_json("/Users/AuthenticateByName", &[], &body)?;
    Credentials::from_result(result)
}

pub fn quick_connect_enabled(client: &JellyfinClient) -> bool {
    client
        .get_json::<Value>("/QuickConnect/Enabled", &[])
        .map(|value| value.as_bool().unwrap_or(false))
        .unwrap_or(false)
}

/// Starts a Quick Connect attempt. Jellyfin moved this from GET to POST in
/// 10.9, so both are tried before giving up.
pub fn quick_connect_initiate(client: &JellyfinClient) -> Result<QuickConnectResult, ApiError> {
    match client.post_json::<Value, QuickConnectResult>("/QuickConnect/Initiate", &[], &json!({})) {
        Ok(result) => Ok(result),
        Err(ApiError::Status { status }) if status == 404 || status == 405 => {
            client.get_json("/QuickConnect/Initiate", &[])
        }
        Err(error) => Err(error),
    }
}

pub fn quick_connect_state(
    client: &JellyfinClient,
    secret: &str,
) -> Result<QuickConnectResult, ApiError> {
    client.get_json("/QuickConnect/Connect", &[("secret", secret.to_string())])
}

pub fn authenticate_with_quick_connect(
    client: &JellyfinClient,
    secret: &str,
) -> Result<Credentials, ApiError> {
    let body = json!({ "Secret": secret });
    let result: AuthenticationResult =
        client.post_json("/Users/AuthenticateWithQuickConnect", &[], &body)?;
    Credentials::from_result(result)
}

/// Best-effort session teardown; a failure here must not block local logout.
pub fn logout(client: &JellyfinClient) -> Result<(), ApiError> {
    client.post_empty("/Sessions/Logout", &[], &json!({}))
}

#[cfg(test)]
mod tests {
    use super::Credentials;
    use crate::jellyfin::api::client::ApiError;
    use crate::jellyfin::api::model::AuthenticationResult;

    fn result(json: &str) -> Result<Credentials, ApiError> {
        Credentials::from_result(serde_json::from_str::<AuthenticationResult>(json).unwrap())
    }

    #[test]
    fn reads_token_and_user_from_the_authentication_result() {
        let credentials =
            result(r#"{"AccessToken":"tok","ServerId":"srv","User":{"Id":"uid","Name":"pho"}}"#)
                .expect("credentials");
        assert_eq!(credentials.token, "tok");
        assert_eq!(credentials.user_id, "uid");
        assert_eq!(credentials.user_name, "pho");
        assert_eq!(credentials.server_id, "srv");
    }

    #[test]
    fn rejects_results_without_a_token_or_user() {
        assert!(matches!(
            result(r#"{"AccessToken":"","User":{"Id":"uid"}}"#),
            Err(ApiError::Decode(_))
        ));
        assert!(matches!(
            result(r#"{"AccessToken":"tok"}"#),
            Err(ApiError::Decode(_))
        ));
        assert!(matches!(
            result(r#"{"AccessToken":"tok","User":{"Id":"  "}}"#),
            Err(ApiError::Decode(_))
        ));
    }
}
