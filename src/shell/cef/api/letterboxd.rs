use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["integrations", "letterboxd"] if request.is("GET") => letterboxd_profiles(services),
        ["integrations", "letterboxd"] if request.is("POST") => {
            letterboxd_add_profile(services, request)
        }
        ["integrations", "letterboxd", id] if request.is("PATCH") => {
            letterboxd_set_enabled(services, &percent_decode(id), request)
        }
        ["integrations", "letterboxd", id] if request.is("DELETE") => {
            letterboxd_remove_profile(services, &percent_decode(id))
        }
        ["integrations", "letterboxd", id, "refresh"] if request.is("POST") => {
            letterboxd_refresh_profile(services, &percent_decode(id))
        }
        ["integrations", "letterboxd", id, "open"] if request.is("POST") => {
            letterboxd_open_profile(services, &percent_decode(id))
        }
        ["letterboxd", "movie", tmdb_id] if request.is("GET") => {
            movie_letterboxd(services, &percent_decode(tmdb_id))
        }
        ["item", id, "letterboxd"] if request.is("GET") => {
            item_letterboxd(services, &percent_decode(id))
        }
        _ => return None,
    };
    Some(response)
}

fn letterboxd_scope(services: &Arc<Services>) -> Result<(String, String), ApiResponse> {
    if !services.session.is_authenticated() {
        return Err(ApiResponse::error(
            401,
            "sign in to manage connected profiles",
        ));
    }
    let credentials = services.library.credentials();
    match (credentials.server_id, credentials.user_id) {
        (Some(server_id), Some(user_id)) => Ok((server_id, user_id)),
        _ => Err(ApiResponse::error(
            401,
            "sign in to manage connected profiles",
        )),
    }
}

fn valid_profile_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn letterboxd_profiles(services: &Arc<Services>) -> ApiResponse {
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    match services
        .library
        .external_profiles("letterboxd", &server_id, &user_id)
    {
        Ok(profiles) => ApiResponse::ok(json!({ "profiles": profiles })),
        Err(error) => storage_failure(&error),
    }
}

fn letterboxd_add_profile(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let profile = match letterboxd_integration::normalize_profile(
        request.json()["profile"].as_str().unwrap_or_default(),
    ) {
        Ok(profile) => profile,
        Err(error) => return ApiResponse::error(400, error),
    };
    let existing = match services
        .library
        .external_profiles("letterboxd", &server_id, &user_id)
    {
        Ok(existing) => existing,
        Err(error) => return storage_failure(&error),
    };
    if existing.len() >= letterboxd_integration::MAX_CONNECTED_PROFILES
        && !existing
            .iter()
            .any(|saved| saved.profile_key == profile.username)
    {
        return ApiResponse::error(
            409,
            format!(
                "up to {} Letterboxd profiles can be connected",
                letterboxd_integration::MAX_CONNECTED_PROFILES
            ),
        );
    }
    let verification = letterboxd_integration::verify(&profile);
    let display_name = verification
        .display_name()
        .unwrap_or(&profile.username)
        .to_string();
    let now = unix_now();
    let record = ExternalProfile {
        id: crate::app::ids::random_hex(16),
        provider: "letterboxd".to_string(),
        profile_key: profile.username.clone(),
        display_name,
        canonical_url: profile.canonical_url,
        enabled: true,
        verification_status: verification.as_str().to_string(),
        created_at: now,
        last_checked_at: Some(now),
        jellyfin_server_id: server_id,
        jellyfin_user_id: user_id,
    };
    match services.library.save_external_profile(&record) {
        Ok(profile) => ApiResponse::ok(json!({ "profile": profile })),
        Err(error) => storage_failure(&error),
    }
}

fn letterboxd_set_enabled(services: &Arc<Services>, id: &str, request: &ApiRequest) -> ApiResponse {
    if !valid_profile_id(id) {
        return ApiResponse::error(404, "profile not found");
    }
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let Some(enabled) = request.json()["enabled"].as_bool() else {
        return ApiResponse::error(400, "enabled must be true or false");
    };
    match services
        .library
        .set_external_profile_enabled(id, &server_id, &user_id, enabled)
    {
        Ok(Some(profile)) => ApiResponse::ok(json!({ "profile": profile })),
        Ok(None) => ApiResponse::error(404, "profile not found"),
        Err(error) => storage_failure(&error),
    }
}

fn letterboxd_remove_profile(services: &Arc<Services>, id: &str) -> ApiResponse {
    if !valid_profile_id(id) {
        return ApiResponse::error(404, "profile not found");
    }
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    match services
        .library
        .remove_external_profile(id, &server_id, &user_id)
    {
        Ok(true) => ApiResponse::ok(json!({ "removed": true })),
        Ok(false) => ApiResponse::error(404, "profile not found"),
        Err(error) => storage_failure(&error),
    }
}

