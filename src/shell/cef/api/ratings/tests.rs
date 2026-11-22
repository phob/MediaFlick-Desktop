use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::app::services::test_support::TestServices;
use crate::library::test_support::dto;
use crate::library::{CachedRatings, RatingTarget, now_unix};
use crate::shell::cef::api::tests::{post, respond, send, server_url};
use crate::shell::cef::api::{ApiRequest, ApiResponse};

const COMPANION_INFO: &str =
    r#"{"pluginVersion":"0.2.2","apiVersion":1,"capabilities":["ratings-v1"]}"#;

fn post_with_cancellation(path: &str, body: &[u8], cancelled: Arc<AtomicBool>) -> ApiRequest {
    ApiRequest {
        cancelled,
        ..post(path, body)
    }
}

fn batch_request(ids: &[&str], cancelled: Arc<AtomicBool>) -> ApiRequest {
    let body = serde_json::to_vec(&json!({ "ids": ids })).expect("batch body");
    post_with_cancellation("/api/ratings/batch", &body, cancelled)
}

fn json_body(response: &ApiResponse) -> Value {
    serde_json::from_slice(&response.body).expect("json body")
}

fn read_http_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).expect("request line");
    let mut length = 0usize;
    let mut header = String::new();
    while reader.read_line(&mut header).expect("header") > 2 {
        if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().expect("content length");
        }
        header.clear();
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).expect("body");
    (request_line, body)
}

