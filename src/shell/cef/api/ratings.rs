use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["integrations", "ratings"] if request.is("GET") => ratings_status(services),
        ["integrations", "ratings", "credential", provider] if request.is("PUT") => {
            ratings_save_credential(services, &percent_decode(provider), request)
        }
        ["integrations", "ratings", "credential", provider] if request.is("DELETE") => {
            ratings_remove_credential(services, &percent_decode(provider))
        }
        [
            "integrations",
            "ratings",
            "credential",
            provider,
            "validate",
        ] if request.is("POST") => ratings_validate_credential(services, &percent_decode(provider)),
        ["integrations", "ratings", "credential", provider, "reveal"] if request.is("POST") => {
            ratings_reveal_credential(services, &percent_decode(provider))
        }
        ["ratings", "batch"] if request.is("POST") => ratings_batch(services, request),
        _ => return None,
    };
    Some(response)
}

fn ratings_sources(services: &Arc<Services>) -> Vec<String> {
    services.preferences.snapshot().appearance.rating_sources
}

fn ratings_status(services: &Arc<Services>) -> ApiResponse {
    ApiResponse::ok(services.ratings.status(&ratings_sources(services)))
}

fn ratings_save_credential(
    services: &Arc<Services>,
    provider: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let Some(key) = request
        .json()
        .get("key")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return ApiResponse::error(400, "an API key is required");
    };
    match services
        .ratings
        .save_credential(provider, &key, &ratings_sources(services))
    {
        Ok(status) => ApiResponse::ok(status),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn ratings_remove_credential(services: &Arc<Services>, provider: &str) -> ApiResponse {
    match services
        .ratings
        .remove_credential(provider, &ratings_sources(services))
    {
        Ok(status) => ApiResponse::ok(status),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn ratings_validate_credential(services: &Arc<Services>, provider: &str) -> ApiResponse {
    match services
        .ratings
        .validate_credential(provider, &ratings_sources(services))
    {
        Ok(status) => ApiResponse::ok(status),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn ratings_reveal_credential(services: &Arc<Services>, provider: &str) -> ApiResponse {
    match services.ratings.reveal_credential(provider) {
        Ok(key) => ApiResponse::ok(json!({ "key": key })),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn ratings_batch(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let ids = request
        .json()
        .get("ids")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match services.ratings.batch(&ids) {
        Ok(ratings) => ApiResponse::ok(ratings),
        Err(error) => ApiResponse::error(500, error.to_string()),
    }
}
