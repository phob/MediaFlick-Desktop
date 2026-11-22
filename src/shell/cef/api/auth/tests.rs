use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::app::services::test_support::TestServices;
use crate::jellyfin::playback_reporter::{
    PlaybackReporter, TestHttpRequest, flush_playstate_reports, read_test_http_request,
};
use crate::playback::{
    AudioDisplay, AudioKind, AudioPlan, AudioSource, AudioStart, AudioTrack, HttpHeader,
    NativeWindowHandle, PlaybackCoordinator, PlaybackOwner, PlaybackRequest, PlayerBackend,
    PlayerCommand, PlayerSnapshot, PreparedAudio, ReportingState,
};
use crate::preferences::{FullscreenBehavior, PlayerPreferences};
use crate::shell::cef::api::tests::{
    error_of, post, read_request_with_headers, respond, send, server_url, silent_server,
};
use crate::shell::cef::api::{ApiRequest, dispatch};

use super::logout_with_timeouts;

static PLAYSTATE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn post_with_cancellation(path: &str, body: &[u8], cancelled: Arc<AtomicBool>) -> ApiRequest {
    ApiRequest {
        cancelled,
        ..post(path, body)
    }
}

fn spawn_login_server(
    listener: TcpListener,
    played_tx: mpsc::Sender<(String, String)>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    listener.set_nonblocking(true).expect("nonblocking y");
    thread::spawn(move || {
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
                    thread::sleep(Duration::from_millis(10));
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
    let (probe_reached_tx, probe_reached_rx) = mpsc::channel();
    let (release_probe_tx, release_probe_rx) = mpsc::channel::<()>();
    let (played_tx, played_rx) = mpsc::channel();
    let stop_y = Arc::new(AtomicBool::new(false));

    let server_x = thread::spawn(move || {
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
    let probe_worker = thread::spawn(move || {
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
    let (reached_tx, reached_rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_counter = Arc::clone(&attempts);
    let server = thread::spawn(move || {
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
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let services = fixture.services.clone();
    let started = Instant::now();
    let worker = thread::spawn(move || {
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
    let (reached_tx, reached_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&attempts);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);

    let server = thread::spawn(move || {
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
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let services = fixture.services.clone();
    let worker = thread::spawn(move || {
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

#[derive(Debug, Clone)]
struct RecordedRequest {
    request_line: String,
    token: Option<String>,
    body: String,
    status: u16,
}

fn write_status(stream: &mut TcpStream, status: u16) {
    let reason = match status {
        204 => "No Content",
        401 => "Unauthorized",
        _ => "OK",
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let _ = stream.flush();
}

struct TokenRevokingServer {
    url: String,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl TokenRevokingServer {
    fn start(on_stopped: impl Fn(&TestHttpRequest) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
        let addr = listener.local_addr().expect("listener address");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let url = format!("http://{addr}");
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let revoked = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let on_stopped = Arc::new(on_stopped);
        let server_recorded = Arc::clone(&recorded);
        let server_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let mut workers = Vec::new();
            while !server_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let recorded = Arc::clone(&server_recorded);
                        let revoked = Arc::clone(&revoked);
                        let on_stopped = Arc::clone(&on_stopped);
                        workers.push(thread::spawn(move || {
                            let request = read_test_http_request(&mut stream);
                            if request.request_line.is_empty() {
                                return;
                            }
                            let status =
                                if request.request_line.starts_with("POST /Sessions/Logout") {
                                    revoked.store(true, Ordering::SeqCst);
                                    204
                                } else if request
                                    .request_line
                                    .starts_with("POST /Sessions/Playing/Stopped")
                                {
                                    on_stopped(&request);
                                    if revoked.load(Ordering::SeqCst)
                                        || request.token.as_deref() != Some("token")
                                    {
                                        401
                                    } else {
                                        204
                                    }
                                } else {
                                    204
                                };
                            recorded.lock().expect("recorded").push(RecordedRequest {
                                request_line: request.request_line,
                                token: request.token,
                                body: request.body,
                                status,
                            });
                            write_status(&mut stream, status);
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
            for worker in workers {
                let _ = worker.join();
            }
        });
        Self {
            url,
            recorded,
            stop,
            handle: Some(handle),
        }
    }

    fn recorded(&self) -> Vec<RecordedRequest> {
        self.recorded.lock().expect("recorded").clone()
    }

    fn finish(mut self) -> Vec<RecordedRequest> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.recorded()
    }
}

impl Drop for TokenRevokingServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn playback_request(server_url: &str, item_id: &str) -> PlaybackRequest {
    let mut request = PlaybackRequest::new(format!("{server_url}/Videos/{item_id}/stream.mkv"));
    request.item_id = Some(item_id.to_string());
    request.play_session_id = Some("play-session-1".to_string());
    request.headers = vec![HttpHeader {
        name: "X-Emby-Token".to_string(),
        value: "token".to_string(),
    }];
    request
}

struct AlwaysCurrentOwner;

impl PlaybackOwner for AlwaysCurrentOwner {
    fn is_current(&self) -> bool {
        true
    }
}

struct ReportingTestBackend {
    active: Mutex<Option<(PlaybackReporter, ReportingState)>>,
}

impl ReportingTestBackend {
    fn new(reporter: PlaybackReporter, position_ticks: i64) -> Self {
        Self {
            active: Mutex::new(Some((
                reporter,
                ReportingState {
                    position_ticks,
                    duration_ticks: Some(900_000_000),
                    ..ReportingState::default()
                },
            ))),
        }
    }

    fn report_stop_if_active(&self) {
        if let Some((reporter, state)) = self.active.lock().expect("active").take() {
            reporter.report_stopped(&state, false);
        }
    }
}

impl PlayerBackend for ReportingTestBackend {
    fn warm(&self, _path: String, _fullscreen: FullscreenBehavior) {}

    fn native_window(&self, _timeout: Duration) -> Option<NativeWindowHandle> {
        None
    }

    fn load(&self, _path: String, _fullscreen: FullscreenBehavior, _request: PlaybackRequest) {}

    fn control(&self, command: PlayerCommand) {
        if matches!(command, PlayerCommand::Stop) {
            self.report_stop_if_active();
        }
    }

    fn stop_and_wait(&self, _timeout: Duration) -> bool {
        self.report_stop_if_active();
        true
    }

    fn set_preferences(&self, _preferences: PlayerPreferences) {}

    fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            active: self.active.lock().expect("active").is_some(),
            ..PlayerSnapshot::default()
        }
    }

    fn shutdown(&self) {}
}

struct BlockingStopBackend {
    entered: Mutex<Option<mpsc::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl PlayerBackend for BlockingStopBackend {
    fn warm(&self, _path: String, _fullscreen: FullscreenBehavior) {}

    fn native_window(&self, _timeout: Duration) -> Option<NativeWindowHandle> {
        None
    }

    fn load(&self, _path: String, _fullscreen: FullscreenBehavior, _request: PlaybackRequest) {}

    fn control(&self, _command: PlayerCommand) {}

    fn stop_and_wait(&self, _timeout: Duration) -> bool {
        if let Some(entered) = self.entered.lock().expect("entered").take() {
            let _ = entered.send(());
        }
        let _ = self.release.lock().expect("release").recv();
        true
    }

    fn set_preferences(&self, _preferences: PlayerPreferences) {}

    fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            active: true,
            ..PlayerSnapshot::default()
        }
    }

    fn shutdown(&self) {}
}

#[test]
fn sign_out_flushes_active_playback_stopped_report_before_revoking_token() {
    let _guard = PLAYSTATE_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let server = TokenRevokingServer::start(|_| {});
    let fixture = TestServices::signed_in(&server.url);
    let request = playback_request(&server.url, "movie-393");
    let reporter = PlaybackReporter::from_launch(&request).expect("reporter");
    let (shell_tx, _shell_rx) = mpsc::channel();
    let coordinator = Arc::new(PlaybackCoordinator::new(shell_tx, move |_| {
        Box::new(ReportingTestBackend::new(reporter, 420_000_000))
    }));
    coordinator.open(
        "mpv".to_string(),
        FullscreenBehavior::Windowed,
        request,
        Arc::new(AlwaysCurrentOwner),
    );
    fixture.services.attach_playback(coordinator);

    let response = send(&fixture, &post("/api/auth/logout", b"{}"));
    assert_eq!(response.status, 200);
    assert!(!fixture.services.session.is_authenticated());
    assert!(flush_playstate_reports(Duration::from_secs(2)));

    let requests = server.finish();
    let stopped_index = requests
        .iter()
        .position(|entry| {
            entry
                .request_line
                .starts_with("POST /Sessions/Playing/Stopped")
        })
        .expect("expected Stopped report");
    let logout_index = requests
        .iter()
        .position(|entry| entry.request_line.starts_with("POST /Sessions/Logout"))
        .expect("expected Logout request");
    assert!(
        stopped_index < logout_index,
        "Stopped must arrive before Logout, got {requests:?}"
    );
    let stopped = &requests[stopped_index];
    assert_eq!(stopped.status, 204);
    assert_eq!(stopped.token.as_deref(), Some("token"));
    let body: Value = serde_json::from_str(&stopped.body).expect("stopped json");
    assert_eq!(body["ItemId"], "movie-393");
    assert_eq!(body["PositionTicks"], 420_000_000);
}

#[test]
fn sign_out_flushes_already_queued_stopped_report_before_revoking_token() {
    let _guard = PLAYSTATE_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let (entered_tx, entered_rx) = mpsc::channel();
    let server = TokenRevokingServer::start(move |_| {
        let _ = entered_tx.send(());
        thread::sleep(Duration::from_millis(150));
    });
    let fixture = TestServices::signed_in(&server.url);
    let request = playback_request(&server.url, "episode-393");
    let reporter = PlaybackReporter::from_launch(&request).expect("reporter");
    reporter.report_stopped(
        &ReportingState {
            position_ticks: 180_000_000,
            ..ReportingState::default()
        },
        false,
    );
    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("Stopped report reached server");

    let response = send(&fixture, &post("/api/auth/logout", b"{}"));
    assert_eq!(response.status, 200);
    assert!(!fixture.services.session.is_authenticated());
    assert!(flush_playstate_reports(Duration::from_secs(2)));

    let requests = server.finish();
    let stopped_index = requests
        .iter()
        .position(|entry| {
            entry
                .request_line
                .starts_with("POST /Sessions/Playing/Stopped")
        })
        .expect("expected Stopped report");
    let logout_index = requests
        .iter()
        .position(|entry| entry.request_line.starts_with("POST /Sessions/Logout"))
        .expect("expected Logout request");
    assert!(
        stopped_index < logout_index,
        "already-queued Stopped must finish before Logout, got {requests:?}"
    );
    assert_eq!(requests[stopped_index].status, 204);
}

#[test]
fn sign_out_does_not_hang_when_playstate_flush_stalls() {
    let _guard = PLAYSTATE_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let (stalled_tx, stalled_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let server = TokenRevokingServer::start(move |_| {
        let _ = stalled_tx.send(());
        let _ = release_rx.lock().expect("release").recv();
    });
    let fixture = TestServices::signed_in(&server.url);
    let request = playback_request(&server.url, "stalled-393");
    let reporter = PlaybackReporter::from_launch(&request).expect("reporter");
    let (shell_tx, _shell_rx) = mpsc::channel();
    let coordinator = Arc::new(PlaybackCoordinator::new(shell_tx, move |_| {
        Box::new(ReportingTestBackend::new(reporter, 90_000_000))
    }));
    coordinator.open(
        "mpv".to_string(),
        FullscreenBehavior::Windowed,
        request,
        Arc::new(AlwaysCurrentOwner),
    );
    fixture.services.attach_playback(coordinator);

    let started = Instant::now();
    let result = logout_with_timeouts(
        &fixture.services,
        false,
        Duration::from_millis(250),
        Duration::from_millis(250),
    );
    let elapsed = started.elapsed();

    assert!(
        result.is_ok(),
        "logout should still succeed on flush timeout"
    );
    assert!(!fixture.services.session.is_authenticated());
    assert!(
        stalled_rx.try_recv().is_ok(),
        "Stopped report must have been dispatched before logout timed out"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "logout must stay bounded when playstate flush stalls, took {elapsed:?}"
    );

    let _ = release_tx.send(());
    assert!(flush_playstate_reports(Duration::from_secs(2)));
    let requests = server.finish();
    assert!(
        requests
            .iter()
            .any(|entry| entry.request_line.starts_with("POST /Sessions/Logout"))
    );
}

#[test]
fn sign_out_does_not_hang_when_player_stop_stalls() {
    let _guard = PLAYSTATE_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let server = TokenRevokingServer::start(|_| {});
    let fixture = TestServices::signed_in(&server.url);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (shell_tx, _shell_rx) = mpsc::channel();
    let coordinator = Arc::new(PlaybackCoordinator::new(shell_tx, move |_| {
        Box::new(BlockingStopBackend {
            entered: Mutex::new(Some(entered_tx)),
            release: Mutex::new(release_rx),
        })
    }));
    fixture.services.attach_playback(coordinator);

    let started = Instant::now();
    let result = logout_with_timeouts(
        &fixture.services,
        false,
        Duration::from_millis(150),
        Duration::from_millis(150),
    );
    let elapsed = started.elapsed();

    assert!(
        result.is_ok(),
        "logout should still succeed on stop timeout"
    );
    assert!(!fixture.services.session.is_authenticated());
    assert!(
        entered_rx.try_recv().is_ok(),
        "logout must ask the playback backend to stop before signing out"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "logout must stay bounded when player stop stalls, took {elapsed:?}"
    );

    let _ = release_tx.send(());
    let _ = server.finish();
}

struct ReadyAudioSource {
    request: PlaybackRequest,
    loaded_tx: Mutex<Option<mpsc::Sender<()>>>,
}

impl PlaybackOwner for ReadyAudioSource {
    fn is_current(&self) -> bool {
        true
    }
}

impl AudioSource for ReadyAudioSource {
    fn prepare(&self, _track: &AudioTrack, _start: AudioStart) -> Result<PreparedAudio, String> {
        Ok(PreparedAudio {
            player_path: "mpv".to_string(),
            request: self.request.clone(),
        })
    }

    fn dispatch_if_current(&self, dispatch: &mut dyn FnMut()) -> bool {
        dispatch();
        if let Some(tx) = self.loaded_tx.lock().expect("loaded").take() {
            let _ = tx.send(());
        }
        true
    }
}

#[test]
fn sign_out_flushes_active_audio_playback_stopped_report_before_revoking_token() {
    let _guard = PLAYSTATE_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let server = TokenRevokingServer::start(|_| {});
    let fixture = TestServices::signed_in(&server.url);
    let mut request = playback_request(&server.url, "audiobook-393");
    request.media_url = format!("{}/Audio/audiobook-393/stream.m4b", server.url);
    let reporter = PlaybackReporter::from_launch(&request).expect("reporter");
    let (shell_tx, _shell_rx) = mpsc::channel();
    let coordinator = Arc::new(PlaybackCoordinator::new(shell_tx, move |_| {
        Box::new(ReportingTestBackend::new(reporter, 310_000_000))
    }));
    let track = AudioTrack {
        kind: AudioKind::Audiobook,
        display: AudioDisplay {
            item_id: "audiobook-393".to_string(),
            title: "Chapter 1".to_string(),
            artists: Vec::new(),
            album: None,
            album_id: None,
            artwork: None,
            disc_number: None,
            track_number: Some(1),
            runtime_ticks: Some(900_000_000),
        },
    };
    let plan = AudioPlan::new(vec![track], 0, AudioStart::Default).expect("audio plan");
    let (loaded_tx, loaded_rx) = mpsc::channel();
    coordinator.play_audio(
        plan,
        Arc::new(ReadyAudioSource {
            request,
            loaded_tx: Mutex::new(Some(loaded_tx)),
        }),
    );
    loaded_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("audio load dispatched");
    fixture.services.attach_playback(coordinator);

    let response = send(&fixture, &post("/api/auth/logout", b"{}"));
    assert_eq!(response.status, 200);
    assert!(!fixture.services.session.is_authenticated());
    assert!(flush_playstate_reports(Duration::from_secs(2)));

    let requests = server.finish();
    let stopped_index = requests
        .iter()
        .position(|entry| {
            entry
                .request_line
                .starts_with("POST /Sessions/Playing/Stopped")
        })
        .expect("expected Stopped report");
    let logout_index = requests
        .iter()
        .position(|entry| entry.request_line.starts_with("POST /Sessions/Logout"))
        .expect("expected Logout request");
    assert!(
        stopped_index < logout_index,
        "audio Stopped must arrive before Logout, got {requests:?}"
    );
    assert_eq!(requests[stopped_index].status, 204);
    let body: Value = serde_json::from_str(&requests[stopped_index].body).expect("stopped json");
    assert_eq!(body["ItemId"], "audiobook-393");
    assert_eq!(body["PositionTicks"], 310_000_000);
}
