//! Blocking Jellyfin REST client shared by auth, the library sync thread, the
//! app-scheme API, and the play path.

use std::fmt;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::app::build_info;
use crate::app::urls::{build_query, join_url};

/// A sync page carries full metadata for hundreds of items, which a remote
/// server can take a while to assemble, so the overall budget is generous.
const HTTP_TIMEOUT: Duration = Duration::from_secs(120);
/// Reaching the server at all must still fail fast, so an unreachable address
/// on the sign-in screen does not appear to hang.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ATTEMPTS: u32 = 3;
/// The budget above is per attempt, so retries must share one deadline of their
/// own: otherwise a stalling server holds a caller — the sign-in screen, the UI
/// thread waiting on an API answer — for `MAX_ATTEMPTS` × `HTTP_TIMEOUT`.
const RETRY_BUDGET: Duration = HTTP_TIMEOUT;

/// Value reported to the Jellyfin Devices dashboard as the client name.
pub const CLIENT_NAME: &str = build_info::APP_NAME;

#[derive(Debug, Clone)]
pub struct ByteResponse {
    pub status: u16,
    pub content_type: String,
    pub content_range: Option<String>,
    pub accept_ranges: Option<String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// The server rejected our token. The session must be re-established.
    Unauthorized,
    /// Any other non-success HTTP status.
    Status { status: u16 },
    /// The server asked clients to slow down. Seconds are retained from a
    /// numeric `Retry-After` header so background work does not hammer it.
    RateLimited { retry_after_secs: Option<u64> },
    /// A typed companion endpoint refused the action and supplied a safe
    /// user-facing explanation.
    Remote { status: u16, message: String },
    /// Connection, DNS, TLS, or timeout failure.
    Transport(String),
    /// The response body did not match the expected shape.
    Decode(String),
    /// No server URL is configured, or no session exists yet.
    NotConfigured,
    /// Local application shutdown cancelled work between bounded requests.
    Cancelled,
}

impl ApiError {
    /// Whether retrying the same request could plausibly succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) | Self::RateLimited { .. } => true,
            Self::Status { status } | Self::Remote { status, .. } => *status >= 500,
            Self::Unauthorized | Self::Decode(_) | Self::NotConfigured | Self::Cancelled => false,
        }
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited {
                retry_after_secs: Some(seconds),
            } => Some(Duration::from_secs(*seconds)),
            _ => None,
        }
    }

    /// HTTP status to surface to our own UI for this failure.
    pub fn client_status(&self) -> u16 {
        match self {
            Self::Unauthorized => 401,
            Self::RateLimited { .. } => 429,
            Self::Status { status } | Self::Remote { status, .. } => *status,
            Self::NotConfigured => 409,
            Self::Cancelled => 499,
            Self::Transport(_) | Self::Decode(_) => 502,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => write!(formatter, "the Jellyfin server rejected the session"),
            Self::Status { status } => write!(formatter, "Jellyfin returned HTTP {status}"),
            Self::RateLimited {
                retry_after_secs: Some(seconds),
            } => write!(
                formatter,
                "Jellyfin rate limited requests; retrying after {seconds} seconds"
            ),
            Self::RateLimited { .. } => write!(formatter, "Jellyfin rate limited requests"),
            Self::Remote { message, .. } => write!(formatter, "{message}"),
            Self::Transport(message) => write!(formatter, "could not reach Jellyfin: {message}"),
            Self::Decode(message) => {
                write!(formatter, "unexpected Jellyfin response: {message}")
            }
            Self::NotConfigured => write!(formatter, "no Jellyfin server is configured"),
            Self::Cancelled => write!(formatter, "the request was cancelled during shutdown"),
        }
    }
}

impl std::error::Error for ApiError {}

/// An authenticated (or anonymous) handle to one Jellyfin server.
#[derive(Clone)]
pub struct JellyfinClient {
    agent: ureq::Agent,
    companion_agent: ureq::Agent,
    base_url: String,
    device_id: String,
    device_name: String,
    token: Option<String>,
}

