use super::IpcCommandFailure;
use super::test_server::{COMMAND_CONNECTION, FakeMpv};
use serde_json::json;
use std::time::Duration;

#[test]
fn one_persistent_writer_carries_every_command_and_matches_replies_by_id() {
    let fake = FakeMpv::start(|command| {
        let id = command["request_id"].clone();
        match command["command"][0].as_str() {
            Some("first") => vec![
                json!({ "event": "property-change", "name": "pause", "data": true }),
                json!({ "request_id": 999_999, "error": "success" }),
                json!({ "request_id": id, "error": "success" }),
            ],
            Some("rejected") => {
                vec![json!({ "request_id": id, "error": "property unavailable" })]
            }
            Some("late") => Vec::new(),
            Some("after-async") => vec![
                json!({ "request_id": 4, "error": "success" }),
                json!({ "request_id": id, "error": "success" }),
            ],
            _ => vec![json!({ "request_id": id, "error": "success" })],
        }
    });
    let (worker, events) = fake.connect();
    let send = |name: &str, id: i64| {
        worker.send_with_timeout(
            json!({ "command": [name], "request_id": id }),
            Duration::from_secs(5),
        )
    };

    assert!(send("first", 1).is_ok());
    assert!(matches!(
        send("rejected", 2),
        Err(IpcCommandFailure::Rejected(_))
    ));
    assert!(
        send("after-rejection", 3).is_ok(),
        "a rejection keeps the writer"
    );
    assert!(worker.is_writer_alive());
    for expected in ["first", "rejected", "after-rejection"] {
        assert_eq!(
            fake.next_command_on(COMMAND_CONNECTION)["command"][0],
            expected
        );
    }

    let asynchronous = worker.send_with_timeout(
        json!({ "command": ["late"], "request_id": 4, "async": true }),
        Duration::from_secs(5),
    );
    assert!(asynchronous.is_ok());
    assert!(send("after-async", 5).is_ok());
    for expected in ["late", "after-async"] {
        assert_eq!(
            fake.next_command_on(COMMAND_CONNECTION)["command"][0],
            expected
        );
    }

    fake.send_event(&json!({ "event": "file-loaded" }));
    let event = events
        .recv_timeout(Duration::from_secs(5))
        .expect("event through the reader connection");
    assert_eq!(event.name, "file-loaded");
    fake.finish(worker);
}

#[test]
fn a_command_reply_slower_than_the_event_poll_is_accepted_within_its_deadline() {
    let fake = FakeMpv::start(|command| {
        std::thread::sleep(Duration::from_millis(2_500));
        vec![json!({ "request_id": command["request_id"].clone(), "error": "success" })]
    });
    let (worker, _events) = fake.connect();

    let reply = worker.send_with_timeout(
        json!({ "command": ["sub-add", "https://example.test/sub.srt", "select"], "request_id": 1 }),
        Duration::from_secs(10),
    );

    assert!(
        reply.is_ok(),
        "{:?}",
        reply.err().map(|error| error.to_string())
    );
    assert!(worker.is_writer_alive());
    fake.finish(worker);
}

#[test]
fn disconnect_returns_promptly_while_server_keeps_streams_open() {
    let fake = FakeMpv::start(|command| {
        std::thread::sleep(Duration::from_millis(200));
        vec![json!({ "request_id": command["request_id"].clone(), "error": "success" })]
    });
    let (worker, _events) = fake.connect();
    let timed_out = worker.send_with_timeout(
        json!({ "command": ["get_property", "chapter-list"], "request_id": 1 }),
        Duration::from_millis(20),
    );
    assert!(matches!(timed_out, Err(IpcCommandFailure::Transport(_))));
    assert!(!worker.is_writer_alive());

    let started = std::time::Instant::now();
    worker.disconnect();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "disconnect blocked for {:?}",
        started.elapsed()
    );
    fake.stop();
}

#[cfg(unix)]
#[test]
fn unix_ipc_path_stays_under_sun_len_for_long_temp_dirs_and_preserves_short_dirs() {
    let short_dir = std::path::PathBuf::from("/tmp/mf-short");
    let short_path = super::build_unix_ipc_path(&short_dir, 42, 123_456, 7);
    assert!(
        short_path.starts_with("/tmp/mf-short/"),
        "expected short path to stay in preferred_dir, got {short_path}"
    );
    assert!(
        short_path.len() < 96,
        "short path too long: {}",
        short_path.len()
    );

    let long_segment = "a".repeat(85);
    let long_dir = std::path::PathBuf::from(format!("/tmp/{long_segment}"));
    let bounded = super::build_unix_ipc_path(&long_dir, 999_999, u128::MAX / 2, 12345);
    assert!(
        bounded.len() < 96,
        "IPC socket path {} bytes exceeds 96-byte SUN_LEN margin: {bounded}",
        bounded.len()
    );
    assert!(
        bounded.starts_with("/tmp/"),
        "expected /tmp fallback for long dir, got {bounded}"
    );

    let listener = std::os::unix::net::UnixListener::bind(&bounded)
        .expect("bind UnixListener on bounded IPC socket path");
    drop(listener);
    super::cleanup_ipc_path(&bounded);
    assert!(!std::path::Path::new(&bounded).exists());
}
