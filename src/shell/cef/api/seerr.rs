use super::images::{cache_key, mime_for_image, store_image};
use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["seerr", "status"] if request.is("GET") => match services.requests_provider().status() {
            Ok(value) => ApiResponse::ok(value),
            Err(error) => ApiResponse::from_provider_error(&error),
        },
        ["seerr", "connect"] if request.is("POST") => seerr_connect(services, request),
        ["seerr", "link"] if request.is("POST") => seerr_link(services),
        ["seerr", "link", "poll"] if request.is("POST") => seerr_link_poll(services, request),
        ["seerr", "link", "password"] if request.is("POST") => {
            seerr_link_password(services, request)
        }
        ["seerr", "unlink"] if request.is("POST") => ApiResponse::ok(services.seerr.unlink()),
        ["seerr", "search"] if request.is("GET") => seerr_search(services, request),
        ["seerr", "person", tmdb_id, "credits"] if request.is("GET") => {
            seerr_person_credits(services, &percent_decode(tmdb_id), request)
        }
        ["seerr", "discover", kind] if request.is("GET") => {
            seerr_discover(services, &percent_decode(kind), request)
        }
        ["seerr", "genres", media_type] if request.is("GET") => {
            seerr_genres(services, &percent_decode(media_type))
        }
        ["seerr", "media", media_type, tmdb_id] if request.is("GET") => seerr_media(
            services,
            &percent_decode(media_type),
            &percent_decode(tmdb_id),
        ),
        ["seerr", "request-options", media_type] if request.is("GET") => {
            seerr_request_options(services, &percent_decode(media_type), request)
        }
        ["seerr", "request"] if request.is("POST") => seerr_request(services, request),
        ["seerr", "requests"] if request.is("GET") => seerr_requests(services, request),
        ["seerr", "request", id] if request.is("DELETE") => {
            seerr_cancel_request(services, &percent_decode(id))
        }
        ["seerr", "image", size, file] if request.is("GET") => {
            seerr_image(&percent_decode(size), &percent_decode(file))
        }
        _ => return None,
    };
    Some(response)
}

// ---------------------------------------------------------------------- seerr

fn seerr_connect(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    match services
        .seerr
        .connect(body["server"].as_str().unwrap_or_default())
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_seerr_error(&error),
    }
}

/// Starts the password-less link. Answers `{"method":"password"}` — not an
/// error — whenever Quick Connect is unavailable on either side, since every
/// Seerr release supports the password path this then falls back to.
fn seerr_link(services: &Arc<Services>) -> ApiResponse {
    match services.seerr.link_start() {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_seerr_error(&error),
    }
}

fn seerr_link_poll(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    match services
        .seerr
        .link_poll(body["secret"].as_str().unwrap_or_default())
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_seerr_error(&error),
    }
}

fn seerr_link_password(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let result = services.seerr.link_with_password(
        body["username"].as_str().unwrap_or_default(),
        body["password"].as_str().unwrap_or_default(),
    );
    match result {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_seerr_error(&error),
    }
}