enum BatchAction {
    Immediate(&'static str, String),
    Gated {
        entered: mpsc::Sender<Value>,
        release: mpsc::Receiver<(&'static str, String)>,
    },
}

struct ScriptedCompanion {
    url: String,
    batches: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for ScriptedCompanion {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn spawn_scripted_companion(
    actions: Vec<BatchAction>,
    login_identity: Option<(&'static str, &'static str, &'static str)>,
) -> ScriptedCompanion {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let url = server_url(&listener);
    listener.set_nonblocking(true).expect("nonblocking");
    let batches = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&batches);
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    let queue = Arc::new(Mutex::new(VecDeque::from(actions)));

    thread::spawn(move || {
        while !server_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let recorded = Arc::clone(&recorded);
                    let queue = Arc::clone(&queue);
                    thread::spawn(move || {
                        stream.set_nonblocking(false).expect("blocking");
                        let (line, body) = read_http_request(&mut stream);
                        if line.starts_with("GET /MediaFlick/info") {
                            respond(&mut stream, "200 OK", COMPANION_INFO);
                        } else if line.starts_with("GET /System/Info/Public") {
                            respond(
                                &mut stream,
                                "200 OK",
                                r#"{"ServerName":"Server B","Version":"12.4.0"}"#,
                            );
                        } else if line.starts_with("POST /Users/AuthenticateByName") {
                            let (token, server_id, user_id) =
                                login_identity.unwrap_or(("b-token", "server-b", "bob"));
                            let answer = json!({
                                "AccessToken": token,
                                "ServerId": server_id,
                                "User": {
                                    "Id": user_id,
                                    "Name": "Bob",
                                    "Policy": { "IsAdministrator": false }
                                }
                            })
                            .to_string();
                            respond(&mut stream, "200 OK", &answer);
                        } else if line.starts_with("POST /MediaFlick/ratings/v1/batch") {
                            let parsed: Value = serde_json::from_slice(&body).expect("batch json");
                            recorded.lock().expect("batches").push(parsed.clone());
                            let action = queue.lock().expect("queue").pop_front();
                            let (status, answer) = match action {
                                Some(BatchAction::Immediate(status, answer)) => (status, answer),
                                Some(BatchAction::Gated { entered, release }) => {
                                    let _ = entered.send(parsed);
                                    release
                                        .recv_timeout(Duration::from_secs(5))
                                        .unwrap_or(("500 Internal Server Error", "{}".to_string()))
                                }
                                None => ("500 Internal Server Error", "{}".to_string()),
                            };
                            write!(
                                stream,
                                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                                answer.len()
                            )
                            .expect("write response");
                        } else {
                            respond(&mut stream, "404 Not Found", "{}");
                        }
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });

    ScriptedCompanion { url, batches, stop }
}

fn rating_payload(entries: &[(&str, f64)]) -> String {
    json!({
        "boundaryVersion": 1,
        "items": entries
            .iter()
            .map(|(id, score)| json!({
                "itemId": id,
                "ratings": [{ "sourceId": "imdb", "value": score }]
            }))
            .collect::<Vec<_>>()
    })
    .to_string()
}

fn requested_ids(batch: &Value) -> Vec<String> {
    batch["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|item| item["itemId"].as_str().expect("itemId").to_string())
        .collect()
}

fn response_scores(response: &ApiResponse) -> Vec<(String, f64)> {
    let body = json_body(response);
    let mut scores = body["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|item| {
            (
                item["id"].as_str().expect("id").to_string(),
                item["ratings"][0]["value"].as_f64().expect("value"),
            )
        })
        .collect::<Vec<_>>();
    scores.sort_by(|left, right| left.0.cmp(&right.0));
    scores
}

#[test]
fn aborted_first_batch_followed_by_second_request_returns_item_without_duplicate_fetch() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = spawn_scripted_companion(
        vec![BatchAction::Gated {
            entered: entered_tx,
            release: release_rx,
        }],
        None,
    );
    let fixture = Arc::new(TestServices::signed_in(&server.url));

    let first_cancelled = Arc::new(AtomicBool::new(false));
    let first_fixture = Arc::clone(&fixture);
    let first_flag = Arc::clone(&first_cancelled);
    let first = thread::spawn(move || {
        send(
            &first_fixture,
            &batch_request(&["tmdb:movie:603"], first_flag),
        )
    });

    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("first batch reached companion");
    first_cancelled.store(true, Ordering::Relaxed);

    let second_fixture = Arc::clone(&fixture);
    let (second_tx, second_rx) = mpsc::channel();
    let second = thread::spawn(move || {
        let response = send(
            &second_fixture,
            &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(false))),
        );
        let _ = second_tx.send(response);
    });

    assert!(
        second_rx.recv_timeout(Duration::from_millis(80)).is_err(),
        "second request must wait for the in-flight refresh instead of omitting the item"
    );

    release_tx
        .send(("200 OK", rating_payload(&[("tmdb:movie:603", 8.7)])))
        .expect("release companion");

    let first_response = first.join().expect("first join");
    let second_response = second_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("second response");
    second.join().expect("second join");

    assert_eq!(first_response.status, 499);
    assert_eq!(second_response.status, 200);
    assert_eq!(
        response_scores(&second_response),
        vec![("tmdb:movie:603".to_string(), 8.7)]
    );
    assert_eq!(server.batches.lock().expect("batches").len(), 1);
}

#[test]
fn overlapping_batch_with_cached_inflight_and_new_ids_returns_all_three_without_duplicate_fetch() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = spawn_scripted_companion(
        vec![
            BatchAction::Gated {
                entered: entered_tx,
                release: release_rx,
            },
            BatchAction::Immediate("200 OK", rating_payload(&[("tmdb:movie:604", 7.4)])),
        ],
        None,
    );
    let fixture = Arc::new(TestServices::signed_in(&server.url));

    let now = now_unix();
    fixture
        .services
        .library
        .save_rating_cache(&[(
            RatingTarget {
                item_id: "tmdb:movie:605".to_string(),
                kind: "Movie".to_string(),
                media_type: "movie".to_string(),
                provider: "tmdb".to_string(),
                provider_id: "605".to_string(),
            },
            CachedRatings {
                ratings: json!([{ "sourceId": "imdb", "value": 5.0, "score": null, "votes": null, "scaleMax": 10.0 }]),
                source_updated_at: None,
                fetched_at: now,
                stale_at: now + 3600,
                expires_at: now + 86_400,
                schema_version: 1,
            },
        )])
        .expect("seed cached item");

    let first_fixture = Arc::clone(&fixture);
    let first = thread::spawn(move || {
        send(
            &first_fixture,
            &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(false))),
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("first batch reached companion");

    let second_fixture = Arc::clone(&fixture);
    let second = thread::spawn(move || {
        send(
            &second_fixture,
            &batch_request(
                &["tmdb:movie:603", "tmdb:movie:604", "tmdb:movie:605"],
                Arc::new(AtomicBool::new(false)),
            ),
        )
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while server.batches.lock().expect("batches").len() < 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "second batch should fetch new item 604 while 603 is in flight"
        );
        thread::sleep(Duration::from_millis(10));
    }

    release_tx
        .send(("200 OK", rating_payload(&[("tmdb:movie:603", 8.7)])))
        .expect("release first batch");

    let first_response = first.join().expect("first join");
    let second_response = second.join().expect("second join");
    assert_eq!(first_response.status, 200);
    assert_eq!(second_response.status, 200);
    assert_eq!(
        response_scores(&second_response),
        vec![
            ("tmdb:movie:603".to_string(), 8.7),
            ("tmdb:movie:604".to_string(), 7.4),
            ("tmdb:movie:605".to_string(), 5.0),
        ]
    );
    let batches = server.batches.lock().expect("batches");
    assert_eq!(batches.len(), 2);
    assert_eq!(requested_ids(&batches[0]), vec!["tmdb:movie:603"]);
    assert_eq!(requested_ids(&batches[1]), vec!["tmdb:movie:604"]);
}

#[test]
fn failing_leader_fetch_notifies_waiter_and_releases_inflight_entry() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = spawn_scripted_companion(
        vec![
            BatchAction::Gated {
                entered: entered_tx,
                release: release_rx,
            },
            BatchAction::Immediate("200 OK", rating_payload(&[("tmdb:movie:603", 8.7)])),
        ],
        None,
    );
    let fixture = Arc::new(TestServices::signed_in(&server.url));

    let first_fixture = Arc::clone(&fixture);
    let first = thread::spawn(move || {
        send(
            &first_fixture,
            &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(false))),
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("leader entered");

    let second_fixture = Arc::clone(&fixture);
    let second = thread::spawn(move || {
        send(
            &second_fixture,
            &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(false))),
        )
    });
    thread::sleep(Duration::from_millis(50));

    release_tx
        .send(("503 Service Unavailable", "{}".to_string()))
        .expect("release failure");

    let first_response = first.join().expect("first join");
    let second_response = second.join().expect("second join");
    assert_eq!(first_response.status, 200);
    assert_eq!(second_response.status, 200);
    assert!(json_body(&first_response)["diagnostic"].is_string());
    assert!(json_body(&second_response)["diagnostic"].is_string());

    let third_response = send(
        &fixture,
        &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(false))),
    );
    assert_eq!(third_response.status, 200);
    assert_eq!(
        response_scores(&third_response),
        vec![("tmdb:movie:603".to_string(), 8.7)]
    );
    assert_eq!(server.batches.lock().expect("batches").len(), 2);
}

