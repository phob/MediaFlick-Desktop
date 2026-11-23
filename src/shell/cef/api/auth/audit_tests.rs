use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::app::services::test_support::TestServices;
use crate::jellyfin::playback_reporter::{PlaybackReporter, flush_playstate_reports};
use crate::playback::PlaybackCoordinator;
use crate::preferences::FullscreenBehavior;
use crate::shell::cef::api::dispatch;
use crate::shell::cef::api::tests::{
    error_of, post, read_request_with_headers, respond, send, server_url, silent_server,
};

use super::logout_with_timeouts;
use super::tests::{
    AlwaysCurrentOwner, PLAYSTATE_TEST_LOCK, ReportingTestBackend, TokenRevokingServer,
    playback_request, post_with_cancellation, spawn_login_server,
};

#[test]
fn pre_cancelled_quick_connect_poll_never_contacts_jellyfin() {
    let fixture = TestServices::signed_out();
    let silent = silent_server();
    let body = serde_json::to_vec(&json!({ "server": server_url(&silent), "secret": "qc-secret" }))
        .expect("body");
    let request = post_with_cancellation(
        "/api/auth/quickconnect/poll",
        &body,
        Arc::new(AtomicBool::new(true)),
    );
    let response = send(&fixture, &request);
    assert_eq!(response.status, 499, "{}", error_of(&response));
    assert_eq!(
        silent.accept().expect_err("no request").kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert!(!fixture.services.session.is_authenticated());
}

#[test]
fn cancelling_an_in_flight_quick_connect_poll_does_not_complete_sign_in() {
    let fixture = TestServices::signed_out();
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let url = server_url(&listener);
    let (reached_tx, reached_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();

    let server = thread::spawn(move || {
        let (mut state_stream, _) = listener.accept().expect("qc state");
        let _ = read_request_with_headers(&mut state_stream);
        respond(
            &mut state_stream,
            "200 OK",
            r#"{"Authenticated":true,"Code":"123456","Secret":"qc-secret"}"#,
        );
        let (mut auth_stream, _) = listener.accept().expect("qc auth");
        let _ = read_request_with_headers(&mut auth_stream);
        reached_tx.send(()).expect("reached");
        release_rx.recv().expect("release");
        respond(
            &mut auth_stream,
            "200 OK",
            r#"{"AccessToken":"qc-token","ServerId":"server-qc","User":{"Id":"alice","Name":"Alice","Policy":{"IsAdministrator":false}}}"#,
        );
    });

    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let services = fixture.services.clone();
    let body = serde_json::to_vec(&json!({ "server": url, "secret": "qc-secret" })).expect("body");
    let worker = thread::spawn(move || {
        let request =
            post_with_cancellation("/api/auth/quickconnect/poll", &body, worker_cancelled);
        dispatch(&request, || Ok(services.clone()))
    });

    reached_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("poll reached authenticate");
    cancelled.store(true, Ordering::Relaxed);
    release_tx.send(()).expect("release");

    let response = worker.join().expect("worker");
    server.join().expect("server");
    assert_eq!(response.status, 499, "{}", error_of(&response));
    assert!(!fixture.services.session.is_authenticated());
}

#[test]
fn slow_api_logout_after_account_switch_keeps_the_new_session_and_preferences() {
    let logout_listener = TcpListener::bind("127.0.0.1:0").expect("logout listener");
    let logout_url = server_url(&logout_listener);
    let login_listener = TcpListener::bind("127.0.0.1:0").expect("login listener");
    let login_url = server_url(&login_listener);
    let fixture = TestServices::signed_in_as(&logout_url, "alice");
    let (reached_tx, reached_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (played_tx, _played_rx) = mpsc::channel();
    let stop_login = Arc::new(AtomicBool::new(false));
    let login_server = spawn_login_server(login_listener, played_tx, Arc::clone(&stop_login));
    let logout_server = thread::spawn(move || {
        let (mut stream, _) = logout_listener.accept().expect("logout");
        let _ = read_request_with_headers(&mut stream);
        reached_tx.send(()).expect("reached");
        release_rx.recv().expect("release");
        respond(&mut stream, "204 No Content", "");
    });

    let services = fixture.services.clone();
    let logout_worker =
        thread::spawn(move || dispatch(&post("/api/auth/logout", b"{}"), || Ok(services.clone())));
    reached_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("logout reached server");
    let login_body = serde_json::to_vec(
        &json!({ "server": login_url, "username": "alice", "password": "secret" }),
    )
    .expect("login body");
    assert_eq!(
        send(&fixture, &post("/api/auth/login", &login_body)).status,
        200
    );

    release_tx.send(()).expect("release");
    let logout_response = logout_worker.join().expect("logout worker");
    stop_login.store(true, Ordering::Relaxed);
    logout_server.join().expect("logout server");
    login_server.join().expect("login server");

    assert_eq!(logout_response.status, 409);
    let body: Value = serde_json::from_slice(&logout_response.body).expect("json");
    assert_eq!(body["accountChanged"], true);
    assert!(fixture.services.session.is_authenticated());
    assert_eq!(
        fixture.services.session.server_url().as_deref(),
        Some(login_url.as_str())
    );
}

#[test]
fn logout_stalled_in_playstate_flush_does_not_clear_a_newer_session() {
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
    let login_listener = TcpListener::bind("127.0.0.1:0").expect("login listener");
    let login_url = server_url(&login_listener);
    let (played_tx, _played_rx) = mpsc::channel();
    let stop_login = Arc::new(AtomicBool::new(false));
    let login_server = spawn_login_server(login_listener, played_tx, Arc::clone(&stop_login));

    let fixture = TestServices::signed_in(&server.url);
    let request = playback_request(&server.url, "stalled-408");
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

    let services = fixture.services.clone();
    let logout_worker = thread::spawn(move || {
        logout_with_timeouts(
            &services,
            false,
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
    });

    stalled_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("Stopped report reached server while logout waits in flush");

    let login_body = serde_json::to_vec(
        &json!({ "server": login_url, "username": "alice", "password": "secret" }),
    )
    .expect("login body");
    assert_eq!(
        send(&fixture, &post("/api/auth/login", &login_body)).status,
        200
    );

    let _ = release_tx.send(());
    let logout_result = logout_worker.join().expect("logout worker");
    assert!(flush_playstate_reports(Duration::from_secs(2)));
    stop_login.store(true, Ordering::Relaxed);
    login_server.join().expect("login server");
    let _ = server.finish();

    let Err(response) = logout_result else {
        panic!("stale logout must not clear newer session");
    };
    assert_eq!(response.status, 409);
    let body: Value = serde_json::from_slice(&response.body).expect("json");
    assert_eq!(body["accountChanged"], true);
    assert!(fixture.services.session.is_authenticated());
    assert_eq!(
        fixture.services.session.server_url().as_deref(),
        Some(login_url.as_str())
    );
}
