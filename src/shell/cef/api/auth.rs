use crate::jellyfin::session::{QuickConnectPoll, QuickConnectStart, ServerInfo};

use super::*;

routes! {
    |services, request|
    POST ["auth", "connect"] (body: ServerBody) -> ServerInfo =
        auth_connect(services, request, body),
    POST ["auth", "login"] (body: LoginBody) -> AppStatus = auth_login(services, body),
    POST ["auth", "quickconnect", "start"] (body: ServerBody) -> QuickConnectStart =
        quick_connect_start(services, body),
    POST ["auth", "quickconnect", "poll"] (body: QuickConnectPollBody) -> QuickConnectPoll =
        quick_connect_poll(services, body),
    POST ["auth", "logout"] (body: LogoutBody) -> AppStatus = auth_logout(services, body),
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct ServerBody {
    server: String,
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct LoginBody {
    server: String,
    username: String,
    password: String,
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct QuickConnectPollBody {
    server: String,
    secret: String,
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct LogoutBody {
    #[serde(default)]
    #[ts(optional = nullable)]
    forget_library: bool,
}

fn auth_connect(
    services: &Arc<Services>,
    request: &ApiRequest,
    body: Body<'_, ServerBody>,
) -> Result<ServerInfo, ApiResponse> {
    let body = body.decode()?;
    if request.is_cancelled() {
        return Err(ApiResponse::from_api_error(&ApiError::Cancelled));
    }
    match services
        .session
        .connect(&body.server, Some(Arc::clone(&request.cancelled)))
    {
        Ok(server) => Ok(server),
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn auth_login(
    services: &Arc<Services>,
    body: Body<'_, LoginBody>,
) -> Result<AppStatus, ApiResponse> {
    let body = body.decode()?;
    finish_pending_deletions(services)?;
    let result = services
        .session
        .login(&body.server, &body.username, &body.password);
    match result {
        Ok(_) => {
            remember_server_url(services);
            activate_account_preferences(services)?;
            services.companion.clear();
            if let Err(error) = services.companion.probe(true) {
                tracing::debug!(target: "companion", "post-login probe failed: {error}");
            }
            services.sync.request();
            crate::collections::scheduler::request_run(services.clone());
            Ok(app_status(services))
        }
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn quick_connect_start(
    services: &Arc<Services>,
    body: Body<'_, ServerBody>,
) -> Result<QuickConnectStart, ApiResponse> {
    let body = body.decode()?;
    finish_pending_deletions(services)?;
    match services.session.quick_connect_start(&body.server) {
        Ok(start) => Ok(start),
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn quick_connect_poll(
    services: &Arc<Services>,
    body: Body<'_, QuickConnectPollBody>,
) -> Result<QuickConnectPoll, ApiResponse> {
    let body = body.decode()?;
    let result = services
        .session
        .quick_connect_poll(&body.server, &body.secret);
    match result {
        Ok(poll) => {
            if poll.authenticated {
                remember_server_url(services);
                activate_account_preferences(services)?;
                services.companion.clear();
                if let Err(error) = services.companion.probe(true) {
                    tracing::debug!(target: "companion", "post-login probe failed: {error}");
                }
                services.sync.request();
                crate::collections::scheduler::request_run(services.clone());
            }
            Ok(poll)
        }
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn auth_logout(
    services: &Arc<Services>,
    body: Body<'_, LogoutBody>,
) -> Result<AppStatus, ApiResponse> {
    let body = body.decode()?;
    if let Err(error) = services.session.logout(body.forget_library) {
        tracing::error!("could not clear the local session after logout: {error}");
        return Err(ApiResponse::error(500, "could not clear the local session"));
    }
    if let Err(error) = services.preferences.activate_account(None) {
        tracing::error!("could not clear account preferences after logout: {error}");
        return Err(ApiResponse::error(
            500,
            "could not clear account preferences",
        ));
    }
    services.companion.clear();
    Ok(app_status(services))
}

fn finish_pending_deletions(services: &Arc<Services>) -> Result<(), ApiResponse> {
    crate::app::services::finish_pending_deletions(services).map_err(|error| {
        ApiResponse::error(
            500,
            format!("could not finish deleting local account data: {error}"),
        )
    })
}

fn remember_server_url(services: &Services) {
    let Some(server_url) = services.session.server_url() else {
        return;
    };
    if services.preferences.snapshot().jellyfin_url.as_deref() == Some(server_url.as_str()) {
        return;
    }
    if let Err(error) = services.preferences.set_server_url(server_url) {
        tracing::warn!(target: "jellyfin.session", "failed to save the server URL: {error}");
    }
}

fn activate_account_preferences(services: &Arc<Services>) -> Result<(), ApiResponse> {
    let Some(account) = services.session.account_key() else {
        tracing::error!("authenticated Jellyfin session has no stable account identity");
        return Err(ApiResponse::error(
            500,
            "the signed-in account has no stable identity",
        ));
    };
    services
        .preferences
        .activate_account(Some(account))
        .map(|_| ())
        .map_err(|error| {
            tracing::error!("could not activate account preferences: {error}");
            ApiResponse::error(500, "could not load account preferences")
        })
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use crate::app::services::test_support::TestServices;
    use crate::shell::cef::api::tests::{
        error_of, post, read_request_with_headers, respond, send, server_url, silent_server,
    };
    use crate::shell::cef::api::{ApiRequest, dispatch};

    fn post_with_cancellation(path: &str, body: &[u8], cancelled: Arc<AtomicBool>) -> ApiRequest {
        ApiRequest {
            cancelled,
            ..post(path, body)
        }
    }

    fn spawn_login_server(
        listener: TcpListener,
        played_tx: std::sync::mpsc::Sender<(String, String)>,
        stop: Arc<AtomicBool>,
    ) -> std::thread::JoinHandle<()> {
        listener.set_nonblocking(true).expect("nonblocking y");
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).expect("blocking");
                        let (line, headers) = read_request_with_headers(&mut stream);
                        if line.starts_with("GET /System/Info/Public") {
                            respond(
                                &mut stream,
                                "200 OK",
                                r#"{"ServerName":"Server Y","Version":"12.4.0"}"#,
                            );
                        } else if line.starts_with("POST /Users/AuthenticateByName") {
                            respond(
                                &mut stream,
                                "200 OK",
                                r#"{"AccessToken":"y-token","ServerId":"server-y","User":{"Id":"alice","Name":"Alice","Policy":{"IsAdministrator":false}}}"#,
                            );
                        } else if line
                            .starts_with("POST /UserPlayedItems/0a1b2c3d4e5f60718293a4b5c6d7e8f9")
                        {
                            respond(&mut stream, "204 No Content", "");
                            let _ = played_tx.send((line, headers));
                        } else {
                            respond(&mut stream, "404 Not Found", "");
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        })
    }

    #[test]
    fn a_late_sign_in_probe_cannot_repoint_the_live_session_to_another_server() {
        let listener_x = TcpListener::bind("127.0.0.1:0").expect("listener x");
        let url_x = server_url(&listener_x);
        let listener_y = TcpListener::bind("127.0.0.1:0").expect("listener y");
        let url_y = server_url(&listener_y);
        let fixture = TestServices::signed_out();
        let (probe_reached_tx, probe_reached_rx) = std::sync::mpsc::channel();
        let (release_probe_tx, release_probe_rx) = std::sync::mpsc::channel::<()>();
        let (played_tx, played_rx) = std::sync::mpsc::channel();
        let stop_y = Arc::new(AtomicBool::new(false));

        let server_x = std::thread::spawn(move || {
            let (mut info, _) = listener_x.accept().expect("probe info");
            let (info_line, _) = read_request_with_headers(&mut info);
            probe_reached_tx.send(()).expect("signal probe");
            release_probe_rx.recv().expect("release probe");
            respond(
                &mut info,
                "200 OK",
                r#"{"ServerName":"Server X","Version":"12.4.0"}"#,
            );
            let (mut quick, _) = listener_x.accept().expect("probe quick connect");
            let _ = read_request_with_headers(&mut quick);
            respond(&mut quick, "200 OK", "true");
            listener_x.set_nonblocking(true).expect("nonblocking");
            (listener_x, info_line)
        });
        let server_y = spawn_login_server(listener_y, played_tx, Arc::clone(&stop_y));

        let services = fixture.services.clone();
        let probe_body = serde_json::to_vec(&json!({ "server": url_x })).expect("probe body");
        let probe_worker = std::thread::spawn(move || {
            dispatch(&post("/api/auth/connect", &probe_body), || {
                Ok(services.clone())
            })
        });

        probe_reached_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("probe reached server X");

        let login_body = serde_json::to_vec(&json!({
            "server": url_y,
            "username": "alice",
            "password": "secret"
        }))
        .expect("login body");
        let login_response = send(&fixture, &post("/api/auth/login", &login_body));
        assert_eq!(login_response.status, 200, "{}", error_of(&login_response));

        release_probe_tx.send(()).expect("release probe");
        let probe_response = probe_worker.join().expect("probe worker");
        assert_eq!(probe_response.status, 200, "{}", error_of(&probe_response));
        let probe_json: Value = serde_json::from_slice(&probe_response.body).expect("probe json");
        assert_eq!(probe_json["serverUrl"], url_x);
        assert_eq!(probe_json["serverName"], "Server X");

        assert_eq!(
            fixture.services.session.server_url().as_deref(),
            Some(url_y.as_str())
        );
        let status = fixture.services.session.status();
        assert!(status.authenticated && !status.expired);
        assert_eq!(status.server_url.as_deref(), Some(url_y.as_str()));
        assert_eq!(status.server_name, None);

        let played_response = send(
            &fixture,
            &post(
                "/api/item/0a1b2c3d4e5f60718293a4b5c6d7e8f9/played",
                br#"{"played":true}"#,
            ),
        );
        assert_eq!(
            played_response.status,
            200,
            "{}",
            error_of(&played_response)
        );

        let (y_line, y_headers) = played_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("played write reached server Y");
        stop_y.store(true, Ordering::Relaxed);
        server_y.join().expect("server Y");
        assert!(
            y_line.starts_with("POST /UserPlayedItems/0a1b2c3d4e5f60718293a4b5c6d7e8f9"),
            "{y_line}"
        );
        assert!(y_headers.contains("Token=\"y-token\""), "{y_headers}");

        let (listener_x, info_line) = server_x.join().expect("server X");
        assert!(
            info_line.starts_with("GET /System/Info/Public"),
            "{info_line}"
        );
        assert_eq!(
            listener_x
                .accept()
                .expect_err("no extra request to X")
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn cancelling_a_server_probe_stops_retries_immediately() {
        let fixture = TestServices::signed_out();
        let silent = silent_server();
        let pre_cancelled_body =
            serde_json::to_vec(&json!({ "server": server_url(&silent) })).expect("body");
        let pre_cancelled = post_with_cancellation(
            "/api/auth/connect",
            &pre_cancelled_body,
            Arc::new(AtomicBool::new(true)),
        );
        assert_eq!(send(&fixture, &pre_cancelled).status, 499);
        assert_eq!(
            silent.accept().expect_err("no request").kind(),
            std::io::ErrorKind::WouldBlock
        );

        let listener = TcpListener::bind("127.0.0.1:0").expect("retry listener");
        let url = server_url(&listener);
        listener.set_nonblocking(true).expect("nonblocking");
        let (reached_tx, reached_rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_counter = Arc::clone(&attempts);
        let server = std::thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).expect("blocking");
                        let _ = read_request_with_headers(&mut stream);
                        let count = attempts_counter.fetch_add(1, Ordering::Relaxed) + 1;
                        if count == 1 {
                            let _ = write!(
                                stream,
                                "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 5\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            );
                            let _ = reached_tx.send(());
                        } else {
                            respond(&mut stream, "404 Not Found", "");
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let services = fixture.services.clone();
        let started = Instant::now();
        let worker = std::thread::spawn(move || {
            let body = serde_json::to_vec(&json!({ "server": url })).expect("body");
            let request = post_with_cancellation("/api/auth/connect", &body, worker_cancelled);
            dispatch(&request, || Ok(services.clone()))
        });
        reached_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first probe attempt");
        cancelled.store(true, Ordering::Relaxed);
        let response = worker.join().expect("worker");
        let elapsed = started.elapsed();
        stop.store(true, Ordering::Relaxed);
        server.join().expect("server");
        assert_eq!(response.status, 499, "{}", error_of(&response));
        assert!(elapsed < Duration::from_secs(2), "elapsed: {elapsed:?}");
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cancelling_an_in_flight_probe_skips_the_quick_connect_check() {
        let fixture = TestServices::signed_out();
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let url = server_url(&listener);
        let (reached_tx, reached_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let attempts = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&attempts);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);

        let server = std::thread::spawn(move || {
            let (mut info, _) = listener.accept().expect("info");
            let _ = read_request_with_headers(&mut info);
            counter.fetch_add(1, Ordering::Relaxed);
            reached_tx.send(()).expect("reached");
            release_rx.recv().expect("release");
            respond(
                &mut info,
                "200 OK",
                r#"{"ServerName":"Slow Server","Version":"12.4.0"}"#,
            );
            listener.set_nonblocking(true).expect("nonblocking");
            while !stop_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut extra, _)) => {
                        extra.set_nonblocking(false).expect("blocking");
                        let _ = read_request_with_headers(&mut extra);
                        counter.fetch_add(1, Ordering::Relaxed);
                        respond(&mut extra, "200 OK", "true");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let services = fixture.services.clone();
        let worker = std::thread::spawn(move || {
            let body = serde_json::to_vec(&json!({ "server": url })).expect("body");
            let request = post_with_cancellation("/api/auth/connect", &body, worker_cancelled);
            dispatch(&request, || Ok(services.clone()))
        });
        reached_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("probe reached server");
        cancelled.store(true, Ordering::Relaxed);
        release_tx.send(()).expect("release");
        let response = worker.join().expect("worker");
        stop.store(true, Ordering::Relaxed);
        server.join().expect("server");
        assert_eq!(response.status, 499, "{}", error_of(&response));
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
    }
}
