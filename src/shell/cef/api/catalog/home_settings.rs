use crate::collections::CollectionMode;
use crate::library::Facet;
use crate::library::libraries::LibraryRegistry;
use crate::preferences::HomeWatchingSettings;

use super::*;

pub(super) struct ResolvedHome {
    pub(super) account: AccountKey,
    pub(super) settings: HomeSettings,
    defaults: HomeSettings,
    pub(super) genres: HashSet<String>,
    pub(super) profiles: Vec<crate::collections::CollectionProfile>,
    libraries: LibraryRegistry,
    pub(super) mediaflick_collections: bool,
}

pub(super) fn resolved_home(services: &Services) -> Result<ResolvedHome, ApiResponse> {
    let account = services
        .session
        .account_key()
        .ok_or_else(|| ApiResponse::error(401, "sign in to configure Home"))?;
    resolved_home_for_account(services, &account)
}

fn resolved_home_for_account(
    services: &Services,
    account: &AccountKey,
) -> Result<ResolvedHome, ApiResponse> {
    let genres = services
        .library
        .facet_values(Facet::Genre, None)
        .map_err(|error| storage_failure(&error))?;
    let profiles = services.collections.account(account).profiles;
    let libraries = services
        .library
        .libraries()
        .map_err(|error| storage_failure(&error))?;
    let readiness = services.companion.cached_collection_readiness();
    let has_results = crate::collections::snapshots::SnapshotRepository::new(&services.library)
        .has_account_results(account)
        .unwrap_or(false);
    let mediaflick_collections =
        services
            .collections
            .effective_mode(account, &readiness, has_results)
            == CollectionMode::MediaFlick;
    let mut settings = services
        .accounts
        .home(account)
        .unwrap_or_else(|| HomeSettings::fresh(&genres));
    reconcile_home_elements(&mut settings, &genres, &profiles, &libraries);
    let defaults = home_defaults(&settings, &genres, &profiles, &libraries);
    Ok(ResolvedHome {
        account: account.clone(),
        settings,
        defaults,
        genres: genres.into_iter().collect(),
        profiles,
        libraries,
        mediaflick_collections,
    })
}

fn home_defaults(
    settings: &HomeSettings,
    genres: &[String],
    profiles: &[crate::collections::CollectionProfile],
    libraries: &LibraryRegistry,
) -> HomeSettings {
    let mut defaults = HomeSettings::fresh(genres);
    reconcile_home_elements(&mut defaults, genres, profiles, libraries);
    let mut default_ids = defaults
        .elements
        .iter()
        .map(|element| element.element.clone())
        .collect::<HashSet<_>>();
    let mut position = library_block_end(&defaults.elements);
    for element in &settings.elements {
        if !default_ids.insert(element.element.clone()) {
            continue;
        }
        if matches!(element.element, HomeElementId::Library { .. }) {
            defaults.elements.insert(
                position,
                HomeElement {
                    element: element.element.clone(),
                    enabled: true,
                },
            );
            position += 1;
        } else {
            defaults.elements.push(HomeElement {
                element: element.element.clone(),
                enabled: false,
            });
        }
    }
    defaults
}

fn library_block_end(elements: &[HomeElement]) -> usize {
    let is_library =
        |element: &HomeElement| matches!(element.element, HomeElementId::Library { .. });
    let is_built_in =
        |element: &HomeElement| matches!(element.element, HomeElementId::BuiltIn { .. });
    elements
        .iter()
        .rposition(is_library)
        .or_else(|| elements.iter().rposition(is_built_in))
        .map_or(0, |index| index + 1)
}

fn reconcile_home_elements(
    settings: &mut HomeSettings,
    genres: &[String],
    profiles: &[crate::collections::CollectionProfile],
    libraries: &LibraryRegistry,
) {
    let eligible_collections = profiles
        .iter()
        .filter(|profile| profile.available_on_home)
        .map(|profile| profile.id.as_str())
        .collect::<HashSet<_>>();
    let known = match libraries {
        LibraryRegistry::Known(libraries) => Some(libraries.as_slice()),
        LibraryRegistry::Unknown => None,
    };
    settings.elements.retain(|element| match &element.element {
        HomeElementId::Collection { id } => eligible_collections.contains(id.as_str()),
        HomeElementId::Library { id } => {
            known.is_none_or(|libraries| libraries.iter().any(|library| &library.id == id))
        }
        _ => true,
    });
    let mut present = settings
        .elements
        .iter()
        .map(|element| element.element.clone())
        .collect::<HashSet<_>>();
    let mut position = library_block_end(&settings.elements);
    for library in known.unwrap_or_default() {
        let element = HomeElementId::Library {
            id: library.id.clone(),
        };
        if present.insert(element.clone()) {
            settings.elements.insert(
                position,
                HomeElement {
                    element,
                    enabled: true,
                },
            );
            position += 1;
        }
    }
    for genre in genres {
        let element = HomeElementId::Genre { id: genre.clone() };
        if present.insert(element.clone()) {
            settings.elements.push(HomeElement {
                element,
                enabled: false,
            });
        }
    }
    for profile in profiles.iter().filter(|profile| profile.available_on_home) {
        let element = HomeElementId::Collection {
            id: profile.id.clone(),
        };
        if present.insert(element.clone()) {
            settings.elements.push(HomeElement {
                element,
                enabled: false,
            });
        }
    }
}

