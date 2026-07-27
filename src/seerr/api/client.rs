//! Blocking Seerr REST client.
//!
//! Shaped after the Jellyfin client, with two deliberate differences:
//!
//! * **Retries are GET-only.** The Jellyfin client retries POSTs too, which is
//!   fine for its idempotent endpoints but not for `POST /request` — an
//!   uncertain outcome must never turn into a second request.
//! * **Cookies are carried here, not in ureq's agent jar**, which is in-memory
//!   and per-agent while a Seerr session has to survive a restart. The caller
//!   ([`SeerrSession`](crate::seerr::SeerrSession)) reads the jar back after each
//!   call and persists it.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::app::build_info;
use crate::app::urls::{build_query, join_url};

use super::error::SeerrError;

/// Seerr proxies TMDB for discovery, so the overall budget matches Jellyfin's
/// rather than assuming every answer is a small local one.
const HTTP_TIMEOUT: Duration = Duration::from_secs(120);
/// Reaching the instance at all must still fail fast, so a wrong address in
/// the setup dialog does not appear to hang.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ATTEMPTS: u32 = 3;
/// The budget above is per attempt, so retries share one deadline of their own.
const RETRY_BUDGET: Duration = HTTP_TIMEOUT;
/// Everything Seerr exposes lives under one prefix.
const API_PREFIX: &str = "/api/v1";
/// The Express session cookie. Its presence is what "linked" means.
const SESSION_COOKIE: &str = "connect.sid";
/// The readable half of csurf's cookie pair; the `_csrf` secret is httpOnly.
const CSRF_COOKIE: &str = "XSRF-TOKEN";

/// The cookies one Seerr session is made of: `connect.sid`, plus the `_csrf` /
/// `XSRF-TOKEN` pair when the instance has CSRF protection turned on.
///
/// The pair is set by the very first unauthenticated GET, and the *first* POST
/// — login, or the Quick Connect initiate — already needs it, so capturing
/// only after authentication would fail against a CSRF-enabled Seerr.
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

/// A handle to one Seerr instance, carrying whatever session it was built with.
pub struct SeerrClient {
    agent: ureq::Agent,
    base_url: String,
    cookies: Mutex<SessionCookies>,
}

impl SeerrClient {
    pub fn new(base_url: &str, cookies: SessionCookies) -> Self {
        // Non-success statuses are handled here rather than raised by ureq:
        // an error response still carries `Set-Cookie`, and a CSRF pair that
        // arrives with a 401 is exactly the one the retry needs.
        //
        // Redirects are *not* followed. Seerr's API answers where it is asked;
        // a 3xx means something in front of it intercepted the call, and the
        // only thing that ever does is a sign-on proxy. Chasing that redirect
        // lands on a login page, which at best parses as a "web page instead of
        // JSON" and at worst — as with an authentik outpost behind Cloudflare —
        // dies in the middle of a chunked HTML body and reports a transport
        // fault that says nothing about the real problem. Stopping here is what
        // lets [`Self::finish`] name it.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .http_status_as_error(false)
            .max_redirects(0)
            .user_agent(format!("mediaflick-desktop/{}", build_info::APP_VERSION))
            .build()
            .into();
        Self {
            agent,
            base_url: base_url.trim_end_matches('/').to_string(),
            cookies: Mutex::new(cookies),
        }
    }

    /// The instance this client talks to, normalized. Callers compare it
    /// against the stored link before writing a session back.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The cookie set as it now stands, for persisting after a call that
    /// established or rotated it.
    pub fn cookies(&self) -> SessionCookies {
        self.cookies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// `path` is relative to `/api/v1`, which every Seerr route lives under.
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
    ) -> Result<T, SeerrError> {
        let url = self.url(path, query);
        let payload = self.with_retry(path, || {
            let mut request = self.agent.get(url.as_str());
            request = request.header("Accept", "application/json");
            if let Some(cookie) = self.cookies().header() {
                request = request.header("Cookie", cookie);
            }
            self.finish(path, request.call().map_err(map_ureq_error)?)
        })?;
        decode(path, &payload)
    }

    /// A POST whose response body is not needed.
    pub fn post_empty(&self, path: &str) -> Result<(), SeerrError> {
        self.post(path, &json!({})).map(|_| ())
    }

