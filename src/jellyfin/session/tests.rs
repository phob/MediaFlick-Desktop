use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use super::{Live, Session, UserAccess};
use crate::app::services::{ShellBridge, ShellRequest};
use crate::jellyfin::api::ApiError;
use crate::jellyfin::api::auth::Credentials;
use crate::library::Library;
use crate::library::test_support::dto;

fn session() -> Session {
    Session::restore(
        Arc::new(Library::open_in_memory().expect("library")),
        Arc::default(),
    )
}

fn credentials(user: &str) -> Credentials {
    Credentials {
        token: format!("token-{user}"),
        user_id: user.to_string(),
        user_name: user.to_string(),
        server_id: "server".to_string(),
        access: UserAccess {
            restricted: false,
            administrator: false,
        },
    }
}

#[test]
fn restoring_reads_persisted_credentials() {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    let mut credentials = library.credentials();
    credentials.server_url = Some("http://server:8096".to_string());
    credentials.user_id = Some("uid".to_string());
    credentials.server_id = Some("server".to_string());
    credentials.token = Some("tok".to_string());
    library.save_credentials(&credentials).expect("save");

    let session = Session::restore(library, Arc::default());
    assert!(session.is_authenticated());
    let scope = session.scope().expect("scope");
    assert_eq!(scope.client().base_url(), "http://server:8096");
    assert_eq!(scope.client().token(), Some("tok"));
    assert_eq!(scope.user_id(), "uid");
}

#[test]
fn an_expired_token_blocks_client_creation_until_re_login() {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    let mut credentials = library.credentials();
    credentials.server_url = Some("http://server:8096".to_string());
    credentials.user_id = Some("uid".to_string());
    credentials.server_id = Some("server".to_string());
    credentials.token = Some("tok".to_string());
    library.save_credentials(&credentials).expect("save");

    let session = Session::restore(library, Arc::default());
    let scope = session.scope().expect("scope");
    session.note_scoped_error(&scope, &ApiError::Unauthorized);
    assert!(!session.is_authenticated());
    assert!(matches!(session.scope(), Err(ApiError::Unauthorized)));
    assert!(session.status().expired);
}

#[test]
fn other_errors_do_not_expire_the_session() {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    let mut credentials = library.credentials();
    credentials.server_url = Some("http://server:8096".to_string());
    credentials.user_id = Some("uid".to_string());
    credentials.server_id = Some("server".to_string());
    credentials.token = Some("tok".to_string());
    library.save_credentials(&credentials).expect("save");

    let session = Session::restore(library, Arc::default());
    let scope = session.scope().expect("scope");
    session.note_scoped_error(&scope, &ApiError::Status { status: 500 });
    session.note_scoped_error(&scope, &ApiError::Transport("reset".to_string()));
    assert!(session.is_authenticated());
}

#[test]
fn connecting_to_an_unusable_url_is_rejected_before_any_request() {
    let session = session();
    assert!(matches!(
        session.connect("file:///etc/passwd", None),
        Err(ApiError::NotConfigured)
    ));
    assert!(matches!(
        session.login("javascript:alert(1)", "u", "p"),
        Err(ApiError::NotConfigured)
    ));
}

#[test]
fn old_successes_and_unauthorized_errors_are_discarded_after_account_switch() {
    let session = session();
    session
        .accept("http://server:8096", credentials("alice"), None)
        .expect("sign in Alice");
    let alice = session.scope().expect("Alice scope");
    session
        .accept("http://server:8096", credentials("bob"), None)
        .expect("sign in Bob");

    let result = session.commit_if_current(
        &alice,
        || "stale",
        |_live| -> Result<(), &str> { panic!("an obsolete success committed") },
    );
    assert_eq!(result, Err("stale"));
    session.note_scoped_error(&alice, &ApiError::Unauthorized);
    assert!(session.is_authenticated());
    assert_eq!(session.status().user_id.as_deref(), Some("bob"));
    assert!(!session.status().expired);
}

#[test]
fn a_restricted_user_gets_an_empty_cache_after_a_normal_logout_and_switch() {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    let session = Session::restore(library.clone(), Arc::default());
    session
        .accept("http://server:8096", credentials("alice"), None)
        .expect("sign in Alice");
    library
        .upsert_page(
            &Live::for_test(),
            &[dto(
                r#"{"Id":"alice-movie","Name":"Alice Only","Type":"Movie"}"#,
            )],
        )
        .expect("cache Alice's library");

    session.clear_local(false).expect("normal logout");
    assert_eq!(library.stats().total, 1);

    let mut bob = credentials("bob");
    bob.access.restricted = true;
    session
        .accept("http://server:8096", bob, None)
        .expect("sign in restricted Bob");

    assert_eq!(library.stats().total, 0);
    assert!(session.user_restricted());
}

