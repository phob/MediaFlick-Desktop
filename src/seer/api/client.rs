//! Blocking Seer REST client.
//!
//! Shaped after the Jellyfin client, with two deliberate differences:
//!
//! * **Retries are GET-only.** The Jellyfin client retries POSTs too, which is
//!   fine for its idempotent endpoints but not for `POST /request` — an
//!   uncertain outcome must never turn into a second request.
//! * **Cookies are carried here, not in ureq's agent jar**, which is in-memory
//!   and per-agent while a Seer session has to survive a restart. The caller
//!   ([`SeerSession`](crate::seer::SeerSession)) reads the jar back after each
//!   call and persists it.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde_json::json;

use crate::app::build_info;
use crate::app::urls::{build_query, join_url};

use super::error::SeerError;

/// Seer proxies TMDB for discovery, so the overall budget matches Jellyfin's
/// rather than assuming every answer is a small local one.
const HTTP_TIMEOUT: Duration = Duration::from_secs(120);
/// Reaching the instance at all must still fail fast, so a wrong address in
/// the setup dialog does not appear to hang.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ATTEMPTS: u32 = 3;
/// The budget above is per attempt, so retries share one deadline of their own.
const RETRY_BUDGET: Duration = HTTP_TIMEOUT;
/// Everything Seer exposes lives under one prefix.
const API_PREFIX: &str = "/api/v1";
/// The Express session cookie. Its presence is what "linked" means.
const SESSION_COOKIE: &str = "connect.sid";
/// The readable half of csurf's cookie pair; the `_csrf` secret is httpOnly.
const CSRF_COOKIE: &str = "XSRF-TOKEN";

/// The cookies one Seer session is made of: `connect.sid`, plus the `_csrf` /
/// `XSRF-TOKEN` pair when the instance has CSRF protection turned on.
///
/// The pair is set by the very first unauthenticated GET, and the *first* POST
/// — login, or the Quick Connect initiate — already needs it, so capturing
/// only after authentication would fail against a CSRF-enabled Seer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionCookies {
    entries: BTreeMap<String, String>,
}

