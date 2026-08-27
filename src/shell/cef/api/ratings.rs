use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["integrations", "ratings"] if request.is("GET") => ratings_status(services),
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
