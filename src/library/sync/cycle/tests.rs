use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use serde_json::json;

use super::PAGE_SIZE;
use crate::app::services::{ShellBridge, ShellRequest};
use crate::jellyfin::api::ApiError;
use crate::jellyfin::session::Session;
use crate::library::libraries::{LibraryId, LibraryRegistry, Membership};
use crate::library::sync::{
    IDENTITY_PAGE_SIZE, MAX_IDENTITY_PAGES, META_BOOTSTRAP_DONE, META_EMPTY_SWEEP_AT,
    META_LAST_BOOTSTRAP, META_LAST_IDENTITY_SWEEP, SyncHandle, SyncReport, Trigger,
    bootstrap_progress, now_unix, ownership_available,
};
use crate::library::{ItemQuery, Library, LibraryChangeBatch, StoredCredentials};

mod deletions;
mod libraries;

fn receive_target(listener: &TcpListener) -> (TcpStream, String) {
    let (mut stream, _) = listener.accept().expect("accept");
    let mut request = Vec::new();
    let mut buffer = [0_u8; 2_048];
    loop {
        let read = stream.read(&mut buffer).expect("read request");
        assert!(read > 0, "connection closed before headers");
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let target = String::from_utf8(request)
        .expect("utf8 request")
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("request target")
        .to_string();
    (stream, target)
}

fn send_json(stream: TcpStream, body: &str) {
    send(stream, 200, body);
}

fn send(mut stream: TcpStream, status: u16, body: &str) {
    write!(
        stream,
        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("write response");
}

fn authenticated_session(library: &Arc<Library>, server_url: &str) -> Session {
    let previous = library.credentials();
    library
        .save_credentials(&StoredCredentials {
            server_url: Some(server_url.to_string()),
            user_id: Some("user-1".to_string()),
            user_name: Some("Test User".to_string()),
            server_id: Some("server-1".to_string()),
            device_id: previous.device_id,
            token: Some("token-1".to_string()),
        })
        .expect("credentials");
    Session::restore(library.clone(), Arc::default())
}

#[test]
fn first_catalog_page_is_ready_while_background_paging_continues() {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    library
        .set_meta(META_LAST_IDENTITY_SWEEP, &now_unix().to_string())
        .expect("skip unrelated identity sweep");

    let first_items = (0..PAGE_SIZE)
        .map(|index| {
            serde_json::json!({
                "Id": format!("item-{index:03}"),
                "Name": format!("Item {index}"),
                "Type": "Movie",
                "DateCreated": format!("2024-01-01T00:{:02}:00Z", index % 60),
                "ImageTags": { "Primary": format!("poster-{index}") },
                "UserData": { "Played": false, "IsFavorite": index == 0 },
            })
        })
        .collect::<Vec<_>>();
    let first_body = serde_json::json!({
        "Items": first_items,
        "TotalRecordCount": PAGE_SIZE + 1,
        "StartIndex": 0,
    })
    .to_string();
    let second_body = serde_json::json!({
        "Items": [{
            "Id": "item-last",
            "Name": "Last item",
            "Type": "Movie",
            "DateCreated": "2024-02-01T00:00:00Z"
        }],
        "TotalRecordCount": PAGE_SIZE + 1,
        "StartIndex": PAGE_SIZE,
    })
    .to_string();
    let empty_body = r#"{"Items":[],"TotalRecordCount":201}"#.to_string();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let (second_started_tx, second_started_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let mut targets = Vec::new();
        let (stream, target) = receive_target(&listener);
        targets.push(target);
        send_json(stream, EMPTY_PAGE);

        let (stream, target) = receive_target(&listener);
        targets.push(target);
        send_json(stream, &first_body);

        let (stream, target) = receive_target(&listener);
        targets.push(target);
        second_started_tx.send(()).expect("signal second page");
        continue_rx.recv().expect("release second page");
        send_json(stream, &second_body);

        let (stream, target) = receive_target(&listener);
        targets.push(target);
        send_json(stream, &empty_body);
        targets
    });
    let session = authenticated_session(&library, &format!("http://{address}"));
    let worker_library = library.clone();
    let worker =
        thread::spawn(move || super::run_cycle(&worker_library, &session, Trigger::Scheduled));

    second_started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("worker continued to page two");
    let first_page = bootstrap_progress(&library);
    assert!(first_page.ready);
    assert!(!first_page.complete);
    assert_eq!(first_page.processed, PAGE_SIZE);
    assert_eq!(first_page.total, Some(PAGE_SIZE + 1));
    assert_eq!(library.stats().total, PAGE_SIZE);

    continue_tx.send(()).expect("continue catalog");
    let report = worker.join().expect("worker").expect("sync cycle");
    assert_eq!(report.bootstrapped as i64, PAGE_SIZE + 1);
    assert!(bootstrap_progress(&library).complete);
    assert_eq!(library.stats().total, PAGE_SIZE + 1);

    let targets = server.join().expect("server");
    assert!(targets[0].starts_with("/UserViews?"), "{}", targets[0]);
    assert!(targets[1].contains("StartIndex=0"));
    assert!(targets[2].contains(&format!("StartIndex={PAGE_SIZE}")));
    for field in ["ProviderIds", "Genres", "SortName", "ParentId"] {
        assert!(
            targets[1].contains(field),
            "{field} missing: {}",
            targets[1]
        );
    }
}

