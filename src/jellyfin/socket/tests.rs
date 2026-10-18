use super::{
    Duration, Endpoint, LibraryChange, RemoteCommand, ServerMessage, SocketHandle,
    apply_library_change, apply_user_data, handle_message, parse_message, spawn,
};
use crate::app::services::ShellBridge;
use crate::jellyfin::session::Session;
use crate::library::sync::SyncHandle;
use crate::library::{Library, UserDataRecord};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Instant;
use tungstenite::Message;

fn alice_library(server_url: &str) -> Arc<Library> {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    let mut credentials = library.credentials();
    credentials.server_url = Some(server_url.to_string());
    credentials.user_id = Some("alice".to_string());
    credentials.user_name = Some("Alice".to_string());
    credentials.server_id = Some("server".to_string());
    credentials.token = Some("alice-token".to_string());
    library.save_credentials(&credentials).expect("save");
    library
}

#[test]
fn remote_commands_reach_the_registered_handler_in_order() {
    let library = alice_library("http://127.0.0.1:9");
    let session = Session::restore(library.clone(), Arc::default());
    let scope = session.scope().expect("Alice scope");
    let handle = SocketHandle::detached();
    let sync = SyncHandle::detached();
    let message = |kind: &str| format!(r#"{{"MessageType":"{kind}","Data":{{"Command":"x"}}}}"#);

    handle_message(&message("Play"), &library, &session, &sync, &handle, &scope);

    let (seen_tx, seen) = mpsc::channel();
    handle.on_remote_command(Arc::new(move |command| {
        let _ = seen_tx.send(match command {
            RemoteCommand::Play { data, scope } => {
                format!("play:{}:{}", scope.user_id(), data["Command"])
            }
            RemoteCommand::Playstate { data, scope } => {
                format!("playstate:{}:{}", scope.user_id(), data["Command"])
            }
            RemoteCommand::General { data } => format!("general:{}", data["Command"]),
        });
    }));
    for kind in ["Play", "Playstate", "GeneralCommand"] {
        handle_message(&message(kind), &library, &session, &sync, &handle, &scope);
    }

    assert_eq!(
        seen.try_iter().collect::<Vec<_>>(),
        [
            r#"play:alice:"x""#,
            r#"playstate:alice:"x""#,
            r#"general:"x""#
        ]
    );
}

fn receive_request(listener: &TcpListener) -> (TcpStream, String) {
    let (mut stream, _) = listener.accept().expect("accept");
    let mut request = Vec::new();
    let mut buffer = [0_u8; 2_048];
    let header_end = loop {
        let read = stream.read(&mut buffer).expect("read request");
        assert!(read > 0, "connection closed before headers");
        request.extend_from_slice(&buffer[..read]);
        if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let head = String::from_utf8_lossy(&request[..header_end]).into_owned();
    let body_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while request.len() < header_end + body_length {
        let read = stream.read(&mut buffer).expect("read request body");
        assert!(read > 0, "connection closed before the body");
        request.extend_from_slice(&buffer[..read]);
    }
    let target = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("request target")
        .to_string();
    (stream, target)
}

fn send_json(mut stream: TcpStream, body: &str) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("write response");
}

#[test]
fn a_pushed_change_fetched_before_an_account_switch_is_not_cached() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let server_url = format!("http://{}", listener.local_addr().expect("address"));
    let (stale_fetch_tx, stale_fetch_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let server = thread::spawn(move || {
        let mut targets = Vec::new();
        let (stream, target) = receive_request(&listener);
        targets.push(target);
        send_json(
            stream,
            r#"{"Items":[{"Id":"alice-first","Name":"First","Type":"Movie"}],"TotalRecordCount":1}"#,
        );

        let (stale, target) = receive_request(&listener);
        targets.push(target);
        stale_fetch_tx.send(()).expect("signal the in-flight fetch");

        let (stream, target) = receive_request(&listener);
        targets.push(target);
        send_json(
            stream,
            r#"{"AccessToken":"bob-token","ServerId":"server",
                "User":{"Id":"bob","Name":"Bob","Policy":{"IsAdministrator":true}}}"#,
        );

        release_rx.recv().expect("release the stale fetch");
        send_json(
            stale,
            r#"{"Items":[{"Id":"alice-late","Name":"Late","Type":"Movie"}],"TotalRecordCount":1}"#,
        );
        targets
    });

    let library = alice_library(&server_url);
    let session = Arc::new(Session::restore(library.clone(), Arc::default()));
    let alice = session.scope().expect("Alice scope");
    let sync = SyncHandle::detached();

    apply_library_change(
        &library,
        &session,
        &sync,
        &ShellBridge::default(),
        &alice,
        &LibraryChange {
            changed: vec!["alice-first".to_string()],
            ..LibraryChange::default()
        },
    );
    assert!(library.item("alice-first").expect("item").is_some());

    let worker = {
        let library = library.clone();
        let session = session.clone();
        thread::spawn(move || {
            apply_library_change(
                &library,
                &session,
                &sync,
                &ShellBridge::default(),
                &alice,
                &LibraryChange {
                    changed: vec!["alice-late".to_string()],
                    ..LibraryChange::default()
                },
            );
        })
    };
    stale_fetch_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the pushed fetch reached the server");
    session
        .login(&server_url, "bob", "secret")
        .expect("switch to Bob");
    release_tx.send(()).expect("release");
    worker.join().expect("push worker");

    server.join().expect("server");
    assert_eq!(session.user_id().as_deref(), Some("bob"));
    assert!(library.item("alice-late").expect("item").is_none());
    assert_eq!(library.stats().total, 0);
}

