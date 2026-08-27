//! Account-scoped collections API. Provider configuration never crosses this
//! boundary; the browser receives readiness and normalized titles only.

use super::*;

mod browse;
mod profiles;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    if let Some(response) = configuration_route(services, segments, request) {
        return Some(response);
    }
    let response = match segments {
        ["collections"] if request.is("GET") => browse::redirect_state(services),
        ["collections", "mine"] if request.is("GET") => browse::mine(services),
        ["collections", "mine", profile_id] if request.is("GET") => {
            browse::profile_detail(services, &percent_decode(profile_id))
        }
        ["collections", "franchises"] if request.is("GET") => browse::franchises(services, request),
        ["collections", "franchises", collection_id] if request.is("GET") => {
            browse::franchise_detail(services, &percent_decode(collection_id), request)
        }
        ["collections", "jellyfin"] if request.is("GET") => jellyfin_index(services),
        ["collections", "jellyfin", boxset_id] if request.is("GET") => {
            jellyfin_detail(services, &percent_decode(boxset_id))
        }
        ["collections", "local-account"] if request.is("DELETE") => {
            delete_local_account(services, request)
        }
        ["collections", "movie", tmdb_id] if request.is("GET") => {
            browse::movie_franchise(services, &percent_decode(tmdb_id))
        }
        ["collections", "title", media_type, tmdb_id] if request.is("GET") => browse::title(
            services,
            &percent_decode(media_type),
            &percent_decode(tmdb_id),
        ),
        _ => return None,
    };
    Some(response)
}

fn configuration_route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    match segments {
        ["collections", "settings"] if request.is("GET") => {
            Some(profiles::settings(services, false))
        }
        ["collections", "settings", "reprobe"] if request.is("POST") => {
            Some(profiles::settings(services, true))
        }
        ["collections", "settings"] if request.is("PATCH") => {
            Some(profiles::patch_settings(services, request))
        }
        ["collections", "templates"] if request.is("GET") => Some(profiles::templates(services)),
        ["collections", "mdblist", "search"] if request.is("POST") => {
            Some(search_public_lists(services, request))
        }
        ["collections", "mdblist", "validate"] if request.is("POST") => {
            Some(validate_public_list(services, request))
        }
        ["collections", "provider-artwork"] if request.is("GET") => {
            Some(provider_artwork(services, request))
        }
        ["collections", "artwork"] if request.is("POST") => Some(upload_artwork(services, request)),
        ["collections", "artwork", artwork_id] if request.is("GET") => {
            Some(custom_artwork(services, &percent_decode(artwork_id)))
        }
        ["collections", "preview"] if request.is("POST") => {
            Some(profiles::preview(services, request))
        }
        ["collections", "profiles"] if request.is("GET") => Some(profiles::list(services)),
        ["collections", "profiles"] if request.is("POST") => {
            Some(profiles::create(services, request))
        }
        ["collections", "profiles", "order"] if request.is("PUT") => {
            Some(profiles::reorder(services, request))
        }
        ["collections", "profiles", profile_id] if request.is("GET") => {
            Some(profiles::read(services, &percent_decode(profile_id)))
        }
        ["collections", "profiles", profile_id] if request.is("PATCH") => Some(profiles::edit(
            services,
            &percent_decode(profile_id),
            request,
        )),
        ["collections", "profiles", profile_id] if request.is("DELETE") => {
            Some(profiles::delete(services, &percent_decode(profile_id)))
        }
        ["collections", "profiles", profile_id, "refresh"] if request.is("POST") => {
            Some(profiles::refresh(services, &percent_decode(profile_id)))
        }
        _ => None,
    }
}

fn active_account(services: &Arc<Services>) -> Result<AccountKey, ApiResponse> {
    services
        .session
        .account_key()
        .ok_or_else(|| ApiResponse::error(401, "sign in to use collections"))
}

