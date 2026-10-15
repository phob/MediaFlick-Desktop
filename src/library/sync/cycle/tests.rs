use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use serde_json::json;

use super::PAGE_SIZE;
use crate::jellyfin::api::ApiError;
use crate::jellyfin::session::Session;
use crate::library::libraries::{LibraryId, LibraryRegistry};
use crate::library::sync::{
    META_BOOTSTRAP_DONE, META_LAST_BOOTSTRAP, META_LAST_IDENTITY_SWEEP, SyncReport, Trigger,
    bootstrap_progress, now_unix, ownership_available,
};
use crate::library::{ItemQuery, Library, StoredCredentials};

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

fn serve(replies: Vec<(u16, String)>) -> FakeJellyfin {
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
            let (status, body) = replies.next().unwrap_or((404, String::new()));
            if status != DROPPED {
                send(stream, status, &body);
            }
            targets.push(target);
        }
    });
    FakeJellyfin { url, server }
}

fn ok(body: &str) -> (u16, String) {
    (200, body.to_string())
}

const DROPPED: u16 = 0;
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
    replies: Vec<(u16, String)>,
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

fn views(libraries: &[(&str, &str)]) -> (u16, String) {
    let items = libraries
        .iter()
        .map(|(id, collection_type)| {
            json!({ "Id": id, "Name": collection_type, "CollectionType": collection_type })
        })
        .collect::<Vec<_>>();
    ok(&json!({ "Items": items, "TotalRecordCount": items.len() }).to_string())
}

fn members(ids: &[&str], total: usize) -> (u16, String) {
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

fn pending(library: &Library) -> Vec<String> {
    match library.libraries().expect("registry") {
        LibraryRegistry::Unknown => Vec::new(),
        LibraryRegistry::Known(libraries) => libraries
            .into_iter()
            .filter(|library| library.membership_pending)
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

#[test]
fn a_requested_cycle_reconciles_server_deletions() {
    let server = serve(vec![
        ok(EMPTY_PAGE),
        ok(EMPTY_PAGE),
        ok(r#"{"Items":[{"Id":"kept"}],"TotalRecordCount":1}"#),
    ]);
    let library = Arc::new(Library::open_in_memory().expect("library"));
    let session = authenticated_session(&library, &server.url);
    library
        .ingest_page(&[
            serde_json::from_str(r#"{"Id":"kept","Name":"Kept","Type":"Movie"}"#).expect("dto"),
            serde_json::from_str(r#"{"Id":"gone","Name":"Gone","Type":"Movie"}"#).expect("dto"),
        ])
        .expect("seed");
    settled_catalog(&library);

    let requested = super::run_cycle(&library, &session, Trigger::Requested).expect("cycle");
    assert_eq!(requested.deleted, 1);
    assert!(library.item("gone").expect("query").is_none());
    assert!(library.item("kept").expect("query").is_some());

    server.requests();
}

#[test]
fn a_failed_library_list_still_fills_a_fresh_catalog() {
    let library = Arc::new(Library::open_in_memory().expect("library"));

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![ok(NOT_JSON), ok(ONE_MOVIE), ok(ONE_MOVIE)],
    );

    result.expect("the catalog synced without the library list");
    assert_eq!(steps(&requests), ["views", "bootstrap", "incremental"]);
    assert!(bootstrap_progress(&library).complete);
    assert!(ownership_available(&library));
    assert_eq!(listed(&library), None);
}

#[test]
fn a_failed_library_list_keeps_the_stored_libraries_and_membership() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
    sync_libraries(&library, &[(MOVIES, "movies", &["m1", "m2"])]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![ok(NOT_JSON), ok(EMPTY_PAGE)],
    );

    result.expect("the catalog synced without the library list");
    assert_eq!(steps(&requests), ["views", "incremental"]);
    assert_eq!(listed(&library), Some(vec![MOVIES.to_string()]));
    assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
    assert!(ownership_available(&library));
}

#[test]
fn a_failed_membership_walk_keeps_its_membership_and_only_it_is_walked_again() {
    let library = settled_library(&[
        ("m1", "Movie"),
        ("m2", "Movie"),
        ("m3", "Movie"),
        ("s1", "Series"),
        ("s2", "Series"),
    ]);
    sync_libraries(
        &library,
        &[
            (MOVIES, "movies", &["m1", "m2"]),
            (SHOWS, "tvshows", &["s1"]),
        ],
    );
    let listing = || views(&[(MOVIES, "movies"), (SHOWS, "tvshows")]);

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![
            listing(),
            ok(EMPTY_PAGE),
            members(&["m1", "m2", "m3", "s1", "s2"], 5),
            ok(NOT_JSON),
            members(&["s1", "s2"], 2),
        ],
    );
    result.expect("the catalog synced without one library's members");
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk movies", "walk shows"]
    );
    assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
    assert_eq!(grid(&library, SHOWS), ["s1", "s2"]);
    assert_eq!(pending(&library), [MOVIES]);
    assert!(ownership_available(&library));

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE), members(&["m1", "m2", "m3"], 3)],
    );
    result.expect("the next cycle");
    assert_eq!(steps(&requests), ["views", "incremental", "walk movies"]);
    assert_eq!(grid(&library, MOVIES), ["m1", "m2", "m3"]);
    assert!(pending(&library).is_empty());

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn a_library_added_by_a_failed_cycle_is_walked_by_the_next() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
    let listing = || views(&[(MOVIES, "movies"), (MUSIC, "music")]);

    let (result, requests) = cycle(&library, Trigger::Scheduled, vec![listing(), ok(NOT_JSON)]);
    assert!(matches!(result, Err(ApiError::Decode(_))), "{result:?}");
    assert_eq!(steps(&requests), ["views", "incremental"]);
    assert_eq!(pending(&library), [MOVIES]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE), members(&["m1", "m2"], 2)],
    );
    result.expect("the next cycle");
    assert_eq!(steps(&requests), ["views", "incremental", "walk movies"]);
    assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
    assert!(pending(&library).is_empty());
}

