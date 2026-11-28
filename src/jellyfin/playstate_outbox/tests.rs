use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use rusqlite::params;
use serde_json::{Value, json};

use super::{OutboxEntry, OutboxKind, PlaystateOutbox};
use crate::app::services::ShellBridge;
use crate::jellyfin::api::ApiError;
use crate::jellyfin::session::Session;
use crate::library::{Library, StoredCredentials};
use crate::preferences::AccountKey;

const SECRET_TOKEN: &str = "super-secret-token-xyz";

struct TempDbDir(PathBuf);

impl TempDbDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mf-outbox-{tag}-{}",
            crate::app::ids::random_hex(6)
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        Self(dir)
    }

    fn db_path(&self) -> PathBuf {
        self.0.join("library.db")
    }
}

impl Drop for TempDbDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn account(idx: u8) -> AccountKey {
    AccountKey::new(format!("server-{idx}"), format!("user-{idx}")).expect("account")
}

fn entry(item_id: &str, ps: &str, method: &str, ticks: i64, paused: bool) -> OutboxEntry {
    OutboxEntry {
        item_id: item_id.to_string(),
        play_session_id: ps.to_string(),
        position_ticks: ticks,
        is_paused: paused,
        failed: false,
        play_method: method.to_string(),
        device_id: "dev-1".to_string(),
        payload: json!({
            "ItemId": item_id,
            "PlaySessionId": ps,
            "PositionTicks": ticks,
            "IsPaused": paused,
            "PlayMethod": method,
        }),
    }
}

#[derive(Debug, Clone)]
enum Op {
    Start {
        acc: u8,
        item: u8,
        ps: u8,
        transcode: bool,
        ticks: i64,
    },
    Progress {
        acc: u8,
        item: u8,
        ps: u8,
        transcode: bool,
        ticks: i64,
        paused: bool,
    },
    Stopped {
        acc: u8,
        item: u8,
        ps: u8,
        transcode: bool,
        ticks: i64,
    },
    DeliverSuccess(u8),
    FailTransient(u8),
    FailPermanent(u8),
    DeleteAccount(u8),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u8..2, 0u8..2, 0u8..2, any::<bool>(), 0i64..1_000_000).prop_map(
            |(acc, item, ps, transcode, ticks)| Op::Start {
                acc,
                item,
                ps,
                transcode,
                ticks
            }
        ),
        (
            0u8..2,
            0u8..2,
            0u8..2,
            any::<bool>(),
            0i64..1_000_000,
            any::<bool>()
        )
            .prop_map(|(acc, item, ps, transcode, ticks, paused)| Op::Progress {
                acc,
                item,
                ps,
                transcode,
                ticks,
                paused
            }),
        (0u8..2, 0u8..2, 0u8..2, any::<bool>(), 0i64..1_000_000).prop_map(
            |(acc, item, ps, transcode, ticks)| Op::Stopped {
                acc,
                item,
                ps,
                transcode,
                ticks
            }
        ),
        (0u8..2).prop_map(Op::DeliverSuccess),
        (0u8..2).prop_map(Op::FailTransient),
        (0u8..2).prop_map(Op::FailPermanent),
        (0u8..2).prop_map(Op::DeleteAccount),
    ]
}

fn row_count(library: &Library) -> i64 {
    library
        .with_connection(|con| {
            con.query_row("SELECT count(*) FROM playstate_outbox", [], |r| r.get(0))
        })
        .expect("count")
}