impl JellyfinClient {
    pub fn new(base_url: &str, device_id: &str, token: Option<&str>) -> Self {
        let build_agent = |http_status_as_error| -> ureq::Agent {
            ureq::Agent::config_builder()
                .timeout_global(Some(HTTP_TIMEOUT))
                .timeout_connect(Some(CONNECT_TIMEOUT))
                .http_status_as_error(http_status_as_error)
                .user_agent(format!("mediaflick-desktop/{}", build_info::APP_VERSION))
                .build()
                .into()
        };
        Self {
            // Keep core error responses available so `Retry-After` can be
            // honored instead of being discarded into `ureq::Error::StatusCode`.
            agent: build_agent(false),
            companion_agent: build_agent(false),
            base_url: base_url.trim_end_matches('/').to_string(),
            device_id: device_id.to_string(),
            device_name: device_name(),
            token: token
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_string),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Only the tests read the raw token now: everything that sends it uses
    /// [`Self::authorization_header`] or [`Self::auth_headers`], so it never
    /// reaches a URL.
    #[cfg(test)]
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// The `Authorization: MediaBrowser …` header every Jellyfin client sends.
    pub fn authorization_header(&self) -> String {
        let mut header = format!(
            "MediaBrowser Client=\"{}\", Device=\"{}\", DeviceId=\"{}\", Version=\"{}\"",
            quote(CLIENT_NAME),
            quote(&self.device_name),
            quote(&self.device_id),
            quote(build_info::APP_VERSION)
        );
        if let Some(token) = &self.token {
            header.push_str(&format!(", Token=\"{}\"", quote(token)));
        }
        header
    }

    /// Auth headers suitable for handing to an external player or another agent.
    pub fn auth_headers(&self) -> Vec<crate::playback::HttpHeader> {
        let mut headers = vec![crate::playback::HttpHeader {
            name: "Authorization".to_string(),
            value: self.authorization_header(),
        }];
        if let Some(token) = &self.token {
            headers.push(crate::playback::HttpHeader {
                name: "X-Emby-Token".to_string(),
                value: token.clone(),
            });
        }
        headers
    }

    pub fn url(&self, path: &str, query: &[(&str, String)]) -> String {
        let url = join_url(&self.base_url, path);
        if query.is_empty() {
            url
        } else {
            format!("{url}?{}", build_query(query))
        }
    }

    pub fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, ApiError> {
        let url = self.url(path, query);
        self.with_retry(path, || {
            let mut response = core_response(
                self.agent
                    .get(url.as_str())
                    .header("Accept", "application/json")
                    .header("Authorization", self.authorization_header())
                    .call()
                    .map_err(map_ureq_error)?,
            )?;
            response
                .body_mut()
                .read_json::<T>()
                .map_err(|error| ApiError::Decode(error.to_string()))
        })
    }

    pub fn get_bytes(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<(Vec<u8>, String), ApiError> {
        let response = self.get_bytes_range(path, query, None)?;
        Ok((response.body, response.content_type))
    }

    /// Fetches one bounded byte range without ever placing the token in a URL.
    ///
    /// Video elements seek by issuing range requests. Keeping the upstream
    /// status and range headers lets the app scheme behave like the Jellyfin
    /// resource while still authenticating it natively.
    pub fn get_bytes_range(
        &self,
        path: &str,
        query: &[(&str, String)],
        range: Option<&str>,
    ) -> Result<ByteResponse, ApiError> {
        let url = self.url(path, query);
        self.with_retry(path, || {
            let mut request = self
                .agent
                .get(url.as_str())
                .header("Authorization", self.authorization_header());
            if let Some(range) = range {
                request = request.header("Range", range);
            }
            let mut response = core_response(request.call().map_err(map_ureq_error)?)?;
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            let content_range = response
                .headers()
                .get("content-range")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let accept_ranges = response
                .headers()
                .get("accept-ranges")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let bytes = response
                .body_mut()
                .with_config()
                .limit(64 * 1024 * 1024)
                .read_to_vec()
                .map_err(|error| ApiError::Transport(error.to_string()))?;
            Ok(ByteResponse {
                status,
                content_type,
                content_range,
                accept_ranges,
                body: bytes,
            })
        })
    }

    /// Authenticated plugin GET that preserves a 403 as an action refusal.
    ///
    /// Core Jellyfin calls historically treat 401 and 403 alike as a rejected
    /// session. Companion endpoints sit behind that session but may themselves
    /// return 403 for an upstream Seerr permission, which must not sign the
    /// user out of Jellyfin.
    pub fn companion_get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, ApiError> {
        let url = self.url(path, query);
        self.with_retry(path, || {
            let response = self
                .companion_agent
                .get(url.as_str())
                .header("Accept", "application/json")
                .header("Authorization", self.authorization_header())
                .call()
                .map_err(map_companion_ureq_error)?;
            companion_json(response)
        })
    }

    pub fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path, query);
        self.with_retry(path, || {
            let mut response = core_response(
                self.agent
                    .post(url.as_str())
                    .header("Accept", "application/json")
                    .header("Authorization", self.authorization_header())
                    .send_json(body)
                    .map_err(map_ureq_error)?,
            )?;
            response
                .body_mut()
                .read_json::<T>()
                .map_err(|error| ApiError::Decode(error.to_string()))
        })
    }

