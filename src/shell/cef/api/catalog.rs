use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["calendar"] if request.is("GET") => calendar(services, request),
        ["home", "resume"] if request.is("GET") => home_resume(services),
        ["home"] if request.is("GET") => home(services),
        ["billboard"] if request.is("GET") => billboard(services),
        ["items"] if request.is("GET") => query_items(services, request),
        ["genres"] if request.is("GET") => match services.library.genres() {
            Ok(genres) => ApiResponse::ok(json!({ "genres": genres })),
            Err(error) => storage_failure(&error),
        },
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

fn calendar(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let Some(start) = request.param("start") else {
        return ApiResponse::error(400, "calendar start is required");
    };
    let Some(end) = request.param("end") else {
        return ApiResponse::error(400, "calendar end is required");
    };
    if !is_iso_date(&start) || !is_iso_date(&end) || end < start {
        return ApiResponse::error(
            400,
            "calendar dates must be YYYY-MM-DD with end after start",
        );
    }
    match services.companion.calendar(&start, &end) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
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

// ------------------------------------------------------------------ browsing

fn home(services: &Arc<Services>) -> ApiResponse {
    let library = &services.library;
    let resume = library
        .continue_watching(HOME_ROW_LIMIT)
        .unwrap_or_default();
    let recent = library.recently_added(HOME_ROW_LIMIT).unwrap_or_default();
    let latest_movies = latest_home_items(library, "Movie");
    let latest_shows = latest_home_items(library, "Series");

    // This response is the startup snapshot. Keep it entirely SQLite-backed so
    // a healthy durable cache can paint while the loading screen is still up;
    // live Next Up enrichment arrives independently from `home_resume`.
    ApiResponse::ok(json!({
        "rows": [
            { "id": "resume", "title": "Continue Watching", "items": resume },
            { "id": "recent", "title": "Recently Added", "items": recent },
            { "id": "latest-movies", "title": "Latest Movies", "items": latest_movies },
            { "id": "latest-shows", "title": "Latest Series", "items": latest_shows },
        ],
    }))
}

/// Enriches the cached Continue Watching shelf with Jellyfin's server-owned
/// Next Up decisions without holding the rest of the home page behind a
/// network request.
fn home_resume(services: &Arc<Services>) -> ApiResponse {
    let resume = services
        .library
        .continue_watching(HOME_ROW_LIMIT)
        .unwrap_or_default();
    // Next Up is server-side logic; replicating it locally would drift.
    let next_up = services
        .session
        .client_and_user()
        .and_then(|(client, user_id)| items::fetch_next_up(&client, &user_id, None, HOME_ROW_LIMIT))
        .map(|response| {
            response
                .items
                .iter()
                .map(summary_from_dto)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|error| {
            services.session.note_error(&error);
            tracing::debug!(target: "app.api", "Next Up unavailable: {error}");
            Vec::new()
        });

    ApiResponse::ok(json!({
        "items": merge_next_up(resume, next_up),
    }))
}

/// New releases are separate from Recently Added: importing an older title
/// moves it to the front of the latter, but not to the front of these shelves.
fn latest_home_items(library: &Library, kind: &str) -> Vec<Value> {
    library
        .query(&ItemQuery {
            kinds: vec![kind.to_string()],
            sort: ItemSort::Year,
            limit: HOME_ROW_LIMIT,
            ..Default::default()
        })
        .map(|page| page.items)
        .unwrap_or_default()
}

fn billboard(services: &Arc<Services>) -> ApiResponse {
    match services.library.random_billboard_titles(BILLBOARD_LIMIT) {
        Ok(items) => ApiResponse::ok(json!({ "items": items })),
        Err(error) => storage_failure(&error),
    }
}

/// Continue Watching and Next Up share one row: half-watched items first, in the
/// order the cache returned them, then the next unwatched episode of everything
/// else.
///
/// A show can legitimately show up in both lists — Jellyfin counts an
/// in-progress episode as that series' Next Up — so entries are keyed by series
/// as well as by id. Without the series key a partly watched episode and its own
/// Next Up successor would sit next to each other as two cards for one show.
///
/// Both sources are already capped at `HOME_ROW_LIMIT`, and the merged row keeps
/// all of what survives deduplication rather than re-applying that cap to the
/// total: trimming it back to one row's worth would put a library with a handful
/// of half-watched items and many unwatched series out of reach entirely.
fn merge_next_up(resume: Vec<Value>, next_up: Vec<Value>) -> Vec<Value> {
    fn key(item: &Value, field: &str) -> Option<String> {
        item[field].as_str().map(str::to_string)
    }

    let mut seen: HashSet<String> = resume
        .iter()
        .flat_map(|item| [key(item, "id"), key(item, "seriesId")])
        .flatten()
        .collect();

    let mut merged = resume;
    for item in next_up {
        let keys: Vec<String> = [key(&item, "id"), key(&item, "seriesId")]
            .into_iter()
            .flatten()
            .collect();
        if keys.iter().any(|key| seen.contains(key)) {
            continue;
        }
        seen.extend(keys);
        merged.push(item);
    }
    merged
}

fn query_items(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    if let Some(person_id) = request.param("personId") {
        return query_person_items(services, &person_id, request);
    }

    let query = ItemQuery {
        search: request.param("search"),
        kinds: request
            .param("kind")
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|kind| !kind.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        genre: request.param("genre"),
        release_decade: request
            .param("decade")
            .as_deref()
            .and_then(crate::library::release_decade_from_id),
        parent_id: request.param("parentId"),
        series_id: request.param("seriesId"),
        watched: request
            .param("watched")
            .and_then(|value| match value.as_str() {
                "true" | "1" => Some(true),
                "false" | "0" => Some(false),
                _ => None,
            }),
        favorite: request
            .param("favorite")
            .map(|value| value == "true" || value == "1"),
        sort: request
            .param("sort")
            .as_deref()
            .and_then(ItemSort::from_id)
            .unwrap_or_default(),
        offset: request
            .param("offset")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        limit: request
            .param("limit")
            .and_then(|value| value.parse().ok())
            .unwrap_or(60),
    };
    match services.library.query(&query) {
        Ok(page) => ApiResponse::ok(json!({ "items": page.items, "total": page.total })),
        Err(error) => storage_failure(&error),
    }
}

fn query_person_items(
    services: &Arc<Services>,
    person_id: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let offset = request
        .param("offset")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let limit = request
        .param("limit")
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_person_items(&client, &user_id, person_id, offset, limit) {
        Ok(response) => ApiResponse::ok(json!({
            "items": response.items.iter().map(summary_from_dto).collect::<Vec<_>>(),
            "total": response.total_record_count,
        })),
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
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
fn resolve_person(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let jellyfin_id = request.param("jellyfinId");
    let tmdb_id = request
        .param("tmdbId")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0);
    let name = request.param("name").unwrap_or_default();
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };

    if let Some(jellyfin_id) = jellyfin_id {
        return match items::fetch_item(&client, &user_id, &jellyfin_id) {
            Ok(Some(person)) if person.item_type.as_deref() == Some("Person") => {
                let provider_id = person
                    .provider_id("Tmdb")
                    .and_then(|id| id.parse::<i64>().ok())
                    .filter(|id| *id > 0);
                if tmdb_id.is_some() && provider_id.is_some() && tmdb_id != provider_id {
                    return ApiResponse::error(
                        409,
                        "the Jellyfin and TMDB person ids do not match",
                    );
                }
                ApiResponse::ok(json!({
                    "person": person_identity(&person, tmdb_id),
                    "candidates": [],
                    "ambiguous": false,
                }))
            }
            Ok(Some(_)) => ApiResponse::error(409, "that Jellyfin id is not a person"),
            Ok(None) => ApiResponse::error(404, "the server has no person with that id"),
            Err(error) => {
                services.session.note_error(&error);
                ApiResponse::from_api_error(&error)
            }
        };
    }

    if name.trim().is_empty() {
        return ApiResponse::error(400, "a person name is required to resolve that deep link");
    }
    match items::fetch_people(&client, &user_id, &name) {
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
                return ApiResponse::ok(json!({
                    "person": person_identity(&exact[0], tmdb_id),
                    "candidates": [],
                    "ambiguous": false,
                }));
            }
            let candidates = exact
                .iter()
                .map(|person| person_identity(person, tmdb_id))
                .collect::<Vec<_>>();
            ApiResponse::ok(json!({
                "person": Value::Null,
                "ambiguous": candidates.len() > 1,
                "candidates": candidates,
            }))
        }
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn item_detail(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    match services.library.item(item_id) {
        // The thin catalog row answers instantly; prose, cast, and critic
        // scores arrive separately through the live `about` endpoint.
        Ok(Some(cached)) => ApiResponse::ok(cached),
        // A deep link can outrun the catalog; fetch that one item and cache it.
        Ok(None) => fetch_and_cache_item(services, item_id),
        Err(error) => storage_failure(&error),
    }
}

fn item_synopsis(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_item_synopsis(&client, &user_id, item_id) {
        Ok(Some(dto)) => ApiResponse::ok(json!({ "overview": dto.overview })),
        Ok(None) => {
            forget_item(services, item_id);
            ApiResponse::error(404, "the server has no item with that id")
        }
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

const ITEM_ABOUT_CAST_LIMIT: usize = 24;
const ITEM_ABOUT_CREW_LIMIT: usize = 24;
const ITEM_ABOUT_CREW_PER_JOB_LIMIT: usize = 6;

fn is_cast_credit(person: &BaseItemPerson) -> bool {
    person.person_type.as_deref() == Some("Actor")
        || (person.person_type.is_none()
            && person
                .role
                .as_deref()
                .is_some_and(|role| !role.trim().is_empty()))
}

/// Keeps the live about payload and its headshot fan-out bounded while
/// preserving Jellyfin's credit order and a useful spread of crew jobs.
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

/// Rich metadata for one item, fetched live from Jellyfin and never persisted.
/// The detail page draws the cached thin row first and fills this in when it
/// lands; when the server is unreachable the UI keeps its plain error state.
fn item_about(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_item_about(&client, &user_id, item_id) {
        Ok(Some(dto)) => {
            let people = bounded_about_people(&dto.people);
            let studios = dto
                .studios
                .iter()
                .filter_map(|studio| studio.name.clone())
                .collect::<Vec<_>>();
            ApiResponse::ok(json!({
                "overview": dto.overview,
                "criticRating": dto.critic_rating,
                "people": people,
                "tags": dto.tags,
                "studios": studios,
            }))
        }
        Ok(None) => {
            forget_item(services, item_id);
            ApiResponse::error(404, "the server has no item with that id")
        }
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn fetch_and_cache_item(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_item(&client, &user_id, item_id) {
        Ok(Some(dto)) => {
            let _ = services.library.ingest_page(std::slice::from_ref(&dto));
            match services.library.item(item_id) {
                Ok(Some(item)) => ApiResponse::ok(item),
                Ok(None) => ApiResponse::ok(summary_from_dto(&dto)),
                Err(error) => storage_failure(&error),
            }
        }
        Ok(None) => {
            forget_item(services, item_id);
            ApiResponse::error(404, "the server has no item with that id")
        }
        Err(error) => {
            if matches!(error, ApiError::Status { status: 404 }) {
                forget_item(services, item_id);
            }
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn children(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    // Only containers have a child list worth asking the server about; a movie
    // detail page asks for children too and must not pay for a round trip.
    let overviews = if matches!(
        services.library.kind(item_id).as_deref(),
        Some("Series" | "Season")
    ) {
        reconcile_children(services, item_id)
    } else {
        None
    };
    match services.library.children(item_id) {
        Ok(mut children) => {
            // Episode synopses are not cached; they ride along from the live
            // reconcile that just answered. Offline, rows simply have none.
            if let Some(overviews) = &overviews {
                for child in &mut children {
                    let Some(id) = child["id"].as_str().map(str::to_string) else {
                        continue;
                    };
                    if let (Some(overview), Some(object)) =
                        (overviews.get(&id), child.as_object_mut())
                    {
                        object.insert("overview".to_string(), overview.clone());
                    }
                }
            }
            ApiResponse::ok(json!({ "items": children }))
        }
        Err(error) => storage_failure(&error),
    }
}

/// Re-reads one parent's child list from the server before answering.
///
/// The cache alone cannot be trusted on a detail page: deleting episodes in
/// Jellyfin leaves their rows behind until the next identity sweep, and the
/// season view is exactly where those ghosts surface — a wall of art-less cards
/// with a dead Play button. The image-404 eviction only cleans up rows whose
/// poster happens to be requested, so it misses lazily-loaded cards below the
/// fold and episodes that never had artwork.
///
/// One small non-recursive request per navigation buys a correct list, and it
/// also makes newly added episodes appear without waiting for a sweep.
///
/// Returns each live child's synopsis so the response can carry it without the
/// cache ever storing prose; `None` means the server could not be asked.
fn reconcile_children(services: &Arc<Services>, parent_id: &str) -> Option<HashMap<String, Value>> {
    let (client, user_id) = services.session.client_and_user().ok()?;

    let mut live_items = Vec::new();
    let mut overviews = HashMap::new();
    let mut offset = 0;
    loop {
        let page = match items::fetch_children(&client, &user_id, parent_id, offset) {
            Ok(page) => page,
            Err(error) => {
                // Offline, or the server is unwell. The cached list is still the
                // best answer available, so leave it exactly as it is.
                tracing::debug!(
                    target: "app.api",
                    "could not reconcile the children of {parent_id}: {error}"
                );
                services.session.note_error(&error);
                return None;
            }
        };
        let received = page.items.len() as i64;
        if page.items.is_empty() {
            break;
        }
        for item in &page.items {
            overviews.insert(item.id.clone(), json!(item.overview));
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
    match services.library.reconcile_children(parent_id, &live_items) {
        Ok(changes) => {
            if !changes.is_empty() {
                tracing::info!(
                    target: "app.api",
                    changed = changes.item_ids.len(),
                    parent_id,
                    "reconciled changed child rows"
                );
                crate::app::services::notify_library_changed(changes);
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
    fn next_up_entries_are_shaped_like_cached_rows() {
        let dto: BaseItemDto = serde_json::from_str(
            r#"{"Id":"e1","Name":"Half Loop","Type":"Episode","SeriesName":"Severance",
                "IndexNumber":2,"ParentIndexNumber":1,
                "ImageTags":{"Primary":"still-tag","Thumb":"thumb-tag","Logo":"logo-tag"},
                "BackdropImageTags":["backdrop-tag"],
                "UserData":{"Played":false,"PlaybackPositionTicks":42}}"#,
        )
        .expect("dto");
        let summary = summary_from_dto(&dto);
        assert_eq!(summary["id"], "e1");
        assert_eq!(summary["kind"], "Episode");
        assert_eq!(summary["seriesName"], "Severance");
        assert_eq!(summary["positionTicks"], 42);
        assert_eq!(summary["played"], false);
        assert_eq!(summary["favorite"], false);
        assert_eq!(summary["primaryImageTag"], "still-tag");
        assert_eq!(summary["thumbImageTag"], "thumb-tag");
        assert_eq!(summary["logoImageTag"], "logo-tag");
        assert_eq!(summary["backdropImageTag"], "backdrop-tag");
    }

    #[test]
    fn live_about_credits_are_bounded_before_serialization() {
        let mut people = vec![json!({
            "Name": "Unclassified performer",
            "Role": "Self",
        })];
        people.extend((0..30).map(|index| {
            json!({ "Id": format!("actor-{index}"), "Name": format!("Actor {index}"), "Type": "Actor" })
        }));
        for job in ["Director", "Writer", "Producer", "Composer", "Editor"] {
            people.extend(
                (0..10).map(|index| json!({ "Name": format!("{job} {index}"), "Type": job })),
            );
        }
        for kind in ["Movie", "Series"] {
            let dto: BaseItemDto = serde_json::from_value(json!({
                "Type": kind,
                "People": people.clone(),
            }))
            .expect("dto");

            let selected = bounded_about_people(&dto.people);
            assert_eq!(
                selected.len(),
                ITEM_ABOUT_CAST_LIMIT + ITEM_ABOUT_CREW_LIMIT,
                "{kind}"
            );
            assert_eq!(selected[0]["name"], "Unclassified performer");
            assert_eq!(
                selected
                    .iter()
                    .filter(|person| person["type"] == "Actor" || person["type"].is_null())
                    .count(),
                ITEM_ABOUT_CAST_LIMIT,
                "{kind}"
            );
            assert_eq!(
                selected
                    .iter()
                    .filter(|person| person["type"] != "Actor" && !person["type"].is_null())
                    .count(),
                ITEM_ABOUT_CREW_LIMIT,
                "{kind}"
            );
            for job in ["Director", "Writer", "Producer", "Composer", "Editor"] {
                assert!(
                    selected
                        .iter()
                        .filter(|person| person["type"] == job)
                        .count()
                        <= super::ITEM_ABOUT_CREW_PER_JOB_LIMIT,
                    "{kind} {job}"
                );
            }
        }
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

        let movies = latest_home_items(&library, "Movie");
        let shows = latest_home_items(&library, "Series");

        assert_eq!(movies[0]["id"], "new-movie");
        assert_eq!(movies[1]["id"], "old-movie");
        assert!(movies.iter().all(|item| item["kind"] == "Movie"));
        assert_eq!(shows[0]["id"], "new-show");
        assert_eq!(shows[1]["id"], "old-show");
        assert!(shows.iter().all(|item| item["kind"] == "Series"));
    }

    fn episode(id: &str, series_id: &str) -> serde_json::Value {
        json!({ "id": id, "kind": "Episode", "seriesId": series_id })
    }

    #[test]
    fn continue_watching_comes_before_next_up_in_the_merged_row() {
        let merged = merge_next_up(
            vec![episode("e1", "sev"), json!({ "id": "m1", "kind": "Movie" })],
            vec![episode("e9", "silo"), episode("e8", "andor")],
        );
        let ids: Vec<&str> = merged
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["e1", "m1", "e9", "e8"]);
    }

    /// The in-progress episode is also its series' Next Up, so the naive
    /// concatenation would show the same card twice.
    #[test]
    fn a_show_already_being_watched_is_not_repeated_by_next_up() {
        let merged = merge_next_up(
            vec![episode("e1", "sev")],
            // Same episode by id, then the successor within the same series.
            vec![
                episode("e1", "sev"),
                episode("e2", "sev"),
                episode("e9", "silo"),
            ],
        );
        let ids: Vec<&str> = merged
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["e1", "e9"]);
    }

    /// A full Continue Watching list must not squeeze Next Up off the row.
    #[test]
    fn next_up_survives_a_continue_watching_list_that_fills_a_row_on_its_own() {
        let limit = HOME_ROW_LIMIT as usize;
        let resume: Vec<_> = (0..limit)
            .map(|index| json!({ "id": format!("r{index}"), "kind": "Movie" }))
            .collect();
        let merged = merge_next_up(resume, vec![episode("e9", "silo")]);
        assert_eq!(merged.len(), limit + 1);
        assert_eq!(merged.last().expect("last")["id"], "e9");
    }
}