fn snapshot_db(library: &Library) -> Vec<(String, String, String, String, String, i64)> {
    library
        .with_connection(|con| {
            let mut stmt = con.prepare(
                "SELECT server_id, user_id, item_id, play_session_id, kind, position_ticks
                 FROM playstate_outbox
                 ORDER BY server_id, user_id, item_id, play_session_id,
                     CASE kind
                         WHEN 'start' THEN 0
                         WHEN 'progress' THEN 1
                         WHEN 'stopped' THEN 2
                         ELSE 3
                     END, id",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .expect("snapshot")
}

fn kind_rank(kind: &str) -> usize {
    match kind {
        "start" => 0,
        "progress" => 1,
        "stopped" => 2,
        "stop_encoding" => 3,
        _ => 99,
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        cases: 64,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_outbox_coalescing_and_ordering_hold(ops in prop::collection::vec(op_strategy(), 1..35)) {
        let library = Arc::new(Library::open_in_memory().expect("open"));
        let session = Arc::new(Session::restore(library.clone(), Arc::new(ShellBridge::default())));
        let outbox = PlaystateOutbox::new_manual(library.clone(), session);

        let mut model: BTreeMap<(u8, String, String, String), i64> = BTreeMap::new();
        let mut stopped_sessions: HashSet<(u8, String, String)> = HashSet::new();
        let mut clock = 1000_i64;

        for op in ops {
            clock += 100;
            match op {
                Op::Start { acc, item, ps, transcode, ticks } => {
                    let method = if transcode { "Transcode" } else { "DirectPlay" };
                    let item_s = format!("item-{item}");
                    let ps_s = format!("ps-{ps}");
                    outbox.enqueue_start(&account(acc), &entry(&item_s, &ps_s, method, ticks, false));
                    if !stopped_sessions.contains(&(acc, item_s.clone(), ps_s.clone()))
                        && !model.contains_key(&(acc, item_s.clone(), ps_s.clone(), "stopped".to_string()))
                    {
                        model.insert((acc, item_s, ps_s, "start".to_string()), ticks);
                    }
                }
                Op::Progress { acc, item, ps, transcode, ticks, paused } => {
                    let method = if transcode { "Transcode" } else { "DirectPlay" };
                    let item_s = format!("item-{item}");
                    let ps_s = format!("ps-{ps}");
                    outbox.enqueue_progress(&account(acc), &entry(&item_s, &ps_s, method, ticks, paused));
                    if !stopped_sessions.contains(&(acc, item_s.clone(), ps_s.clone()))
                        && !model.contains_key(&(acc, item_s.clone(), ps_s.clone(), "stopped".to_string()))
                    {
                        model.insert((acc, item_s, ps_s, "progress".to_string()), ticks);
                    }
                }
                Op::Stopped { acc, item, ps, transcode, ticks } => {
                    let method = if transcode { "Transcode" } else { "DirectPlay" };
                    let item_s = format!("item-{item}");
                    let ps_s = format!("ps-{ps}");
                    outbox.enqueue_stopped(&account(acc), &entry(&item_s, &ps_s, method, ticks, false));
                    stopped_sessions.insert((acc, item_s.clone(), ps_s.clone()));
                    model.remove(&(acc, item_s.clone(), ps_s.clone(), "progress".to_string()));
                    model.insert((acc, item_s.clone(), ps_s.clone(), "stopped".to_string()), ticks);
                    if transcode {
                        model.insert((acc, item_s, ps_s, "stop_encoding".to_string()), 0);
                    }
                }
                Op::DeliverSuccess(acc) => {
                    let a = account(acc);
                    if let Some(due) = outbox.next_due_row_for_test(&a, clock).expect("due") {
                        outbox.record_outcome_for_test(&a, due.id, due.revision, due.kind, Ok(()), clock).expect("ok");
                        model.remove(&(acc, due.item_id, due.play_session_id, due.kind.as_str().to_string()));
                    }
                }
                Op::FailTransient(acc) => {
                    let a = account(acc);
                    if let Some(due) = outbox.next_due_row_for_test(&a, clock).expect("due") {
                        outbox
                            .record_outcome_for_test(&a, due.id, due.revision, due.kind, Err(ApiError::Status { status: 500 }), clock)
                            .expect("err");
                    }
                }
                Op::FailPermanent(acc) => {
                    let a = account(acc);
                    if let Some(due) = outbox.next_due_row_for_test(&a, clock).expect("due") {
                        outbox
                            .record_outcome_for_test(&a, due.id, due.revision, due.kind, Err(ApiError::Status { status: 400 }), clock)
                            .expect("perm");
                        model.remove(&(acc, due.item_id, due.play_session_id, due.kind.as_str().to_string()));
                    }
                }
                Op::DeleteAccount(acc) => {
                    outbox.delete_account(&account(acc)).expect("delete");
                    model.retain(|(a, _, _, _), _| *a != acc);
                    stopped_sessions.retain(|(a, _, _)| *a != acc);
                }
            }

            let rows = snapshot_db(&library);
            prop_assert_eq!(rows.len(), model.len());
            for (srv, _usr, item, ps, kind, ticks) in &rows {
                let acc_num = if srv == "server-0" { 0 } else { 1 };
                prop_assert_eq!(
                    model.get(&(acc_num, item.clone(), ps.clone(), kind.clone())).copied(),
                    Some(*ticks)
                );
                if kind == "stopped" {
                    prop_assert!(
                        !rows.iter().any(|(s2, _, i2, p2, k2, _)| s2 == srv && i2 == item && p2 == ps && k2 == "progress")
                    );
                }
            }

            for acc_num in 0u8..2 {
                let a = account(acc_num);
                if let Some(due) = outbox.next_due_row_for_test(&a, clock + 10_000).expect("check order") {
                    let min_rank = rows
                        .iter()
                        .filter(|(s, _, i, p, _, _)| {
                            s == a.server_id() && i == &due.item_id && p == &due.play_session_id
                        })
                        .map(|(_, _, _, _, k, _)| kind_rank(k))
                        .min()
                        .unwrap_or(99);
                    prop_assert!(kind_rank(due.kind.as_str()) <= min_rank);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct RecordedHttp {
    method: String,
    path: String,
    token: Option<String>,
    body: Value,
}

struct StubJellyfin {
    port: u16,
    requests: Arc<Mutex<Vec<RecordedHttp>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl StubJellyfin {
    fn start_on_port(port: u16, status: u16) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("addr").port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let reqs_bg = Arc::clone(&requests);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_bg = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !stop_bg.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let req = read_raw_http(&mut stream);
                        if !req.path.is_empty() {
                            reqs_bg.lock().expect("lock").push(req);
                        }
                        let resp = format!(
                            "HTTP/1.1 {status} OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                        let _ = stream.write_all(resp.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            port,
            requests,
            stop,
            handle: Some(handle),
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn received(&self) -> Vec<RecordedHttp> {
        self.requests.lock().expect("lock").clone()
    }
}

impl Drop for StubJellyfin {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn read_raw_http(stream: &mut std::net::TcpStream) -> RecordedHttp {
    let mut raw = Vec::new();
    let mut buf = [0u8; 2048];
    let mut hdr_end = None;
    let mut len = 0usize;
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if hdr_end.is_none()
            && let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n")
        {
            let end = pos + 4;
            hdr_end = Some(end);
            for line in String::from_utf8_lossy(&raw[..pos]).lines().skip(1) {
                if let Some((k, v)) = line.split_once(':')
                    && k.eq_ignore_ascii_case("content-length")
                {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
        }
        if let Some(end) = hdr_end
            && raw.len() >= end + len
        {
            break;
        }
    }
    let Some(end) = hdr_end else {
        return RecordedHttp {
            method: String::new(),
            path: String::new(),
            token: None,
            body: Value::Null,
        };
    };
    let head = String::from_utf8_lossy(&raw[..end - 4]);
    let mut lines = head.lines();
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let token = lines.find_map(|line| {
        let (k, v) = line.split_once(':')?;
        if k.eq_ignore_ascii_case("authorization") && v.contains("Token=\"") {
            let start = v.find("Token=\"")? + 7;
            let rest = &v[start..];
            let end = rest.find('"')?;
            return Some(rest[..end].to_string());
        }
        None
    });
    let body_bytes = &raw[end..(end + len).min(raw.len())];
    let body = serde_json::from_slice(body_bytes).unwrap_or(Value::Null);
    RecordedHttp {
        method,
        path,
        token,
        body,
    }
}

fn free_closed_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

fn save_creds(library: &Library, server_url: &str, server_id: &str, user_id: &str, token: &str) {
    library
        .save_credentials(&StoredCredentials {
            server_url: Some(server_url.to_string()),
            user_id: Some(user_id.to_string()),
            user_name: Some("tester".to_string()),
            server_id: Some(server_id.to_string()),
            device_id: "dev-1".to_string(),
            token: Some(token.to_string()),
        })
        .expect("save creds");
}

#[test]
fn crash_and_restart_persists_stopped_and_drains_on_reopen() {
    let dir = TempDbDir::new("crash");
    let port = free_closed_port();
    let url = format!("http://127.0.0.1:{port}");
    let acc = AccountKey::new("srv-crash", "usr-crash").expect("acc");

    {
        let library = Arc::new(Library::open(&dir.db_path()).expect("open 1"));
        save_creds(&library, &url, acc.server_id(), acc.user_id(), SECRET_TOKEN);
        let session = Arc::new(Session::restore(
            library.clone(),
            Arc::new(ShellBridge::default()),
        ));
        let outbox = PlaystateOutbox::new(library.clone(), session);
        outbox.enqueue_start(
            &acc,
            &entry("item-crash", "ps-c", "DirectPlay", 10_000, false),
        );
        outbox.enqueue_stopped(
            &acc,
            &entry("item-crash", "ps-c", "DirectPlay", 950_000, false),
        );
        assert_eq!(row_count(&library), 2);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && outbox.flush_active(Duration::from_millis(50)) {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(row_count(&library), 2);
        outbox.shutdown();
    }

    let stub = StubJellyfin::start_on_port(port, 204);
    let library = Arc::new(Library::open(&dir.db_path()).expect("open 2"));
    save_creds(
        &library,
        &stub.base_url(),
        acc.server_id(),
        acc.user_id(),
        SECRET_TOKEN,
    );
    let session = Arc::new(Session::restore(
        library.clone(),
        Arc::new(ShellBridge::default()),
    ));
    let outbox = PlaystateOutbox::new(library.clone(), session);
    outbox.wake_account(&acc);
    assert!(outbox.flush_active(Duration::from_secs(3)));
    assert_eq!(row_count(&library), 0);

    let reqs = stub.received();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].method, "POST");
    assert_eq!(reqs[0].path, "/Sessions/Playing");
    assert_eq!(reqs[1].method, "POST");
    assert_eq!(reqs[1].path, "/Sessions/Playing/Stopped");
    assert_eq!(reqs[1].body["PositionTicks"], 950_000);
    assert_eq!(reqs[1].token.as_deref(), Some(SECRET_TOKEN));
    outbox.shutdown();
}

#[test]
fn account_switch_isolates_outbox_and_delete_removes_only_target() {
    let dir = TempDbDir::new("switch");
    let stub_a = StubJellyfin::start_on_port(0, 500);
    let stub_b = StubJellyfin::start_on_port(0, 204);
    let acc_a = AccountKey::new("srv-a", "usr-a").expect("acc a");
    let acc_b = AccountKey::new("srv-b", "usr-b").expect("acc b");

    let library = Arc::new(Library::open(&dir.db_path()).expect("open"));
    save_creds(
        &library,
        &stub_a.base_url(),
        acc_a.server_id(),
        acc_a.user_id(),
        "tok-a",
    );
    let session_a = Arc::new(Session::restore(
        library.clone(),
        Arc::new(ShellBridge::default()),
    ));
    let outbox_a = PlaystateOutbox::new(library.clone(), session_a);
    outbox_a.enqueue_stopped(
        &acc_a,
        &entry("item-a", "ps-a", "DirectPlay", 321_000, false),
    );
    assert!(!outbox_a.flush_active(Duration::from_millis(500)));
    assert_eq!(row_count(&library), 1);
    outbox_a.shutdown();

    save_creds(
        &library,
        &stub_b.base_url(),
        acc_b.server_id(),
        acc_b.user_id(),
        "tok-b",
    );
    let session_b = Arc::new(Session::restore(
        library.clone(),
        Arc::new(ShellBridge::default()),
    ));
    let outbox_b = PlaystateOutbox::new(library.clone(), session_b);
    outbox_b.wake_account(&acc_b);
    assert!(outbox_b.flush_active(Duration::from_secs(2)));
    assert!(stub_b.received().is_empty());
    assert_eq!(row_count(&library), 1);
    outbox_b.shutdown();

    drop(stub_a);
    let stub_a_up = StubJellyfin::start_on_port(0, 204);
    save_creds(
        &library,
        &stub_a_up.base_url(),
        acc_a.server_id(),
        acc_a.user_id(),
        "tok-a",
    );
    let session_a2 = Arc::new(Session::restore(
        library.clone(),
        Arc::new(ShellBridge::default()),
    ));
    let outbox_a2 = PlaystateOutbox::new(library.clone(), session_a2);
    outbox_a2.wake_account(&acc_a);
    assert!(outbox_a2.flush_active(Duration::from_secs(3)));
    assert_eq!(row_count(&library), 0);
    assert_eq!(stub_a_up.received().len(), 1);
    assert_eq!(stub_a_up.received()[0].body["PositionTicks"], 321_000);

    outbox_a2.enqueue_stopped(&acc_a, &entry("item-a2", "ps-a2", "DirectPlay", 100, false));
    outbox_a2.enqueue_stopped(&acc_b, &entry("item-b2", "ps-b2", "DirectPlay", 200, false));
    outbox_a2.delete_account(&acc_a).expect("delete a");
    let left = snapshot_db(&library);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].0, "srv-b");
    outbox_a2.shutdown();
}

#[test]
fn transcode_stopped_sends_active_encodings_delete() {
    let dir = TempDbDir::new("transcode");
    let stub = StubJellyfin::start_on_port(0, 204);
    let acc = AccountKey::new("srv-t", "usr-t").expect("acc t");
    let library = Arc::new(Library::open(&dir.db_path()).expect("open"));
    save_creds(
        &library,
        &stub.base_url(),
        acc.server_id(),
        acc.user_id(),
        SECRET_TOKEN,
    );
    let session = Arc::new(Session::restore(
        library.clone(),
        Arc::new(ShellBridge::default()),
    ));
    let outbox = PlaystateOutbox::new(library.clone(), session);
    outbox.enqueue_stopped(&acc, &entry("item-t", "ps-t", "Transcode", 400_000, false));
    assert!(outbox.flush_active(Duration::from_secs(3)));
    assert_eq!(row_count(&library), 0);
    let reqs = stub.received();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].method, "POST");
    assert_eq!(reqs[0].path, "/Sessions/Playing/Stopped");
    assert_eq!(reqs[1].method, "DELETE");
    assert_eq!(
        reqs[1].path,
        "/Videos/ActiveEncodings?deviceId=dev-1&playSessionId=ps-t"
    );
    outbox.shutdown();
}

#[test]
fn retry_classification_and_zero_secret_persistence() {
    let dir = TempDbDir::new("retry");
    let library = Arc::new(Library::open(&dir.db_path()).expect("open"));
    let stub = StubJellyfin::start_on_port(0, 204);
    save_creds(&library, &stub.base_url(), "srv-r", "usr-r", SECRET_TOKEN);
    let session = Arc::new(Session::restore(
        library.clone(),
        Arc::new(ShellBridge::default()),
    ));
    let outbox = PlaystateOutbox::new_manual(library.clone(), session);
    let acc = AccountKey::new("srv-r", "usr-r").expect("acc");

    outbox.enqueue_stopped(&acc, &entry("item-r", "ps-r", "Transcode", 500_000, false));
    library
        .with_connection(|con| {
            let mut stmt = con.prepare(
                "SELECT server_id, user_id, item_id, play_session_id, kind,
                        play_method, device_id, payload_json
                 FROM playstate_outbox",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                for idx in 0..8 {
                    let val: String = row.get(idx)?;
                    assert!(!val.contains(SECRET_TOKEN));
                    assert!(!val.contains("127.0.0.1"));
                }
            }
            Ok(())
        })
        .expect("secret scan");

    let row1 = outbox
        .next_due_row_for_test(&acc, 1000)
        .expect("row1")
        .expect("some row1");
    assert_eq!(row1.kind, OutboxKind::Stopped);
    outbox
        .record_outcome_for_test(
            &acc,
            row1.id,
            row1.revision,
            row1.kind,
            Err(ApiError::Status { status: 500 }),
            1000,
        )
        .expect("500");
    let after_500 = library
        .with_connection(|con| {
            con.query_row(
                "SELECT attempts, next_retry_at FROM playstate_outbox WHERE id = ?1",
                params![row1.id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
        })
        .expect("query after 500");
    assert_eq!(after_500.0, 1);
    assert!(after_500.1 > 1000);

    outbox
        .record_outcome_for_test(
            &acc,
            row1.id,
            row1.revision,
            row1.kind,
            Err(ApiError::Unauthorized),
            2000,
        )
        .expect("401");
    assert!(outbox.is_account_paused(&acc));
    assert_eq!(row_count(&library), 2);
    assert!(
        outbox
            .next_due_row_for_test(&acc, 999_999)
            .expect("paused")
            .is_none()
    );

    outbox.wake_account(&acc);
    assert!(!outbox.is_account_paused(&acc));
    let due_after_wake = outbox
        .next_due_row_for_test(&acc, 2001)
        .expect("wake")
        .expect("some due");
    outbox
        .record_outcome_for_test(
            &acc,
            due_after_wake.id,
            due_after_wake.revision,
            due_after_wake.kind,
            Err(ApiError::Status { status: 400 }),
            2001,
        )
        .expect("400");
    assert_eq!(row_count(&library), 1);
}

#[test]
fn stale_progress_ack_does_not_delete_or_clobber_newer_queued_progress() {
    let library = Arc::new(Library::open_in_memory().expect("open"));
    let session = Arc::new(Session::restore(
        library.clone(),
        Arc::new(ShellBridge::default()),
    ));
    let outbox = PlaystateOutbox::new_manual(library.clone(), session);
    let acc = AccountKey::new("srv-cas", "usr-cas").expect("acc");

    outbox.enqueue_progress(
        &acc,
        &entry("item-cas", "ps-cas", "DirectPlay", 100_000, false),
    );
    let in_flight = outbox
        .next_due_row_for_test(&acc, 1000)
        .expect("due")
        .expect("row");
    assert_eq!(in_flight.revision, 0);

    outbox.enqueue_progress(
        &acc,
        &entry("item-cas", "ps-cas", "DirectPlay", 250_000, false),
    );

    outbox
        .record_outcome_for_test(
            &acc,
            in_flight.id,
            in_flight.revision,
            in_flight.kind,
            Ok(()),
            1001,
        )
        .expect("stale ok");
    outbox
        .record_outcome_for_test(
            &acc,
            in_flight.id,
            in_flight.revision,
            in_flight.kind,
            Err(ApiError::Status { status: 400 }),
            1002,
        )
        .expect("stale 400");
    outbox
        .record_outcome_for_test(
            &acc,
            in_flight.id,
            in_flight.revision,
            in_flight.kind,
            Err(ApiError::Status { status: 500 }),
            1003,
        )
        .expect("stale 500");

    assert_eq!(row_count(&library), 1);
    let current = outbox
        .next_due_row_for_test(&acc, 1004)
        .expect("due")
        .expect("newer progress survives immediately due");
    assert_eq!(current.revision, 1);
    let body: Value = serde_json::from_str(&current.payload_json).expect("json");
    assert_eq!(body["PositionTicks"], 250_000);

    outbox
        .record_outcome_for_test(
            &acc,
            current.id,
            current.revision,
            current.kind,
            Ok(()),
            1005,
        )
        .expect("current ack");
    assert_eq!(row_count(&library), 0);
}

#[test]
fn retrying_start_blocks_stopped_and_same_second_transitions_keep_fifo_order() {
    let library = Arc::new(Library::open_in_memory().expect("open"));
    let session = Arc::new(Session::restore(
        library.clone(),
        Arc::new(ShellBridge::default()),
    ));
    let outbox = PlaystateOutbox::new_manual(library.clone(), session);
    let acc = AccountKey::new("srv-ord", "usr-ord").expect("acc");

    outbox.enqueue_start(&acc, &entry("ep-1", "ps-1", "DirectPlay", 0, false));
    outbox.enqueue_stopped(&acc, &entry("ep-1", "ps-1", "DirectPlay", 500_000, false));
    outbox.enqueue_start(&acc, &entry("ep-2", "ps-2", "DirectPlay", 0, false));
    library
        .with_connection(|con| {
            con.execute(
                "UPDATE playstate_outbox SET created_at = 5000, updated_at = 5000",
                [],
            )
        })
        .expect("same created_at second");

    let first = outbox
        .next_due_row_for_test(&acc, 5000)
        .expect("due")
        .expect("first");
    assert_eq!(first.item_id, "ep-1");
    assert_eq!(first.kind, OutboxKind::Start);

    outbox
        .record_outcome_for_test(
            &acc,
            first.id,
            first.revision,
            first.kind,
            Err(ApiError::Status { status: 503 }),
            5000,
        )
        .expect("retry start");
    assert!(
        outbox
            .next_due_row_for_test(&acc, 5001)
            .expect("due")
            .is_none(),
        "backing-off start must block ep-1 stopped and ep-2 start"
    );

    let after_backoff = outbox
        .next_due_row_for_test(&acc, 5100)
        .expect("due")
        .expect("start ready");
    assert_eq!(after_backoff.item_id, "ep-1");
    assert_eq!(after_backoff.kind, OutboxKind::Start);
    outbox
        .record_outcome_for_test(
            &acc,
            after_backoff.id,
            after_backoff.revision,
            after_backoff.kind,
            Ok(()),
            5100,
        )
        .expect("start ack");

    let second = outbox
        .next_due_row_for_test(&acc, 5100)
        .expect("due")
        .expect("ep-1 stopped before ep-2 start even in same created_at second");
    assert_eq!(second.item_id, "ep-1");
    assert_eq!(second.kind, OutboxKind::Stopped);
    outbox
        .record_outcome_for_test(&acc, second.id, second.revision, second.kind, Ok(()), 5100)
        .expect("stopped ack");

    let third = outbox
        .next_due_row_for_test(&acc, 5100)
        .expect("due")
        .expect("ep-2 start");
    assert_eq!(third.item_id, "ep-2");
    assert_eq!(third.kind, OutboxKind::Start);
}

#[test]
fn empty_play_session_id_does_not_poison_future_plays_of_same_item() {
    let library = Arc::new(Library::open_in_memory().expect("open"));
    let session = Arc::new(Session::restore(
        library.clone(),
        Arc::new(ShellBridge::default()),
    ));
    let outbox = PlaystateOutbox::new_manual(library.clone(), session);
    let acc = AccountKey::new("srv-empty", "usr-empty").expect("acc");

    outbox.enqueue_stopped(&acc, &entry("item-x", "", "DirectPlay", 10_000, false));
    outbox.enqueue_start(&acc, &entry("item-x", "", "DirectPlay", 20_000, false));
    outbox.enqueue_progress(&acc, &entry("item-x", "", "DirectPlay", 30_000, false));
    assert_eq!(row_count(&library), 3);

    let first = outbox
        .next_due_row_for_test(&acc, 1000)
        .expect("due")
        .expect("older empty-session stopped must drain before newer start");
    assert_eq!(first.kind, OutboxKind::Stopped);
    outbox
        .record_outcome_for_test(&acc, first.id, first.revision, first.kind, Ok(()), 1000)
        .expect("ack");

    let second = outbox
        .next_due_row_for_test(&acc, 1001)
        .expect("due")
        .expect("newer empty-session start");
    assert_eq!(second.kind, OutboxKind::Start);
}

#[test]
fn in_flight_progress_ack_never_deletes_stopped_report() {
    let tmp = TempDbDir::new("inflight-stop");
    let library = Arc::new(Library::open(&tmp.db_path()).expect("open"));
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let server_url = format!("http://{}", listener.local_addr().expect("addr"));
    let acc = AccountKey::new("srv-race", "usr-race").expect("acc");
    library
        .save_credentials(&StoredCredentials {
            server_url: Some(server_url),
            user_id: Some(acc.user_id().to_string()),
            user_name: Some("Tester".to_string()),
            server_id: Some(acc.server_id().to_string()),
            device_id: "dev-race".to_string(),
            token: Some(SECRET_TOKEN.to_string()),
        })
        .expect("creds");

    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let delivered = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let delivered_srv = Arc::clone(&delivered);
    let server = thread::spawn(move || {
        for idx in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let req = crate::jellyfin::playback_reporter::read_test_http_request(&mut stream);
            let parsed: Value = serde_json::from_str(&req.body).unwrap_or(Value::Null);
            delivered_srv
                .lock()
                .expect("lock")
                .push((req.request_line, parsed));
            if idx == 0 {
                let _ = seen_tx.send(());
                let _ = release_rx.recv();
            }
            let _ = stream.write_all(
                b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            let _ = stream.flush();
        }
    });

    let session = Arc::new(Session::restore(
        library.clone(),
        Arc::new(ShellBridge::default()),
    ));
    let outbox = PlaystateOutbox::new(library.clone(), session);
    assert!(outbox.enqueue_progress(&acc, &entry("item-r", "ps-r", "DirectPlay", 100_000, false)));
    seen_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("progress reached server");
    assert!(outbox.enqueue_stopped(&acc, &entry("item-r", "ps-r", "DirectPlay", 250_000, false)));
    release_tx.send(()).expect("release");

    assert!(outbox.flush_active(Duration::from_secs(5)));
    outbox.shutdown();
    server.join().expect("server");

    let entries = delivered.lock().expect("lock");
    assert_eq!(
        entries.len(),
        2,
        "both progress and stopped must be delivered"
    );
    assert!(entries[1].0.starts_with("POST /Sessions/Playing/Stopped"));
    assert_eq!(entries[1].1["PositionTicks"], 250_000);
    assert_eq!(row_count(&library), 0);
}