const FINISHED: &str = "/finished";

struct FakeJellyfin {
    url: String,
    server: thread::JoinHandle<Vec<String>>,
}

impl FakeJellyfin {
    fn requests(self) -> Vec<String> {
        let mut stream =
            TcpStream::connect(self.url.trim_start_matches("http://")).expect("connect");
        write!(stream, "GET {FINISHED} HTTP/1.1\r\n\r\n").expect("finish");
        self.server.join().expect("server")
    }
}

fn serve(replies: Vec<Reply>) -> FakeJellyfin {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let server = thread::spawn(move || {
        let mut replies = replies.into_iter();
        let mut targets = Vec::new();
        loop {
            let (stream, target) = receive_target(&listener);
            if target == FINISHED {
                return targets;
            }
            match replies.next().unwrap_or(Reply::Answer(404, String::new())) {
                Reply::Answer(status, body) => send(stream, status, &body),
                Reply::Drop => drop(stream),
            }
            targets.push(target);
        }
    });
    FakeJellyfin { url, server }
}

enum Reply {
    Answer(u16, String),
    Drop,
}

fn ok(body: &str) -> Reply {
    Reply::Answer(200, body.to_string())
}

const EMPTY_PAGE: &str = r#"{"Items":[],"TotalRecordCount":0}"#;
const UNCOUNTED: &str = "{}";
const NOT_JSON: &str = "not json";
const MOVIES: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const MORE_MOVIES: &str = "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";
const SHOWS: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
const MUSIC: &str = "d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4";
const ONE_MOVIE: &str = r#"{"Items":[{"Id":"m1","Name":"M1","Type":"Movie","DateCreated":"2024-01-01T00:00:00Z"}],"TotalRecordCount":1}"#;

fn cycle(
    library: &Arc<Library>,
    trigger: Trigger,
    replies: Vec<Reply>,
) -> (Result<SyncReport, ApiError>, Vec<String>) {
    let server = serve(replies);
    let session = authenticated_session(library, &server.url);
    let result = super::run_cycle(library, &session, trigger);
    (result, server.requests())
}

fn steps(targets: &[String]) -> Vec<&str> {
    targets
        .iter()
        .map(|target| {
            let parent = target
                .split(['?', '&'])
                .find_map(|pair| pair.strip_prefix("parentId="));
            match parent {
                Some(MOVIES) => "walk movies",
                Some(MORE_MOVIES) => "walk more movies",
                Some(SHOWS) => "walk shows",
                Some(_) => "walk elsewhere",
                None if target.starts_with("/UserViews") => "views",
                None if target.contains("SortOrder=Descending") => "incremental",
                None if target.contains("EnableImages=false") => "sweep",
                None if target.starts_with("/Items") => "bootstrap",
                None => target.as_str(),
            }
        })
        .collect()
}

fn views(libraries: &[(&str, &str)]) -> Reply {
    let items = libraries
        .iter()
        .map(|(id, collection_type)| {
            json!({ "Id": id, "Name": collection_type, "CollectionType": collection_type })
        })
        .collect::<Vec<_>>();
    ok(&json!({ "Items": items, "TotalRecordCount": items.len() }).to_string())
}

fn members(ids: &[&str], total: usize) -> Reply {
    let items = ids.iter().map(|id| json!({ "Id": id })).collect::<Vec<_>>();
    ok(&json!({ "Items": items, "TotalRecordCount": total }).to_string())
}

