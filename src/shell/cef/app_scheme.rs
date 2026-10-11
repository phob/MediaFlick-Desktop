//! Serves the own UI and its JSON API on `mediaflick-desktop://app/`.
//!
//! The scheme is registered as `STANDARD | SECURE | CORS_ENABLED |
//! FETCH_ENABLED`, so this is a real same-origin web app with no localhost
//! port and no jellyfin-web involved.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Instant;

use cef::*;

use super::api::{self, ApiRequest, ApiResponse, Lane};

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
            let api_request = ApiRequest {
                method: CefString::from(&request.method()).to_string(),
                path,
                query,
                body: read_post_body(request),
                range: request_header(request, "Range"),
                cancelled: Arc::new(AtomicBool::new(false)),
            };
            Some(AppResourceHandler::new(
                Arc::new(api_request),
                Arc::new(Mutex::new(ResponseState::default())),
                Instant::now(),
            ))
        }
    }
}

/// What `response_headers` needs from a prepared response. The body can be
/// megabytes of artwork or trailer bytes, so it is never cloned for this.
struct ResponseHead {
    status: u16,
    content_type: String,
    cache_control: &'static str,
    headers: Vec<(String, String)>,
    length: usize,
}

impl ResponseHead {
    fn of(response: &ApiResponse) -> Self {
        Self {
            status: response.status,
            content_type: response.content_type.clone(),
            cache_control: response.cache_control,
            headers: response.headers.clone(),
            length: response.body.len(),
        }
    }

    /// `open` always stores a response first; this only covers a poisoned lock.
    fn internal_error() -> Self {
        Self {
            status: 500,
            content_type: "text/plain; charset=utf-8".to_string(),
            cache_control: "no-store",
            headers: Vec::new(),
            length: 0,
        }
    }
}

/// Runs the handler and stores its response for `response_headers` and `read`.
fn serve(request: &ApiRequest, state: &Mutex<ResponseState>, created: Instant) {
    let started = Instant::now();
    let response = api::handle(request);
    if request.path.starts_with("/api/") || response.status >= 400 {
        tracing::debug!(
            target: "app.scheme",
            method = %request.method,
            path = %request.path,
            status = response.status,
            queue_ms = started.duration_since(created).as_millis(),
            handler_ms = started.elapsed().as_millis(),
            thread = ?std::thread::current().id(),
            "app request served"
        );
    }
    if let Ok(mut state) = state.lock() {
        state.response = Some(response);
        state.offset = 0;
    }
}

/// Network workers for [`Lane::Remote`]. Chromium had 16 of its
/// browser-process workers blocked in these handlers during sign-in before
/// they moved here, so a server sees the same concurrency as before, and a
/// fast library scroll still cannot open a connection per poster.
const REMOTE_WORKERS: usize = 16;

/// A [`Lane::Remote`] request waiting for a network worker.
struct RemoteJob {
    request: Arc<ApiRequest>,
    state: Arc<Mutex<ResponseState>>,
    created: Instant,
    callback: Callback,
}

impl RemoteJob {
    fn run(self) {
        // The page gave up while this waited behind other server calls.
        if self.request.cancelled.load(Ordering::Relaxed) {
            self.callback.cancel();
            return;
        }
        serve(&self.request, &self.state, self.created);
        self.callback.cont();
    }
}

/// The network workers' queue, started on first use. `None` when not one
/// worker could start; remote requests are then served inline.
fn remote_jobs() -> Option<&'static mpsc::Sender<RemoteJob>> {
    static JOBS: OnceLock<Option<mpsc::Sender<RemoteJob>>> = OnceLock::new();
    JOBS.get_or_init(start_remote_workers).as_ref()
}

fn start_remote_workers() -> Option<mpsc::Sender<RemoteJob>> {
    let (sender, receiver) = mpsc::channel();
    let receiver = Arc::new(Mutex::new(receiver));
    let mut started = 0;
    for index in 0..REMOTE_WORKERS {
        let receiver = receiver.clone();
        match std::thread::Builder::new()
            .name(format!("app-remote-{index}"))
            .spawn(move || remote_worker(&receiver))
        {
            Ok(_) => started += 1,
            Err(error) => {
                tracing::warn!(target: "app.scheme", "could not start a network worker: {error}");
            }
        }
    }
    (started > 0).then_some(sender)
}

fn remote_worker(jobs: &Mutex<mpsc::Receiver<RemoteJob>>) {
    loop {
        // One idle worker waits in `recv`; the others wait for the lock.
        let job = match jobs.lock() {
            Ok(jobs) => jobs.recv(),
            Err(_) => return,
        };
        let Ok(job) = job else {
            return;
        };
        job.run();
    }
}

wrap_resource_handler! {
    struct AppResourceHandler {
        request: Arc<ApiRequest>,
        state: Arc<Mutex<ResponseState>>,
        created: Instant,
    }

    impl ResourceHandler {
        // CEF raises this on its IO thread while the handler may still be
        // running on a CEF background thread or a network worker; the shared
        // flag is the only channel through which that handler can notice.
        fn cancel(&self) {
            self.request.cancelled.store(true, Ordering::Relaxed);
        }

        fn open(
            &self,
            _request: Option<&mut Request>,
            handle_request: Option<&mut i32>,
            callback: Option<&mut Callback>,
        ) -> i32 {
            let Some(handle_request) = handle_request else {
                return 0;
            };
            // CEF calls this on Chromium's shared browser-process workers. A
            // server call here holds one of them, and local requests queue
            // behind it, so remote handlers answer later through the callback.
            if api::lane(&self.request) == Lane::Remote
                && let (Some(jobs), Some(callback)) = (remote_jobs(), callback)
            {
                let job = RemoteJob {
                    request: self.request.clone(),
                    state: self.state.clone(),
                    created: self.created,
                    callback: callback.clone(),
                };
                // A send fails only once every worker has exited; the request
                // is then served inline below.
                if jobs.send(job).is_ok() {
                    *handle_request = 0;
                    return 1;
                }
            }

            // CEF calls this on a background thread, which is what makes the
            // blocking SQLite calls in local handlers safe.
            debug_assert_eq!(currently_on(ThreadId::UI), 0);
            debug_assert_eq!(currently_on(ThreadId::IO), 0);
            serve(&self.request, &self.state, self.created);
            *handle_request = 1;
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
            // Only the head is copied out; the body stays in place for `read`.
            let prepared = self
                .state
                .lock()
                .ok()
                .and_then(|state| state.response.as_ref().map(ResponseHead::of))
                .unwrap_or_else(ResponseHead::internal_error);

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
                *response_length = prepared.length as i64;
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
    use super::{APP_URL, is_app_url};

    #[test]
    fn only_the_app_host_is_served_by_this_handler() {
        assert!(is_app_url(APP_URL));
        assert!(is_app_url("mediaflick-desktop://app/api/items?limit=1"));
        assert!(is_app_url("mediaflick-desktop://APP/app.js"));
        assert!(!is_app_url("mediaflick-desktop://client-settings?token=x"));
        assert!(!is_app_url("mediaflick-desktop://app-exit"));
        assert!(!is_app_url("http://server:8096/web/"));
    }
}