#[test]
fn deletion_tombstone_blocks_new_and_in_flight_account_work() {
    let session = session();
    session
        .accept("http://server:8096", credentials("alice"), None)
        .expect("sign in Alice");
    let scope = session.scope().expect("Alice scope");
    assert!(session.begin_account_deletion(scope.account()));

    assert!(!session.is_authenticated());
    assert!(session.scope().is_err());
    assert_eq!(
        session.commit_if_current(&scope, || "stale", |_live| Ok::<_, &str>(())),
        Err("stale")
    );
}

struct AuthCase<'a> {
    label: &'a str,
    server_url: Option<&'a str>,
    server_id: Option<&'a str>,
    user_id: Option<&'a str>,
    token: Option<&'a str>,
    expected: bool,
}

impl<'a> AuthCase<'a> {
    const fn new(
        label: &'a str,
        server_url: Option<&'a str>,
        server_id: Option<&'a str>,
        user_id: Option<&'a str>,
        token: Option<&'a str>,
        expected: bool,
    ) -> Self {
        Self {
            label,
            server_url,
            server_id,
            user_id,
            token,
            expected,
        }
    }
}

const AUTH_URL: Option<&str> = Some("http://server:8096");

const AUTH_CASES: &[AuthCase<'_>] = &[
    AuthCase::new(
        "missing server_id",
        AUTH_URL,
        None,
        Some("uid"),
        Some("tok"),
        false,
    ),
    AuthCase::new(
        "blank server_id",
        AUTH_URL,
        Some("   "),
        Some("uid"),
        Some("tok"),
        false,
    ),
    AuthCase::new(
        "missing server_url",
        None,
        Some("server"),
        Some("uid"),
        Some("tok"),
        false,
    ),
    AuthCase::new(
        "blank server_url",
        Some("   "),
        Some("server"),
        Some("uid"),
        Some("tok"),
        false,
    ),
    AuthCase::new(
        "missing user_id",
        AUTH_URL,
        Some("server"),
        None,
        Some("tok"),
        false,
    ),
    AuthCase::new(
        "blank user_id",
        AUTH_URL,
        Some("server"),
        Some(" "),
        Some("tok"),
        false,
    ),
    AuthCase::new(
        "missing token",
        AUTH_URL,
        Some("server"),
        Some("uid"),
        None,
        false,
    ),
    AuthCase::new(
        "blank token",
        AUTH_URL,
        Some("server"),
        Some("uid"),
        Some(" "),
        false,
    ),
    AuthCase::new(
        "complete",
        AUTH_URL,
        Some("server"),
        Some("uid"),
        Some("tok"),
        true,
    ),
];

#[test]
fn status_is_authenticated_and_scope_always_agree() {
    for case in AUTH_CASES {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let mut stored = library.credentials();
        stored.server_url = case.server_url.map(str::to_string);
        stored.server_id = case.server_id.map(str::to_string);
        stored.user_id = case.user_id.map(str::to_string);
        stored.token = case.token.map(str::to_string);
        library.save_credentials(&stored).expect("save");

        let session = Session::restore(library, Arc::default());
        assert_eq!(
            session.status().authenticated,
            case.expected,
            "{}: status",
            case.label
        );
        assert_eq!(
            session.is_authenticated(),
            case.expected,
            "{}: is_authenticated",
            case.label
        );
        assert_eq!(
            session.scope().is_ok(),
            case.expected,
            "{}: scope",
            case.label
        );
    }
}

fn spawn_holding_logout_server() -> (
    String,
    mpsc::Receiver<()>,
    mpsc::Sender<()>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let (reached_tx, reached_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("logout");
        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|n| n > 0) && line != "\r\n" {
            line.clear();
        }
        drop(reader);
        reached_tx.send(()).expect("reached");
        release_rx.recv().expect("release");
        let _ = stream.write_all(
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    });
    (url, reached_rx, release_tx, server)
}

#[test]
fn slow_logout_does_not_clear_a_newer_session() {
    let (url, reached_rx, release_tx, server) = spawn_holding_logout_server();
    let session = Arc::new(session());
    session
        .accept(&url, credentials("alice"), None)
        .expect("sign in Alice");
    let worker_session = Arc::clone(&session);
    let worker = thread::spawn(move || {
        let target = worker_session.capture_logout();
        worker_session.logout(&target, false)
    });

    reached_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("logout reached server");
    session
        .accept(&url, credentials("bob"), None)
        .expect("sign in Bob");
    release_tx.send(()).expect("release");

    assert_eq!(
        worker.join().expect("worker"),
        Err(ApiError::AccountChanged)
    );
    server.join().expect("server");
    assert!(session.is_authenticated());
    assert_eq!(session.status().user_id.as_deref(), Some("bob"));
}