fn settled_library(titles: &[(&str, &str)]) -> Arc<Library> {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    let dtos = titles
        .iter()
        .map(|(id, kind)| {
            serde_json::from_value(json!({ "Id": id, "Name": id, "Type": kind })).expect("dto")
        })
        .collect::<Vec<_>>();
    library.ingest_page(&dtos).expect("seed");
    settled_catalog(&library);
    library
}

fn sync_libraries(library: &Arc<Library>, libraries: &[(&str, &str, &[&str])]) {
    let listed = libraries
        .iter()
        .map(|(id, collection_type, _)| (*id, *collection_type))
        .collect::<Vec<_>>();
    let mut replies = vec![views(&listed), ok(EMPTY_PAGE)];
    replies.extend(libraries.iter().map(|(_, _, ids)| members(ids, ids.len())));
    let (result, _) = cycle(library, Trigger::Scheduled, replies);
    result.expect("libraries synced");
}

fn listed(library: &Library) -> Option<Vec<String>> {
    match library.libraries().expect("registry") {
        LibraryRegistry::Unknown => None,
        LibraryRegistry::Known(libraries) => Some(
            libraries
                .iter()
                .map(|library| library.id.as_str().to_string())
                .collect(),
        ),
    }
}

fn walks_due(library: &Library) -> Vec<String> {
    match library.libraries().expect("registry") {
        LibraryRegistry::Unknown => Vec::new(),
        LibraryRegistry::Known(libraries) => libraries
            .into_iter()
            .filter(|library| {
                library.catalog_title().is_some() && library.membership != Membership::Current
            })
            .map(|library| library.id.as_str().to_string())
            .collect(),
    }
}

fn grid(library: &Library, id: &str) -> Vec<String> {
    let mut ids = library
        .query(&ItemQuery {
            library: LibraryId::parse(id),
            limit: 500,
            ..ItemQuery::default()
        })
        .expect("grid")
        .items
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn set_meta(library: &Library, key: &str, value: &str) {
    library.set_meta(key, value).expect("meta");
}

fn settled_catalog(library: &Library) {
    let now = now_unix().to_string();
    set_meta(library, META_BOOTSTRAP_DONE, "1");
    set_meta(library, META_LAST_BOOTSTRAP, &now);
    set_meta(library, META_LAST_IDENTITY_SWEEP, &now);
}

fn seed(library: &Library, dtos: &[serde_json::Value]) {
    let dtos = dtos
        .iter()
        .map(|dto| serde_json::from_value(dto.clone()).expect("dto"))
        .collect::<Vec<_>>();
    library.ingest_page(&dtos).expect("seed");
}

#[test]
fn an_unreadable_sync_state_stops_the_cycle_instead_of_restarting_the_catalog() {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    let session = authenticated_session(&library, "http://127.0.0.1:9");
    settled_catalog(&library);
    library
        .db
        .with_connection(|connection| connection.execute_batch("DROP TABLE meta"))
        .expect("break the sync state");

    assert!(matches!(
        super::run_cycle(&library, &session, Trigger::Requested),
        Err(ApiError::Storage(_))
    ));
    assert!(!bootstrap_progress(&library).complete);
}

struct Announced {
    result: Result<SyncReport, ApiError>,
    events: Vec<&'static str>,
    hooks: usize,
}

fn announced_cycle(library: &Arc<Library>, trigger: Trigger, replies: Vec<Reply>) -> Announced {
    let shell = Arc::new(ShellBridge::default());
    let events = shell.subscribe();
    let control = SyncHandle::new(shell);
    let hooks = Arc::new(AtomicUsize::new(0));
    let counted = hooks.clone();
    control.on_cycle_completed(Arc::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    }));
    let server = serve(replies);
    let session = authenticated_session(library, &server.url);
    let result = super::run_cycle_inner(library, &session, trigger, Some(&control));
    server.requests();
    let events = events
        .try_iter()
        .map(|event| match event {
            ShellRequest::LibrariesChanged => "libraries changed",
            ShellRequest::LibraryMembershipsSettled => "memberships settled",
            ShellRequest::CatalogChanged { .. } => "catalog page",
            ShellRequest::LibraryChanged { item_ids, .. } if item_ids.is_empty() => {
                "cycle completed"
            }
            ShellRequest::LibraryChanged { .. } => "titles changed",
            _ => "other",
        })
        .collect();
    Announced {
        result,
        events,
        hooks: hooks.load(Ordering::SeqCst),
    }
}

