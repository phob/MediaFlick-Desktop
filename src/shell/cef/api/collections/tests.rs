use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use serde_json::json;

use super::super::tests::{read_request_with_headers, respond};
use super::*;
use crate::app::services::ShellRequest;
use crate::app::services::test_support::TestServices;
use crate::collections::snapshots::SnapshotRepository;
use crate::collections::{
    CollectionProfile, CollectionSource, MediaType, RefreshCadence, ResultLimit, TemplateReference,
};
use crate::jellyfin::session::Live;
use crate::preferences::backup_path;

#[derive(Clone)]
struct CompanionBehavior {
    info_status: &'static str,
    info_body: String,
    franchises_status: &'static str,
    franchises_body: String,
    results_status: &'static str,
    results_body: String,
}

struct ResultsGate {
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

struct LoopbackCompanion {
    addr: SocketAddr,
    url: String,
    behavior: Arc<Mutex<CompanionBehavior>>,
    results_gate: Arc<Mutex<Option<Arc<ResultsGate>>>>,
    identities_hits: Arc<AtomicUsize>,
    franchises_hits: Arc<AtomicUsize>,
    results_hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

impl LoopbackCompanion {
    fn start(behavior: CompanionBehavior) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = Self {
            addr,
            url: format!("http://{addr}"),
            behavior: Arc::new(Mutex::new(behavior)),
            results_gate: Arc::new(Mutex::new(None)),
            identities_hits: Arc::default(),
            franchises_hits: Arc::default(),
            results_hits: Arc::default(),
            stop: Arc::default(),
        };
        let behavior = server.behavior.clone();
        let results_gate = server.results_gate.clone();
        let identities_hits = server.identities_hits.clone();
        let franchises_hits = server.franchises_hits.clone();
        let results_hits = server.results_hits.clone();
        let stop = server.stop.clone();
        std::thread::spawn(move || {
            for incoming in listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = incoming else {
                    break;
                };
                let (request_line, _) = read_request_with_headers(&mut stream);
                let path = request_line.split_whitespace().nth(1).unwrap_or("");
                let snapshot = behavior.lock().expect("behavior").clone();
                let (status, response_body) = if path.starts_with("/MediaFlick/info") {
                    (snapshot.info_status, snapshot.info_body)
                } else if path.starts_with("/MediaFlick/collection-experience/v1/identities") {
                    identities_hits.fetch_add(1, Ordering::SeqCst);
                    ("200 OK", r#"{"mappings":[]}"#.to_string())
                } else if path.starts_with("/MediaFlick/collection-experience/v1/franchises") {
                    franchises_hits.fetch_add(1, Ordering::SeqCst);
                    (snapshot.franchises_status, snapshot.franchises_body)
                } else if path.starts_with("/MediaFlick/collection-experience/v1/results") {
                    results_hits.fetch_add(1, Ordering::SeqCst);
                    let gate = results_gate.lock().expect("gate").take();
                    if let Some(gate) = gate {
                        let _ = gate.entered.send(());
                        let _ = gate.release.lock().expect("release").recv();
                    }
                    (snapshot.results_status, snapshot.results_body)
                } else {
                    ("404 Not Found", "{}".to_string())
                };
                respond(&mut stream, status, &response_body);
            }
        });
        server
    }

    fn pause_next_results(&self) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        *self.results_gate.lock().expect("gate") = Some(Arc::new(ResultsGate {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        }));
        (entered_rx, release_tx)
    }

    fn update(&self, f: impl FnOnce(&mut CompanionBehavior)) {
        let mut guard = self.behavior.lock().expect("behavior");
        f(&mut guard);
    }

    fn identities_hits(&self) -> usize {
        self.identities_hits.load(Ordering::SeqCst)
    }

    fn franchises_hits(&self) -> usize {
        self.franchises_hits.load(Ordering::SeqCst)
    }

    fn results_hits(&self) -> usize {
        self.results_hits.load(Ordering::SeqCst)
    }
}

impl Drop for LoopbackCompanion {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
    }
}

fn info_json(capabilities: &[&str]) -> String {
    json!({
        "pluginVersion": "1.0.0",
        "apiVersion": 1,
        "capabilities": capabilities,
        "services": {
            "tmdb": true,
            "mdblist": true,
        }
    })
    .to_string()
}