fn built_in_label(id: HomeBuiltIn) -> &'static str {
    match id {
        HomeBuiltIn::Watching => "Watching",
        HomeBuiltIn::BecauseYouWatched => "Because You Watched",
        HomeBuiltIn::RecentlyAdded => "Recently Added Movies",
        HomeBuiltIn::RecentlyAddedShows => "Recently Added Shows",
        HomeBuiltIn::Upcoming => "Releases",
        HomeBuiltIn::LatestMovies => "Latest Movies",
        HomeBuiltIn::LatestShows => "Latest Shows",
        HomeBuiltIn::MyList => "Favorites",
    }
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct HomeConfiguration {
    billboard: bool,
    watching: HomeWatchingSettings,
    elements: Vec<HomeElementView>,
}

#[derive(Serialize, ts_rs::TS)]
struct HomeElementView {
    #[serde(flatten)]
    element: HomeElement,
    label: String,
    available: bool,
    category: HomeElementCategory,
}

#[derive(Serialize, ts_rs::TS)]
enum HomeElementCategory {
    #[serde(rename = "Built-in")]
    BuiltIn,
    Genre,
    #[serde(rename = "My Collection")]
    MyCollection,
    Library,
}

pub(super) fn settings_view(home: &ResolvedHome, settings: &HomeSettings) -> HomeConfiguration {
    let profiles = home
        .profiles
        .iter()
        .map(|profile| (profile.id.as_str(), profile.title.as_str()))
        .collect::<HashMap<_, _>>();
    let libraries = match &home.libraries {
        LibraryRegistry::Known(libraries) => libraries
            .iter()
            .map(|library| (&library.id, library.name.as_str()))
            .collect::<HashMap<_, _>>(),
        LibraryRegistry::Unknown => HashMap::new(),
    };
    let elements = settings
        .elements
        .iter()
        .map(|element| {
            let (label, available, category) = match &element.element {
                HomeElementId::BuiltIn { id } => (
                    built_in_label(*id).to_string(),
                    true,
                    HomeElementCategory::BuiltIn,
                ),
                HomeElementId::Genre { id } => (
                    id.clone(),
                    home.genres.contains(id),
                    HomeElementCategory::Genre,
                ),
                HomeElementId::Collection { id } => (
                    profiles
                        .get(id.as_str())
                        .copied()
                        .unwrap_or("Collection")
                        .to_string(),
                    home.mediaflick_collections && profiles.contains_key(id.as_str()),
                    HomeElementCategory::MyCollection,
                ),
                HomeElementId::Library { id } => match libraries.get(id) {
                    Some(name) => (
                        format!("Latest in {name}"),
                        true,
                        HomeElementCategory::Library,
                    ),
                    None => (
                        "Latest in library".to_string(),
                        false,
                        HomeElementCategory::Library,
                    ),
                },
            };
            HomeElementView {
                element: element.clone(),
                label,
                available,
                category,
            }
        })
        .collect();
    HomeConfiguration {
        billboard: settings.billboard,
        watching: settings.watching,
        elements,
    }
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct HomeSettingsResponse {
    settings: HomeConfiguration,
    defaults: HomeConfiguration,
    collection_mode: CollectionMode,
}

fn home_settings_response(home: &ResolvedHome) -> HomeSettingsResponse {
    HomeSettingsResponse {
        settings: settings_view(home, &home.settings),
        defaults: settings_view(home, &home.defaults),
        collection_mode: if home.mediaflick_collections {
            CollectionMode::MediaFlick
        } else {
            CollectionMode::Jellyfin
        },
    }
}

fn home_scope(services: &Services, request: &ApiRequest) -> Result<SessionScope, ApiResponse> {
    expected_scope(services, request, || {
        ApiResponse::error(401, "sign in to configure Home")
    })
}

pub(super) fn home_settings(
    services: &Arc<Services>,
    request: &ApiRequest,
) -> Result<HomeSettingsResponse, ApiResponse> {
    let scope = home_scope(services, request)?;
    Ok(home_settings_response(&resolved_home_for_account(
        services,
        scope.account(),
    )?))
}

pub(super) fn patch_home_settings(
    services: &Arc<Services>,
    request: &ApiRequest,
    body: Body<'_, HomeSettings>,
) -> Result<HomeSettingsResponse, ApiResponse> {
    let requested = body.decode()?;
    if let Err(error) = requested.validate() {
        return Err(ApiResponse::error(400, error.to_string()));
    }
    let scope = home_scope(services, request)?;
    let current = resolved_home_for_account(services, scope.account())?;
    let requested_ids = requested
        .elements
        .iter()
        .map(|element| &element.element)
        .collect::<HashSet<_>>();
    let current_ids = current
        .settings
        .elements
        .iter()
        .map(|element| &element.element)
        .collect::<HashSet<_>>();
    if requested_ids != current_ids || requested.elements.len() != current.settings.elements.len() {
        return Err(ApiResponse::error(
            409,
            "Home options changed while the page was open",
        ));
    }
    services
        .session
        .commit_if_current(&scope, stale_account_response, |live| {
            services
                .accounts
                .save_home(live.account(), &requested)
                .map_err(|error| {
                    ApiResponse::error(500, format!("could not save Home settings: {error}"))
                })
        })?;
    Ok(home_settings_response(&resolved_home_for_account(
        services,
        scope.account(),
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jellyfin::api::model::UserViewDto;
    use crate::library::libraries::{LibraryId, LibraryRegistry, MediaLibrary};

    const MOVIES: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHOWS: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const MUSIC: &str = "cccccccccccccccccccccccccccccccc";
    const GONE: &str = "dddddddddddddddddddddddddddddddd";

    fn known(views: &[(&str, &str)]) -> LibraryRegistry {
        LibraryRegistry::Known(
            views
                .iter()
                .filter_map(|(id, collection_type)| {
                    MediaLibrary::from_view(&UserViewDto {
                        id: (*id).into(),
                        collection_type: Some((*collection_type).into()),
                        ..Default::default()
                    })
                })
                .collect(),
        )
    }

    fn server_libraries() -> LibraryRegistry {
        known(&[(MOVIES, "movies"), (SHOWS, "tvshows"), (MUSIC, "music")])
    }

    fn library(id: &str, enabled: bool) -> HomeElement {
        HomeElement {
            element: HomeElementId::Library {
                id: LibraryId::canonical(id).expect("test id"),
            },
            enabled,
        }
    }

    fn genre(id: &str, enabled: bool) -> HomeElement {
        HomeElement {
            element: HomeElementId::Genre { id: id.into() },
            enabled,
        }
    }

    fn built_ins() -> Vec<HomeElement> {
        HomeSettings::fresh(&[]).elements
    }

    fn with_elements(elements: Vec<HomeElement>) -> HomeSettings {
        HomeSettings {
            elements,
            ..HomeSettings::fresh(&[])
        }
    }

    #[test]
    fn known_libraries_join_after_the_built_ins_in_server_order_and_enabled() {
        let mut settings = HomeSettings::fresh(&["Action".to_owned()]);
        reconcile_home_elements(
            &mut settings,
            &["Action".to_owned()],
            &[],
            &server_libraries(),
        );

        let mut expected = built_ins();
        expected.extend([
            library(MOVIES, true),
            library(SHOWS, true),
            library(MUSIC, true),
            genre("Action", true),
        ]);
        assert_eq!(settings.elements, expected);
    }

    #[test]
    fn new_libraries_follow_the_last_saved_library_and_gone_ones_leave() {
        let mut elements = built_ins();
        elements.extend([
            genre("Action", true),
            library(SHOWS, false),
            library(GONE, true),
            genre("Drama", false),
        ]);
        let mut settings = with_elements(elements);
        let genres = ["Action".to_owned(), "Drama".to_owned()];
        reconcile_home_elements(&mut settings, &genres, &[], &server_libraries());

        let mut expected = built_ins();
        expected.extend([
            genre("Action", true),
            library(SHOWS, false),
            library(MOVIES, true),
            library(MUSIC, true),
            genre("Drama", false),
        ]);
        assert_eq!(settings.elements, expected);
    }

    #[test]
    fn an_unknown_registry_keeps_saved_libraries_and_adds_none() {
        let mut elements = built_ins();
        elements.extend([
            genre("Action", true),
            library(SHOWS, false),
            library(GONE, true),
        ]);
        let mut settings = with_elements(elements.clone());
        reconcile_home_elements(
            &mut settings,
            &["Action".to_owned()],
            &[],
            &LibraryRegistry::Unknown,
        );
        assert_eq!(settings.elements, elements);
    }

    #[test]
    fn defaults_enable_every_library_and_keep_saved_ones_the_registry_lacks() {
        let genres = ["Action".to_owned()];
        let saved = HomeSettings::fresh(&genres);
        let defaults = home_defaults(&saved, &genres, &[], &server_libraries());
        let mut expected = built_ins();
        expected.extend([
            library(MOVIES, true),
            library(SHOWS, true),
            library(MUSIC, true),
            genre("Action", true),
        ]);
        assert_eq!(defaults.elements, expected);

        let mut elements = built_ins();
        elements.extend([
            library(SHOWS, false),
            genre("Action", false),
            library(MOVIES, true),
            genre("Gone", true),
        ]);
        let defaults = home_defaults(
            &with_elements(elements),
            &genres,
            &[],
            &LibraryRegistry::Unknown,
        );
        let mut expected = built_ins();
        expected.extend([
            library(SHOWS, true),
            library(MOVIES, true),
            genre("Action", true),
            genre("Gone", false),
        ]);
        assert_eq!(defaults.elements, expected);
    }

    #[test]
    fn patch_home_settings_checks_session_scope_before_resolving_home() {
        use crate::app::services::test_support::TestServices;
        use crate::shell::cef::api::tests::{post, send};

        let fixture = TestServices::signed_in("http://127.0.0.1:8096");
        let scope = fixture.services.session.scope().expect("scope");
        fixture
            .services
            .session
            .note_scoped_error(&scope, &ApiError::Unauthorized);

        let mismatched = HomeSettings::fresh(&["Action".to_owned()]);
        let body = serde_json::to_vec(&mismatched).expect("body");
        let request = ApiRequest {
            method: "PATCH".to_string(),
            ..post("/api/settings/home", &body)
        };
        let response = send(&fixture, &request);
        assert_eq!(response.status, 401);
    }

    #[test]
    fn home_settings_requests_carrying_previous_account_reject_with_409_after_account_switch() {
        use std::net::TcpListener;

        use serde_json::Value;

        use crate::app::services::test_support::TestServices;
        use crate::shell::cef::api::tests::{login_as, post, send, server_url};

        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let url = server_url(&listener);
        let fixture = TestServices::signed_in_as(&url, "alice");
        let bob = AccountKey::new("server", "bob").expect("bob");

        let mut bob_home = HomeSettings::fresh(&[]);
        bob_home.billboard = false;
        fixture
            .services
            .accounts
            .save_home(&bob, &bob_home)
            .expect("save bob home");

        login_as(&fixture, listener, &url, "bob", "Bob");
        let expected = crate::app::urls::encode_path_segment(&format!("{url}:alice"));
        let query = format!("account={expected}");

        let alice_write = HomeSettings::fresh(&[]);
        let body = serde_json::to_vec(&alice_write).expect("body");
        let patch = ApiRequest {
            method: "PATCH".to_string(),
            query: query.clone(),
            ..post("/api/settings/home", &body)
        };
        let patch_resp = send(&fixture, &patch);
        assert_eq!(patch_resp.status, 409);
        let patch_json: Value = serde_json::from_slice(&patch_resp.body).expect("json");
        assert_eq!(patch_json["accountChanged"], true);

        let get = ApiRequest {
            method: "GET".to_string(),
            query,
            ..post("/api/settings/home", b"")
        };
        let get_resp = send(&fixture, &get);
        assert_eq!(get_resp.status, 409);
        let get_json: Value = serde_json::from_slice(&get_resp.body).expect("json");
        assert_eq!(get_json["accountChanged"], true);

        assert_eq!(fixture.services.accounts.home(&bob), Some(bob_home));

        fixture
            .services
            .session
            .clear_local(false)
            .expect("sign out");
        let signed_out_patch = send(&fixture, &patch);
        assert_eq!(signed_out_patch.status, 409);
        let signed_out_patch_json: Value =
            serde_json::from_slice(&signed_out_patch.body).expect("json");
        assert_eq!(signed_out_patch_json["accountChanged"], true);

        let signed_out_get = send(&fixture, &get);
        assert_eq!(signed_out_get.status, 409);
        let signed_out_get_json: Value =
            serde_json::from_slice(&signed_out_get.body).expect("json");
        assert_eq!(signed_out_get_json["accountChanged"], true);
    }
}
