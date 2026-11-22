use crate::collections::{CollectionProfile, ProviderResult};

use super::*;

mod browse;
mod profiles;

pub(super) use browse::{
    CollectionProfileDetail, CollectionTitle, FranchiseCollection, FranchiseCollectionsIndex,
    MovieCollection,
};
pub(super) use profiles::{
    CollectionProfilesIndex, CollectionSettings, CollectionTemplates, CommittedProfile,
    ProfileDeleted, ProfileDraft, ProfileEdit, ReorderBody, SettingsPatch,
};

routes! {
    |services, request|
    GET ["collections", "settings"] -> CollectionSettings = profiles::settings(services, false),
    POST ["collections", "settings", "reprobe"] -> CollectionSettings =
        profiles::settings(services, true),
    PATCH ["collections", "settings"] (body: SettingsPatch) -> CollectionSettings =
        profiles::patch_settings(services, body),
    GET ["collections", "templates"] -> CollectionTemplates = profiles::templates(services),
    POST ["collections", "mdblist", "search"] (body: PublicListSearchBody) -> Relay =
        search_public_lists(services, body),
    POST ["collections", "mdblist", "validate"] (body: PublicListValidateBody) -> Relay =
        validate_public_list(services, body),
    GET ["collections", "provider-artwork"] -> Binary = provider_artwork(services, request),
    POST ["collections", "artwork"] (body: Bytes) -> UploadedArtwork =
        upload_artwork(services, body),
    GET ["collections", "artwork", artwork_id] -> Binary =
        custom_artwork(services, &percent_decode(artwork_id)),
    POST ["collections", "preview"] (body: ProfileDraft) -> ProviderResult =
        profiles::preview(services, body),
    GET ["collections", "profiles"] -> CollectionProfilesIndex = profiles::list(services),
    POST ["collections", "profiles"] (body: ProfileDraft) -> CommittedProfile =
        profiles::create(services, body),
    PUT ["collections", "profiles", "order"] (body: ReorderBody) -> CollectionProfilesIndex =
        profiles::reorder(services, body),
    GET ["collections", "profiles", profile_id] -> CollectionProfile =
        profiles::read(services, &percent_decode(profile_id)),
    PATCH ["collections", "profiles", profile_id] (body: ProfileDraft) -> ProfileEdit =
        profiles::edit(services, &percent_decode(profile_id), body),
    DELETE ["collections", "profiles", profile_id] -> ProfileDeleted =
        profiles::delete(services, &percent_decode(profile_id)),
    POST ["collections", "profiles", profile_id, "refresh"] -> CommittedProfile =
        profiles::refresh(services, &percent_decode(profile_id)),
    GET ["collections"] -> CollectionSettings = browse::redirect_state(services),
    GET ["collections", "mine"] -> CollectionProfilesIndex = browse::mine(services),
    GET ["collections", "mine", profile_id] -> CollectionProfileDetail =
        browse::profile_detail(services, &percent_decode(profile_id)),
    GET ["collections", "franchises"] -> FranchiseCollectionsIndex =
        browse::franchises(services, request),
    GET ["collections", "franchises", collection_id] -> FranchiseCollection =
        browse::franchise_detail(services, &percent_decode(collection_id), request),
    GET ["collections", "jellyfin"] -> JellyfinCollections = jellyfin_index(services),
    GET ["collections", "jellyfin", boxset_id] -> JellyfinCollectionDetail =
        jellyfin_detail(services, &percent_decode(boxset_id)),
    DELETE ["collections", "local-account"] (body: ConfirmationBody) -> AppStatus =
        delete_local_account(services, body),
    GET ["collections", "movie", tmdb_id] -> MovieCollection =
        browse::movie_franchise(services, &percent_decode(tmdb_id)),
    GET ["collections", "title", media_type, tmdb_id] -> CollectionTitle =
        browse::title(services, &percent_decode(media_type), &percent_decode(tmdb_id)),
}

fn active_scope(services: &Arc<Services>) -> Result<SessionScope, ApiResponse> {
    services
        .session
        .scope()
        .map_err(|_| ApiResponse::error(401, "sign in to use collections"))
}

