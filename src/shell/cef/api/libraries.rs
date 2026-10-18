use crate::library::browse::{self, BrowseError, FilterOptions, Grid};
use crate::library::libraries::{
    BrowseCapabilities, CROSS_LIBRARY_BROWSE, LibraryId, LibraryKind, LibraryRegistry, Membership,
};
use crate::library::{ItemPage, SortKey};

use super::catalog::{ItemLetters, Params, library_query, param};
use super::*;

const LATEST_LIMIT: i64 = 40;

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct LibrariesResponse {
    /// False until a sync cycle has read the server's library list.
    known: bool,
    /// In the user's Jellyfin order.
    libraries: Vec<LibraryView>,
    cross_library: &'static BrowseCapabilities,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct LibraryView {
    id: LibraryId,
    name: String,
    kind: LibraryKind,
    image_tag: Option<String>,
    /// True while a movies or TV shows library's titles are not known yet:
    /// during the first catalog download, and until the library's first
    /// membership walk lands. A later walk keeps the stored titles, so it
    /// never sets this.
    membership_pending: bool,
    browse: &'static BrowseCapabilities,
}

/// A library's newest titles and the library sort "All" opens with: the
/// shelf's own order for catalog and newest-first libraries, date added for
/// grouped ones.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct LibraryLatest {
    items: Vec<ItemSummary>,
    sort: SortKey,
}

routes! {
    |services, request|
    GET ["libraries"] -> LibrariesResponse = libraries(services),
    GET ["libraries", id, "items"] -> ItemPage = items(services, id, request),
    GET ["libraries", id, "letters"] -> ItemLetters = letters(services, id, request),
    GET ["libraries", id, "options"] -> FilterOptions = options(services, id),
    GET ["libraries", id, "latest"] -> LibraryLatest = latest(services, id, request),
}

fn libraries(services: &Arc<Services>) -> Result<LibrariesResponse, ApiResponse> {
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
            LibraryView {
                id: library.id.clone(),
                name: library.name.clone(),
                kind,
                image_tag: library.primary_image_tag.clone(),
                membership_pending: filling,
                browse: kind.profile().browse,
            }
        })
        .collect();
    Ok(LibrariesResponse {
        known,
        libraries,
        cross_library: &CROSS_LIBRARY_BROWSE,
    })
}

fn items(
    services: &Arc<Services>,
    id: &str,
    request: &ApiRequest,
) -> Result<ItemPage, ApiResponse> {
    let query = library_query(request, Params::Strict)?;
    let id = library_id(id)?;
    let scope = services.session.scope();
    let grid = browse::library_page(&services.library, &id, &query).map_err(browse_failure)?;
    answer(services, scope, grid)
}

fn letters(
    services: &Arc<Services>,
    id: &str,
    request: &ApiRequest,
) -> Result<ItemLetters, ApiResponse> {
    let query = library_query(request, Params::Strict)?;
    let letters = browse::library_letters(&services.library, &library_id(id)?, &query)
        .map_err(browse_failure)?;
    Ok(ItemLetters::new(letters))
}

fn options(services: &Arc<Services>, id: &str) -> Result<FilterOptions, ApiResponse> {
    browse::library_options(&services.library, &library_id(id)?).map_err(browse_failure)
}

fn latest(
    services: &Arc<Services>,
    id: &str,
    request: &ApiRequest,
) -> Result<LibraryLatest, ApiResponse> {
    let limit = param(request, Params::Strict, "limit", |value| {
        value.parse::<i64>().ok()
    })?
    .unwrap_or(HOME_ROW_LIMIT)
    .clamp(1, LATEST_LIMIT);
    let id = library_id(id)?;
    let scope = services.session.scope();
    let latest = browse::latest(&services.library, &id, limit).map_err(browse_failure)?;
    let page = answer(services, scope, latest.grid)?;
    Ok(LibraryLatest {
        items: page.items,
        sort: latest.sort,
    })
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
    let response = fetch_current(services, &scope, |client, user_id| {
        items::fetch_live(client, user_id, &query)
    })?;
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