    pub fn companion_post_json_once<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path, &[]);
        let response = self
            .companion_agent
            .post(url.as_str())
            .header("Accept", "application/json")
            .header("Authorization", self.authorization_header())
            .send_json(body)
            .map_err(map_companion_ureq_error)?;
        companion_json(response)
    }

    /// Typed companion boundary for rating data. Unlike general companion
    /// routes, no server response text is allowed to become an ApiError: the
    /// plugin is the credential owner and an upstream diagnostic may contain
    /// credential-shaped data.
    pub fn companion_post_ratings_json_once<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path, &[]);
        let response = self
            .companion_agent
            .post(url.as_str())
            .header("Accept", "application/json")
            .header("Authorization", self.authorization_header())
            .send_json(body)
            .map_err(map_companion_ureq_error_safe)?;
        companion_json_safe(response)
    }

    /// Info feeds the native capability/status surface. Keep malformed or
    /// failed plugin diagnostics out of that publicly serialized state too.
    pub fn companion_get_info_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let url = self.url(path, &[]);
        self.with_retry(path, || {
            let response = self
                .companion_agent
                .get(url.as_str())
                .header("Accept", "application/json")
                .header("Authorization", self.authorization_header())
                .call()
                .map_err(map_companion_ureq_error_safe)?;
            companion_json_safe(response)
        })
    }

    /// POST that ignores the response body (Jellyfin often answers 204).
    pub fn post_empty<B: Serialize>(
        &self,
        path: &str,
        query: &[(&str, String)],
        body: &B,
    ) -> Result<(), ApiError> {
        let url = self.url(path, query);
        self.with_retry(path, || {
            core_response(
                self.agent
                    .post(url.as_str())
                    .header("Accept", "application/json")
                    .header("Authorization", self.authorization_header())
                    .send_json(body)
                    .map_err(map_ureq_error)?,
            )
            .map(|_| ())
        })
    }

    pub fn delete(&self, path: &str, query: &[(&str, String)]) -> Result<(), ApiError> {
        let url = self.url(path, query);
        self.with_retry(path, || {
            core_response(
                self.agent
                    .delete(url.as_str())
                    .header("Accept", "application/json")
                    .header("Authorization", self.authorization_header())
                    .call()
                    .map_err(map_ureq_error)?,
            )
            .map(|_| ())
        })
    }

    pub fn companion_delete_once(&self, path: &str) -> Result<(), ApiError> {
        let url = self.url(path, &[]);
        let response = self
            .companion_agent
            .delete(url.as_str())
            .header("Accept", "application/json")
            .header("Authorization", self.authorization_header())
            .call()
            .map_err(map_companion_ureq_error)?;
        companion_empty(response)
    }

    fn with_retry<T>(
        &self,
        path: &str,
        mut call: impl FnMut() -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let deadline = Instant::now() + RETRY_BUDGET;
        let mut attempt = 0;
        loop {
            attempt += 1;
            match call() {
                Ok(value) => return Ok(value),
                Err(error) if error.is_retryable() && attempt < MAX_ATTEMPTS => {
                    let backoff = error
                        .retry_after()
                        .unwrap_or_else(|| Duration::from_millis(250 * (1u64 << (attempt - 1))));
                    if deadline.saturating_duration_since(Instant::now()) <= backoff {
                        tracing::debug!(
                            target: "jellyfin.api",
                            path,
                            attempt,
                            "giving up on the Jellyfin request, the retry budget is spent: {error}"
                        );
                        return Err(error);
                    }
                    tracing::debug!(
                        target: "jellyfin.api",
                        path,
                        attempt,
                        "retrying Jellyfin request after {error}"
                    );
                    std::thread::sleep(backoff);
                }
                Err(error) => {
                    if error == ApiError::Unauthorized {
                        tracing::warn!(target: "jellyfin.api", path, "Jellyfin rejected the session token");
                    } else {
                        tracing::debug!(target: "jellyfin.api", path, "Jellyfin request failed: {error}");
                    }
                    return Err(error);
                }
            }
        }
    }
}

