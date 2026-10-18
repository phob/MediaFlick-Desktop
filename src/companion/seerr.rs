//! Seerr through the Companion: discovery, details and requests, joined
//! against the local library so rows know which titles are owned.

use serde_json::{Value, json};

use crate::jellyfin::api::ApiError;
use crate::jellyfin::session::SessionScope;
use crate::library::{CatalogKind, Library, TitleKind};
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
        join_seerr_results(&self.library, &mut value, OtherKinds::Drop);
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
        join_seerr_results(&self.library, &mut value, OtherKinds::Drop);
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
        join_seerr_results(&self.library, &mut value, OtherKinds::Drop);
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
        join_seerr_results(&self.library, &mut value, OtherKinds::Keep);
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

fn join_seerr_results(library: &Library, value: &mut Value, other_kinds: OtherKinds) {
    join_seerr_rows(library, value, "results", other_kinds);
}

/// What a joined array does with a row whose kind is neither `movie` nor
/// `tv`, such as a person.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum OtherKinds {
    /// Search, discover and person credits feed request cards, which need a
    /// known kind.
    Drop,
    /// The user's requests and a collection's parts show every row, with no
    /// library title.
    Keep,
}

/// Joins one array of shaped Seerr rows against the local cache. `field` is
/// the array's name: discovery pages answer under `results`, collection
/// details under `parts`.
pub(super) fn join_seerr_rows(
    library: &Library,
    value: &mut Value,
    field: &str,
    other_kinds: OtherKinds,
) {
    let Some(results) = value.get_mut(field).and_then(Value::as_array_mut) else {
        return;
    };
    if other_kinds == OtherKinds::Drop {
        results.retain(|item| seerr_kind(item).is_some());
    }
    let kinds = results.iter().map(seerr_kind).collect::<Vec<_>>();
    let movies = tmdb_ids(results, &kinds, TitleKind::Movie);
    let series = tmdb_ids(results, &kinds, TitleKind::Series);
    let movie_ids = library
        .ids_by_tmdb(CatalogKind::Movie, &movies)
        .unwrap_or_default();
    let series_ids = library
        .ids_by_tmdb(CatalogKind::Series, &series)
        .unwrap_or_default();
    let movie_played = library
        .played_by_tmdb(CatalogKind::Movie, &movies)
        .unwrap_or_default();
    for (item, kind) in results.iter_mut().zip(kinds) {
        let Some(tmdb_id) = item["tmdbId"].as_i64().map(|id| id.to_string()) else {
            continue;
        };
        let (owned, played) = match kind {
            Some(TitleKind::Movie) => (movie_ids.get(&tmdb_id), movie_played.get(&tmdb_id)),
            Some(TitleKind::Series) => (series_ids.get(&tmdb_id), None),
            None => (None, None),
        };
        item["libraryItemId"] = owned.map_or(Value::Null, |id| json!(id));
        item["played"] = json!(played.copied().unwrap_or(false));
    }
}

/// Seerr names movies `movie` and series `tv`; persons and anything else
/// have no library title.
fn seerr_kind(item: &Value) -> Option<TitleKind> {
    match item["mediaType"].as_str()? {
        "movie" => Some(TitleKind::Movie),
        "tv" => Some(TitleKind::Series),
        _ => None,
    }
}

fn tmdb_ids(results: &[Value], kinds: &[Option<TitleKind>], kind: TitleKind) -> Vec<String> {
    results
        .iter()
        .zip(kinds)
        .filter(|(_, row_kind)| **row_kind == Some(kind))
        .filter_map(|(item, _)| item["tmdbId"].as_i64())
        .map(|id| id.to_string())
        .collect()
}

fn join_seerr_item(library: &Library, value: &mut Value) {
    let Some(tmdb_id) = value["tmdbId"].as_i64() else {
        return;
    };
    let library_item_id = seerr_kind(value).and_then(|kind| {
        library
            .ids_by_tmdb(kind.into(), &[tmdb_id.to_string()])
            .unwrap_or_default()
            .remove(&tmdb_id.to_string())
    });
    value["libraryItemId"] = library_item_id.map_or(Value::Null, Value::String);
}
