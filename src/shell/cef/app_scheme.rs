//! Serves the own UI and its JSON API on `mediaflick-desktop://app/`.
//!
//! The scheme is registered as `STANDARD | SECURE | CORS_ENABLED |
//! FETCH_ENABLED`, so this is a real same-origin web app with no localhost
//! port and no jellyfin-web involved.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cef::*;

use super::api::{self, ApiRequest, ApiResponse};

pub const APP_SCHEME: &str = "mediaflick-desktop";
pub const APP_HOST: &str = "app";
/// Where the browser window points at startup.
pub const APP_URL: &str = "mediaflick-desktop://app/";

/// True for URLs this handler owns (as opposed to the legacy action URLs the
/// native dialogs still use, which live on other hosts).
pub fn is_app_url(url: &str) -> bool {
    url.strip_prefix("mediaflick-desktop://")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .is_some_and(|host| host.eq_ignore_ascii_case(APP_HOST))
}

/// Must run on the CEF UI thread after context initialisation.
pub fn register() {
    let mut factory = AppSchemeHandlerFactory::new();
    let registered = register_scheme_handler_factory(
        Some(&CefString::from(APP_SCHEME)),
        Some(&CefString::from(APP_HOST)),
        Some(&mut factory),
    );
    if registered == 0 {
        tracing::error!(target: "app.scheme", "failed to register the app scheme handler");
    } else {
        tracing::debug!(target: "app.scheme", url = APP_URL, "app scheme handler registered");
    }
}

/// Splits `mediaflick-desktop://app/api/items?limit=20` into path and query.
fn split_url(url: &str) -> (String, String) {
    let without_scheme = url
        .strip_prefix("mediaflick-desktop://")
        .unwrap_or(url)
        .split('#')
        .next()
        .unwrap_or_default();
    let after_host = match without_scheme.find('/') {
        Some(index) => &without_scheme[index..],
        // `mediaflick-desktop://app?x=1` has no path at all.
        None => "/",
    };
    match after_host.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (after_host.to_string(), String::new()),
    }
}

fn read_post_body(request: &Request) -> Vec<u8> {
    let Some(post_data) = request.post_data() else {
        return Vec::new();
    };
    // `elements` fills in place and takes its capacity from the vector's
    // current length, so it has to be sized up front or nothing is returned.
    let count = post_data.element_count();
    if count == 0 {
        return Vec::new();
    }
    let mut elements = vec![None; count];
    post_data.elements(Some(&mut elements));
    let mut body = Vec::new();
    for element in elements.into_iter().flatten() {
        let count = element.bytes_count();
        if count == 0 {
            continue;
        }
        let mut buffer = vec![0u8; count];
        let read = element.bytes(count, buffer.as_mut_ptr());
        buffer.truncate(read);
        body.extend_from_slice(&buffer);
    }
    body
}

fn request_header(request: &Request, name: &str) -> Option<String> {
    let mut headers = CefStringMultimap::new();
    request.header_map(Some(&mut headers));
    headers.into_iter().find_map(|(key, values)| {
        key.eq_ignore_ascii_case(name)
            .then(|| values.into_iter().next())
            .flatten()
    })
}

#[derive(Default)]
struct ResponseState {
    response: Option<ApiResponse>,
    offset: usize,
}

wrap_scheme_handler_factory! {
    struct AppSchemeHandlerFactory;

    impl SchemeHandlerFactory {
        fn create(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _scheme_name: Option<&CefString>,
            request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            let request = request?;
            let url = CefString::from(&request.url()).to_string();
            let (path, query) = split_url(&url);
            let cancelled = Arc::new(AtomicBool::new(false));
            let api_request = ApiRequest {
                method: CefString::from(&request.method()).to_string(),
                path,
                query,
                body: read_post_body(request),
                range: request_header(request, "Range"),
                cancelled: cancelled.clone(),
            };
            Some(AppResourceHandler::new(
                api_request,
                cancelled,
                Arc::new(Mutex::new(ResponseState::default())),
            ))
        }
    }
}