#[test]
fn a_rate_limit_ends_the_membership_pass_and_a_lost_connection_skips_only_its_library() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie"), ("s1", "Series")]);
    sync_libraries(
        &library,
        &[
            (MOVIES, "movies", &["m1"]),
            (MORE_MOVIES, "movies", &["m2"]),
            (SHOWS, "tvshows", &["s1"]),
        ],
    );
    let listing = || {
        views(&[
            (MOVIES, "movies"),
            (MORE_MOVIES, "movies"),
            (SHOWS, "tvshows"),
        ])
    };
    let limited = || (429, String::new());
    let dropped = || (DROPPED, String::new());

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![
            listing(),
            ok(EMPTY_PAGE),
            members(&["m1", "m2", "s1"], 3),
            limited(),
            limited(),
            limited(),
        ],
    );
    result.expect("the catalog synced without the membership pass");
    assert_eq!(
        steps(&requests),
        [
            "views",
            "incremental",
            "sweep",
            "walk movies",
            "walk movies",
            "walk movies"
        ]
    );
    assert_eq!(pending(&library), [MOVIES, MORE_MOVIES, SHOWS]);
    assert_eq!(grid(&library, MOVIES), ["m1"]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![
            listing(),
            ok(EMPTY_PAGE),
            members(&["m1"], 1),
            dropped(),
            dropped(),
            dropped(),
            members(&["s1"], 1),
        ],
    );
    result.expect("the catalog synced without one library's walk");
    assert_eq!(
        steps(&requests),
        [
            "views",
            "incremental",
            "walk movies",
            "walk more movies",
            "walk more movies",
            "walk more movies",
            "walk shows"
        ]
    );
    assert_eq!(pending(&library), [MORE_MOVIES]);
    assert_eq!(grid(&library, SHOWS), ["s1"]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE), members(&["m2"], 1)],
    );
    result.expect("the cycle that finishes the pass");
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "walk more movies"]
    );
    assert!(pending(&library).is_empty());
    assert_eq!(grid(&library, MORE_MOVIES), ["m2"]);
}

