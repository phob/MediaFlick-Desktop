use crate::library::browse::{self, ChildPage, ChildrenSource, DetailError, LiveDetail};
use crate::library::libraries::FilterKey;
use crate::library::{DEFAULT_PAGE_SIZE, DetailView};

use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<Handled> {
    let response = match segments {
        ["calendar"] if request.is("GET") => calendar(services, request),
        ["settings", "home"] if request.is("GET") => home_settings(services),
        ["settings", "home"] if request.is("PATCH") => patch_home_settings(services, request),
        ["home", "resume"] if request.is("GET") => home_resume(services),
        ["home"] if request.is("GET") => home(services),
        ["billboard"] if request.is("GET") => billboard(services),
        ["items"] if request.is("GET") => query_items(services, request),
        ["items", "letters"] if request.is("GET") => item_letters(services, request),
        ["genres"] if request.is("GET") => Ok(match services.library.genres(None) {
            Ok(genres) => ApiResponse::ok(json!({ "genres": genres })),
            Err(error) => storage_failure(&error),
        }),
        ["official-ratings"] if request.is("GET") => official_ratings(services, request),
        ["person", "resolve"] if request.is("GET") => resolve_person(services, request),
        ["item", id] if request.is("GET") => item_detail(services, id),
        ["item", id, "synopsis"] if request.is("GET") => item_synopsis(services, id),
        ["item", id, "about"] if request.is("GET") => item_about(services, id),
        ["item", id, "children"] if request.is("GET") => children(services, id, request),
        _ => return None,
    };
    Some(response)
}