#[test]
fn cancelled_waiter_exits_promptly_without_cancelling_leader_or_live_waiter() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = spawn_scripted_companion(
        vec![BatchAction::Gated {
            entered: entered_tx,
            release: release_rx,
        }],
        None,
    );
    let fixture = Arc::new(TestServices::signed_in(&server.url));

    let leader_fixture = Arc::clone(&fixture);
    let leader = thread::spawn(move || {
        send(
            &leader_fixture,
            &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(false))),
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("leader entered");

    let cancelled_flag = Arc::new(AtomicBool::new(false));
    let aborted_fixture = Arc::clone(&fixture);
    let aborted_flag = Arc::clone(&cancelled_flag);
    let (aborted_tx, aborted_rx) = mpsc::channel();
    let aborted_waiter = thread::spawn(move || {
        let response = send(
            &aborted_fixture,
            &batch_request(&["tmdb:movie:603"], aborted_flag),
        );
        let _ = aborted_tx.send(response);
    });

    let live_fixture = Arc::clone(&fixture);
    let live_waiter = thread::spawn(move || {
        send(
            &live_fixture,
            &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(false))),
        )
    });

    thread::sleep(Duration::from_millis(50));
    cancelled_flag.store(true, Ordering::Relaxed);

    let aborted_response = aborted_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("cancelled waiter exits promptly");
    aborted_waiter.join().expect("aborted waiter join");
    assert_eq!(aborted_response.status, 499);

    release_tx
        .send(("200 OK", rating_payload(&[("tmdb:movie:603", 8.7)])))
        .expect("release leader");

    let leader_response = leader.join().expect("leader join");
    let live_response = live_waiter.join().expect("live waiter join");
    assert_eq!(leader_response.status, 200);
    assert_eq!(live_response.status, 200);
    assert_eq!(
        response_scores(&live_response),
        vec![("tmdb:movie:603".to_string(), 8.7)]
    );
    assert_eq!(server.batches.lock().expect("batches").len(), 1);
}

