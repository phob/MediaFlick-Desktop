//! TMDB movie collections, resolved by the Companion plugin. The whole
//! surface degrades to a capability error when the plugin is absent or too
//! old; the UI hides the category in that case instead of showing it.

use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["collections"] if request.is("GET") => match services.companion.collections() {
            Ok(value) => ApiResponse::ok(value),
            Err(error) => ApiResponse::from_api_error(&error),
        },
        ["collections", "movie", tmdb_id] if request.is("GET") => {
            let id = positive_id(&percent_decode(tmdb_id));
            match id {
                Ok(id) => match services.companion.movie_collection(id) {
                    Ok(value) => ApiResponse::ok(value),
                    Err(error) => ApiResponse::from_api_error(&error),
                },
                Err(message) => ApiResponse::error(400, message),
            }
        }
        ["collections", collection_id] if request.is("GET") => {
            let id = positive_id(&percent_decode(collection_id));
            match id {
                Ok(id) => match services.companion.collection_detail(id) {
                    Ok(value) => ApiResponse::ok(value),
                    Err(error) => ApiResponse::from_api_error(&error),
                },
                Err(message) => ApiResponse::error(400, message),
            }
        }
        _ => return None,
    };
    Some(response)
}

fn positive_id(raw: &str) -> Result<i64, &'static str> {
    raw.parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or("that is not a TMDB collection or movie id")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_must_be_positive_integers() {
        assert_eq!(positive_id("10"), Ok(10));
        assert_eq!(positive_id("603").ok(), Some(603));
        for bad in ["not-a-number", "0", "-5", ""] {
            assert!(positive_id(bad).is_err(), "{bad}");
        }
    }
}