    pub fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, SeerrError> {
        let payload = self.post(path, body)?;
        decode(path, &payload)
    }

    /// Never retried: Seerr's writes are not idempotent, and an uncertain
    /// outcome must stay one attempt.
    fn post<B: Serialize>(&self, path: &str, body: &B) -> Result<Payload, SeerrError> {
        let url = self.url(path, &[]);
        let mut request = self.agent.post(url.as_str());
        for (name, value) in self.write_headers() {
            request = request.header(name, value);
        }
        let response = request.send_json(body).map_err(map_ureq_error)?;
        self.finish(path, response)
    }

    /// `DELETE`, which is how Seerr cancels a request. Not retried either: a
    /// second attempt would answer 404 for a cancellation that did work, which
    /// reads to the user as a failure.
    pub fn delete(&self, path: &str) -> Result<(), SeerrError> {
        let url = self.url(path, &[]);
        let mut request = self.agent.delete(url.as_str());
        for (name, value) in self.write_headers() {
            request = request.header(name, value);
        }
        let response = request.call().map_err(map_ureq_error)?;
        self.finish(path, response).map(|_| ())
    }

    /// The headers every non-GET carries. csurf accepts `X-XSRF-TOKEN` among
    /// its default header names; echoing the cookie whenever it exists costs
    /// nothing on an instance with CSRF protection off.
    fn write_headers(&self) -> Vec<(&'static str, String)> {
        let cookies = self.cookies();
        let mut headers = vec![("Accept", "application/json".to_string())];
        if let Some(cookie) = cookies.header() {
            headers.push(("Cookie", cookie));
        }
        if let Some(token) = cookies.csrf_token() {
            headers.push(("X-XSRF-TOKEN", token.to_string()));
        }
        headers
    }

    /// Absorbs any `Set-Cookie`, then turns the status into a [`SeerrError`].
    fn finish(
        &self,
        path: &str,
        mut response: ureq::http::Response<ureq::Body>,
    ) -> Result<Payload, SeerrError> {
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
        if (300..400).contains(&status) {
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            tracing::warn!(
                target: "seerr.api",
                path,
                status,
                target = %host_of(&location).unwrap_or_default(),
                "the Seerr API redirected instead of answering"
            );
            return Err(SeerrError::Unusable(intercepted_message(&location)));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let bytes = response
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_to_vec()
            .map_err(|error| SeerrError::Transport(error.to_string()))?;
        if (200..300).contains(&status) {
            return Ok(Payload {
                bytes,
                content_type,
            });
        }
        if status == 401 {
            return Err(SeerrError::Unauthorized);
        }
        tracing::debug!(
            target: "seerr.api",
            path,
            status,
            "Seerr refused the request"
        );
        Err(SeerrError::Status { status })
    }

    fn with_retry<T>(
        &self,
        path: &str,
        mut call: impl FnMut() -> Result<T, SeerrError>,
    ) -> Result<T, SeerrError> {
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
                            target: "seerr.api",
                            path,
                            attempt,
                            "giving up on the Seerr request, the retry budget is spent: {error}"
                        );
                        return Err(error);
                    }
                    tracing::debug!(
                        target: "seerr.api",
                        path,
                        attempt,
                        "retrying Seerr request after {error}"
                    );
                    std::thread::sleep(backoff);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// The art host every Seerr `posterPath` / `backdropPath` is relative to.
const TMDB_IMAGE_HOST: &str = "https://image.tmdb.org/t/p";
/// The rendition names the proxy will ask for. An allowlist rather than a
/// pass-through: the size lands in a URL path segment.
const TMDB_SIZES: &[&str] = &[
    "w92", "w154", "w185", "w300", "w342", "w500", "w780", "w1280",
];

static IMAGE_AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();

/// Builds a TMDB art URL from a named size and file, or `None` when either is
/// not the plain token TMDB uses.
///
/// The same posture as `external_url` in the shell API: the UI names a size and
/// a file, never an address, so nothing a page can say turns into a request to
/// an arbitrary host.
pub fn tmdb_image_url(size: &str, file: &str) -> Option<String> {
    if !TMDB_SIZES.contains(&size) {
        return None;
    }
    let file = file.trim_start_matches('/');
    let (stem, extension) = file.rsplit_once('.')?;
    let plain = |value: &str, extra: &[u8]| {
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || extra.contains(&byte))
    };
    if !plain(stem, b"-_") || !matches!(extension, "jpg" | "jpeg" | "png" | "svg" | "webp") {
        return None;
    }
    Some(format!("{TMDB_IMAGE_HOST}/{size}/{stem}.{extension}"))
}