#[test]
fn pre_cancelled_batch_request_returns_499_without_calling_companion() {
    let server = spawn_scripted_companion(
        vec![BatchAction::Immediate(
            "200 OK",
            rating_payload(&[("tmdb:movie:603", 8.7)]),
        )],
        None,
    );
    let fixture = TestServices::signed_in(&server.url);
    let response = send(
        &fixture,
        &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(true))),
    );
    assert_eq!(response.status, 499);
    assert!(server.batches.lock().expect("batches").is_empty());
}

#[test]
fn account_switch_isolates_inflight_batches_and_unblocks_old_waiter_with_409() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server_a = spawn_scripted_companion(
        vec![BatchAction::Gated {
            entered: entered_tx,
            release: release_rx,
        }],
        None,
    );
    let server_b = spawn_scripted_companion(
        vec![BatchAction::Immediate(
            "200 OK",
            rating_payload(&[("tmdb:movie:603", 9.3)]),
        )],
        Some(("token-b", "server-b", "bob")),
    );
    let fixture = Arc::new(TestServices::signed_in_as(&server_a.url, "alice"));

    let leader_fixture = Arc::clone(&fixture);
    let leader_a = thread::spawn(move || {
        send(
            &leader_fixture,
            &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(false))),
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("server A entered");

    let waiter_fixture = Arc::clone(&fixture);
    let (waiter_tx, waiter_rx) = mpsc::channel();
    let waiter_a = thread::spawn(move || {
        let response = send(
            &waiter_fixture,
            &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(false))),
        );
        let _ = waiter_tx.send(response);
    });
    thread::sleep(Duration::from_millis(50));

    let login_body = serde_json::to_vec(&json!({
        "server": server_b.url,
        "username": "bob",
        "password": "secret"
    }))
    .expect("login body");
    let login = send(&fixture, &post("/api/auth/login", &login_body));
    assert_eq!(login.status, 200);

    let waiter_a_response = waiter_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("account A waiter exits promptly on account switch");
    waiter_a.join().expect("waiter A join");
    assert_eq!(waiter_a_response.status, 409);
    assert_eq!(json_body(&waiter_a_response)["accountChanged"], true);

    let account_b_response = send(
        &fixture,
        &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(false))),
    );
    assert_eq!(account_b_response.status, 200);
    assert_eq!(
        response_scores(&account_b_response),
        vec![("tmdb:movie:603".to_string(), 9.3)]
    );

    release_tx
        .send(("200 OK", rating_payload(&[("tmdb:movie:603", 4.1)])))
        .expect("release server A");
    let leader_a_response = leader_a.join().expect("leader A join");
    assert_eq!(leader_a_response.status, 409);
    assert_eq!(json_body(&leader_a_response)["accountChanged"], true);

    let cached = send(
        &fixture,
        &batch_request(&["tmdb:movie:603"], Arc::new(AtomicBool::new(false))),
    );
    assert_eq!(cached.status, 200);
    assert_eq!(
        response_scores(&cached),
        vec![("tmdb:movie:603".to_string(), 9.3)]
    );
}

#[test]
fn shared_provider_target_within_one_batch_does_not_deadlock_and_returns_both_ids() {
    let server = spawn_scripted_companion(
        vec![BatchAction::Immediate(
            "200 OK",
            rating_payload(&[("local-movie", 8.7), ("tmdb:movie:603", 8.7)]),
        )],
        None,
    );
    let fixture = TestServices::signed_in(&server.url);
    fixture
        .services
        .library
        .upsert_page(
            &crate::jellyfin::session::Live::for_test(),
            &[dto(
                r#"{"Id":"local-movie","Name":"The Matrix","Type":"Movie","ProviderIds":{"Tmdb":"603"}}"#,
            )],
        )
        .expect("seed library");

    let response = send(
        &fixture,
        &batch_request(
            &["local-movie", "tmdb:movie:603"],
            Arc::new(AtomicBool::new(false)),
        ),
    );
    assert_eq!(response.status, 200);
    assert_eq!(
        response_scores(&response),
        vec![
            ("local-movie".to_string(), 8.7),
            ("tmdb:movie:603".to_string(), 8.7),
        ]
    );
    let batches = server.batches.lock().expect("batches");
    assert_eq!(batches.len(), 1);
    assert_eq!(requested_ids(&batches[0]).len(), 1);
}
