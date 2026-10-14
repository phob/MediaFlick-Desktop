use crate::library::browse::{self, BrowseError, BrowseQuery};
use crate::library::libraries::{FilterKey, LibraryId, LibraryRegistry, TITLE_BROWSE};

use super::*;

const DEFAULT_PAGE_LIMIT: i64 = 60;
const LATEST_LIMIT: i64 = 40;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<Handled> {
    let response = match segments {
        ["libraries"] if request.is("GET") => libraries(services),
        ["libraries", id, "items"] if request.is("GET") => items(services, id, request),
        ["libraries", id, "letters"] if request.is("GET") => letters(services, id, request),
        ["libraries", id, "options"] if request.is("GET") => options(services, id),
        ["libraries", id, "latest"] if request.is("GET") => latest(services, id, request),
        _ => return None,
    };
    Some(response)
}

fn libraries(services: &Arc<Services>) -> Handled {
    let registry = services
        .library
        .libraries()
        .map_err(|error| storage_failure(&error))?;
    let (known, libraries) = match &registry {
        LibraryRegistry::Unknown => (false, [].as_slice()),
        LibraryRegistry::Known(libraries) => (true, libraries.as_slice()),
    };
    let libraries = libraries
        .iter()
        .map(|library| {
            let kind = library.kind();
            json!({
                "id": library.id,
                "name": library.name,
                "kind": kind,
                "imageTag": library.primary_image_tag,
                "browse": kind.profile().browse,
            })
        })
        .collect::<Vec<_>>();
    Ok(ApiResponse::ok(json!({
        "known": known,
        "libraries": libraries,
        "titles": &TITLE_BROWSE,
    })))
}

fn items(services: &Arc<Services>, id: &str, request: &ApiRequest) -> Handled {
    let page = browse::library_page(&services.library, &library_id(id)?, &browse_query(request)?)
        .map_err(browse_failure)?;
    Ok(ApiResponse::ok(
        json!({ "items": page.items, "total": page.total }),
    ))
}

fn letters(services: &Arc<Services>, id: &str, request: &ApiRequest) -> Handled {
    let letters =
        browse::library_letters(&services.library, &library_id(id)?, &browse_query(request)?)
            .map_err(browse_failure)?;
    Ok(ApiResponse::ok(json!({
        "letters": letters.into_iter().collect::<std::collections::BTreeMap<_, _>>(),
    })))
}

fn options(services: &Arc<Services>, id: &str) -> Handled {
    browse::library_options(&services.library, &library_id(id)?)
        .map(ApiResponse::ok)
        .map_err(browse_failure)
}

fn latest(services: &Arc<Services>, id: &str, request: &ApiRequest) -> Handled {
    let limit = parsed(request, "limit", |value| value.parse::<i64>().ok())?
        .unwrap_or(HOME_ROW_LIMIT)
        .clamp(1, LATEST_LIMIT);
    let items =
        browse::latest(&services.library, &library_id(id)?, limit).map_err(browse_failure)?;
    Ok(ApiResponse::ok(json!({ "items": items })))
}

fn library_id(segment: &str) -> Result<LibraryId, ApiResponse> {
    LibraryId::parse(&percent_decode(segment))
        .ok_or_else(|| ApiResponse::error(400, "invalid library id"))
}

/// Unlike `/api/items`, a parameter that is present but invalid is a 400
/// rather than ignored: a library route never answers a different question
/// than the one asked.
fn browse_query(request: &ApiRequest) -> Result<BrowseQuery, ApiResponse> {
    let seed = parsed(request, "seed", |value| value.parse::<u32>().ok())?;
    Ok(BrowseQuery {
        sort: parsed(request, "sort", |value| ItemSort::from_id(value, seed))?.unwrap_or_default(),
        direction: parsed(request, "order", SortDirection::from_id)?,
        genre: request.param(FilterKey::Genre.id()),
        release_decade: parsed(
            request,
            FilterKey::Decade.id(),
            crate::library::release_decade_from_id,
        )?,
        watched: parsed(request, FilterKey::Watched.id(), WatchFilter::from_id)?,
        official_rating: request
            .param(FilterKey::Parental.id())
            .map(|value| value.trim().to_string()),
        series_status: parsed(request, FilterKey::SeriesStatus.id(), SeriesStatus::from_id)?,
        favorite: parsed(request, FilterKey::Favorite.id(), |value| match value {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        })?,
        offset: parsed(request, "offset", |value| value.parse::<i64>().ok())?.unwrap_or(0),
        limit: parsed(request, "limit", |value| value.parse::<i64>().ok())?
            .unwrap_or(DEFAULT_PAGE_LIMIT),
    })
}

fn parsed<T>(
    request: &ApiRequest,
    key: &str,
    parse: impl FnOnce(&str) -> Option<T>,
) -> Result<Option<T>, ApiResponse> {
    request
        .param(key)
        .map(|value| {
            parse(&value).ok_or_else(|| ApiResponse::error(400, format!("invalid {key} {value:?}")))
        })
        .transpose()
}

fn browse_failure(error: BrowseError) -> ApiResponse {
    match error {
        BrowseError::UnknownLibrary => {
            ApiResponse::error(404, "that is not one of this account's libraries")
        }
        BrowseError::Unsupported(message) => ApiResponse::error(400, message),
        BrowseError::LiveLibrary => {
            ApiResponse::error(501, "this version cannot browse this kind of library yet")
        }
        BrowseError::Storage(error) => storage_failure(&error),
    }
}