/// Fetches one TMDB image.
///
/// Deliberately not a [`SeerrClient`] method: this goes to a third-party art
/// host, carries no session cookie, and must never be handed a URL that did not
/// come out of [`tmdb_image_url`].
pub fn fetch_tmdb_image(url: &str) -> Result<(Vec<u8>, String), SeerrError> {
    let agent = IMAGE_AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .user_agent(format!("mediaflick-desktop/{}", build_info::APP_VERSION))
            .build()
            .into()
    });
    let mut response = agent.get(url).call().map_err(map_ureq_error)?;
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes = response
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_to_vec()
        .map_err(|error| SeerrError::Transport(error.to_string()))?;
    Ok((bytes, content_type))
}

/// What to say when Seerr's API answered with a redirect.
///
/// This is not a Seerr fault and not something a retry will settle: a sign-on
/// proxy — authentik, Authelia, oauth2-proxy, Cloudflare Access — has taken the
/// call before Seerr saw it. MediaFlick signs in *to Seerr*, with the user's own
/// media-server account, and holds no credential for a layer in front of it, so
/// the message says what has to change rather than what failed.
fn intercepted_message(location: &str) -> String {
    let destination = match host_of(location) {
        Some(host) => format!("redirects to {host}"),
        None => "redirects away".to_string(),
    };
    format!(
        "that address {destination} instead of answering, so a sign-on proxy is sitting in \
         front of Seerr. MediaFlick signs in to Seerr itself and cannot get through one — \
         exempt Seerr's /api/ paths from the proxy, or use an address that reaches Seerr \
         directly."
    )
}

/// The host of an absolute URL, for naming where a redirect went. Only the host
/// is ever used: the rest of a sign-on redirect is query state and tokens.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?.trim();
    (!host.is_empty() && host.len() <= 253).then(|| host.to_string())
}

/// A successful response body together with what the server called it.
///
/// The content type is carried this far for one reason: the most common way a
/// Seerr address goes wrong is that it reaches something else — a reverse proxy,
/// an SSO login page, or Seerr's own web front end rather than its API — and all
/// of those answer 200 with HTML. Reporting a JSON parser position for that
/// sends the user looking for a fault in Seerr instead of in the address.
pub struct Payload {
    bytes: Vec<u8>,
    content_type: String,
}

impl Payload {
    fn is_json(&self) -> bool {
        self.content_type
            .split(';')
            .next()
            .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"))
    }

    /// What the answer was, in words, for a message the user can act on.
    fn describe(&self) -> String {
        match self.content_type.split(';').next().map(str::trim) {
            Some("text/html" | "application/xhtml+xml") => "a web page".to_string(),
            Some("") | None => "an unlabelled response".to_string(),
            Some(kind) => format!("{kind} content"),
        }
    }
}

/// Reads a JSON body, distinguishing "Seerr changed shape" from "that was not
/// Seerr's API at all".
fn decode<T: DeserializeOwned>(path: &str, payload: &Payload) -> Result<T, SeerrError> {
    serde_json::from_slice(&payload.bytes).map_err(|error| {
        // The body itself is deliberately never logged or surfaced: it can
        // carry session material, and the two facts that actually identify the
        // problem are what the server called it and how big it was.
        tracing::debug!(
            target: "seerr.api",
            path,
            content_type = %payload.content_type,
            bytes = payload.bytes.len(),
            "could not read the Seerr response as JSON"
        );
        if payload.is_json() {
            SeerrError::Decode(error.to_string())
        } else {
            SeerrError::Unusable(format!(
                "that address answered with {} instead of Seerr's API. Check it points at \
                 Seerr itself — the address of its web page works, but a proxy or sign-on \
                 page in front of it does not.",
                payload.describe()
            ))
        }
    })
}

