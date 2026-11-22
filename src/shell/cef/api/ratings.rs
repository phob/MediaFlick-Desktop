use crate::integrations::mdblist::{RatingsBatchResponse, RatingsError, RatingsIntegrationStatus};

use super::*;

routes! {
    |services, request|
    GET ["integrations", "ratings"] -> RatingsIntegrationStatus = Ok(ratings_status(services)),
    POST ["ratings", "batch"] (body: RatingsBatchBody) -> RatingsBatchResponse =
        ratings_batch(services, request, body),
}

fn ratings_sources(services: &Arc<Services>) -> Vec<String> {
    services.preferences.snapshot().appearance.rating_sources
}

fn ratings_status(services: &Arc<Services>) -> RatingsIntegrationStatus {
    services.ratings.status(ratings_sources(services))
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct RatingsBatchBody {
    ids: Vec<String>,
}

fn ratings_batch(
    services: &Arc<Services>,
    request: &ApiRequest,
    body: Body<'_, RatingsBatchBody>,
) -> Result<RatingsBatchResponse, ApiResponse> {
    let body = body.decode()?;
    if request.is_cancelled() {
        return Err(ApiResponse::from_api_error(&ApiError::Cancelled));
    }
    match services.ratings.batch(&body.ids, Some(&request.cancelled)) {
        Ok(ratings) => Ok(ratings),
        Err(RatingsError::AccountChanged) => Err(stale_account_response()),
        Err(RatingsError::Cancelled) => Err(ApiResponse::from_api_error(&ApiError::Cancelled)),
        Err(error) => Err(ApiResponse::error(500, error.to_string())),
    }
}

#[cfg(test)]
mod tests;