fn franchise_success_json() -> String {
    json!({
        "memberships": [
            { "tmdbId": 272, "collectionId": 263 },
            { "tmdbId": 155, "collectionId": 263 }
        ],
        "franchises": [{
            "collectionId": 263,
            "name": "The Dark Knight Collection",
            "posterPath": null,
            "backdropPath": null,
            "committedAt": 100,
            "items": [
                {
                    "mediaType": "movie",
                    "tmdbId": 272,
                    "title": "Batman Begins",
                    "originalTitle": null,
                    "year": 2005,
                    "overview": "",
                    "releaseDate": "2005-06-10",
                    "sourceOrder": 0,
                    "posterPath": null,
                    "backdropPath": null,
                    "adult": false
                },
                {
                    "mediaType": "movie",
                    "tmdbId": 155,
                    "title": "The Dark Knight",
                    "originalTitle": null,
                    "year": 2008,
                    "overview": "",
                    "releaseDate": "2008-07-16",
                    "sourceOrder": 1,
                    "posterPath": null,
                    "backdropPath": null,
                    "adult": false
                }
            ]
        }]
    })
    .to_string()
}

fn profile_results_json() -> String {
    json!({
        "items": [{
            "mediaType": "movie",
            "tmdbId": 272,
            "title": "Batman Begins",
            "originalTitle": null,
            "year": 2005,
            "overview": "",
            "releaseDate": "2005-06-10",
            "sourceOrder": 0,
            "posterPath": null,
            "backdropPath": null,
            "adult": false
        }],
        "total": 1,
        "movies": 1,
        "series": 0,
        "sourceIdentity": "12345"
    })
    .to_string()
}

fn get_franchises_request() -> ApiRequest {
    ApiRequest {
        method: "GET".to_string(),
        path: "/api/collections/franchises".to_string(),
        query: "localDate=2026-08-30".to_string(),
        body: RawBody::new(Vec::new()),
        range: None,
        cancelled: Arc::new(AtomicBool::new(false)),
    }
}

fn seed_library_and_profile(fixture: &TestServices) -> (AccountKey, CollectionProfile) {
    let live = Live::for_test();
    let movies: Vec<BaseItemDto> = serde_json::from_value(json!([
        {
            "Id": "0a1b2c3d4e5f60718293a4b5c6d7e8f1",
            "Name": "Batman Begins",
            "Type": "Movie",
            "ProviderIds": { "Tmdb": "272" }
        },
        {
            "Id": "0a1b2c3d4e5f60718293a4b5c6d7e8f4",
            "Name": "The Dark Knight",
            "Type": "Movie",
            "ProviderIds": { "Tmdb": "155" }
        },
        {
            "Id": "0a1b2c3d4e5f60718293a4b5c6d7e8f2",
            "Name": "Unresolved Movie",
            "Type": "Movie",
            "ProviderIds": { "Imdb": "tt0468569" }
        }
    ]))
    .expect("movies");
    fixture
        .services
        .library
        .ingest_page(&live, &movies)
        .expect("ingest movies");
    fixture
        .services
        .library
        .set_meta(&live, "sync.bootstrap_done", "1")
        .expect("bootstrap done");
    let account = fixture.services.session.account_key().expect("account");
    let profile = CollectionProfile {
        id: "a".repeat(16),
        revision: "b".repeat(16),
        template: TemplateReference {
            id: "mdblist-public-list".to_string(),
        },
        title: "Daily MDBList".to_string(),
        description: String::new(),
        custom_poster_id: None,
        source: CollectionSource::MdbListPublicList {
            list_id: "12345".to_string(),
        },
        media_type: MediaType::Movie,
        limit: ResultLimit::All,
        cadence: RefreshCadence::Daily,
        available_on_home: false,
    };
    fixture
        .services
        .collections
        .save_profile(&account, profile.clone())
        .expect("save profile");
    (account, profile)
}

fn drain_collections_changed(receiver: &mpsc::Receiver<ShellRequest>) -> usize {
    let mut count = 0;
    while let Ok(event) = receiver.try_recv() {
        if matches!(event, ShellRequest::CollectionsChanged) {
            count += 1;
        }
    }
    count
}