#[test]
fn pushed_removals_and_watch_state_for_a_signed_out_session_are_dropped() {
    let library = alice_library("http://server:8096");
    library
        .ingest_page(
            &crate::jellyfin::session::Live::for_test(),
            &[serde_json::from_str(r#"{"Id":"m1","Name":"Kept","Type":"Movie"}"#).expect("dto")],
        )
        .expect("ingest");
    let session = Session::restore(library.clone(), Arc::default());
    let alice = session.scope().expect("Alice scope");
    session
        .clear_local(false)
        .expect("sign out, keeping the cache");

    apply_user_data(
        &library,
        &session,
        &ShellBridge::default(),
        &alice,
        &[UserDataRecord {
            jellyfin_id: "m1".to_string(),
            played: true,
            ..Default::default()
        }],
    );
    apply_library_change(
        &library,
        &session,
        &SyncHandle::detached(),
        &ShellBridge::default(),
        &alice,
        &LibraryChange {
            removed: vec!["m1".to_string()],
            ..LibraryChange::default()
        },
    );

    let item = library.item("m1").expect("item").expect("still cached");
    assert!(!item.summary.played);
}

#[test]
fn endpoints_map_http_bases_to_socket_urls() {
    assert_eq!(
        Endpoint::parse("http://server:8096"),
        Some(Endpoint {
            host: "server".to_string(),
            port: 8096,
            socket_url: "ws://server:8096/socket".to_string(),
        })
    );
    assert_eq!(
        Endpoint::parse("https://media.example.com/jellyfin"),
        Some(Endpoint {
            host: "media.example.com".to_string(),
            port: 443,
            socket_url: "wss://media.example.com/jellyfin/socket".to_string(),
        })
    );
    assert_eq!(
        Endpoint::parse("http://[::1]:8096"),
        Some(Endpoint {
            host: "::1".to_string(),
            port: 8096,
            socket_url: "ws://[::1]:8096/socket".to_string(),
        })
    );
    assert_eq!(Endpoint::parse("file:///etc/passwd"), None);
    assert_eq!(Endpoint::parse("http://:8096"), None);
}

#[test]
fn force_keep_alive_answers_within_the_announced_timeout() {
    assert!(matches!(
        parse_message(r#"{"MessageType":"ForceKeepAlive","Data":60}"#),
        ServerMessage::KeepAliveInterval(interval)
            if !interval.is_zero() && interval < Duration::from_secs(60)
    ));
    assert_eq!(
        parse_message(r#"{"MessageType":"ForceKeepAlive"}"#),
        ServerMessage::Ignored
    );
}

#[test]
fn library_changed_merges_added_and_updated_and_keeps_removed_separate() {
    let message = parse_message(
        r#"{"MessageType":"LibraryChanged","Data":{
            "ItemsAdded":["a","b"],"ItemsUpdated":["b","c",""],"ItemsRemoved":["gone"],
            "FoldersAddedTo":["f"],"FoldersRemovedFrom":["f","g"]}}"#,
    );
    assert_eq!(
        message,
        ServerMessage::LibraryChanged(LibraryChange {
            changed: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            removed: vec!["gone".to_string()],
            folders: vec!["f".to_string(), "g".to_string()],
        })
    );
}

#[test]
fn pushed_watch_state_reaches_the_library_cache() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let mut socket = tungstenite::accept(stream).expect("handshake");
        socket
            .send(Message::text(
                r#"{"MessageType":"ForceKeepAlive","Data":60}"#,
            ))
            .expect("force keep alive");
        socket
            .send(Message::text(
                r#"{"MessageType":"UserDataChanged","Data":{"UserId":"uid","UserDataList":[
                    {"ItemId":"ep1","Played":true,"PlayCount":3,"PlaybackPositionTicks":0,
                     "IsFavorite":false}]}}"#,
            ))
            .expect("user data");
        loop {
            match socket.read() {
                Ok(Message::Text(_)) => break,
                Ok(_) => {}
                Err(error) => panic!("expected a keep-alive answer: {error}"),
            }
        }
        let _ = socket.close(None);
    });

    let library = Arc::new(Library::open_in_memory().expect("library"));
    let mut credentials = library.credentials();
    credentials.server_url = Some(format!("http://{address}"));
    credentials.user_id = Some("uid".to_string());
    credentials.server_id = Some("server".to_string());
    credentials.token = Some("tok".to_string());
    library.save_credentials(&credentials).expect("save");
    library
        .ingest_page(
            &crate::jellyfin::session::Live::for_test(),
            &[
                serde_json::from_str(r#"{"Id":"ep1","Name":"Pilot","Type":"Episode"}"#)
                    .expect("dto"),
            ],
        )
        .expect("ingest");
    let session = Arc::new(Session::restore(library.clone(), Arc::default()));

    let handle = spawn(
        library.clone(),
        session,
        SyncHandle::detached(),
        Arc::default(),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let played = loop {
        let item = library.item("ep1").expect("item").expect("cached");
        if item.summary.played {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        thread::sleep(Duration::from_millis(50));
    };
    handle.stop();
    server.join().expect("server");
    assert!(played, "the pushed watch state never reached the cache");

    let item = library.item("ep1").expect("item").expect("cached");
    assert_eq!(item.summary.play_count, 3);
}
