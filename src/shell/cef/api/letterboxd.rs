use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<Handled> {
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

fn letterboxd_scope(services: &Arc<Services>) -> Result<SessionScope, ApiResponse> {
    services
        .session
        .scope()
        .map_err(|_| ApiResponse::error(401, "sign in to manage connected profiles"))
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

fn letterboxd_profiles(services: &Arc<Services>) -> Handled {
    let scope = letterboxd_scope(services)?;
    let profiles = services.accounts.letterboxd_profiles(scope.account());
    Ok(ApiResponse::ok(json!({ "profiles": profiles })))
}

#[derive(Deserialize)]
struct AddProfileBody {
    profile: String,
}

#[derive(Deserialize)]
struct EnabledBody {
    enabled: bool,
}

fn letterboxd_add_profile(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let body = request.body::<AddProfileBody>()?;
    let scope = letterboxd_scope(services)?;
    let account = scope.account().clone();
    let profile = match letterboxd_integration::normalize_profile(&body.profile) {
        Ok(profile) => profile,
        Err(error) => return Err(ApiResponse::error(400, error)),
    };
    let existing = services.accounts.letterboxd_profiles(&account);
    if existing.len() >= letterboxd_integration::MAX_CONNECTED_PROFILES
        && !existing
            .iter()
            .any(|saved| saved.profile_key == profile.username)
    {
        return Err(ApiResponse::error(
            409,
            format!(
                "up to {} Letterboxd profiles can be connected",
                letterboxd_integration::MAX_CONNECTED_PROFILES
            ),
        ));
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
        jellyfin_server_id: account.server_id().to_string(),
        jellyfin_user_id: account.user_id().to_string(),
    };
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(
                match services.accounts.save_letterboxd_profile(&account, &record) {
                    Ok(profile) => ApiResponse::ok(json!({ "profile": profile })),
                    Err(error) => account_config_failure(&error),
                },
            )
        })
}

fn letterboxd_set_enabled(services: &Arc<Services>, id: &str, request: &ApiRequest) -> Handled {
    if !valid_profile_id(id) {
        return Err(ApiResponse::error(404, "profile not found"));
    }
    let enabled = request.body::<EnabledBody>()?.enabled;
    let scope = letterboxd_scope(services)?;
    let account = scope.account().clone();
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(
                match services
                    .accounts
                    .set_letterboxd_profile_enabled(&account, id, enabled)
                {
                    Ok(Some(profile)) => ApiResponse::ok(json!({ "profile": profile })),
                    Ok(None) => ApiResponse::error(404, "profile not found"),
                    Err(error) => account_config_failure(&error),
                },
            )
        })
}

fn letterboxd_remove_profile(services: &Arc<Services>, id: &str) -> Handled {
    if !valid_profile_id(id) {
        return Err(ApiResponse::error(404, "profile not found"));
    }
    let scope = letterboxd_scope(services)?;
    let account = scope.account().clone();
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(
                match services.accounts.remove_letterboxd_profile(&account, id) {
                    Ok(true) => ApiResponse::ok(json!({ "removed": true })),
                    Ok(false) => ApiResponse::error(404, "profile not found"),
                    Err(error) => account_config_failure(&error),
                },
            )
        })
}

fn letterboxd_refresh_profile(services: &Arc<Services>, id: &str) -> Handled {
    if !valid_profile_id(id) {
        return Err(ApiResponse::error(404, "profile not found"));
    }
    let scope = letterboxd_scope(services)?;
    let account = scope.account().clone();
    let existing = match services.accounts.letterboxd_profile(&account, id) {
        Some(profile) if profile.provider == "letterboxd" => profile,
        _ => return Err(ApiResponse::error(404, "profile not found")),
    };
    let source = match letterboxd_integration::normalize_profile(&existing.profile_key) {
        Ok(profile) => profile,
        Err(_) => {
            return Err(ApiResponse::error(
                409,
                "stored Letterboxd profile is invalid",
            ));
        }
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
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(
                match services.accounts.save_letterboxd_profile(&account, &record) {
                    Ok(profile) => ApiResponse::ok(json!({ "profile": profile })),
                    Err(error) => account_config_failure(&error),
                },
            )
        })
}

fn letterboxd_open_profile(services: &Arc<Services>, id: &str) -> Handled {
    if !valid_profile_id(id) {
        return Err(ApiResponse::error(404, "profile not found"));
    }
    let scope = letterboxd_scope(services)?;
    match services.accounts.letterboxd_profile(scope.account(), id) {
        Some(profile) if profile.provider == "letterboxd" => {
            super::super::bridge::open_external_link(&profile.canonical_url);
            Ok(ApiResponse::ok(
                json!({ "opened": true, "url": profile.canonical_url }),
            ))
        }
        _ => Err(ApiResponse::error(404, "profile not found")),
    }
}

fn item_letterboxd(services: &Arc<Services>, item_id: &str) -> Handled {
    let scope = letterboxd_scope(services)?;
    let item = match services.library.item(item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return Err(ApiResponse::error(404, "item not found")),
        Err(error) => return Err(storage_failure(&error)),
    };
    // Letterboxd's RSS movieId namespace is TMDB's movie namespace. Refuse a
    // series or episode even if it happens to carry the same numeric provider
    // id, or a TV record could inherit an unrelated film review.
    if item.summary.kind != "Movie" {
        return Ok(ApiResponse::ok(json!({
            "reviews": [],
            "configuredProfiles": 0,
            "unavailableProfiles": 0,
        })));
    }
    let Some(tmdb_id) = item
        .provider_ids
        .tmdb
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return Ok(ApiResponse::ok(json!({
            "reviews": [],
            "configuredProfiles": 0,
            "unavailableProfiles": 0,
        })));
    };
    letterboxd_reviews_for_movie(services, scope.account(), tmdb_id)
}

fn movie_letterboxd(services: &Arc<Services>, tmdb_id: &str) -> Handled {
    let Some(tmdb_id) = canonical_tmdb_movie_id(tmdb_id) else {
        return Err(ApiResponse::error(400, "that is not a TMDB movie id"));
    };
    let scope = letterboxd_scope(services)?;
    letterboxd_reviews_for_movie(services, scope.account(), &tmdb_id)
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
    account: &AccountKey,
    tmdb_id: &str,
) -> Handled {
    let profiles = services.accounts.letterboxd_profiles(account);
    Ok(ApiResponse::ok(json!(
        services.letterboxd.reviews_for_item(&profiles, tmdb_id)
    )))
}

fn account_config_failure(error: &std::io::Error) -> ApiResponse {
    tracing::error!("could not save account configuration: {error}");
    ApiResponse::error(500, "could not save account configuration")
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