fn seerr_search(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let query = request.param("q").unwrap_or_default();
    match services
        .requests_provider()
        .search(&query, page_param(request))
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_person_credits(
    services: &Arc<Services>,
    tmdb_id: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let Ok(tmdb_id) = tmdb_id.parse::<i64>() else {
        return ApiResponse::error(400, "that is not a TMDB person id");
    };
    if tmdb_id <= 0 {
        return ApiResponse::error(400, "that is not a TMDB person id");
    }

    let mut value = match services.requests_provider().person_credits(tmdb_id) {
        Ok(value) => value,
        Err(error) => return ApiResponse::from_provider_error(&error),
    };
    // During progressive catalog fill, SQLite cannot yet prove that every
    // Seerr credit is non-local. An exact Jellyfin identity lets this secondary
    // section verify ownership against the live complete person relation. A
    // failure hides Discover only; the independently loaded server grid stays.
    if let Some(person_id) = request.param("personId")
        && let Err(error) = join_server_person_availability(services, &person_id, &mut value)
    {
        services.session.note_error(&error);
        return ApiResponse::from_api_error(&error);
    }
    ApiResponse::ok(value)
}

fn join_server_person_availability(
    services: &Arc<Services>,
    person_id: &str,
    value: &mut Value,
) -> Result<(), ApiError> {
    let (client, user_id) = services.session.client_and_user()?;
    // The provider first joined against SQLite, which can contain both unseen
    // progressive rows and stale rows awaiting deletion reconciliation. The
    // live exact-person pass is authoritative, so rebuild availability rather
    // than only adding to that provisional answer.
    clear_person_availability(value);
    let mut offset = 0;
    for _ in 0..MAX_PERSON_QUERY_PAGES {
        let page = items::fetch_person_items(
            &client,
            &user_id,
            person_id,
            offset,
            items::PERSON_PAGE_SIZE,
        )?;
        let received = i64::try_from(page.items.len()).unwrap_or(i64::MAX);
        if received == 0 {
            if page.total_record_count > offset {
                return Err(ApiError::Decode(
                    "the server omitted part of an exact person filmography".to_string(),
                ));
            }
            return Ok(());
        }
        join_person_items(value, &page.items);
        offset = offset.saturating_add(received);
        if page.total_record_count > 0 && offset >= page.total_record_count {
            return Ok(());
        }
        if page.total_record_count <= 0 && received < items::PERSON_PAGE_SIZE {
            return Ok(());
        }
    }
    Err(ApiError::Decode(
        "the exact person filmography exceeded the safe paging limit".to_string(),
    ))
}

fn clear_person_availability(value: &mut Value) {
    if let Some(results) = value["results"].as_array_mut() {
        for result in results {
            result["libraryItemId"] = Value::Null;
        }
    }
}

fn join_person_items(value: &mut Value, server_items: &[BaseItemDto]) {
    let local = server_items
        .iter()
        .filter_map(|item| {
            let media_type = match item.item_type.as_deref() {
                Some("Movie") => "movie",
                Some("Series") => "tv",
                _ => return None,
            };
            let tmdb_id = item.provider_id("Tmdb")?.parse::<i64>().ok()?;
            (tmdb_id > 0).then(|| ((media_type.to_string(), tmdb_id), item.id.clone()))
        })
        .collect::<HashMap<_, _>>();
    let Some(results) = value["results"].as_array_mut() else {
        return;
    };
    for result in results {
        let Some(media_type) = result["mediaType"].as_str() else {
            continue;
        };
        let Some(tmdb_id) = result["tmdbId"].as_i64() else {
            continue;
        };
        if let Some(item_id) = local.get(&(media_type.to_string(), tmdb_id)) {
            result["libraryItemId"] = Value::String(item_id.clone());
        }
    }
}

fn seerr_discover(services: &Arc<Services>, kind: &str, request: &ApiRequest) -> ApiResponse {
    let Some(kind) = DiscoverKind::from_id(kind) else {
        return ApiResponse::error(404, "unknown discover row");
    };
    let options = match DiscoverOptions::from_values(
        request.param("genre").as_deref(),
        request.param("sort").as_deref(),
        request.param("minRating").as_deref(),
        request.param("decade").as_deref(),
        request.param("mediaType").as_deref(),
        request.param("timeWindow").as_deref(),
    ) {
        Ok(options) => options,
        Err(error) => return ApiResponse::error(400, &error),
    };
    match services
        .requests_provider()
        .discover(kind, page_param(request), &options)
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_genres(services: &Arc<Services>, media_type: &str) -> ApiResponse {
    if !matches!(media_type, "movie" | "tv") {
        return ApiResponse::error(404, "unknown genre kind");
    }
    match services.requests_provider().genres(media_type) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_media(services: &Arc<Services>, media_type: &str, tmdb_id: &str) -> ApiResponse {
    let Ok(tmdb_id) = tmdb_id.parse::<i64>() else {
        return ApiResponse::error(400, "that is not a TMDB id");
    };
    match services.requests_provider().media(media_type, tmdb_id) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_request_options(
    services: &Arc<Services>,
    media_type: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let is_4k = request
        .param("is4k")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
    match services
        .requests_provider()
        .request_options(media_type, is_4k)
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_request(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    // An explicit list means "these seasons"; its absence means "the whole
    // show", which Seerr expands to whatever it does not already have.
    let seasons = body["seasons"].as_array().map(|seasons| {
        seasons
            .iter()
            .filter_map(serde_json::Value::as_i64)
            .collect::<Vec<_>>()
    });
    let server_id = body["serverId"].as_i64();
    let profile_id = body["profileId"].as_i64();
    let profile = match (server_id, profile_id) {
        (None, None) => None,
        (Some(server_id), Some(profile_id)) if server_id >= 0 && profile_id > 0 => {
            Some(RequestProfileSelection {
                server_id,
                profile_id,
            })
        }
        _ => {
            return ApiResponse::error(
                400,
                "the download destination and quality profile must be selected together",
            );
        }
    };
    let result = services.requests_provider().create(
        body["mediaType"].as_str().unwrap_or_default(),
        body["tmdbId"].as_i64().unwrap_or_default(),
        seasons,
        body["is4k"].as_bool().unwrap_or(false),
        profile,
    );
    match result {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_requests(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let number = |key: &str, fallback: i64| {
        request
            .param(key)
            .and_then(|value| value.parse().ok())
            .unwrap_or(fallback)
    };
    let result = services.requests_provider().requests(
        number("take", 20),
        number("skip", 0),
        &request.param("filter").unwrap_or_else(|| "all".to_string()),
    );
    match result {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

fn seerr_cancel_request(services: &Arc<Services>, request_id: &str) -> ApiResponse {
    let Ok(request_id) = request_id.parse::<i64>() else {
        return ApiResponse::error(400, "that is not a request id");
    };
    match services.requests_provider().cancel(request_id) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_provider_error(&error),
    }
}

/// The TMDB poster proxy, in the same posture as [`open_external`]: the UI
/// names a rendition size and an image file, never an address, and
/// [`tmdb_image_url`] is what decides whether those two compose into a request
/// at all.
///
/// Art for titles the library does not have is exactly the browsing that pulls
/// the most bytes, so it shares the pruned on-disk cache the Jellyfin image
/// proxy already has, and is served as immutable — a TMDB file name addresses
/// one unchanging image.
fn seerr_image(size: &str, file: &str) -> ApiResponse {
    let Some(url) = tmdb_image_url(size, file) else {
        return ApiResponse::error(404, "no such poster");
    };
    let key = cache_key("tmdb", size, file, 0);
    let cache_path = crate::app::paths::image_cache_dir().join(&key);
    if let Ok(bytes) = std::fs::read(&cache_path)
        && !bytes.is_empty()
    {
        return ApiResponse::bytes(mime_for_image(&bytes), bytes, IMMUTABLE_CACHE);
    }
    match fetch_tmdb_image(&url) {
        Ok((bytes, content_type)) => {
            store_image(&cache_path, &bytes);
            ApiResponse::bytes(content_type, bytes, IMMUTABLE_CACHE)
        }
        Err(error) => {
            tracing::debug!(target: "app.api", "could not fetch poster art: {error}");
            ApiResponse::from_seerr_error(&error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_person_items_override_progressively_incomplete_local_availability() {
        let dtos: Vec<BaseItemDto> = [
            r#"{"Id":"m1","Name":"Movie","Type":"Movie","ProviderIds":{"Tmdb":"603"}}"#,
            r#"{"Id":"s1","Name":"Series","Type":"Series","ProviderIds":{"tmdb":"603"}}"#,
            r#"{"Id":"e1","Name":"Episode","Type":"Episode","ProviderIds":{"Tmdb":"603"}}"#,
        ]
        .into_iter()
        .map(|value| serde_json::from_str(value).expect("dto"))
        .collect();
        let mut credits = json!({ "results": [
            { "mediaType": "movie", "tmdbId": 603, "libraryItemId": null },
            { "mediaType": "tv", "tmdbId": 603, "libraryItemId": null },
            { "mediaType": "movie", "tmdbId": 404, "libraryItemId": "stale" }
        ] });

        clear_person_availability(&mut credits);
        join_person_items(&mut credits, &dtos);

        assert_eq!(credits["results"][0]["libraryItemId"], "m1");
        assert_eq!(credits["results"][1]["libraryItemId"], "s1");
        assert_eq!(
            credits["results"][2]["libraryItemId"],
            serde_json::Value::Null
        );
    }
}