#[test]
fn the_first_sync_announces_the_list_before_the_catalog_and_settles_after_it() {
    let library = Arc::new(Library::open_in_memory().expect("library"));

    let first = announced_cycle(
        &library,
        Trigger::Requested,
        vec![
            views(&[(MOVIES, "movies")]),
            members(&["m1"], 1),
            ok(ONE_MOVIE),
            ok(ONE_MOVIE),
        ],
    );

    first.result.expect("the first sync");
    assert_eq!(
        first.events,
        [
            "libraries changed",
            "catalog page",
            "memberships settled",
            "cycle completed"
        ]
    );
    assert_eq!(first.hooks, 1);
}

#[test]
fn a_failing_walk_keeps_the_announcement_of_an_earlier_one() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie"), ("s1", "Series")]);
    sync_libraries(
        &library,
        &[
            (MOVIES, "movies", &["m1", "m2"]),
            (SHOWS, "tvshows", &["s1"]),
        ],
    );
    let listing = || views(&[(MOVIES, "movies"), (SHOWS, "tvshows")]);

    let quiet = announced_cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE)],
    );
    quiet.result.expect("a quiet sync");
    assert!(quiet.events.is_empty(), "{:?}", quiet.events);

    let failed = announced_cycle(
        &library,
        Trigger::Requested,
        vec![
            listing(),
            ok(EMPTY_PAGE),
            members(&["m1", "m2", "s1"], 3),
            members(&["m1"], 1),
            Reply::Answer(401, String::new()),
        ],
    );

    assert_eq!(failed.result.err(), Some(ApiError::Unauthorized));
    assert_eq!(grid(&library, MOVIES), ["m1"]);
    assert_eq!(failed.events, ["memberships settled", "cycle completed"]);
    assert_eq!(
        failed.hooks, 0,
        "a failed cycle does not run the completed-cycle hook"
    );
}

#[test]
fn a_rejected_token_stops_the_cycle_at_any_library_step() {
    let library = settled_library(&[("m1", "Movie"), ("s1", "Series")]);
    sync_libraries(
        &library,
        &[(MOVIES, "movies", &["m1"]), (SHOWS, "tvshows", &["s1"])],
    );

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![Reply::Answer(401, String::new())],
    );
    assert_eq!(result.err(), Some(ApiError::Unauthorized));
    assert_eq!(steps(&requests), ["views"]);

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![
            views(&[(MOVIES, "movies"), (SHOWS, "tvshows")]),
            ok(EMPTY_PAGE),
            members(&["m1", "s1"], 2),
            Reply::Answer(401, String::new()),
        ],
    );
    assert_eq!(result.err(), Some(ApiError::Unauthorized));
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk movies"]
    );
}

#[test]
fn a_sweep_names_each_title_whose_watched_state_changed_on_the_server() {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    let dtos = [
        json!({ "Id": "s1", "Name": "Show", "Type": "Series" }),
        json!({ "Id": "se1", "Name": "Season 1", "Type": "Season",
            "ParentId": "s1", "SeriesId": "s1" }),
        json!({ "Id": "e1", "Name": "Pilot", "Type": "Episode",
            "ParentId": "se1", "SeriesId": "s1", "SeasonId": "se1",
            "UserData": { "Played": false } }),
        json!({ "Id": "m1", "Name": "Movie", "Type": "Movie",
            "ParentId": MOVIES, "UserData": { "IsFavorite": true } }),
    ]
    .into_iter()
    .map(|dto| serde_json::from_value(dto).expect("dto"))
    .collect::<Vec<_>>();
    library.ingest_page(&dtos).expect("seed");
    settled_catalog(&library);
    let sweep = || {
        ok(&json!({
            "Items": [
                { "Id": "s1" },
                { "Id": "se1" },
                { "Id": "e1", "UserData": { "Played": true, "PlayCount": 1 } },
                { "Id": "m1", "UserData": { "IsFavorite": true } },
            ],
            "TotalRecordCount": 4,
        })
        .to_string())
    };

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![views(&[]), ok(EMPTY_PAGE), sweep()],
    );
    let report = result.expect("a sweep that finds the episode played");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 0);
    assert_eq!(report.user_data_refreshed, 1);
    assert_eq!(
        report.changes,
        LibraryChangeBatch {
            item_ids: vec!["e1".to_string()],
            context_ids: vec!["s1".to_string(), "se1".to_string()],
        }
    );

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![views(&[]), ok(EMPTY_PAGE), sweep()],
    );
    let report = result.expect("a sweep that finds nothing new");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.user_data_refreshed, 0);
    assert_eq!(report.changes, LibraryChangeBatch::default());
}