#[test]
fn logout_clears_session_when_same_account_expires_during_server_logout() {
    let (url, reached_rx, release_tx, server) = spawn_holding_logout_server();
    let session = Arc::new(session());
    session
        .accept(&url, credentials("alice"), None)
        .expect("sign in Alice");
    let alice = session.scope().expect("Alice scope");
    let worker_session = Arc::clone(&session);
    let worker = thread::spawn(move || {
        let target = worker_session.capture_logout();
        worker_session.logout(&target, false)
    });

    reached_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("logout reached server");
    session.note_scoped_error(&alice, &ApiError::Unauthorized);
    release_tx.send(()).expect("release");

    assert_eq!(worker.join().expect("worker"), Ok(()));
    server.join().expect("server");
    assert!(!session.is_authenticated());
    assert!(!session.status().expired);
    assert_eq!(session.status().user_id, None);
}

fn spawn_policy_server(
    allow_policy: Arc<AtomicBool>,
    policy_calls: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    listener.set_nonblocking(true).expect("nonblocking");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let server = thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(5));
                continue;
            };
            let _ = stream.set_nonblocking(false);
            let mut reader = BufReader::new(&mut stream);
            let mut request_line = String::new();
            let _ = reader.read_line(&mut request_line);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            drop(reader);
            let (status, body) = if request_line.starts_with("GET /Users/uid") {
                policy_calls.fetch_add(1, Ordering::Relaxed);
                if allow_policy.load(Ordering::Relaxed) {
                    (
                        "200 OK",
                        r#"{"Id":"uid","Name":"Alice","Policy":{"IsAdministrator":true,"EnableAllFolders":true}}"#,
                    )
                } else {
                    ("404 Not Found", "")
                }
            } else {
                ("200 OK", "{}")
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (url, server)
}

fn restored_session_at(url: String, shell: Arc<ShellBridge>) -> Session {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    let mut stored = library.credentials();
    stored.server_url = Some(url);
    stored.server_id = Some("server".to_string());
    stored.user_id = Some("uid".to_string());
    stored.token = Some("tok".to_string());
    library.save_credentials(&stored).expect("save");
    Session::restore(library, shell)
}

#[test]
fn failed_startup_policy_check_retries_after_first_successful_scoped_call() {
    let allow_policy = Arc::new(AtomicBool::new(false));
    let policy_calls = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (url, server) = spawn_policy_server(
        Arc::clone(&allow_policy),
        Arc::clone(&policy_calls),
        Arc::clone(&stop),
    );
    let shell = Arc::new(ShellBridge::default());
    let shell_rx = shell.subscribe();
    let session = restored_session_at(url, shell);

    session.refresh_user_policy();
    assert!(session.user_restricted());
    assert!(!session.status().administrator);
    assert_eq!(policy_calls.load(Ordering::Relaxed), 1);

    allow_policy.store(true, Ordering::Relaxed);
    let scope = session.scope().expect("scope");
    let _ = session
        .fetch_current(&scope, |client, _| {
            client.get_json::<serde_json::Value>("/Items", &[])
        })
        .expect("scoped call");

    let deadline = Instant::now() + Duration::from_secs(3);
    while session.user_restricted() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!session.user_restricted());
    assert!(session.status().administrator);
    assert_eq!(policy_calls.load(Ordering::Relaxed), 2);
    assert!(matches!(
        shell_rx.recv_timeout(Duration::from_secs(1)),
        Ok(ShellRequest::CollectionsChanged)
    ));

    let _ = session
        .fetch_current(&scope, |client, _| {
            client.get_json::<serde_json::Value>("/Items", &[])
        })
        .expect("second scoped call");
    thread::sleep(Duration::from_millis(50));
    assert_eq!(policy_calls.load(Ordering::Relaxed), 2);

    stop.store(true, Ordering::Relaxed);
    server.join().expect("server");
}

#[test]
fn repeated_policy_retry_failure_is_rate_limited() {
    let allow_policy = Arc::new(AtomicBool::new(false));
    let policy_calls = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (url, server) = spawn_policy_server(
        Arc::clone(&allow_policy),
        Arc::clone(&policy_calls),
        Arc::clone(&stop),
    );
    let session = restored_session_at(url, Arc::default());

    session.refresh_user_policy();
    assert_eq!(policy_calls.load(Ordering::Relaxed), 1);

    let scope = session.scope().expect("scope");
    let _ = session
        .fetch_current(&scope, |client, _| {
            client.get_json::<serde_json::Value>("/Items", &[])
        })
        .expect("first scoped call");

    let deadline = Instant::now() + Duration::from_secs(3);
    while policy_calls.load(Ordering::Relaxed) < 2 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(policy_calls.load(Ordering::Relaxed), 2);

    let _ = session
        .fetch_current(&scope, |client, _| {
            client.get_json::<serde_json::Value>("/Items", &[])
        })
        .expect("second scoped call during cooldown");
    thread::sleep(Duration::from_millis(50));
    assert_eq!(policy_calls.load(Ordering::Relaxed), 2);

    stop.store(true, Ordering::Relaxed);
    server.join().expect("server");
}
