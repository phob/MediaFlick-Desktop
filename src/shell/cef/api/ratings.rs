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

#[derive(Deserialize)]
struct RatingsBatchBody {
    ids: Vec<String>,
}

fn ratings_batch(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = match request.body::<RatingsBatchBody>() {
        Ok(body) => body,
        Err(response) => return response,
    };
    match services.ratings.batch(&body.ids) {
        Ok(ratings) => ApiResponse::ok(ratings),
        Err(error) => ApiResponse::error(500, error.to_string()),
    }
}