fn jellyfin_index(services: &Arc<Services>) -> ApiResponse {
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_box_sets(&client, &user_id) {
        Ok(response) => ApiResponse::ok(json!({
            "collections": response.items.iter().map(|item| json!({
                "id": item.id,
                "name": item.display_name(),
                "primaryImageTag": item.primary_image_tag(),
                "backdropImageTag": item.backdrop_image_tags.first(),
                "itemCount": item.child_count,
            })).collect::<Vec<_>>(),
        })),
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn jellyfin_detail(services: &Arc<Services>, id: &str) -> ApiResponse {
    if id.is_empty() {
        return ApiResponse::error(400, "that is not a Jellyfin collection id");
    }
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let set = match items::fetch_item(&client, &user_id, id) {
        Ok(Some(item)) => item,
        Ok(None) => return ApiResponse::error(404, "that Jellyfin collection does not exist"),
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_box_set_children(&client, &user_id, id) {
        Ok(children) => ApiResponse::ok(json!({
            "id": set.id,
            "name": set.display_name(),
            "primaryImageTag": set.primary_image_tag(),
            "backdropImageTag": set.backdrop_image_tags.first(),
            "items": children.items.iter().map(summary_from_dto).collect::<Vec<_>>(),
            "totalRecordCount": children.total_record_count,
        })),
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
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

fn upload_artwork(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    if let Err(response) = active_account(services) {
        return response;
    }
    match services.artwork.stage(&request.body) {
        Ok(id) => ApiResponse::ok(json!({ "id": id })),
        Err(error) => configuration_failure(&error),
    }
}

fn custom_artwork(services: &Arc<Services>, id: &str) -> ApiResponse {
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    if !services
        .collections
        .artwork_ids_for_account(&account)
        .iter()
        .any(|candidate| candidate == id)
    {
        return ApiResponse::error(404, "that custom poster does not exist");
    }
    let Some(path) = services.artwork.path(id) else {
        return ApiResponse::error(404, "that custom poster does not exist");
    };
    let content_type = match path.extension().and_then(|value| value.to_str()) {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => return ApiResponse::error(404, "that custom poster does not exist"),
    };
    match std::fs::read(path) {
        Ok(bytes) => ApiResponse::bytes(content_type.to_string(), bytes, IMMUTABLE_CACHE),
        Err(_) => ApiResponse::error(404, "that custom poster does not exist"),
    }
}

fn search_public_lists(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    if let Err(response) = active_account(services) {
        return response;
    }
    let body = request.json();
    let Some(query) = body
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
    else {
        return ApiResponse::error(400, "enter a public-list search");
    };
    match services.companion.search_public_lists(query) {
        Ok(result) => ApiResponse::ok(result),
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn validate_public_list(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    if let Err(response) = active_account(services) {
        return response;
    }
    let body = request.json();
    let Some(selector) = body
        .get("selector")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|selector| !selector.is_empty())
    else {
        return ApiResponse::error(400, "enter a public-list ID or canonical URL");
    };
    match services.companion.validate_public_list(selector) {
        Ok(result) => ApiResponse::ok(result),
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn provider_artwork(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    if let Err(response) = active_account(services) {
        return response;
    }
    let Some(size) = request.param("size") else {
        return ApiResponse::error(400, "artwork size is required");
    };
    let Some(path) = request.param("path") else {
        return ApiResponse::error(400, "artwork path is required");
    };
    match services.companion.collection_artwork(&size, &path) {
        Ok((bytes, content_type)) if content_type.starts_with("image/") => {
            ApiResponse::bytes(content_type, bytes, IMMUTABLE_CACHE)
        }
        Ok(_) => ApiResponse::error(502, "provider returned invalid artwork"),
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn delete_local_account(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    if request.json().get("confirmed").and_then(Value::as_bool) != Some(true) {
        return ApiResponse::error(400, "confirm the local account deletion first");
    }
    match crate::app::services::delete_local_account_data(services) {
        Ok(()) => ApiResponse::ok(json!({ "deleted": true })),
        Err(error) => ApiResponse::error(500, error),
    }
}