fn wait_for_collections_changed(
    receiver: &mpsc::Receiver<ShellRequest>,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
        match receiver.recv_timeout(remaining) {
            Ok(ShellRequest::CollectionsChanged) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

fn franchises_status(response: &browse::FranchiseCollectionsIndex) -> String {
    serde_json::to_value(response).expect("json")["status"]
        .as_str()
        .expect("status string")
        .to_string()
}

fn draft_request(method: &str, path: &str, title: &str, limit: &serde_json::Value) -> ApiRequest {
    let body = serde_json::to_vec(&json!({
        "template": { "id": "mdblist-public-list" },
        "title": title,
        "description": "",
        "customPosterId": null,
        "source": { "kind": "mdbListPublicList", "listId": "12345" },
        "mediaType": "movie",
        "limit": limit,
        "cadence": "daily",
        "availableOnHome": false
    }))
    .expect("draft json");
    ApiRequest {
        method: method.to_string(),
        path: path.to_string(),
        query: String::new(),
        body: RawBody::new(body),
        range: None,
        cancelled: Arc::new(AtomicBool::new(false)),
    }
}

#[test]
fn failing_franchise_refresh_backs_off_and_breaks_the_collections_changed_loop() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1"]),
        franchises_status: "500 Internal Server Error",
        franchises_body: "{}".to_string(),
        results_status: "503 Service Unavailable",
        results_body: "{}".to_string(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-loop");
    let (account, profile) = seed_library_and_profile(&fixture);
    let shell_rx = fixture.services.shell.subscribe();

    fixture
        .services
        .companion
        .probe(true)
        .expect("initial probe");
    drain_collections_changed(&shell_rx);

    let first = browse::franchises(&fixture.services, &get_franchises_request()).expect("get");
    assert_eq!(franchises_status(&first), "updating");
    assert!(
        wait_for_collections_changed(&shell_rx, Duration::from_secs(5)),
        "first failure must notify the UI to leave updating"
    );
    let identities_after_first = server.identities_hits();
    let results_after_first = server.results_hits();
    assert_eq!(identities_after_first, 1);
    assert_eq!(results_after_first, 1);

    let second =
        browse::franchises(&fixture.services, &get_franchises_request()).expect("second get");
    assert_eq!(franchises_status(&second), "resultsUnavailable");
    assert!(
        !wait_for_collections_changed(&shell_rx, Duration::from_millis(200)),
        "franchises() within backoff must not re-trigger refresh or CollectionsChanged"
    );
    assert_eq!(server.identities_hits(), identities_after_first);
    assert_eq!(server.results_hits(), results_after_first);

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(drain_collections_changed(&shell_rx), 0);
    assert_eq!(server.identities_hits(), identities_after_first);
    assert_eq!(server.results_hits(), results_after_first);

    fixture
        .services
        .companion
        .probe(true)
        .expect("unchanged reprobe");
    assert_eq!(drain_collections_changed(&shell_rx), 0);
    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(server.identities_hits(), identities_after_first);
    assert_eq!(server.results_hits(), results_after_first);

    let repository = SnapshotRepository::new(&fixture.services.library);
    let aged = crate::library::now_unix().saturating_sub(600);
    let mut franchise_state = repository.franchise_refresh_state(&account).expect("state");
    franchise_state.last_attempt = Some(aged);
    repository
        .save_refresh_state(&account, "movie-franchises", &franchise_state)
        .expect("age franchise state");
    let mut profile_state = repository
        .refresh_state(&account, &profile.id)
        .expect("profile state");
    profile_state.last_attempt = Some(aged);
    repository
        .save_refresh_state(&account, &profile.id, &profile_state)
        .expect("age profile state");

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(
        drain_collections_changed(&shell_rx),
        0,
        "repeated identical failures after backoff must not re-emit CollectionsChanged"
    );
    assert_eq!(server.identities_hits(), identities_after_first + 1);
    assert_eq!(server.results_hits(), results_after_first + 1);
}

#[test]
fn capability_gain_and_reconnect_clear_backoff_and_manual_profile_refresh_bypasses_it() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1"]),
        franchises_status: "500 Internal Server Error",
        franchises_body: "{}".to_string(),
        results_status: "503 Service Unavailable",
        results_body: "{}".to_string(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-cap");
    let (account, profile) = seed_library_and_profile(&fixture);
    let shell_rx = fixture.services.shell.subscribe();

    fixture
        .services
        .companion
        .probe(true)
        .expect("initial probe");
    drain_collections_changed(&shell_rx);

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(drain_collections_changed(&shell_rx), 1);
    assert_eq!(server.results_hits(), 1);

    server.update(|behavior| {
        behavior.results_status = "200 OK";
        behavior.results_body = profile_results_json();
    });
    let manual = profiles::refresh(&fixture.services, &profile.id).expect("manual refresh");
    assert_eq!(serde_json::to_value(&manual).expect("json")["total"], 1);
    assert_eq!(server.results_hits(), 2);

    server.update(|behavior| {
        behavior.info_body = info_json(&["collection-experience-v1", "franchise-memberships-v1"]);
        behavior.franchises_status = "200 OK";
        behavior.franchises_body = franchise_success_json();
    });
    fixture
        .services
        .companion
        .probe(true)
        .expect("reprobe with capability");
    assert_eq!(
        drain_collections_changed(&shell_rx),
        1,
        "gaining franchise-memberships-v1 must notify collections changed"
    );

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(drain_collections_changed(&shell_rx), 1);
    assert_eq!(server.franchises_hits(), 1);
    let ready = browse::franchises(&fixture.services, &get_franchises_request()).expect("ready");
    assert_eq!(franchises_status(&ready), "ready");
    assert_eq!(
        serde_json::to_value(&ready).expect("json")["franchises"]
            .as_array()
            .expect("array")
            .len(),
        1
    );

    let repository = SnapshotRepository::new(&fixture.services.library);
    assert!(
        repository
            .franchise_refresh_state(&account)
            .expect("state")
            .initialized
    );
}

#[test]
fn companion_reconnect_after_outage_clears_failure_backoff() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1", "franchise-memberships-v1"]),
        franchises_status: "503 Service Unavailable",
        franchises_body: "{}".to_string(),
        results_status: "200 OK",
        results_body: profile_results_json(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-reconnect");
    let (_account, _profile) = seed_library_and_profile(&fixture);
    let shell_rx = fixture.services.shell.subscribe();

    fixture
        .services
        .companion
        .probe(true)
        .expect("initial probe");
    drain_collections_changed(&shell_rx);

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(server.franchises_hits(), 1);
    drain_collections_changed(&shell_rx);

    server.update(|behavior| {
        behavior.info_status = "503 Service Unavailable";
        behavior.info_body = "{}".to_string();
    });
    assert!(fixture.services.companion.probe(true).is_err());
    assert_eq!(drain_collections_changed(&shell_rx), 0);

    server.update(|behavior| {
        behavior.info_status = "200 OK";
        behavior.info_body = info_json(&["collection-experience-v1", "franchise-memberships-v1"]);
        behavior.franchises_status = "200 OK";
        behavior.franchises_body = franchise_success_json();
    });
    fixture
        .services
        .companion
        .probe(true)
        .expect("reconnect probe");
    assert_eq!(drain_collections_changed(&shell_rx), 1);

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(server.franchises_hits(), 2);
    let ready = browse::franchises(&fixture.services, &get_franchises_request()).expect("ready");
    assert_eq!(franchises_status(&ready), "ready");
}

#[test]
fn failure_backoff_is_isolated_per_account() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1", "franchise-memberships-v1"]),
        franchises_status: "200 OK",
        franchises_body: franchise_success_json(),
        results_status: "200 OK",
        results_body: profile_results_json(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-isolated");
    let (_account_a, _profile) = seed_library_and_profile(&fixture);
    let other = AccountKey::new("server", "other-user").expect("other");
    let repository = SnapshotRepository::new(&fixture.services.library);
    repository
        .save_franchise_refresh_failure(&other, crate::library::now_unix())
        .expect("other account failure");

    fixture.services.companion.probe(true).expect("probe");
    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(server.franchises_hits(), 1);
    let ready = browse::franchises(&fixture.services, &get_franchises_request()).expect("ready");
    assert_eq!(franchises_status(&ready), "ready");

    let other_state = repository
        .franchise_refresh_state(&other)
        .expect("other state");
    assert!(other_state.last_attempt.is_some());
    assert!(other_state.latest_failure.is_some());
    assert!(!other_state.initialized);
}

#[test]
fn already_initialized_franchise_failure_does_not_emit_collections_changed() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1", "franchise-memberships-v1"]),
        franchises_status: "200 OK",
        franchises_body: franchise_success_json(),
        results_status: "200 OK",
        results_body: profile_results_json(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-initialized");
    let (_account, _profile) = seed_library_and_profile(&fixture);
    let shell_rx = fixture.services.shell.subscribe();

    fixture.services.companion.probe(true).expect("probe");
    drain_collections_changed(&shell_rx);

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(drain_collections_changed(&shell_rx), 1);

    let live = Live::for_test();
    let new_movie: Vec<BaseItemDto> = serde_json::from_value(json!([{
        "Id": "0a1b2c3d4e5f60718293a4b5c6d7e8f3",
        "Name": "The Dark Knight Rises",
        "Type": "Movie",
        "ProviderIds": { "Tmdb": "49026" }
    }]))
    .expect("new movie");
    fixture
        .services
        .library
        .ingest_page(&live, &new_movie)
        .expect("ingest new movie");

    server.update(|behavior| {
        behavior.franchises_status = "503 Service Unavailable";
        behavior.franchises_body = "{}".to_string();
    });
    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(
        drain_collections_changed(&shell_rx),
        0,
        "failing an incremental franchise refresh when already initialized leaves UI status Ready"
    );
    assert_eq!(server.franchises_hits(), 2);

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(
        server.franchises_hits(),
        2,
        "incremental franchise failure must also back off"
    );

    profiles::settings(&fixture.services, true).expect("explicit reprobe");
    server.update(|behavior| {
        behavior.franchises_status = "200 OK";
        behavior.franchises_body = json!({
            "memberships": [{ "tmdbId": 49026, "collectionId": 263 }],
            "franchises": [{
                "collectionId": 263,
                "name": "The Dark Knight Collection",
                "posterPath": null,
                "backdropPath": null,
                "committedAt": 200,
                "items": [
                    {
                        "mediaType": "movie",
                        "tmdbId": 272,
                        "title": "Batman Begins",
                        "originalTitle": null,
                        "year": 2005,
                        "overview": "",
                        "releaseDate": "2005-06-10",
                        "sourceOrder": 0,
                        "posterPath": null,
                        "backdropPath": null,
                        "adult": false
                    },
                    {
                        "mediaType": "movie",
                        "tmdbId": 49026,
                        "title": "The Dark Knight Rises",
                        "originalTitle": null,
                        "year": 2012,
                        "overview": "",
                        "releaseDate": "2012-07-17",
                        "sourceOrder": 1,
                        "posterPath": null,
                        "backdropPath": null,
                        "adult": false
                    }
                ]
            }]
        })
        .to_string();
    });
    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(server.franchises_hits(), 3);
    assert_eq!(drain_collections_changed(&shell_rx), 1);
}

#[test]
fn editing_a_profile_deleted_during_refresh_returns_404_without_resurrection_or_rewrite() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1"]),
        franchises_status: "200 OK",
        franchises_body: franchise_success_json(),
        results_status: "200 OK",
        results_body: profile_results_json(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-edit-race");
    let (account, existing) = seed_library_and_profile(&fixture);
    fixture.services.companion.probe(true).expect("probe");

    let (entered_rx, release_tx) = server.pause_next_results();
    let services = fixture.services.clone();
    let profile_id = existing.id.clone();
    let edit_thread = std::thread::spawn(move || {
        let request = draft_request(
            "PATCH",
            &format!("/api/collections/profiles/{profile_id}"),
            "Edited Daily MDBList",
            &json!({ "kind": "maximum", "count": 25 }),
        );
        super::super::dispatch(&request, || Ok(services.clone()))
    });

    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("edit refresh must reach companion");

    profiles::delete(&fixture.services, &existing.id).expect("delete while edit is in flight");
    let collections_path = fixture.collections_path();
    let backup = backup_path(&collections_path);
    let after_delete = std::fs::read(&collections_path).expect("collections after delete");
    let _ = std::fs::remove_file(&backup);

    release_tx.send(()).expect("release companion response");
    let response = edit_thread.join().expect("edit thread");

    assert_eq!(response.status, 404);
    assert!(
        fixture
            .services
            .collections
            .account(&account)
            .profiles
            .is_empty()
    );
    assert_eq!(
        std::fs::read(&collections_path).expect("collections after failed edit"),
        after_delete
    );
    assert!(!backup.exists());

    let repository = SnapshotRepository::new(&fixture.services.library);
    assert!(!repository.has_account_results(&account).expect("results"));
    let state = repository
        .refresh_state(&account, &existing.id)
        .expect("refresh state");
    assert!(!state.initialized);
    assert!(state.last_attempt.is_none());
}

#[test]
fn refreshing_a_profile_deleted_during_refresh_returns_404_without_orphan_cache() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1"]),
        franchises_status: "200 OK",
        franchises_body: franchise_success_json(),
        results_status: "200 OK",
        results_body: profile_results_json(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-refresh-race");
    let (account, existing) = seed_library_and_profile(&fixture);
    fixture.services.companion.probe(true).expect("probe");

    let (entered_rx, release_tx) = server.pause_next_results();
    let services = fixture.services.clone();
    let profile_id = existing.id.clone();
    let refresh_thread = std::thread::spawn(move || profiles::refresh(&services, &profile_id));

    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("manual refresh must reach companion");

    profiles::delete(&fixture.services, &existing.id).expect("delete while refresh is in flight");
    release_tx.send(()).expect("release companion response");

    let response = refresh_thread
        .join()
        .expect("refresh thread")
        .err()
        .expect("refresh of deleted profile must fail");
    assert_eq!(response.status, 404);

    let repository = SnapshotRepository::new(&fixture.services.library);
    assert!(!repository.has_account_results(&account).expect("results"));
    let state = repository
        .refresh_state(&account, &existing.id)
        .expect("refresh state");
    assert!(!state.initialized);
    assert!(state.last_attempt.is_none());
}

#[test]
fn creating_a_new_profile_still_inserts_into_collections() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1"]),
        franchises_status: "200 OK",
        franchises_body: franchise_success_json(),
        results_status: "200 OK",
        results_body: profile_results_json(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-create");
    let (account, existing) = seed_library_and_profile(&fixture);
    profiles::delete(&fixture.services, &existing.id).expect("clear seed profile");
    fixture.services.companion.probe(true).expect("probe");

    let request = draft_request(
        "POST",
        "/api/collections/profiles",
        "Fresh Collection",
        &json!({ "kind": "all" }),
    );
    let response = super::super::dispatch(&request, || Ok(fixture.services.clone()));
    assert_eq!(response.status, 200);
    let json: serde_json::Value = serde_json::from_slice(&response.body).expect("json");
    assert_eq!(json["total"], 1);
    assert_eq!(json["profile"]["title"], "Fresh Collection");

    let stored = fixture.services.collections.account(&account).profiles;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].title, "Fresh Collection");
}