#[test]
fn an_empty_answer_keeps_a_library_s_titles_and_settles_its_walk() {
    let library = settled_library(&[("m1", "Movie")]);
    sync_libraries(&library, &[(MOVIES, "movies", &["m1"])]);
    let listing = || views(&[(MOVIES, "movies")]);

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![
            listing(),
            ok(EMPTY_PAGE),
            members(&["m1"], 1),
            members(&[], 0),
        ],
    );
    result.expect("cycle");
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk movies"]
    );
    assert_eq!(grid(&library, MOVIES), ["m1"]);
    assert!(pending(&library).is_empty());

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn an_owed_sweep_that_lists_nothing_stays_owed_until_its_deletion_runs() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
    sync_libraries(
        &library,
        &[
            (MOVIES, "movies", &["m1"]),
            (MORE_MOVIES, "movies", &["m2"]),
        ],
    );
    let remaining = || views(&[(MOVIES, "movies")]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![remaining(), ok(EMPTY_PAGE), ok(EMPTY_PAGE)],
    );
    let report = result.expect("a cycle whose sweep listed nothing");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 0);
    assert!(library.item("m2").expect("query").is_some());

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![
            remaining(),
            ok(EMPTY_PAGE),
            members(&["m1"], 1),
            members(&["m1"], 1),
        ],
    );
    let report = result.expect("the next cycle");
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk movies"]
    );
    assert_eq!(report.deleted, 1);
    assert!(library.item("m2").expect("query").is_none());

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![remaining(), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn a_server_that_lists_no_library_and_no_title_empties_the_catalog() {
    let library = settled_library(&[("m1", "Movie"), ("s1", "Series")]);
    sync_libraries(
        &library,
        &[(MOVIES, "movies", &["m1"]), (SHOWS, "tvshows", &["s1"])],
    );

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![views(&[]), ok(EMPTY_PAGE), ok(UNCOUNTED)],
    );
    let report = result.expect("a cycle whose sweep answer carries no count");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 0);
    assert_eq!(library.stats().total, 2);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![views(&[]), ok(EMPTY_PAGE), ok(EMPTY_PAGE)],
    );
    let report = result.expect("cycle");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 2);
    assert_eq!(library.stats().total, 0);
    assert_eq!(listed(&library), Some(Vec::new()));

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![views(&[]), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn removing_the_last_catalog_library_empties_the_catalog_while_music_remains() {
    let library = settled_library(&[("m1", "Movie")]);
    let (result, _) = cycle(
        &library,
        Trigger::Scheduled,
        vec![
            views(&[(MOVIES, "movies"), (MUSIC, "music")]),
            ok(EMPTY_PAGE),
            members(&["m1"], 1),
        ],
    );
    result.expect("libraries synced");
    let music_only = || views(&[(MUSIC, "music")]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![music_only(), ok(EMPTY_PAGE), ok(UNCOUNTED)],
    );
    let report = result.expect("a cycle whose sweep answer carries no count");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 0);
    assert!(library.item("m1").expect("query").is_some());

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![music_only(), ok(EMPTY_PAGE), ok(EMPTY_PAGE)],
    );
    let report = result.expect("a cycle whose sweep counts no title");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 1);
    assert_eq!(library.stats().total, 0);
    assert_eq!(listed(&library), Some(vec![MUSIC.to_string()]));

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![music_only(), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn an_empty_identity_answer_deletes_nothing_without_this_cycle_s_library_list() {
    let library = settled_library(&[("m1", "Movie")]);
    let failed_list = || vec![ok(NOT_JSON), ok(EMPTY_PAGE), ok(EMPTY_PAGE)];

    let (result, requests) = cycle(&library, Trigger::Requested, failed_list());
    let report = result.expect("a cycle before the library list is known");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 0);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![views(&[]), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle that stores an empty library list");
    assert_eq!(steps(&requests), ["views", "incremental"]);

    let (result, requests) = cycle(&library, Trigger::Requested, failed_list());
    let report = result.expect("a cycle whose library list failed");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 0);
    assert!(library.item("m1").expect("query").is_some());
}