wrap_resource_handler! {
    struct AppResourceHandler {
        request: ApiRequest,
        cancelled: Arc<AtomicBool>,
        state: Arc<Mutex<ResponseState>>,
    }

    impl ResourceHandler {
        // CEF raises this on its IO thread while `open` may still be blocked
        // in a handler on a background thread; the shared flag is the only
        // channel through which that handler can notice.
        fn cancel(&self) {
            self.cancelled.store(true, Ordering::Relaxed);
        }

        fn open(
            &self,
            _request: Option<&mut Request>,
            handle_request: Option<&mut i32>,
            _callback: Option<&mut Callback>,
        ) -> i32 {
            // CEF calls this on a background thread, which is what makes the
            // blocking SQLite and Jellyfin calls in the handlers safe.
            debug_assert_eq!(currently_on(ThreadId::UI), 0);
            debug_assert_eq!(currently_on(ThreadId::IO), 0);

            let response = api::handle(&self.request);
            if response.status >= 400 {
                tracing::debug!(
                    target: "app.scheme",
                    method = %self.request.method,
                    path = %self.request.path,
                    status = response.status,
                    "app request failed"
                );
            }
            if let Ok(mut state) = self.state.lock() {
                state.response = Some(response);
                state.offset = 0;
            }
            if let Some(handle_request) = handle_request {
                *handle_request = 1;
            }
            1
        }

        fn response_headers(
            &self,
            response: Option<&mut Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            let Some(response) = response else {
                return;
            };
            let prepared = self
                .state
                .lock()
                .ok()
                .and_then(|state| state.response.clone())
                .unwrap_or_else(|| ApiResponse {
                    status: 500,
                    content_type: "text/plain; charset=utf-8".to_string(),
                    body: b"internal error".to_vec(),
                    cache_control: "no-store",
                    headers: Vec::new(),
                });

            response.set_status(i32::from(prepared.status));
            response.set_status_text(Some(&CefString::from(status_text(prepared.status))));
            let (mime_type, charset) = split_content_type(&prepared.content_type);
            response.set_mime_type(Some(&CefString::from(mime_type)));
            if let Some(charset) = charset {
                response.set_charset(Some(&CefString::from(charset)));
            }
            response.set_header_by_name(
                Some(&CefString::from("Cache-Control")),
                Some(&CefString::from(prepared.cache_control)),
                1,
            );
            // The UI is a first-party bundle; nothing may be pulled from the web.
            response.set_header_by_name(
                Some(&CefString::from("Content-Security-Policy")),
                Some(&CefString::from(CONTENT_SECURITY_POLICY)),
                1,
            );
            for (name, value) in &prepared.headers {
                response.set_header_by_name(
                    Some(&CefString::from(name.as_str())),
                    Some(&CefString::from(value.as_str())),
                    1,
                );
            }
            if let Some(response_length) = response_length {
                *response_length = prepared.body.len() as i64;
            }
        }

        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: i32,
            bytes_read: Option<&mut i32>,
            _callback: Option<&mut ResourceReadCallback>,
        ) -> i32 {
            let Some(bytes_read) = bytes_read else {
                return 0;
            };
            *bytes_read = 0;
            if bytes_to_read < 1 || data_out.is_null() {
                return 0;
            }
            let Ok(mut state) = self.state.lock() else {
                return 0;
            };
            let Some(response) = &state.response else {
                return 0;
            };
            let remaining = response.body.len().saturating_sub(state.offset);
            if remaining == 0 {
                return 0;
            }
            let count = remaining.min(bytes_to_read as usize);
            let start = state.offset;
            // SAFETY: CEF guarantees `data_out` points at `bytes_to_read`
            // writable bytes, and `count` never exceeds that.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    response.body[start..start + count].as_ptr(),
                    data_out,
                    count,
                );
            }
            state.offset += count;
            *bytes_read = count as i32;
            1
        }
    }
}

/// Scripts and styles ship inside the binary and images and local trailers come
/// from our own proxy. The sole remote exception is a validated,
/// privacy-enhanced YouTube trailer frame. The whole
/// `mediaflick-desktop:` scheme is allowed to connect because the native
/// remaining native dialogs call the shell on other hosts of it
/// (`update-download`, `app-about`, …), and only our own handlers can answer
/// those.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; img-src 'self' data:; \
style-src 'self' 'unsafe-inline'; script-src 'self'; \
connect-src 'self' mediaflick-desktop:; \
frame-src mediaflick-desktop: https://www.youtube-nocookie.com; \
object-src 'none'; base-uri 'none'; form-action 'none'";

fn split_content_type(content_type: &str) -> (&str, Option<&str>) {
    match content_type.split_once("; charset=") {
        Some((mime_type, charset)) => (mime_type.trim(), Some(charset.trim())),
        None => (content_type.trim(), None),
    }
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        416 => "Range Not Satisfiable",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

#[cfg(test)]
mod tests {
    use super::{APP_URL, is_app_url, split_content_type, split_url, status_text};

    #[test]
    fn only_the_app_host_is_served_by_this_handler() {
        assert!(is_app_url(APP_URL));
        assert!(is_app_url("mediaflick-desktop://app/api/items?limit=1"));
        assert!(is_app_url("mediaflick-desktop://APP/app.js"));
        assert!(!is_app_url("mediaflick-desktop://client-settings?token=x"));
        assert!(!is_app_url("mediaflick-desktop://app-exit"));
        assert!(!is_app_url("http://server:8096/web/"));
    }

    #[test]
    fn urls_split_into_a_path_and_a_query() {
        assert_eq!(split_url(APP_URL), ("/".to_string(), String::new()));
        assert_eq!(
            split_url("mediaflick-desktop://app/api/items?search=a%20b"),
            ("/api/items".to_string(), "search=a%20b".to_string())
        );
        assert_eq!(
            split_url("mediaflick-desktop://app/item/x#anchor"),
            ("/item/x".to_string(), String::new())
        );
        assert_eq!(
            split_url("mediaflick-desktop://app?x=1"),
            ("/".to_string(), String::new())
        );
    }

    #[test]
    fn content_types_are_split_into_mime_type_and_charset() {
        assert_eq!(
            split_content_type("application/json; charset=utf-8"),
            ("application/json", Some("utf-8"))
        );
        assert_eq!(split_content_type("image/png"), ("image/png", None));
    }

    #[test]
    fn status_texts_cover_the_codes_the_api_returns() {
        assert_eq!(status_text(200), "OK");
        assert_eq!(status_text(206), "Partial Content");
        assert_eq!(status_text(401), "Unauthorized");
        assert_eq!(status_text(409), "Conflict");
        assert_eq!(status_text(418), "Error");
    }
}