fn calendar(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let Some(start) = request.param("start") else {
        return Err(ApiResponse::error(400, "calendar start is required"));
    };
    let Some(end) = request.param("end") else {
        return Err(ApiResponse::error(400, "calendar end is required"));
    };
    if !is_iso_date(&start) || !is_iso_date(&end) || end < start {
        return Err(ApiResponse::error(
            400,
            "calendar dates must be YYYY-MM-DD with end after start",
        ));
    }
    let scope = session_scope(services)?;
    match services.companion.calendar(&start, &end) {
        Ok(value) => Ok(ApiResponse::ok(value)),
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

fn is_iso_date(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

struct ResolvedHome {
    account: AccountKey,
    settings: HomeSettings,
    defaults: HomeSettings,
    genres: HashSet<String>,
    profiles: Vec<crate::collections::CollectionProfile>,
    mediaflick_collections: bool,
}

fn resolved_home(services: &Services) -> Result<ResolvedHome, ApiResponse> {
    let account = services
        .session
        .account_key()
        .ok_or_else(|| ApiResponse::error(401, "sign in to configure Home"))?;
    let genres = services
        .library
        .genres(None)
        .map_err(|error| storage_failure(&error))?;
    let profiles = services.collections.account(&account).profiles;
    let readiness = services.companion.cached_collection_readiness();
    let has_results = crate::collections::snapshots::SnapshotRepository::new(&services.library)
        .has_account_results(&account)
        .unwrap_or(false);
    let mediaflick_collections =
        services
            .collections
            .effective_mode(&account, &readiness, has_results)
            == crate::collections::CollectionMode::MediaFlick;
    let mut settings = services
        .accounts
        .home(&account)
        .unwrap_or_else(|| HomeSettings::fresh(&genres));
    reconcile_home_elements(&mut settings, &genres, &profiles);
    let mut defaults = HomeSettings::fresh(&genres);
    reconcile_home_elements(&mut defaults, &genres, &profiles);
    let mut default_ids = defaults
        .elements
        .iter()
        .map(|element| element.element.clone())
        .collect::<HashSet<_>>();
    for element in &settings.elements {
        if default_ids.insert(element.element.clone()) {
            defaults.elements.push(HomeElement {
                element: element.element.clone(),
                enabled: false,
            });
        }
    }
    Ok(ResolvedHome {
        account,
        settings,
        defaults,
        genres: genres.into_iter().collect(),
        profiles,
        mediaflick_collections,
    })
}

fn reconcile_home_elements(
    settings: &mut HomeSettings,
    genres: &[String],
    profiles: &[crate::collections::CollectionProfile],
) {
    let eligible_collections = profiles
        .iter()
        .filter(|profile| profile.available_on_home)
        .map(|profile| profile.id.as_str())
        .collect::<HashSet<_>>();
    settings.elements.retain(|element| match &element.element {
        HomeElementId::Collection { id } => eligible_collections.contains(id.as_str()),
        _ => true,
    });
    let mut present = settings
        .elements
        .iter()
        .map(|element| element.element.clone())
        .collect::<HashSet<_>>();
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

fn settings_view(home: &ResolvedHome, settings: &HomeSettings) -> Value {
    let profiles = home
        .profiles
        .iter()
        .map(|profile| (profile.id.as_str(), profile.title.as_str()))
        .collect::<HashMap<_, _>>();
    let elements = settings
        .elements
        .iter()
        .map(|element| {
            let (label, available, category) = match &element.element {
                HomeElementId::BuiltIn { id } => {
                    (built_in_label(*id).to_string(), true, "Built-in")
                }
                HomeElementId::Genre { id } => (id.clone(), home.genres.contains(id), "Genre"),
                HomeElementId::Collection { id } => (
                    profiles
                        .get(id.as_str())
                        .copied()
                        .unwrap_or("Collection")
                        .to_string(),
                    home.mediaflick_collections && profiles.contains_key(id.as_str()),
                    "My Collection",
                ),
            };
            let mut value = serde_json::to_value(element).unwrap_or_else(|_| json!({}));
            if let Some(object) = value.as_object_mut() {
                object.insert("label".to_string(), json!(label));
                object.insert("available".to_string(), json!(available));
                object.insert("category".to_string(), json!(category));
            }
            value
        })
        .collect::<Vec<_>>();
    json!({
        "billboard": settings.billboard,
        "watching": settings.watching,
        "elements": elements,
    })
}

fn home_settings_response(home: &ResolvedHome) -> ApiResponse {
    ApiResponse::ok(json!({
        "settings": settings_view(home, &home.settings),
        "defaults": settings_view(home, &home.defaults),
        "collectionMode": if home.mediaflick_collections { "mediaFlick" } else { "jellyfin" },
    }))
}

fn home_settings(services: &Arc<Services>) -> Handled {
    Ok(home_settings_response(&resolved_home(services)?))
}

fn patch_home_settings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let requested = request.body::<HomeSettings>()?;
    if let Err(error) = requested.validate() {
        return Err(ApiResponse::error(400, error.to_string()));
    }
    let current = resolved_home(services)?;
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
    let scope = session_scope(services)?;
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            services
                .accounts
                .save_home(&current.account, &requested)
                .map_err(|error| {
                    ApiResponse::error(500, format!("could not save Home settings: {error}"))
                })
        })?;
    Ok(home_settings_response(&resolved_home(services)?))
}

fn home(services: &Arc<Services>) -> Handled {
    let home = resolved_home(services)?;
    let watching_enabled = element_enabled(&home.settings, HomeBuiltIn::Watching);
    let continue_watching = if watching_enabled && home.settings.watching.continue_watching {
        services
            .library
            .continue_watching(HOME_ROW_LIMIT)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut rows = Vec::new();
    for element in home
        .settings
        .elements
        .iter()
        .filter(|element| element.enabled)
    {
        let row = match &element.element {
            HomeElementId::BuiltIn { id } => built_in_home_row(services, &home.account, *id),
            HomeElementId::Genre { id } if home.genres.contains(id) => Some(home_row(
                "genre",
                id,
                id,
                &home_query(
                    &services.library,
                    ItemQuery {
                        genre: Some(id.clone()),
                        sort: ItemSort::CommunityRating,
                        ..Default::default()
                    },
                ),
            )),
            HomeElementId::Collection { id } if home.mediaflick_collections => {
                home_collection(services, &home, id)
            }
            _ => None,
        };
        if let Some(row) = row
            && row["items"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        {
            rows.push(row);
        }
    }
    Ok(ApiResponse::ok(json!({
        "configuration": settings_view(&home, &home.settings),
        "continueWatching": continue_watching,
        "rows": rows,
    })))
}

fn built_in_home_row(services: &Services, account: &AccountKey, id: HomeBuiltIn) -> Option<Value> {
    let (row_id, title, items) = match id {
        HomeBuiltIn::Watching | HomeBuiltIn::Upcoming => return None,
        HomeBuiltIn::BecauseYouWatched => return because_you_watched(services, account),
        HomeBuiltIn::RecentlyAdded => (
            "recentlyAdded",
            "Recently Added Movies",
            home_query(
                &services.library,
                ItemQuery {
                    kinds: KindFilter::one(CatalogKind::Movie),
                    sort: ItemSort::DateAdded,
                    ..Default::default()
                },
            ),
        ),
        HomeBuiltIn::RecentlyAddedShows => (
            "recentlyAddedShows",
            "Recently Added Shows",
            services
                .library
                .recently_added_episodes(HOME_ROW_LIMIT as usize)
                .unwrap_or_default(),
        ),
        HomeBuiltIn::LatestMovies => (
            "latestMovies",
            "Latest Movies",
            latest_home_items(&services.library, CatalogKind::Movie),
        ),
        HomeBuiltIn::LatestShows => (
            "latestShows",
            "Latest Shows",
            latest_home_items(&services.library, CatalogKind::Series),
        ),
        HomeBuiltIn::MyList => (
            "myList",
            "Favorites",
            home_query(
                &services.library,
                ItemQuery {
                    favorite: Some(true),
                    sort: ItemSort::DateAdded,
                    ..Default::default()
                },
            ),
        ),
    };
    Some(home_row("builtIn", row_id, title, &items))
}

fn element_enabled(settings: &HomeSettings, id: HomeBuiltIn) -> bool {
    settings
        .elements
        .iter()
        .any(|element| element.enabled && element.element == HomeElementId::BuiltIn { id })
}

fn home_query(library: &Library, mut query: ItemQuery) -> Vec<ItemSummary> {
    query.limit = HOME_ROW_LIMIT;
    library.query_page(&query).unwrap_or_default()
}

fn home_row(kind: &str, id: &str, title: &str, items: &[ItemSummary]) -> Value {
    json!({ "kind": kind, "id": id, "title": title, "items": items })
}

fn because_you_watched(services: &Services, account: &AccountKey) -> Option<Value> {
    let seed = {
        let mut seeds = services
            .home_watched
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        seeds
            .entry(account.clone())
            .or_insert_with(|| {
                services
                    .library
                    .random_played_movie_with_genre()
                    .unwrap_or_default()
            })
            .clone()
    }?;
    let genre = seed.genres.first()?;
    let items = home_query(
        &services.library,
        ItemQuery {
            genre: Some(genre.to_string()),
            watched: Some(WatchFilter::Unwatched),
            sort: ItemSort::CommunityRating,
            ..Default::default()
        },
    );
    (!items.is_empty()).then(|| {
        home_row(
            "builtIn",
            "becauseYouWatched",
            &format!("Because you watched {}", seed.summary.name),
            &items,
        )
    })
}

fn home_collection(services: &Services, home: &ResolvedHome, profile_id: &str) -> Option<Value> {
    let profile = home
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id && profile.available_on_home)?;
    let snapshot = crate::collections::snapshots::SnapshotRepository::new(&services.library)
        .profile(&home.account, &profile.id, &profile.revision)
        .ok()??;
    let classified = crate::collections::matching::classify(
        &services.library,
        &home.account,
        &snapshot.items,
        crate::collections::matching::OwnershipPolicy {
            complete_sync: crate::library::sync::ownership_available(&services.library),
            restricted_user: services.session.user_restricted(),
        },
    )
    .ok()?;
    let ids = classified
        .owned
        .iter()
        .filter_map(|title| title.local_items.first())
        .map(|item| item.id.clone())
        .take(HOME_ROW_LIMIT as usize)
        .collect::<Vec<_>>();
    let by_id = services
        .library
        .items_by_ids(&ids)
        .ok()?
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect::<HashMap<_, _>>();
    let items = ids
        .iter()
        .filter_map(|id| by_id.get(id).cloned())
        .collect::<Vec<_>>();
    Some(home_row("collection", &profile.id, &profile.title, &items))
}

fn home_resume(services: &Arc<Services>) -> Handled {
    let home = resolved_home(services)?;
    if !element_enabled(&home.settings, HomeBuiltIn::Watching) {
        return Ok(ApiResponse::ok(
            json!({ "continueWatching": [], "nextUp": [] }),
        ));
    }
    let resume = if home.settings.watching.continue_watching {
        services
            .library
            .continue_watching(HOME_ROW_LIMIT)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let next_up = if home.settings.watching.next_up {
        let fetched = services.session.scope().and_then(|scope| {
            items::fetch_next_up(scope.client(), scope.user_id(), None, HOME_ROW_LIMIT)
                .inspect_err(|error| services.session.note_scoped_error(&scope, error))
        });
        fetched
            .map(|response| {
                response
                    .items
                    .iter()
                    .map(ItemSummary::from_dto)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|error| {
                tracing::debug!(target: "app.api", "Next Up unavailable: {error}");
                Vec::new()
            })
    } else {
        Vec::new()
    };
    Ok(ApiResponse::ok(json!({
        "continueWatching": resume,
        "nextUp": deduplicate_next_up(&resume, next_up),
    })))
}

fn deduplicate_next_up(resume: &[ItemSummary], next_up: Vec<ItemSummary>) -> Vec<ItemSummary> {
    fn keys(item: &ItemSummary) -> impl Iterator<Item = &str> {
        [Some(item.id.as_str()), item.series_id.as_deref()]
            .into_iter()
            .flatten()
    }
    let seen = resume.iter().flat_map(keys).collect::<HashSet<_>>();
    next_up
        .into_iter()
        .filter(|item| keys(item).all(|key| !seen.contains(key)))
        .collect()
}

fn latest_home_items(library: &Library, kind: CatalogKind) -> Vec<ItemSummary> {
    home_query(
        library,
        ItemQuery {
            kinds: KindFilter::one(kind),
            sort: ItemSort::ReleaseDate,
            ..Default::default()
        },
    )
}

fn billboard(services: &Arc<Services>) -> Handled {
    if !resolved_home(services)?.settings.billboard {
        return Ok(ApiResponse::ok(json!({ "items": [] })));
    }
    match services.library.random_billboard_titles(BILLBOARD_LIMIT) {
        Ok(items) => Ok(ApiResponse::ok(json!({ "items": items }))),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn query_items(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    if let Some(person_id) = request.param("personId") {
        return query_person_items(services, &person_id, request);
    }

    let query = item_query(request)?;
    match services.library.query(&query) {
        Ok(page) => Ok(ApiResponse::ok(
            json!({ "items": page.items, "total": page.total }),
        )),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn item_letters(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    match services.library.letter_index(&item_query(request)?) {
        Ok(letters) => Ok(ApiResponse::ok(json!({
            "letters": letters.into_iter().collect::<std::collections::BTreeMap<_, _>>(),
        }))),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn official_ratings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let kinds = request
        .param("kind")
        .map(|value| {
            value
                .split(',')
                .filter_map(|kind| CatalogKind::parse(kind.trim()))
                .filter(|kind| CatalogKind::TOP_LEVEL.contains(kind))
                .collect::<Vec<_>>()
        })
        .filter(|kinds| !kinds.is_empty())
        .unwrap_or_else(|| CatalogKind::TOP_LEVEL.to_vec());
    match services.library.official_ratings(&kinds, None) {
        Ok(ratings) => Ok(ApiResponse::ok(json!({ "officialRatings": ratings }))),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn kind_filter(request: &ApiRequest) -> Result<KindFilter, ApiResponse> {
    let value = request.param("kind").unwrap_or_default();
    let kinds = value
        .split(',')
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .map(|kind| {
            CatalogKind::parse(kind)
                .ok_or_else(|| ApiResponse::error(400, format!("unknown item kind {kind:?}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(KindFilter::from_kinds(kinds))
}

fn item_query(request: &ApiRequest) -> Result<ItemQuery, ApiResponse> {
    Ok(ItemQuery {
        search: request.param("search"),
        kinds: kind_filter(request)?,
        ..library_query(request, Params::Lenient)?
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Params {
    Lenient,
    Strict,
}

pub(super) fn library_query(
    request: &ApiRequest,
    params: Params,
) -> Result<ItemQuery, ApiResponse> {
    let seed = param(request, params, "seed", |value| value.parse::<u32>().ok())?;
    Ok(ItemQuery {
        sort: param(request, params, "sort", |value| {
            ItemSort::from_id(value, seed)
        })?
        .unwrap_or_default(),
        direction: param(request, params, "order", SortDirection::from_id)?,
        genre: request.param(FilterKey::Genre.id()),
        release_decade: param(
            request,
            params,
            FilterKey::Decade.id(),
            crate::library::release_decade_from_id,
        )?,
        watched: param(
            request,
            params,
            FilterKey::Watched.id(),
            WatchFilter::from_id,
        )?,
        favorite: param(
            request,
            params,
            FilterKey::Favorite.id(),
            |value| match value {
                "true" | "1" => Some(true),
                "false" | "0" => Some(false),
                _ => None,
            },
        )?,
        official_rating: request
            .param(FilterKey::Parental.id())
            .map(|value| value.trim().to_string()),
        series_status: param(
            request,
            params,
            FilterKey::SeriesStatus.id(),
            SeriesStatus::from_id,
        )?,
        offset: param(request, params, "offset", |value| value.parse::<i64>().ok())?.unwrap_or(0),
        limit: param(request, params, "limit", |value| value.parse::<i64>().ok())?
            .unwrap_or(DEFAULT_PAGE_SIZE),
        ..ItemQuery::default()
    })
}

pub(super) fn param<T>(
    request: &ApiRequest,
    params: Params,
    key: &str,
    parse: impl FnOnce(&str) -> Option<T>,
) -> Result<Option<T>, ApiResponse> {
    let Some(value) = request.param(key) else {
        return Ok(None);
    };
    match (parse(&value), params) {
        (Some(parsed), _) => Ok(Some(parsed)),
        (None, Params::Lenient) => Ok(None),
        (None, Params::Strict) => Err(ApiResponse::error(400, format!("invalid {key} {value:?}"))),
    }
}

fn query_person_items(services: &Arc<Services>, person_id: &str, request: &ApiRequest) -> Handled {
    let offset = request
        .param("offset")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let limit = request
        .param("limit")
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    match items::fetch_person_items(client, user_id, person_id, offset, limit) {
        Ok(response) => Ok(ApiResponse::ok(json!({
            "items": response.items.iter().map(ItemSummary::from_dto).collect::<Vec<_>>(),
            "total": response.total_record_count,
        }))),
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

fn person_identity(dto: &BaseItemDto, fallback_tmdb_id: Option<i64>) -> Value {
    json!({
        "jellyfinId": dto.id,
        "tmdbId": dto
            .provider_id("Tmdb")
            .and_then(|id| id.parse::<i64>().ok())
            .filter(|id| *id > 0)
            .or(fallback_tmdb_id),
        "name": dto.display_name(),
        "imageTag": dto.primary_image_tag(),
    })
}

/// Bridges Jellyfin and TMDB person namespaces without ever treating a fuzzy
/// name match as identity. A missing provider id may use one unambiguous exact
/// name; a known conflicting id is always excluded.
fn resolve_person(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let jellyfin_id = request.param("jellyfinId");
    let tmdb_id = request
        .param("tmdbId")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0);
    let name = request.param("name").unwrap_or_default();
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());

    if let Some(jellyfin_id) = jellyfin_id {
        return match items::fetch_item(client, user_id, &jellyfin_id) {
            Ok(Some(person)) if person.item_type.as_deref() == Some("Person") => {
                let provider_id = person
                    .provider_id("Tmdb")
                    .and_then(|id| id.parse::<i64>().ok())
                    .filter(|id| *id > 0);
                if tmdb_id.is_some() && provider_id.is_some() && tmdb_id != provider_id {
                    return Err(ApiResponse::error(
                        409,
                        "the Jellyfin and TMDB person ids do not match",
                    ));
                }
                Ok(ApiResponse::ok(json!({
                    "person": person_identity(&person, tmdb_id),
                    "candidates": [],
                    "ambiguous": false,
                })))
            }
            Ok(Some(_)) => Err(ApiResponse::error(409, "that Jellyfin id is not a person")),
            Ok(None) => Err(ApiResponse::error(
                404,
                "the server has no person with that id",
            )),
            Err(error) => Err(scoped_failure(services, &scope, &error)),
        };
    }

    if name.trim().is_empty() {
        return Err(ApiResponse::error(
            400,
            "a person name is required to resolve that deep link",
        ));
    }
    match items::fetch_people(client, user_id, &name) {
        Ok(response) => {
            let mut seen = HashSet::new();
            let exact = response
                .items
                .into_iter()
                // `/Persons` is already type-scoped. Some older servers omit
                // `Type` in this lightweight response, so require its stable id
                // and exact name rather than rejecting a valid candidate.
                .filter(|person| !person.id.trim().is_empty())
                .filter(|person| person.display_name().eq_ignore_ascii_case(name.trim()))
                .filter(|person| seen.insert(person.id.clone()))
                .filter(|person| {
                    tmdb_id.is_none_or(|id| {
                        person
                            .provider_id("Tmdb")
                            .and_then(|value| value.parse::<i64>().ok())
                            .filter(|value| *value > 0)
                            .is_none_or(|provider_id| provider_id == id)
                    })
                })
                .collect::<Vec<_>>();
            if exact.len() == 1 {
                return Ok(ApiResponse::ok(json!({
                    "person": person_identity(&exact[0], tmdb_id),
                    "candidates": [],
                    "ambiguous": false,
                })));
            }
            let candidates = exact
                .iter()
                .map(|person| person_identity(person, tmdb_id))
                .collect::<Vec<_>>();
            Ok(ApiResponse::ok(json!({
                "person": Value::Null,
                "ambiguous": candidates.len() > 1,
                "candidates": candidates,
            })))
        }
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

fn item_detail(services: &Arc<Services>, item_id: &str) -> Handled {
    let cached = browse::cached_title(&services.library, item_id)
        .map_err(|error| storage_failure(&error))?;
    if let Some(title) = cached {
        return Ok(ApiResponse::ok(DetailView::Title(title)));
    }
    let scope = session_scope(services)?;
    let dto = match items::fetch_detailed_item(scope.client(), scope.user_id(), item_id) {
        Ok(Some(dto)) => dto,
        Ok(None) => return Err(server_lacks_item(services, &scope, item_id)),
        Err(error) => {
            if matches!(error, ApiError::Status { status: 404 }) {
                forget_item(services, &scope, item_id);
            }
            return Err(scoped_failure(services, &scope, &error));
        }
    };
    let title = match browse::live_detail(&dto) {
        LiveDetail::Generic(generic) => {
            still_signed_in(services, &scope)?;
            return Ok(ApiResponse::ok(DetailView::Generic(*generic)));
        }
        LiveDetail::Title(title) => services.session.commit_if_current(
            &scope,
            || DetailError::AccountChanged,
            || browse::write_live_title(&services.library, title),
        ),
    };
    match title {
        Ok(title) => {
            tracing::debug!(target: "library.browse", item_id, "served a title the cache did not hold");
            Ok(ApiResponse::ok(DetailView::Title(title)))
        }
        Err(DetailError::AccountChanged) => Err(stale_account_response()),
        Err(DetailError::Storage(error)) => Err(storage_failure(&error)),
    }
}

pub(super) fn still_signed_in(
    services: &Services,
    scope: &SessionScope,
) -> Result<(), ApiResponse> {
    if services.session.scope_is_current(scope) {
        Ok(())
    } else {
        Err(stale_account_response())
    }
}

fn server_lacks_item(services: &Arc<Services>, scope: &SessionScope, item_id: &str) -> ApiResponse {
    forget_item(services, scope, item_id);
    ApiResponse::error(404, "the server has no item with that id")
}

fn item_synopsis(services: &Arc<Services>, item_id: &str) -> Handled {
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    match items::fetch_item_synopsis(client, user_id, item_id) {
        Ok(Some(dto)) => Ok(ApiResponse::ok(json!({ "overview": dto.overview }))),
        Ok(None) => Err(server_lacks_item(services, &scope, item_id)),
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

const ITEM_ABOUT_CAST_LIMIT: usize = 24;
const ITEM_ABOUT_CREW_LIMIT: usize = 24;
const ITEM_ABOUT_CREW_PER_JOB_LIMIT: usize = 6;

fn is_cast_credit(person: &BaseItemPerson) -> bool {
    matches!(person.person_type.as_deref(), Some("Actor" | "GuestStar"))
        || (person.person_type.is_none()
            && person
                .role
                .as_deref()
                .is_some_and(|role| !role.trim().is_empty()))
}

fn bounded_about_people(people: &[BaseItemPerson]) -> Vec<Value> {
    let mut selected = Vec::with_capacity(ITEM_ABOUT_CAST_LIMIT + ITEM_ABOUT_CREW_LIMIT);
    let mut cast = 0;
    let mut crew = 0;
    let mut crew_by_job = HashMap::<String, usize>::new();

    for person in people {
        if person
            .name
            .as_deref()
            .is_none_or(|name| name.trim().is_empty())
        {
            continue;
        }
        if is_cast_credit(person) {
            if cast >= ITEM_ABOUT_CAST_LIMIT {
                continue;
            }
            cast += 1;
        } else {
            let Some(job) = person
                .person_type
                .as_deref()
                .filter(|job| !job.trim().is_empty())
            else {
                continue;
            };
            let job_count = crew_by_job.entry(job.to_string()).or_default();
            if crew >= ITEM_ABOUT_CREW_LIMIT || *job_count >= ITEM_ABOUT_CREW_PER_JOB_LIMIT {
                continue;
            }
            crew += 1;
            *job_count += 1;
        }
        selected.push(json!({
            "id": person.id,
            "name": person.name,
            "role": person.role,
            "type": person.person_type,
            "imageTag": person.primary_image_tag,
        }));
    }
    selected
}

fn item_about(services: &Arc<Services>, item_id: &str) -> Handled {
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    match items::fetch_item_about(client, user_id, item_id) {
        Ok(Some(dto)) => {
            let people = bounded_about_people(&dto.people);
            let studios = dto
                .studios
                .iter()
                .filter_map(|studio| studio.name.clone())
                .collect::<Vec<_>>();
            Ok(ApiResponse::ok(json!({
                "overview": dto.overview,
                "criticRating": dto.critic_rating,
                "people": people,
                "tags": dto.tags,
                "studios": studios,
            })))
        }
        Ok(None) => Err(server_lacks_item(services, &scope, item_id)),
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

fn children(services: &Arc<Services>, item_id: &str, request: &ApiRequest) -> Handled {
    let parse = |value: &str| value.parse::<i64>().ok();
    let page = ChildPage::new(
        param(request, Params::Strict, "offset", parse)?,
        param(request, Params::Strict, "limit", parse)?,
    )
    .map_err(|message| ApiResponse::error(400, message))?;
    let cached = services
        .library
        .catalog_kind(item_id)
        .map_err(|error| storage_failure(&error))?;
    let page = match cached {
        Some(kind) => {
            let (library, session, shell) = (&services.library, &services.session, &services.shell);
            let children = sync::title_children(library, session, shell, item_id, kind)
                .map_err(|error| storage_failure(&error))?;
            browse::catalog_children_page(children, page)
        }
        None => live_children(services, item_id, page)?,
    };
    Ok(ApiResponse::ok(
        json!({ "items": page.items, "total": page.total }),
    ))
}

fn live_children(
    services: &Arc<Services>,
    item_id: &str,
    page: Option<ChildPage>,
) -> Result<crate::library::ItemPage, ApiResponse> {
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    let fail = |error: ApiError| scoped_failure(services, &scope, &error);
    let paged = page.is_some();
    let page = page.unwrap_or_default();
    let (offset, limit) = (page.offset(), page.limit());
    let source = match services.children_sources.get(scope.generation(), item_id) {
        Some(source) => source,
        None => {
            let item_type = items::fetch_item_type(client, user_id, item_id)
                .map_err(fail)?
                .ok_or_else(|| ApiResponse::error(404, "the server has no item with that id"))?;
            let source = ChildrenSource::of(&item_type);
            services
                .children_sources
                .remember(scope.generation(), item_id, source);
            source
        }
    };
    let children = match source {
        ChildrenSource::Folder => {
            items::fetch_live_children(client, user_id, item_id, offset, limit)
                .map(|response| browse::live_page(&response))
        }
        ChildrenSource::Playlist => items::fetch_playlist_items(client, user_id, item_id, page)
            .map(|response| browse::playlist_page(&response, page)),
    }
    .map_err(fail)?;
    still_signed_in(services, &scope)?;
    tracing::debug!(target: "library.browse", item_id, paged, ?source, "fetched a container's children from the server");
    Ok(children)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_dates_are_strict_iso_days() {
        assert!(is_iso_date("2026-08-02"));
        assert!(!is_iso_date("2026-8-2"));
        assert!(!is_iso_date("2026/08/02"));
        assert!(!is_iso_date("../../etc"));
    }

    #[test]
    fn latest_home_rows_are_kind_scoped_and_ordered_by_release_year() {
        let library = Library::open_in_memory().expect("library");
        let items: Vec<BaseItemDto> = [
            r#"{"Id":"old-movie","Name":"Old Film","Type":"Movie","ProductionYear":1999}"#,
            r#"{"Id":"new-movie","Name":"New Film","Type":"Movie","ProductionYear":2026}"#,
            r#"{"Id":"old-show","Name":"Old Show","Type":"Series","ProductionYear":2010}"#,
            r#"{"Id":"new-show","Name":"New Show","Type":"Series","ProductionYear":2025}"#,
        ]
        .into_iter()
        .map(|value| serde_json::from_str(value).expect("dto"))
        .collect();
        library.upsert_page(&items).expect("seed");

        let movies = latest_home_items(&library, CatalogKind::Movie);
        let shows = latest_home_items(&library, CatalogKind::Series);

        assert_eq!(movies[0].id, "new-movie");
        assert_eq!(movies[1].id, "old-movie");
        assert!(movies.iter().all(|item| item.kind == "Movie"));
        assert_eq!(shows[0].id, "new-show");
        assert_eq!(shows[1].id, "old-show");
        assert!(shows.iter().all(|item| item.kind == "Series"));
    }

    fn episode(id: &str, series_id: &str) -> ItemSummary {
        ItemSummary {
            id: id.to_string(),
            kind: "Episode".to_string(),
            series_id: Some(series_id.to_string()),
            ..ItemSummary::default()
        }
    }

    #[test]
    fn a_show_already_being_watched_is_removed_from_next_up() {
        let next_up = deduplicate_next_up(
            &[episode("e1", "sev")],
            vec![
                episode("e1", "sev"),
                episode("e2", "sev"),
                episode("e9", "silo"),
            ],
        );
        let ids = next_up
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["e9"]);
    }
}
