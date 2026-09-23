use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<Handled> {
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

fn ratings_status(services: &Arc<Services>) -> Handled {
    Ok(ApiResponse::ok(
        services.ratings.status(&ratings_sources(services)),
    ))
}

#[derive(Deserialize)]
struct RatingsBatchBody {
    ids: Vec<String>,
}

fn ratings_batch(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let body = request.body::<RatingsBatchBody>()?;
    match services.ratings.batch(&body.ids) {
        Ok(ratings) => Ok(ApiResponse::ok(ratings)),
        Err(error) => Err(ApiResponse::error(500, error.to_string())),
    }
}
