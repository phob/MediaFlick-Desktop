use crate::library::browse::{self, BrowseError};
use crate::library::libraries::{LibraryId, LibraryRegistry, TITLE_BROWSE};

use super::catalog::{Params, item_query, param};
use super::*;

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
    let query = item_query(request, Params::Strict)?;
    let page = browse::library_page(&services.library, &library_id(id)?, &query)
        .map_err(browse_failure)?;
    Ok(ApiResponse::ok(
        json!({ "items": page.items, "total": page.total }),
    ))
}

fn letters(services: &Arc<Services>, id: &str, request: &ApiRequest) -> Handled {
    let query = item_query(request, Params::Strict)?;
    let letters = browse::library_letters(&services.library, &library_id(id)?, &query)
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
    let limit = param(request, Params::Strict, "limit", |value| {
        value.parse::<i64>().ok()
    })?
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

fn browse_failure(error: BrowseError) -> ApiResponse {
    match error {
        BrowseError::NotLoaded => ApiResponse::error(
            503,
            "the library list has not been loaded from the server yet",
        ),
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