fn letterboxd_refresh_profile(services: &Arc<Services>, id: &str) -> ApiResponse {
    if !valid_profile_id(id) {
        return ApiResponse::error(404, "profile not found");
    }
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let existing = match services.library.external_profile(id, &server_id, &user_id) {
        Ok(Some(profile)) if profile.provider == "letterboxd" => profile,
        Ok(_) => return ApiResponse::error(404, "profile not found"),
        Err(error) => return storage_failure(&error),
    };
    let source = match letterboxd_integration::normalize_profile(&existing.profile_key) {
        Ok(profile) => profile,
        Err(_) => return ApiResponse::error(409, "stored Letterboxd profile is invalid"),
    };
    let verification = letterboxd_integration::verify(&source);
    let display_name = verification
        .display_name()
        .map(str::to_string)
        .unwrap_or_else(|| existing.display_name.clone());
    let record = ExternalProfile {
        canonical_url: source.canonical_url,
        display_name,
        verification_status: verification.as_str().to_string(),
        last_checked_at: Some(unix_now()),
        ..existing
    };
    match services.library.save_external_profile(&record) {
        Ok(profile) => ApiResponse::ok(json!({ "profile": profile })),
        Err(error) => storage_failure(&error),
    }
}

fn letterboxd_open_profile(services: &Arc<Services>, id: &str) -> ApiResponse {
    if !valid_profile_id(id) {
        return ApiResponse::error(404, "profile not found");
    }
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    match services.library.external_profile(id, &server_id, &user_id) {
        Ok(Some(profile)) if profile.provider == "letterboxd" => {
            super::super::bridge::open_external_link(&profile.canonical_url);
            ApiResponse::ok(json!({ "opened": true, "url": profile.canonical_url }))
        }
        Ok(_) => ApiResponse::error(404, "profile not found"),
        Err(error) => storage_failure(&error),
    }
}

fn item_letterboxd(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let item = match services.library.item(item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return ApiResponse::error(404, "item not found"),
        Err(error) => return storage_failure(&error),
    };
    // Letterboxd's RSS movieId namespace is TMDB's movie namespace. Refuse a
    // series or episode even if it happens to carry the same numeric provider
    // id, or a TV record could inherit an unrelated film review.
    if item["kind"].as_str() != Some("Movie") {
        return ApiResponse::ok(json!({
            "reviews": [],
            "configuredProfiles": 0,
            "unavailableProfiles": 0,
        }));
    }
    let Some(tmdb_id) = item["providerIds"]["tmdb"]
        .as_str()
        .filter(|value| !value.is_empty())
    else {
        return ApiResponse::ok(json!({
            "reviews": [],
            "configuredProfiles": 0,
            "unavailableProfiles": 0,
        }));
    };
    letterboxd_reviews_for_movie(services, &server_id, &user_id, tmdb_id)
}

fn movie_letterboxd(services: &Arc<Services>, tmdb_id: &str) -> ApiResponse {
    let Some(tmdb_id) = canonical_tmdb_movie_id(tmdb_id) else {
        return ApiResponse::error(400, "that is not a TMDB movie id");
    };
    let (server_id, user_id) = match letterboxd_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    letterboxd_reviews_for_movie(services, &server_id, &user_id, &tmdb_id)
}

fn canonical_tmdb_movie_id(value: &str) -> Option<String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let id = value.parse::<i64>().ok()?;
    (id > 0).then(|| id.to_string())
}

fn letterboxd_reviews_for_movie(
    services: &Arc<Services>,
    server_id: &str,
    user_id: &str,
    tmdb_id: &str,
) -> ApiResponse {
    let profiles = match services
        .library
        .external_profiles("letterboxd", server_id, user_id)
    {
        Ok(profiles) => profiles,
        Err(error) => return storage_failure(&error),
    };
    ApiResponse::ok(json!(
        services.letterboxd.reviews_for_item(&profiles, tmdb_id)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovered_letterboxd_lookups_require_a_positive_tmdb_movie_id() {
        assert_eq!(canonical_tmdb_movie_id("603").as_deref(), Some("603"));
        assert_eq!(canonical_tmdb_movie_id("000603").as_deref(), Some("603"));
        for invalid in ["", "0", "+603", "-1", "603.0", "movie-603"] {
            assert_eq!(canonical_tmdb_movie_id(invalid), None, "{invalid}");
        }
    }
}
