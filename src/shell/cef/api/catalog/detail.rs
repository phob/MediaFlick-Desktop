use crate::library::browse::{self, ChildPage, ChildrenSource, DetailError, LiveDetail};
use crate::library::{DetailView, ItemPage};

use super::*;

#[derive(Serialize, ts_rs::TS)]
pub(in super::super) struct ItemSynopsis {
    overview: Option<String>,
}

/// Rich metadata fetched live from Jellyfin by `/api/item/{id}/about` for the
/// detail page. Nothing here is cached locally; with the server unreachable
/// these sections stay in their loading/error states.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct ItemAbout {
    overview: Option<String>,
    critic_rating: Option<f64>,
    people: Vec<Person>,
    tags: Vec<String>,
    studios: Vec<String>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct Person {
    id: Option<String>,
    name: Option<String>,
    role: Option<String>,
    #[serde(rename = "type")]
    person_type: Option<String>,
    image_tag: Option<String>,
}

pub(super) fn item_detail(services: &Arc<Services>, item_id: &str) -> Handled {
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

fn server_lacks_item(services: &Arc<Services>, scope: &SessionScope, item_id: &str) -> ApiResponse {
    forget_item(services, scope, item_id);
    ApiResponse::error(404, "the server has no item with that id")
}

pub(super) fn item_synopsis(services: &Arc<Services>, item_id: &str) -> Handled {
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    match items::fetch_item_synopsis(client, user_id, item_id) {
        Ok(Some(dto)) => Ok(ApiResponse::ok(ItemSynopsis {
            overview: dto.overview,
        })),
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

fn bounded_about_people(people: &[BaseItemPerson]) -> Vec<Person> {
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
        selected.push(Person {
            id: person.id.clone(),
            name: person.name.clone(),
            role: person.role.clone(),
            person_type: person.person_type.clone(),
            image_tag: person.primary_image_tag.clone(),
        });
    }
    selected
}

pub(super) fn item_about(services: &Arc<Services>, item_id: &str) -> Handled {
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
            Ok(ApiResponse::ok(ItemAbout {
                overview: dto.overview,
                critic_rating: dto.critic_rating,
                people,
                tags: dto.tags,
                studios,
            }))
        }
        Ok(None) => Err(server_lacks_item(services, &scope, item_id)),
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

pub(super) fn children(services: &Arc<Services>, item_id: &str, request: &ApiRequest) -> Handled {
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
    Ok(ApiResponse::ok(page))
}

fn live_children(
    services: &Arc<Services>,
    item_id: &str,
    page: Option<ChildPage>,
) -> Result<ItemPage, ApiResponse> {
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    let fail = |error: ApiError| scoped_failure(services, &scope, &error);
    let paged = page.is_some();
    let page = page.unwrap_or_default();
    let remembered = services.children_sources.get(scope.generation(), item_id);
    let source = match remembered {
        Some(source) => source,
        None => items::fetch_item_type(client, user_id, item_id)
            .map_err(fail)?
            .map(|item_type| ChildrenSource::of(&item_type))
            .ok_or_else(|| ApiResponse::error(404, "the server has no item with that id"))?,
    };
    let children = match source {
        ChildrenSource::Folder => {
            items::fetch_live_children(client, user_id, item_id, page.offset(), page.limit())
                .map(|response| browse::live_page(&response))
        }
        ChildrenSource::Playlist => items::fetch_playlist_items(client, user_id, item_id, page)
            .map(|response| browse::playlist_page(&response, page)),
    }
    .map_err(fail)?;
    still_signed_in(services, &scope)?;
    if remembered.is_none() {
        services
            .children_sources
            .remember(scope.generation(), item_id, source);
    }
    tracing::debug!(target: "library.browse", item_id, paged, ?source, "fetched a container's children from the server");
    Ok(children)
}
