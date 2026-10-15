use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::library::libraries::FilterKey;

use super::*;

const DEFAULT_PAGE_LIMIT: i64 = 60;

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
        ["item", id] if request.is("GET") => item_detail(services, &percent_decode(id)),
        ["item", id, "synopsis"] if request.is("GET") => {
            item_synopsis(services, &percent_decode(id))
        }
        ["item", id, "about"] if request.is("GET") => item_about(services, &percent_decode(id)),
        ["item", id, "children"] if request.is("GET") => children(services, &percent_decode(id)),
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

    let query = library_query(request)?;
    match services.library.query(&query) {
        Ok(page) => Ok(ApiResponse::ok(
            json!({ "items": page.items, "total": page.total }),
        )),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn item_letters(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    match services.library.letter_index(&library_query(request)?) {
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

/// A `kind` list names catalog kinds only; anything else is a 400, so no
/// route builds an empty kind list. No `kind` means movies and series.
fn kind_filter(request: &ApiRequest) -> Result<KindFilter, ApiResponse> {
    let Some(value) = request.param("kind") else {
        return Ok(KindFilter::TopLevel);
    };
    let kinds = value
        .split(',')
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .map(|kind| {
            CatalogKind::parse(kind)
                .ok_or_else(|| ApiResponse::error(400, format!("unknown item kind {kind:?}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(match kinds.split_first() {
        None => KindFilter::TopLevel,
        Some((first, rest)) => KindFilter::Only {
            first: *first,
            rest: rest.to_vec(),
        },
    })
}

fn library_query(request: &ApiRequest) -> Result<ItemQuery, ApiResponse> {
    Ok(ItemQuery {
        search: request.param("search"),
        kinds: kind_filter(request)?,
        ..item_query(request, Params::Lenient)?
    })
}

/// What an item list does with a parameter that is present but invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Params {
    /// Ignore it, as `/api/items` always has.
    Lenient,
    /// Answer 400, so a route never answers a different question than the
    /// one asked.
    Strict,
}

/// The sort, filters and page of an item list. The caller supplies kinds,
/// search and library scope.
pub(super) fn item_query(request: &ApiRequest, params: Params) -> Result<ItemQuery, ApiResponse> {
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
            .unwrap_or(DEFAULT_PAGE_LIMIT),
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
    match services.library.item(item_id) {
        Ok(Some(cached)) => Ok(ApiResponse::ok(cached)),
        Ok(None) => fetch_and_cache_item(services, item_id),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn item_synopsis(services: &Arc<Services>, item_id: &str) -> Handled {
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    match items::fetch_item_synopsis(client, user_id, item_id) {
        Ok(Some(dto)) => Ok(ApiResponse::ok(json!({ "overview": dto.overview }))),
        Ok(None) => {
            forget_item(services, &scope, item_id);
            Err(ApiResponse::error(
                404,
                "the server has no item with that id",
            ))
        }
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
        Ok(None) => {
            forget_item(services, &scope, item_id);
            Err(ApiResponse::error(
                404,
                "the server has no item with that id",
            ))
        }
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

fn fetch_and_cache_item(services: &Arc<Services>, item_id: &str) -> Handled {
    let scope = session_scope(services)?;
    match items::fetch_item(scope.client(), scope.user_id(), item_id) {
        Ok(Some(dto)) => {
            let cached = services
                .session
                .commit_if_current(&scope, stale_account_response, || {
                    // A storage failure still answers from the fetched DTO.
                    let _ = services.library.ingest_page(std::slice::from_ref(&dto));
                    Ok(())
                });
            cached?;
            match services.library.item(item_id) {
                Ok(Some(item)) => Ok(ApiResponse::ok(item)),
                Ok(None) => Ok(ApiResponse::ok(ItemSummary::from_dto(&dto))),
                Err(error) => Err(storage_failure(&error)),
            }
        }
        Ok(None) => {
            forget_item(services, &scope, item_id);
            Err(ApiResponse::error(
                404,
                "the server has no item with that id",
            ))
        }
        Err(error) => {
            if matches!(error, ApiError::Status { status: 404 }) {
                forget_item(services, &scope, item_id);
            }
            Err(scoped_failure(services, &scope, &error))
        }
    }
}

/// How long one parent's reconciled child list is trusted before a later
/// navigation asks the server again. It also absorbs the refetch that the
/// reconcile's own change notification triggers.
const CHILD_RECONCILE_INTERVAL: Duration = Duration::from_secs(60);
static CHILD_RECONCILES: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

fn claim_child_reconcile(parent_id: &str) -> bool {
    let now = Instant::now();
    let mut reconciles = CHILD_RECONCILES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reconciles.retain(|_, started| now.duration_since(*started) < CHILD_RECONCILE_INTERVAL);
    if reconciles.contains_key(parent_id) {
        return false;
    }
    reconciles.insert(parent_id.to_string(), now);
    true
}

fn children(services: &Arc<Services>, item_id: &str) -> Handled {
    let container = matches!(
        services.library.kind(item_id).as_deref(),
        Some("Series" | "Season")
    );
    let cached = match services.library.children(item_id) {
        Ok(children) => children,
        Err(error) => return Err(storage_failure(&error)),
    };
    if !container {
        return Ok(ApiResponse::ok(json!({ "items": cached })));
    }
    // The synced catalog already holds every season and episode, so a cached
    // list answers at once and the reconcile runs behind it; its change
    // notification refreshes the page if the server disagreed. Only a
    // container with nothing cached yet waits for the server.
    if !cached.is_empty() {
        if claim_child_reconcile(item_id) {
            let services = Arc::clone(services);
            let parent_id = item_id.to_string();
            if let Err(error) = std::thread::Builder::new()
                .name("reconcile-children".to_string())
                .spawn(move || {
                    reconcile_children(&services, &parent_id);
                })
            {
                tracing::warn!(target: "app.api", "could not start a child reconcile: {error}");
            }
        }
        return Ok(ApiResponse::ok(json!({ "items": cached })));
    }
    claim_child_reconcile(item_id);
    let overviews = reconcile_children(services, item_id);
    match services.library.children(item_id) {
        Ok(mut children) => {
            if let Some(overviews) = &overviews {
                for child in &mut children {
                    child.overview = overviews.get(&child.id).cloned().flatten();
                }
            }
            Ok(ApiResponse::ok(json!({ "items": children })))
        }
        Err(error) => Err(storage_failure(&error)),
    }
}

/// Re-reads one parent's child list from the server.
///
/// The cache alone cannot be trusted on a detail page: deleting episodes in
/// Jellyfin leaves their rows behind until the next identity sweep, and the
/// season view is exactly where those ghosts surface — a wall of art-less cards
/// with a dead Play button. The image-404 eviction only cleans up rows whose
/// poster happens to be requested, so it misses lazily-loaded cards below the
/// fold and episodes that never had artwork.
///
/// One small non-recursive request per navigation buys a correct list, and it
/// also makes newly added episodes appear without waiting for a sweep. Changed
/// rows reach an open page through the library change notification.
///
/// Returns each live child's synopsis so the response can carry it without the
/// cache ever storing prose; `None` means the server could not be asked.
fn reconcile_children(
    services: &Arc<Services>,
    parent_id: &str,
) -> Option<HashMap<String, Option<String>>> {
    let scope = services.session.scope().ok()?;
    let (client, user_id) = (scope.client(), scope.user_id());

    let mut live_items = Vec::new();
    let mut overviews = HashMap::new();
    let mut offset = 0;
    loop {
        let page = match items::fetch_children(client, user_id, parent_id, offset) {
            Ok(page) => page,
            Err(error) => {
                tracing::debug!(
                    target: "app.api",
                    "could not reconcile the children of {parent_id}: {error}"
                );
                services.session.note_scoped_error(&scope, &error);
                return None;
            }
        };
        let received = page.items.len() as i64;
        if page.items.is_empty() {
            break;
        }
        for item in &page.items {
            overviews.insert(item.id.clone(), item.overview.clone());
        }
        live_items.extend(page.items);
        offset += received;
        if received < items::CHILDREN_PAGE_SIZE {
            break;
        }
    }

    // An empty `live_items` here came from a successful request, so it is the server
    // saying this parent has no children left — unlike the library-wide sweep,
    // where the blast radius makes that answer too dangerous to trust.
    // The list is the server's answer for the account that asked.
    let reconciled = services.session.commit_if_current(
        &scope,
        || (),
        || Ok(services.library.reconcile_children(parent_id, &live_items)),
    );
    let Ok(reconciled) = reconciled else {
        tracing::debug!(
            target: "app.api",
            parent_id,
            "skipped a child reconcile: the account changed while it ran"
        );
        return None;
    };
    match reconciled {
        Ok(changes) => {
            if !changes.is_empty() {
                tracing::info!(
                    target: "app.api",
                    changed = changes.item_ids.len(),
                    parent_id,
                    "reconciled changed child rows"
                );
                services.shell.library_changed(changes);
            }
        }
        Err(error) => {
            tracing::warn!(target: "app.api", "could not commit reconciled children: {error}");
            return None;
        }
    }
    Some(overviews)
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