/// With `http_status_as_error` off, anything reaching here is a transport
/// failure; the status arm stays as a belt-and-braces mapping.
fn map_ureq_error(error: ureq::Error) -> SeerrError {
    match error {
        ureq::Error::StatusCode(401) => SeerrError::Unauthorized,
        ureq::Error::StatusCode(status) => SeerrError::Status { status },
        other => SeerrError::Transport(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{SeerrClient, SessionCookies, host_of, intercepted_message, tmdb_image_url};

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
        let client = SeerrClient::new("https://seerr.test/", SessionCookies::default());
        assert_eq!(
            client.url("settings/public", &[]),
            "https://seerr.test/api/v1/settings/public"
        );
        assert_eq!(
            client.url("/search", &[("query", "the matrix".to_string())]),
            "https://seerr.test/api/v1/search?query=the%20matrix"
        );
    }

    #[test]
    fn a_client_reports_back_the_jar_it_was_built_with() {
        let jar = cookies(&["connect.sid=abc"]);
        let client = SeerrClient::new("https://seerr.test", jar.clone());
        assert_eq!(client.cookies(), jar);
    }

    #[test]
    fn poster_addresses_are_built_from_a_named_size_and_file() {
        assert_eq!(
            tmdb_image_url("w300", "/f89U3ADr1oiB1s9GkdPOEpXUk5H.jpg").as_deref(),
            Some("https://image.tmdb.org/t/p/w300/f89U3ADr1oiB1s9GkdPOEpXUk5H.jpg")
        );
        // Seerr hands the path out with a leading slash; the UI may strip it.
        assert_eq!(
            tmdb_image_url("w780", "abc-_1.png").as_deref(),
            Some("https://image.tmdb.org/t/p/w780/abc-_1.png")
        );
    }

    /// The size and the file both land in a URL path, so anything that is not
    /// the plain token TMDB uses must produce no address at all.
    #[test]
    fn poster_addresses_reject_anything_that_is_not_a_plain_token() {
        let too_long = format!("{}.jpg", "a".repeat(65));
        for (size, file) in [
            ("w300", "../../../etc/passwd"),
            ("w300", "abc.jpg?x=1"),
            ("w300", "abc.jpg#f"),
            ("w300", "ab c.jpg"),
            ("w300", "abc.exe"),
            ("w300", "abc"),
            ("w300", ""),
            ("w300", too_long.as_str()),
            // Not on the allowlist: `original` is a real TMDB size, but the
            // point of the list is that the proxy never forwards an unbounded
            // rendition for a poster wall.
            ("original", "abc.jpg"),
            ("../w300", "abc.jpg"),
            ("", "abc.jpg"),
        ] {
            assert_eq!(tmdb_image_url(size, file), None, "{size}/{file}");
        }
    }

    /// Only the host is taken out of a redirect: the rest of a sign-on
    /// redirect is state and tokens, and none of it belongs in a message.
    #[test]
    fn a_redirect_is_named_by_its_host_alone() {
        let message = intercepted_message(
            "https://auth.example.de/application/o/authorize/?client_id=secret&state=jwt",
        );
        assert!(
            message.contains("redirects to auth.example.de"),
            "{message}"
        );
        assert!(!message.contains("client_id"), "{message}");
        assert!(message.contains("sign-on proxy"));
        assert!(message.contains("/api/"));

        // A redirect with no usable target still has to say what is wrong.
        let message = intercepted_message("/login");
        assert!(message.contains("redirects away"), "{message}");
    }

    #[test]
    fn hosts_are_read_only_from_absolute_urls() {
        assert_eq!(
            host_of("https://auth.test/x?y").as_deref(),
            Some("auth.test")
        );
        assert_eq!(host_of("http://host:8443").as_deref(), Some("host:8443"));
        assert_eq!(host_of("/relative"), None);
        assert_eq!(host_of(""), None);
        assert_eq!(host_of("https:///nohost"), None);
    }

    #[test]
    fn a_poisoned_cookie_lock_keeps_the_existing_jar() {
        let jar = cookies(&["connect.sid=abc"]);
        let client = SeerrClient::new("https://seerr.test", jar.clone());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _cookies = client.cookies.lock().expect("cookies");
            panic!("poison the cookie lock");
        }));

        assert_eq!(client.cookies(), jar);
    }
}
