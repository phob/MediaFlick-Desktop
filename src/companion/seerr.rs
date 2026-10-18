//! Seerr through the Companion: discovery, details and requests, joined
//! against the local library so rows know which titles are owned.

use serde_json::{Value, json};

use crate::jellyfin::api::ApiError;
use crate::jellyfin::session::SessionScope;
use crate::library::Library;
use crate::seerr::{DiscoverKind, DiscoverOptions, RequestProfileSelection};

use super::CompanionSession;

impl CompanionSession {
    fn get_seerr(
        &self,
        scope: &SessionScope,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value, ApiError> {
        self.fetch(scope, |client, _| client.companion_get_json(path, query))
    }

    fn post_seerr(
        &self,
        scope: &SessionScope,
        path: &str,
        body: &Value,
    ) -> Result<Value, ApiError> {
        self.fetch(scope, |client, _| {
            client.companion_post_json_once(path, body)
        })
    }

    pub fn seerr_status(&self, scope: &SessionScope) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.get_seerr(scope, "/MediaFlick/seerr/status", &[])
    }

    pub fn seerr_search(
        &self,
        scope: &SessionScope,
        query: &str,
        page: i64,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.get_seerr(
            scope,
            "/MediaFlick/seerr/search",
            &[("query", query.to_string()), ("page", page.to_string())],
        )?;
        join_seerr_results(&self.library, &mut value);
        Ok(value)
    }

    pub fn seerr_person_credits(
        &self,
        scope: &SessionScope,
        tmdb_id: i64,
    ) -> Result<Value, ApiError> {
        self.require_capability(
            "seerr-person-discovery",
            "the MediaFlick Companion plugin must be updated for cast discovery",
        )?;
        let mut value = self.get_seerr(
            scope,
            &format!("/MediaFlick/seerr/person/{tmdb_id}/credits"),
            &[],
        )?;
        join_seerr_results(&self.library, &mut value);
        Ok(value)
    }

    pub fn seerr_discover(
        &self,
        scope: &SessionScope,
        kind: DiscoverKind,
        page: i64,
        options: &DiscoverOptions,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let query = options.query_pairs(kind, page);
        let mut value = self.get_seerr(
            scope,
            &format!("/MediaFlick/seerr/discover/{}", kind.id()),
            query.as_slice(),
        )?;
        join_seerr_results(&self.library, &mut value);
        Ok(value)
    }

    pub fn seerr_genres(&self, scope: &SessionScope, media_type: &str) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.get_seerr(
            scope,
            &format!("/MediaFlick/seerr/genres/{media_type}"),
            &[],
        )
    }

    pub fn seerr_media(
        &self,
        scope: &SessionScope,
        media_type: &str,
        tmdb_id: i64,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.get_seerr(
            scope,
            &format!("/MediaFlick/seerr/media/{media_type}/{tmdb_id}"),
            &[],
        )?;
        join_seerr_item(&self.library, &mut value);
        Ok(value)
    }

    pub fn seerr_request_options(
        &self,
        scope: &SessionScope,
        media_type: &str,
        is_4k: bool,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.get_seerr(
            scope,
            &format!("/MediaFlick/seerr/request-options/{media_type}"),
            &[("is4k", is_4k.to_string())],
        )
    }

    pub fn seerr_create_request(
        &self,
        scope: &SessionScope,
        media_type: &str,
        tmdb_id: i64,
        seasons: Option<&[i64]>,
        is_4k: bool,
        profile: Option<RequestProfileSelection>,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.post_seerr(
            scope,
            "/MediaFlick/seerr/request",
            &json!({
                "mediaType": media_type,
                "tmdbId": tmdb_id,
                "seasons": seasons,
                "is4k": is_4k,
                "serverId": profile.map(|selection| selection.server_id),
                "profileId": profile.map(|selection| selection.profile_id),
            }),
        )?;
        join_seerr_item(&self.library, &mut value);
        Ok(value)
    }

    pub fn seerr_requests(
        &self,
        scope: &SessionScope,
        take: i64,
        skip: i64,
        filter: &str,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.get_seerr(
            scope,
            "/MediaFlick/seerr/requests",
            &[
                ("take", take.to_string()),
                ("skip", skip.to_string()),
                ("filter", filter.to_string()),
            ],
        )?;
        join_seerr_results(&self.library, &mut value);
        Ok(value)
    }

    pub fn seerr_cancel_request(
        &self,
        scope: &SessionScope,
        request_id: i64,
    ) -> Result<(), ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.fetch(scope, |client, _| {
            client.companion_delete_once(&format!("/MediaFlick/seerr/request/{request_id}"))
        })?;
        Ok(())
    }
}

fn join_seerr_results(library: &Library, value: &mut Value) {
    join_seerr_rows(library, value, "results");
}

/// Joins one array of shaped Seerr rows against the local cache. `field` is
/// the array's name: discovery pages answer under `results`, collection
/// details under `parts`.
pub(super) fn join_seerr_rows(library: &Library, value: &mut Value, field: &str) {
    let Some(results) = value.get_mut(field).and_then(Value::as_array_mut) else {
        return;
    };
    let movies = provider_ids(results, "movie");
    let series = provider_ids(results, "tv");
    let movie_ids = library.ids_by_tmdb("Movie", &movies).unwrap_or_default();
    let series_ids = library.ids_by_tmdb("Series", &series).unwrap_or_default();
    let movie_played = library.played_by_tmdb("Movie", &movies).unwrap_or_default();
    for item in results {
        let media_type = item["mediaType"].as_str().unwrap_or_default();
        let Some(tmdb_id) = item["tmdbId"].as_i64() else {
            continue;
        };
        let (owned, played) = if media_type == "movie" {
            (&movie_ids, movie_played.get(&tmdb_id.to_string()).copied())
        } else {
            (&series_ids, None)
        };
        item["libraryItemId"] = owned
            .get(&tmdb_id.to_string())
            .map_or(Value::Null, |id| json!(id));
        item["played"] = json!(played.unwrap_or(false));
    }
}

fn provider_ids(results: &[Value], media_type: &str) -> Vec<String> {
    results
        .iter()
        .filter(|item| item["mediaType"].as_str() == Some(media_type))
        .filter_map(|item| item["tmdbId"].as_i64())
        .map(|id| id.to_string())
        .collect()
}

fn join_seerr_item(library: &Library, value: &mut Value) {
    let media_type = value["mediaType"].as_str().unwrap_or_default();
    let Some(tmdb_id) = value["tmdbId"].as_i64() else {
        return;
    };
    let kind = if media_type == "movie" {
        "Movie"
    } else {
        "Series"
    };
    value["libraryItemId"] = library
        .ids_by_tmdb(kind, &[tmdb_id.to_string()])
        .unwrap_or_default()
        .remove(&tmdb_id.to_string())
        .map_or(Value::Null, Value::String);
}
