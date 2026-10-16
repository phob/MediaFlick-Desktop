use crate::library::ItemPage;
use crate::library::browse::{self, BrowseError, Grid};
use crate::library::libraries::{CROSS_LIBRARY_BROWSE, LibraryId, LibraryRegistry, Membership};

use super::catalog::{Params, library_query, param, still_signed_in};
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
    let downloading = sync::first_catalog_download(&services.library).unwrap_or_else(|error| {
        tracing::warn!(target: "library.sync", "could not read catalog progress: {error}");
        true
    });
    let libraries = libraries
        .iter()
        .map(|library| {
            let kind = library.kind();
            let filling = library.catalog_title().is_some()
                && (downloading || library.membership == Membership::Unwalked);
            json!({
                "id": library.id,
                "name": library.name,
                "kind": kind,
                "imageTag": library.primary_image_tag,
                "membershipPending": filling,
                "browse": kind.profile().browse,
            })
        })
        .collect::<Vec<_>>();
    Ok(ApiResponse::ok(json!({
        "known": known,
        "libraries": libraries,
        "crossLibrary": &CROSS_LIBRARY_BROWSE,
    })))
}

fn items(services: &Arc<Services>, id: &str, request: &ApiRequest) -> Handled {
    let query = library_query(request, Params::Strict)?;
    let id = library_id(id)?;
    let scope = services.session.scope();
    let grid = browse::library_page(&services.library, &id, &query).map_err(browse_failure)?;
    let page = answer(services, scope, grid)?;
    Ok(ApiResponse::ok(
        json!({ "items": page.items, "total": page.total }),
    ))
}

fn letters(services: &Arc<Services>, id: &str, request: &ApiRequest) -> Handled {
    let query = library_query(request, Params::Strict)?;
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
    let id = library_id(id)?;
    let scope = services.session.scope();
    let grid = browse::latest(&services.library, &id, limit).map_err(browse_failure)?;
    let page = answer(services, scope, grid)?;
    Ok(ApiResponse::ok(json!({ "items": page.items })))
}

fn answer(
    services: &Arc<Services>,
    scope: Result<SessionScope, ApiError>,
    grid: Grid,
) -> Result<ItemPage, ApiResponse> {
    let query = match grid {
        Grid::Cached(page) => return Ok(page),
        Grid::Live(query) => query,
    };
    let scope = scope.map_err(|error| ApiResponse::from_api_error(&error))?;
    let response = items::fetch_live(scope.client(), scope.user_id(), &query)
        .map_err(|error| scoped_failure(services, &scope, &error))?;
    still_signed_in(services, &scope)?;
    Ok(browse::live_page(&response))
}

fn library_id(segment: &str) -> Result<LibraryId, ApiResponse> {
    LibraryId::parse(&percent_decode(segment))
        .ok_or_else(|| ApiResponse::error(400, "invalid library id"))
}

fn browse_failure(error: BrowseError) -> ApiResponse {
    match error {
        BrowseError::RegistryUnknown => ApiResponse::error(
            503,
            "the library list has not been loaded from the server yet",
        ),
        BrowseError::UnknownLibrary => {
            ApiResponse::error(404, "that is not one of this account's libraries")
        }
        BrowseError::Unsupported(message) => ApiResponse::error(400, message),
        BrowseError::Storage(error) => storage_failure(&error),
    }
}