#[test]
fn a_library_removed_by_a_failed_cycle_is_swept_by_the_next() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
    sync_libraries(
        &library,
        &[
            (MOVIES, "movies", &["m1"]),
            (MORE_MOVIES, "movies", &["m2"]),
        ],
    );
    let remaining = || views(&[(MOVIES, "movies")]);

    let (result, _) = cycle(
        &library,
        Trigger::Scheduled,
        vec![remaining(), ok(NOT_JSON)],
    );
    assert!(matches!(result, Err(ApiError::Decode(_))), "{result:?}");

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![
            remaining(),
            ok(EMPTY_PAGE),
            members(&["m1"], 1),
            members(&["m1"], 1),
        ],
    );
    let report = result.expect("the next cycle");
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk movies"]
    );
    assert_eq!(report.deleted, 1);
    assert!(library.item("m2").expect("query").is_none());

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![remaining(), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn renaming_reordering_or_new_artwork_does_not_walk_membership() {
    let library = settled_library(&[("m1", "Movie"), ("s1", "Series")]);
    sync_libraries(
        &library,
        &[(MOVIES, "movies", &["m1"]), (SHOWS, "tvshows", &["s1"])],
    );
    let relisted = json!({
        "Items": [
            { "Id": SHOWS, "Name": "tvshows", "CollectionType": "tvshows" },
            {
                "Id": MOVIES,
                "Name": "Films",
                "CollectionType": "movies",
                "ImageTags": { "Primary": "new-artwork" },
            },
        ],
        "TotalRecordCount": 2,
    });

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![ok(&relisted.to_string()), ok(EMPTY_PAGE)],
    );

    result.expect("cycle");
    assert_eq!(steps(&requests), ["views", "incremental"]);
    assert_eq!(
        listed(&library),
        Some(vec![SHOWS.to_string(), MOVIES.to_string()])
    );
    assert_eq!(grid(&library, MOVIES), ["m1"]);
}

#[test]
fn the_first_sync_walks_each_library_once() {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    let listing = || views(&[(MOVIES, "movies"), (SHOWS, "tvshows")]);

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![
            listing(),
            members(&["m1"], 1),
            members(&[], 0),
            ok(ONE_MOVIE),
            ok(ONE_MOVIE),
        ],
    );
    result.expect("the first sync");
    assert_eq!(
        steps(&requests),
        [
            "views",
            "walk movies",
            "walk shows",
            "bootstrap",
            "incremental"
        ]
    );
    assert_eq!(grid(&library, MOVIES), ["m1"]);
    assert!(pending(&library).is_empty());

    let (result, requests) = cycle(&library, Trigger::Scheduled, vec![listing(), ok(ONE_MOVIE)]);
    result.expect("the second sync");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn a_membership_walk_that_ends_short_of_its_total_keeps_the_previous_membership() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
    sync_libraries(&library, &[(MOVIES, "movies", &["m1", "m2"])]);
    let listing = || views(&[(MOVIES, "movies")]);
    let first = std::iter::once("m1".to_string())
        .chain((1..1_000).map(|n| format!("x{n:04}")))
        .collect::<Vec<_>>();
    let second = (900..1_400).map(|n| format!("x{n:04}")).collect::<Vec<_>>();
    let page = |ids: &[String]| members(&ids.iter().map(String::as_str).collect::<Vec<_>>(), 1_500);

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![
            listing(),
            ok(EMPTY_PAGE),
            members(&["m1", "m2"], 2),
            page(&first),
            page(&second),
        ],
    );
    result.expect("cycle");
    assert_eq!(
        steps(&requests),
        [
            "views",
            "incremental",
            "sweep",
            "walk movies",
            "walk movies"
        ]
    );
    assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
    assert!(
        requests[3].contains("SortBy=DateCreated%2CSortName"),
        "{}",
        requests[3]
    );

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE), members(&["m1"], 1)],
    );
    result.expect("the next cycle");
    assert_eq!(steps(&requests), ["views", "incremental", "walk movies"]);
    assert_eq!(grid(&library, MOVIES), ["m1"]);
}

#[test]
fn a_rejected_token_stops_the_cycle_at_any_library_step() {
    let library = settled_library(&[("m1", "Movie"), ("s1", "Series")]);
    sync_libraries(
        &library,
        &[(MOVIES, "movies", &["m1"]), (SHOWS, "tvshows", &["s1"])],
    );

    let (result, requests) = cycle(&library, Trigger::Scheduled, vec![(401, String::new())]);
    assert_eq!(result.err(), Some(ApiError::Unauthorized));
    assert_eq!(steps(&requests), ["views"]);

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![
            views(&[(MOVIES, "movies"), (SHOWS, "tvshows")]),
            ok(EMPTY_PAGE),
            members(&["m1", "s1"], 2),
            (401, String::new()),
        ],
    );
    assert_eq!(result.err(), Some(ApiError::Unauthorized));
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk movies"]
    );
}
