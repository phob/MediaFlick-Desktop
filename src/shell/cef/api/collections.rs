//! Collections. Two sources share this surface:
//!
//! - Native Jellyfin BoxSets (`collections-v2`): the Companion plugin mirrors
//!   TMDB collections into real server collections, and Desktop lists and
//!   browses them through ordinary `/Items` queries.
//! - Derived TMDB summaries (`collections-v1`): the fallback when mirroring is
//!   off or the plugin predates it.
//!
//! The whole surface degrades to a capability error when the plugin is absent
//! or too old; the UI hides the category in that case instead of showing it.

use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["collections"] if request.is("GET") => {
            if services.companion.native_collections() {
                native_index(services)
            } else {
                match services.companion.collections() {
                    Ok(value) => ApiResponse::ok(value),
                    Err(error) => ApiResponse::from_api_error(&error),
                }
            }
        }
        ["collections", "movie", tmdb_id] if request.is("GET") => {
            let id = positive_id(&percent_decode(tmdb_id));
            match id {
                Ok(id) => match services.companion.movie_collection(id) {
                    Ok(mut value) => {
                        // When mirroring is on, detail links point at the
                        // server's own BoxSet page rather than a derived view.
                        if services.companion.native_collections() {
                            link_boxset_for_movie(services, &mut value);
                        }
                        ApiResponse::ok(value)
                    }
                    Err(error) => ApiResponse::from_api_error(&error),
                },
                Err(message) => ApiResponse::error(400, message),
            }
        }
        ["collections", "boxset", boxset_id] if request.is("GET") => {
            boxset_detail(services, &percent_decode(boxset_id))
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

/// The native listing answers from the server's BoxSets directly, so it works
/// even while Seerr mappings are still converging in the background sync.
fn native_index(services: &Arc<Services>) -> ApiResponse {
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_box_sets(&client, &user_id) {
        Ok(response) => {
            let collections = response
                .items
                .iter()
                .map(|dto| {
                    json!({
                        "id": dto.id,
                        "tmdbId": dto
                            .provider_id("Tmdb")
                            .and_then(|value| value.parse::<i64>().ok())
                            .filter(|id| *id > 0),
                        "name": dto.display_name(),
                        "posterPath": Value::Null,
                        "backdropPath": Value::Null,
                        "primaryImageTag": dto.primary_image_tag(),
                        "backdropImageTag": dto.backdrop_image_tags.first(),
                        "movieCount": dto.child_count,
                    })
                })
                .collect::<Vec<_>>();
            ApiResponse::ok(json!({
                "source": "jellyfin",
                "collections": collections,
            }))
        }
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

/// One BoxSet's own record joined with its movie children as ordinary card rows.
fn boxset_detail(services: &Arc<Services>, id: &str) -> ApiResponse {
    if id.is_empty() {
        return ApiResponse::error(400, "that is not a collection id");
    }
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };

    let set = match items::fetch_item(&client, &user_id, id) {
        Ok(Some(dto)) => dto,
        Ok(None) => return ApiResponse::error(404, "that collection does not exist"),
        Err(error) => {
            services.session.note_error(&error);
            return ApiResponse::from_api_error(&error);
        }
    };
    let children = match items::fetch_box_set_children(&client, &user_id, id) {
        Ok(response) => response,
        Err(error) => {
            services.session.note_error(&error);
            return ApiResponse::from_api_error(&error);
        }
    };

    ApiResponse::ok(json!({
        "id": set.id,
        "name": set.display_name(),
        "tmdbId": set
            .provider_id("Tmdb")
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|id| *id > 0),
        "primaryImageTag": set.primary_image_tag(),
        "backdropImageTag": set.backdrop_image_tags.first(),
        "items": children.items.iter().map(summary_from_dto).collect::<Vec<_>>(),
        "totalRecordCount": children.total_record_count,
    }))
}

/// Rewrites a movie's collection link to the mirrored BoxSet when one exists;
/// a set the background sync has not created yet keeps the TMDB identity, and
/// the derived detail view still renders it.
fn link_boxset_for_movie(services: &Arc<Services>, value: &mut Value) {
    let Some(tmdb_id) = value["collection"]
        .get("id")
        .and_then(Value::as_i64)
        .filter(|id| *id > 0)
    else {
        return;
    };
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(_) => return,
    };
    let Ok(response) = items::fetch_box_sets(&client, &user_id) else {
        return;
    };
    rewrite_collection_to_boxset(value, tmdb_id, &response.items);
}

/// Matches by TMDB provider id only — never by name, which two automations
/// can disagree on.
fn rewrite_collection_to_boxset(value: &mut Value, tmdb_id: i64, box_sets: &[BaseItemDto]) {
    for dto in box_sets {
        if dto
            .provider_id("Tmdb")
            .and_then(|value| value.parse::<i64>().ok())
            == Some(tmdb_id)
        {
            value["collection"]["id"] = json!(dto.id);
            return;
        }
    }
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
    fn a_movie_collection_link_moves_to_the_mirrored_boxset() {
        let mirrored = serde_json::from_str::<BaseItemDto>(
            r#"{"Id":"boxset-1","Name":"The Matrix Collection","ProviderIds":{"Tmdb":"10"}}"#,
        )
        .expect("box set");
        let other = serde_json::from_str::<BaseItemDto>(
            r#"{"Id":"boxset-2","Name":"Alien Collection","ProviderIds":{"Tmdb":"2"}}"#,
        )
        .expect("box set");
        let mut value = json!({ "tmdbId": 603, "collection": { "id": 10, "name": "x" } });

        rewrite_collection_to_boxset(&mut value, 10, &[other.clone(), mirrored]);

        assert_eq!(value["collection"]["id"], "boxset-1");

        // No matching BoxSet yet keeps the derived identity.
        let mut unmirrored = json!({ "tmdbId": 603, "collection": { "id": 10 } });
        rewrite_collection_to_boxset(&mut unmirrored, 10, &[other]);
        assert_eq!(unmirrored["collection"]["id"], 10);
    }

    #[test]
    fn ids_must_be_positive_integers() {
        assert_eq!(positive_id("10"), Ok(10));
        assert_eq!(positive_id("603").ok(), Some(603));
        for bad in ["not-a-number", "0", "-5", ""] {
            assert!(positive_id(bad).is_err(), "{bad}");
        }
    }
}