fn core_response(
    response: ureq::http::Response<ureq::Body>,
) -> Result<ureq::http::Response<ureq::Body>, ApiError> {
    let status = response.status().as_u16();
    match status {
        200..=399 => Ok(response),
        401 | 403 => Err(ApiError::Unauthorized),
        429 => {
            let retry_after_secs = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<u64>().ok());
            Err(ApiError::RateLimited { retry_after_secs })
        }
        _ => Err(ApiError::Status { status }),
    }
}

fn map_ureq_error(error: ureq::Error) -> ApiError {
    match error {
        ureq::Error::StatusCode(401) | ureq::Error::StatusCode(403) => ApiError::Unauthorized,
        ureq::Error::StatusCode(429) => ApiError::RateLimited {
            retry_after_secs: None,
        },
        ureq::Error::StatusCode(status) => ApiError::Status { status },
        other => ApiError::Transport(other.to_string()),
    }
}

fn map_companion_ureq_error(error: ureq::Error) -> ApiError {
    match error {
        ureq::Error::StatusCode(401) => ApiError::Unauthorized,
        ureq::Error::StatusCode(status) => ApiError::Status { status },
        other => ApiError::Transport(other.to_string()),
    }
}

fn map_companion_ureq_error_safe(error: ureq::Error) -> ApiError {
    match error {
        ureq::Error::StatusCode(401) => ApiError::Unauthorized,
        ureq::Error::StatusCode(status) => ApiError::Status { status },
        _ => ApiError::Transport("could not reach the MediaFlick Companion plugin".to_string()),
    }
}