#[test]
fn failing_refresh_for_profile_deleted_in_flight_leaves_no_orphan_refresh_state() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1"]),
        franchises_status: "200 OK",
        franchises_body: franchise_success_json(),
        results_status: "503 Service Unavailable",
        results_body: "{}".to_string(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-failed-refresh-race");
    let (account, existing) = seed_library_and_profile(&fixture);
    fixture.services.companion.probe(true).expect("probe");

    let (entered_rx, release_tx) = server.pause_next_results();
    let services = fixture.services.clone();
    let profile_id = existing.id.clone();
    let edit_thread = std::thread::spawn(move || {
        let request = draft_request(
            "PATCH",
            &format!("/api/collections/profiles/{profile_id}"),
            "Edited Daily MDBList",
            &json!({ "kind": "maximum", "count": 25 }),
        );
        super::super::dispatch(&request, || Ok(services.clone()))
    });

    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("edit refresh must reach companion");

    profiles::delete(&fixture.services, &existing.id).expect("delete while refresh is failing");
    release_tx.send(()).expect("release companion response");

    let response = edit_thread.join().expect("edit thread");
    assert_ne!(response.status, 200);
    assert!(
        fixture
            .services
            .collections
            .account(&account)
            .profiles
            .is_empty()
    );

    let repository = SnapshotRepository::new(&fixture.services.library);
    assert!(!repository.has_account_results(&account).expect("results"));
    let state = repository
        .refresh_state(&account, &existing.id)
        .expect("refresh state");
    assert!(!state.initialized);
    assert!(state.last_attempt.is_none());
    assert!(state.latest_failure.is_none());
}

#[test]
fn editing_to_duplicate_title_rolls_back_staged_snapshot_revision() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1"]),
        franchises_status: "200 OK",
        franchises_body: franchise_success_json(),
        results_status: "200 OK",
        results_body: profile_results_json(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-dup-title");
    let (account, existing) = seed_library_and_profile(&fixture);
    let second = CollectionProfile {
        id: "c".repeat(16),
        title: "Other Collection".to_string(),
        ..existing.clone()
    };
    fixture
        .services
        .collections
        .save_profile(&account, second)
        .expect("save second profile");
    fixture.services.companion.probe(true).expect("probe");

    let request = draft_request(
        "PATCH",
        &format!("/api/collections/profiles/{}", existing.id),
        "Other Collection",
        &json!({ "kind": "maximum", "count": 25 }),
    );
    let response = super::super::dispatch(&request, || Ok(fixture.services.clone()));
    assert_eq!(response.status, 400);

    let repository = SnapshotRepository::new(&fixture.services.library);
    assert!(!repository.has_account_results(&account).expect("results"));
    let stored = fixture.services.collections.account(&account).profiles;
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].title, "Daily MDBList");
    assert_eq!(stored[0].revision, existing.revision);
}