fn active_account(services: &Arc<Services>) -> Result<AccountKey, ApiResponse> {
    active_scope(services).map(|scope| scope.account().clone())
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct JellyfinCollections {
    collections: Vec<JellyfinCollectionSummary>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct JellyfinCollectionSummary {
    id: String,
    name: String,
    primary_image_tag: Option<String>,
    backdrop_image_tag: Option<String>,
    item_count: Option<i64>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct JellyfinCollectionDetail {
    id: String,
    name: String,
    primary_image_tag: Option<String>,
    backdrop_image_tag: Option<String>,
    items: Vec<ItemSummary>,
    total_record_count: i64,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct UploadedArtwork {
    id: String,
}

fn jellyfin_index(services: &Arc<Services>) -> Result<JellyfinCollections, ApiResponse> {
    let scope = session_scope(services)?;
    let response = fetch_current(services, &scope, items::fetch_box_sets)?;
    Ok(JellyfinCollections {
        collections: response
            .items
            .iter()
            .map(|item| JellyfinCollectionSummary {
                id: item.id.clone(),
                name: item.display_name().to_string(),
                primary_image_tag: item.primary_image_tag().map(str::to_string),
                backdrop_image_tag: item.backdrop_image_tags.first().cloned(),
                item_count: item.child_count,
            })
            .collect(),
    })
}

fn jellyfin_detail(
    services: &Arc<Services>,
    id: &str,
) -> Result<JellyfinCollectionDetail, ApiResponse> {
    if id.is_empty() {
        return Err(ApiResponse::error(
            400,
            "that is not a Jellyfin collection id",
        ));
    }
    let scope = session_scope(services)?;
    let Some(set) = fetch_current(services, &scope, |client, user_id| {
        items::fetch_item(client, user_id, id)
    })?
    else {
        return Err(ApiResponse::error(
            404,
            "that Jellyfin collection does not exist",
        ));
    };
    let children = fetch_current(services, &scope, |client, user_id| {
        items::fetch_box_set_children(client, user_id, id)
    })?;
    Ok(JellyfinCollectionDetail {
        name: set.display_name().to_string(),
        primary_image_tag: set.primary_image_tag().map(str::to_string),
        backdrop_image_tag: set.backdrop_image_tags.first().cloned(),
        items: children.items.iter().map(ItemSummary::from_dto).collect(),
        total_record_count: children.total_or_received(),
        id: set.id,
    })
}

fn configuration_failure(error: &std::io::Error) -> ApiResponse {
    tracing::warn!(target: "collections", "collection configuration failed: {error}");
    let status = match error.kind() {
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput => 400,
        std::io::ErrorKind::Unsupported => 409,
        _ => 500,
    };
    ApiResponse::error(status, error.to_string())
}

fn upload_artwork(
    services: &Arc<Services>,
    body: Body<'_, Bytes>,
) -> Result<UploadedArtwork, ApiResponse> {
    let scope = active_scope(services)?;
    match services.artwork.stage(body.bytes()) {
        Ok(id) => {
            match services
                .session
                .commit_if_current(&scope, stale_account_response, |_live| {
                    Ok(UploadedArtwork { id: id.clone() })
                }) {
                Ok(uploaded) => Ok(uploaded),
                Err(response) => {
                    if let Err(error) = services.artwork.remove(&id) {
                        tracing::warn!(target: "collections", "could not remove stale custom artwork: {error}");
                    }
                    Err(response)
                }
            }
        }
        Err(error) => Err(configuration_failure(&error)),
    }
}

fn custom_artwork(services: &Arc<Services>, id: &str) -> Result<BinaryAnswer, ApiResponse> {
    let account = active_account(services)?;
    if !services
        .collections
        .artwork_ids_for_account(&account)
        .iter()
        .any(|candidate| candidate == id)
    {
        return Err(ApiResponse::error(404, "that custom poster does not exist"));
    }
    let Some(path) = services.artwork.path(id) else {
        return Err(ApiResponse::error(404, "that custom poster does not exist"));
    };
    let content_type = match path.extension().and_then(|value| value.to_str()) {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => return Err(ApiResponse::error(404, "that custom poster does not exist")),
    };
    match std::fs::read(path) {
        Ok(bytes) => Ok(BinaryAnswer::bytes(
            content_type.to_string(),
            bytes,
            IMMUTABLE_CACHE,
        )),
        Err(_) => Err(ApiResponse::error(404, "that custom poster does not exist")),
    }
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct PublicListSearchBody {
    #[serde(default)]
    query: String,
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct PublicListValidateBody {
    #[serde(default)]
    selector: String,
}

#[derive(Deserialize, ts_rs::TS)]
#[ts(rename = "LocalAccountDeletionBody")]
pub(super) struct ConfirmationBody {
    #[serde(default)]
    confirmed: bool,
}

fn search_public_lists(
    services: &Arc<Services>,
    body: Body<'_, PublicListSearchBody>,
) -> Result<Value, ApiResponse> {
    let body = body.decode()?;
    let scope = active_scope(services)?;
    let query = body.query.trim();
    if query.is_empty() {
        return Err(ApiResponse::error(400, "enter a public-list search"));
    }
    services
        .companion
        .search_public_lists(&scope, query)
        .map_err(|error| ApiResponse::from_api_error(&error))
}

fn validate_public_list(
    services: &Arc<Services>,
    body: Body<'_, PublicListValidateBody>,
) -> Result<Value, ApiResponse> {
    let body = body.decode()?;
    let scope = active_scope(services)?;
    let selector = body.selector.trim();
    if selector.is_empty() {
        return Err(ApiResponse::error(
            400,
            "enter a public-list ID or canonical URL",
        ));
    }
    services
        .companion
        .validate_public_list(&scope, selector)
        .map_err(|error| ApiResponse::from_api_error(&error))
}

fn provider_artwork(
    services: &Arc<Services>,
    request: &ApiRequest,
) -> Result<BinaryAnswer, ApiResponse> {
    let scope = active_scope(services)?;
    let Some(size) = request.param("size") else {
        return Err(ApiResponse::error(400, "artwork size is required"));
    };
    let Some(path) = request.param("path") else {
        return Err(ApiResponse::error(400, "artwork path is required"));
    };
    match services.companion.collection_artwork(&scope, &size, &path) {
        Ok((bytes, content_type)) if content_type.starts_with("image/") => {
            Ok(BinaryAnswer::bytes(content_type, bytes, IMMUTABLE_CACHE))
        }
        Ok(_) => Err(ApiResponse::error(502, "provider returned invalid artwork")),
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn delete_local_account(
    services: &Arc<Services>,
    body: Body<'_, ConfirmationBody>,
) -> Result<AppStatus, ApiResponse> {
    let confirmed = body.decode()?.confirmed;
    if !confirmed {
        return Err(ApiResponse::error(
            400,
            "confirm the local account deletion first",
        ));
    }
    match crate::app::services::delete_local_account_data(services) {
        Ok(()) => Ok(app_status(services)),
        Err(error) => Err(ApiResponse::error(500, error)),
    }
}

#[cfg(test)]
mod tests {
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
        CollectionProfile, CollectionSource, MediaType, RefreshCadence, ResultLimit,
        TemplateReference,
    };
    use crate::jellyfin::session::Live;

    #[derive(Clone)]
    struct CompanionBehavior {
        info_status: &'static str,
        info_body: String,
        franchises_status: &'static str,
        franchises_body: String,
        results_status: &'static str,
        results_body: String,
    }

    struct LoopbackCompanion {
        addr: SocketAddr,
        url: String,
        behavior: Arc<Mutex<CompanionBehavior>>,
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
                identities_hits: Arc::default(),
                franchises_hits: Arc::default(),
                results_hits: Arc::default(),
                stop: Arc::default(),
            };
            let behavior = server.behavior.clone();
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
                        (snapshot.results_status, snapshot.results_body)
                    } else {
                        ("404 Not Found", "{}".to_string())
                    };
                    respond(&mut stream, status, &response_body);
                }
            });
            server
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
            behavior.info_body =
                info_json(&["collection-experience-v1", "franchise-memberships-v1"]);
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
        let ready =
            browse::franchises(&fixture.services, &get_franchises_request()).expect("ready");
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
            behavior.info_body =
                info_json(&["collection-experience-v1", "franchise-memberships-v1"]);
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
        let ready =
            browse::franchises(&fixture.services, &get_franchises_request()).expect("ready");
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
        let ready =
            browse::franchises(&fixture.services, &get_franchises_request()).expect("ready");
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
}