fn companion_json<T: DeserializeOwned>(
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<T, ApiError> {
    let status = response.status().as_u16();
    if status == 401 {
        return Err(ApiError::Unauthorized);
    }
    if status >= 400 {
        return Err(companion_status_error(&mut response, status));
    }
    response
        .body_mut()
        .read_json::<T>()
        .map_err(|error| ApiError::Decode(error.to_string()))
}

fn companion_json_safe<T: DeserializeOwned>(
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<T, ApiError> {
    let status = response.status().as_u16();
    if status == 401 {
        return Err(ApiError::Unauthorized);
    }
    if status >= 400 {
        return Err(ApiError::Status { status });
    }
    response.body_mut().read_json::<T>().map_err(|_| {
        ApiError::Decode("the companion returned an invalid rating response".to_string())
    })
}

fn companion_empty(mut response: ureq::http::Response<ureq::Body>) -> Result<(), ApiError> {
    let status = response.status().as_u16();
    if status == 401 {
        return Err(ApiError::Unauthorized);
    }
    if status >= 400 {
        return Err(companion_status_error(&mut response, status));
    }
    Ok(())
}

fn companion_status_error(
    response: &mut ureq::http::Response<ureq::Body>,
    status: u16,
) -> ApiError {
    let message = response
        .body_mut()
        .read_json::<serde_json::Value>()
        .ok()
        .and_then(|body| body["error"].as_str().map(str::to_string))
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| format!("the companion returned HTTP {status}"));
    ApiError::Remote { status, message }
}

/// Jellyfin shows this in the Devices dashboard; the hostname keeps it useful.
fn device_name() -> String {
    for variable in ["COMPUTERNAME", "HOSTNAME", "HOST"] {
        if let Some(value) = std::env::var_os(variable)
            && let Some(value) = value.to_str()
            && !value.trim().is_empty()
        {
            return value.trim().to_string();
        }
    }
    "Desktop".to_string()
}

/// Header parameter values are double-quoted, so quotes and control characters
/// must not survive into them.
fn quote(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '"' | '\r' | '\n' | '\0'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ApiError, JellyfinClient, map_companion_ureq_error, map_ureq_error, quote};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn client() -> JellyfinClient {
        JellyfinClient::new("http://server:8096/", "device-1", Some("secret"))
    }

    #[test]
    fn authorization_header_carries_client_identity_and_token() {
        let header = client().authorization_header();
        assert!(header.starts_with("MediaBrowser Client=\"MediaFlick Desktop\""));
        assert!(header.contains("DeviceId=\"device-1\""));
        assert!(header.contains("Token=\"secret\""));
    }

    #[test]
    fn anonymous_clients_omit_the_token_parameter() {
        let header = JellyfinClient::new("http://server", "device-1", None).authorization_header();
        assert!(!header.contains("Token="));
    }

    #[test]
    fn blank_tokens_are_treated_as_anonymous() {
        let client = JellyfinClient::new("http://server", "device-1", Some("   "));
        assert_eq!(client.token(), None);
    }

    #[test]
    fn urls_join_the_base_and_encode_the_query() {
        let client = client();
        assert_eq!(client.url("/Items", &[]), "http://server:8096/Items");
        assert_eq!(
            client.url("Items", &[("searchTerm", "a b".to_string())]),
            "http://server:8096/Items?searchTerm=a%20b"
        );
    }

    #[test]
    fn auth_headers_include_the_token_header() {
        let headers = client().auth_headers();
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[1].name, "X-Emby-Token");
        assert_eq!(headers[1].value, "secret");
    }

    #[test]
    fn quotes_and_newlines_cannot_escape_a_header_parameter() {
        assert_eq!(quote("evil\", Token=\"x\r\n"), "evil, Token=x");
    }

    #[test]
    fn unauthorized_statuses_map_to_the_session_error() {
        assert_eq!(
            map_ureq_error(ureq::Error::StatusCode(401)),
            ApiError::Unauthorized
        );
        assert_eq!(
            map_ureq_error(ureq::Error::StatusCode(403)),
            ApiError::Unauthorized
        );
        assert_eq!(
            map_ureq_error(ureq::Error::StatusCode(404)),
            ApiError::Status { status: 404 }
        );
    }

    #[test]
    fn retry_after_seconds_are_preserved_for_background_scheduling() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0_u8; 2_048];
            let _ = stream.read(&mut request).expect("request");
            write!(
                stream,
                "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .expect("response");
        });
        let client = JellyfinClient::new(&format!("http://{address}"), "device", Some("token"));

        let error = client
            .get_json::<serde_json::Value>("/Items", &[])
            .expect_err("rate limit");

        assert_eq!(
            error,
            ApiError::RateLimited {
                retry_after_secs: Some(600)
            }
        );
        server.join().expect("server");
    }

    #[test]
    fn companion_permissions_do_not_expire_the_jellyfin_session() {
        assert_eq!(
            map_companion_ureq_error(ureq::Error::StatusCode(401)),
            ApiError::Unauthorized
        );
        assert_eq!(
            map_companion_ureq_error(ureq::Error::StatusCode(403)),
            ApiError::Status { status: 403 }
        );
    }

    #[test]
    fn only_transport_and_server_errors_are_retried() {
        assert!(ApiError::Transport("reset".to_string()).is_retryable());
        assert!(ApiError::Status { status: 503 }.is_retryable());
        assert!(
            ApiError::RateLimited {
                retry_after_secs: Some(30)
            }
            .is_retryable()
        );
        assert!(!ApiError::Status { status: 404 }.is_retryable());
        assert!(!ApiError::Unauthorized.is_retryable());
    }

    #[test]
    fn client_status_maps_failures_to_our_own_api() {
        assert_eq!(ApiError::Unauthorized.client_status(), 401);
        assert_eq!(ApiError::NotConfigured.client_status(), 409);
        assert_eq!(ApiError::Status { status: 404 }.client_status(), 404);
        assert_eq!(
            ApiError::RateLimited {
                retry_after_secs: Some(30)
            }
            .client_status(),
            429
        );
        assert_eq!(
            ApiError::Remote {
                status: 409,
                message: "not mapped".to_string()
            }
            .client_status(),
            409
        );
        assert_eq!(ApiError::Transport("x".to_string()).client_status(), 502);
    }
}