impl SessionCookies {
    /// Reads back the form [`Self::to_json`] persists. Anything unreadable is
    /// treated as no cookies at all, which costs a re-link and nothing worse.
    pub fn from_json(text: &str) -> Self {
        Self {
            entries: serde_json::from_str(text).unwrap_or_default(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.entries).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether an actual session — rather than only a CSRF pair — is held.
    pub fn has_session(&self) -> bool {
        self.entries.contains_key(SESSION_COOKIE)
    }

    /// Folds `other` in, keeping its values where both carry a name. Used to
    /// refresh a rotated CSRF pair without dropping the session cookie.
    pub fn merge(&mut self, other: Self) {
        self.entries.extend(other.entries);
    }

    /// Takes one `Set-Cookie` header. An empty value is a deletion, which is
    /// how Express clears `connect.sid` on logout.
    fn absorb(&mut self, header: &str) {
        let mut parts = header.split(';');
        let Some((name, value)) = parts.next().and_then(|pair| pair.split_once('=')) else {
            return;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty() {
            return;
        }
        let expired = value.is_empty()
            || parts.any(|attribute| {
                attribute
                    .trim()
                    .split_once('=')
                    .is_some_and(|(key, value)| {
                        key.trim().eq_ignore_ascii_case("max-age") && value.trim() == "0"
                    })
            });
        if expired {
            self.entries.remove(name);
        } else {
            self.entries.insert(name.to_string(), value.to_string());
        }
    }

    fn header(&self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        Some(
            self.entries
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    fn csrf_token(&self) -> Option<&str> {
        self.entries.get(CSRF_COOKIE).map(String::as_str)
    }
}

/// A handle to one Seer instance, carrying whatever session it was built with.
pub struct SeerClient {
    agent: ureq::Agent,
    base_url: String,
    cookies: Mutex<SessionCookies>,
}

impl SeerClient {
    pub fn new(base_url: &str, cookies: SessionCookies) -> Self {
        // Non-success statuses are handled here rather than raised by ureq:
        // an error response still carries `Set-Cookie`, and a CSRF pair that
        // arrives with a 401 is exactly the one the retry needs.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .http_status_as_error(false)
            .user_agent(format!("mediaflick-desktop/{}", build_info::APP_VERSION))
            .build()
            .into();
        Self {
            agent,
            base_url: base_url.trim_end_matches('/').to_string(),
            cookies: Mutex::new(cookies),
        }
    }

    /// The cookie set as it now stands, for persisting after a call that
    /// established or rotated it.
    pub fn cookies(&self) -> SessionCookies {
        self.cookies
            .lock()
            .map(|cookies| cookies.clone())
            .unwrap_or_default()
    }

    /// `path` is relative to `/api/v1`, which every Seer route lives under.
    pub fn url(&self, path: &str, query: &[(&str, String)]) -> String {
        let url = join_url(&join_url(&self.base_url, API_PREFIX), path);
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
    ) -> Result<T, SeerError> {
        let url = self.url(path, query);
        let body = self.with_retry(path, || {
            let mut request = self.agent.get(url.as_str());
            request = request.header("Accept", "application/json");
            if let Some(cookie) = self.cookies().header() {
                request = request.header("Cookie", cookie);
            }
            self.finish(path, request.call().map_err(map_ureq_error)?)
        })?;
        decode(&body)
    }

    /// A POST whose response body is not needed. Never retried: Seer's writes
    /// are not idempotent, and an uncertain outcome must stay one attempt.
    pub fn post_empty(&self, path: &str) -> Result<(), SeerError> {
        let url = self.url(path, &[]);
        let cookies = self.cookies();
        let mut request = self.agent.post(url.as_str());
        request = request.header("Accept", "application/json");
        if let Some(cookie) = cookies.header() {
            request = request.header("Cookie", cookie);
        }
        // csurf accepts `X-XSRF-TOKEN` among its default header names. Echoing
        // the cookie whenever it exists costs nothing when the setting is off.
        if let Some(token) = cookies.csrf_token() {
            request = request.header("X-XSRF-TOKEN", token);
        }
        let response = request.send_json(json!({})).map_err(map_ureq_error)?;
        self.finish(path, response).map(|_| ())
    }

    /// Absorbs any `Set-Cookie`, then turns the status into a [`SeerError`].
    fn finish(
        &self,
        path: &str,
        mut response: ureq::http::Response<ureq::Body>,
    ) -> Result<Vec<u8>, SeerError> {
        let set_cookie = response
            .headers()
            .get_all("set-cookie")
            .into_iter()
            .filter_map(|value| value.to_str().ok().map(str::to_string))
            .collect::<Vec<_>>();
        if !set_cookie.is_empty()
            && let Ok(mut cookies) = self.cookies.lock()
        {
            for header in &set_cookie {
                cookies.absorb(header);
            }
        }

        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_to_vec()
            .map_err(|error| SeerError::Transport(error.to_string()))?;
        if (200..300).contains(&status) {
            return Ok(body);
        }
        if status == 401 {
            return Err(SeerError::Unauthorized);
        }
        tracing::debug!(
            target: "seer.api",
            path,
            status,
            "Seer refused the request: {}",
            String::from_utf8_lossy(&body).chars().take(200).collect::<String>()
        );
        Err(SeerError::Status { status })
    }

    fn with_retry<T>(
        &self,
        path: &str,
        mut call: impl FnMut() -> Result<T, SeerError>,
    ) -> Result<T, SeerError> {
        let deadline = Instant::now() + RETRY_BUDGET;
        let mut attempt = 0;
        loop {
            attempt += 1;
            match call() {
                Ok(value) => return Ok(value),
                Err(error) if error.is_retryable() && attempt < MAX_ATTEMPTS => {
                    let backoff = Duration::from_millis(250 * (1u64 << (attempt - 1)));
                    if deadline.saturating_duration_since(Instant::now()) <= backoff {
                        tracing::debug!(
                            target: "seer.api",
                            path,
                            attempt,
                            "giving up on the Seer request, the retry budget is spent: {error}"
                        );
                        return Err(error);
                    }
                    tracing::debug!(
                        target: "seer.api",
                        path,
                        attempt,
                        "retrying Seer request after {error}"
                    );
                    std::thread::sleep(backoff);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, SeerError> {
    serde_json::from_slice(body).map_err(|error| SeerError::Decode(error.to_string()))
}

/// With `http_status_as_error` off, anything reaching here is a transport
/// failure; the status arm stays as a belt-and-braces mapping.
fn map_ureq_error(error: ureq::Error) -> SeerError {
    match error {
        ureq::Error::StatusCode(401) => SeerError::Unauthorized,
        ureq::Error::StatusCode(status) => SeerError::Status { status },
        other => SeerError::Transport(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{SeerClient, SessionCookies};

    fn cookies(headers: &[&str]) -> SessionCookies {
        let mut jar = SessionCookies::default();
        for header in headers {
            jar.absorb(header);
        }
        jar
    }

    #[test]
    fn the_csrf_pair_is_captured_from_an_unauthenticated_probe() {
        let jar = cookies(&[
            "_csrf=secret; Path=/; HttpOnly; SameSite=Strict",
            "XSRF-TOKEN=token123; Path=/",
        ]);
        assert_eq!(jar.csrf_token(), Some("token123"));
        assert!(!jar.has_session());
        assert_eq!(
            jar.header().as_deref(),
            Some("XSRF-TOKEN=token123; _csrf=secret")
        );
    }

    #[test]
    fn a_session_cookie_is_what_makes_the_jar_a_link() {
        let jar = cookies(&["connect.sid=s%3Aabc.def; Path=/; HttpOnly"]);
        assert!(jar.has_session());
        assert!(!jar.is_empty());
    }

    #[test]
    fn a_cleared_cookie_is_removed_rather_than_stored_empty() {
        let mut jar = cookies(&["connect.sid=abc; Path=/"]);
        jar.absorb("connect.sid=; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT");
        assert!(!jar.has_session());

        let mut jar = cookies(&["connect.sid=abc; Path=/"]);
        jar.absorb("connect.sid=abc; Path=/; Max-Age=0");
        assert!(!jar.has_session());
    }

    #[test]
    fn malformed_set_cookie_headers_are_ignored() {
        let jar = cookies(&["", "   ", "no-equals-sign", "=orphan; Path=/"]);
        assert!(jar.is_empty());
    }

    #[test]
    fn merging_refreshes_a_rotated_csrf_pair_without_dropping_the_session() {
        let mut jar = cookies(&["connect.sid=abc", "XSRF-TOKEN=old", "_csrf=old-secret"]);
        jar.merge(cookies(&["XSRF-TOKEN=new", "_csrf=new-secret"]));
        assert!(jar.has_session());
        assert_eq!(jar.csrf_token(), Some("new"));
    }

    #[test]
    fn the_jar_round_trips_through_its_persisted_form() {
        let jar = cookies(&["connect.sid=abc", "XSRF-TOKEN=token"]);
        let restored = SessionCookies::from_json(&jar.to_json());
        assert_eq!(restored, jar);
        assert_eq!(
            SessionCookies::from_json("not json"),
            SessionCookies::default()
        );
    }

    #[test]
    fn urls_are_built_under_the_api_prefix_and_encode_the_query() {
        let client = SeerClient::new("https://seer.test/", SessionCookies::default());
        assert_eq!(
            client.url("settings/public", &[]),
            "https://seer.test/api/v1/settings/public"
        );
        assert_eq!(
            client.url("/search", &[("query", "the matrix".to_string())]),
            "https://seer.test/api/v1/search?query=the%20matrix"
        );
    }

    #[test]
    fn a_client_reports_back_the_jar_it_was_built_with() {
        let jar = cookies(&["connect.sid=abc"]);
        let client = SeerClient::new("https://seer.test", jar.clone());
        assert_eq!(client.cookies(), jar);
    }
}
