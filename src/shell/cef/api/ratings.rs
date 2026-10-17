use crate::integrations::mdblist::{RatingsBatchResponse, RatingsIntegrationStatus};

use super::*;

routes! {
    |services, request|
    GET ["integrations", "ratings"] -> RatingsIntegrationStatus = Ok(ratings_status(services)),
    POST ["ratings", "batch"] (body: RatingsBatchBody) -> RatingsBatchResponse =
        ratings_batch(services, body),
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
    body: Body<'_, RatingsBatchBody>,
) -> Result<RatingsBatchResponse, ApiResponse> {
    let body = body.decode()?;
    match services.ratings.batch(&body.ids) {
        Ok(ratings) => Ok(ratings),
        Err(error) => Err(ApiResponse::error(500, error.to_string())),
    }
}
