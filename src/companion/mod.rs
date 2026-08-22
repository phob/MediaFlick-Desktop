//! MediaFlick Companion discovery and provider selection.
//!
//! The server plugin is optional. Its API is probed with the authenticated
//! Jellyfin client and cached for the life of that login; every feature then
//! selects the plugin backend only when the advertised v1 capability exists.

use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::jellyfin::api::items;
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::session::Session;
use crate::library::Library;
use crate::seerr::api::error::SeerrError;
use crate::seerr::{DiscoverKind, DiscoverOptions, RequestProfileSelection, SeerrSession};

const MIN_API_VERSION: i64 = 1;
const MAX_API_VERSION: i64 = 1;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CompanionInfo {
    pub plugin_version: String,
    pub api_version: i64,
    pub capabilities: Vec<String>,
    pub services: std::collections::BTreeMap<String, bool>,
}

impl CompanionInfo {
    pub fn is_compatible(&self) -> bool {
        (MIN_API_VERSION..=MAX_API_VERSION).contains(&self.api_version)
    }

    pub fn supports(&self, capability: &str) -> bool {
        self.is_compatible()
            && self
                .capabilities
                .iter()
                .any(|candidate| candidate == capability)
    }
}

#[derive(Debug, Clone, Default)]
struct ProbeState {
    checked: bool,
    info: Option<CompanionInfo>,
    error: Option<String>,
}

pub struct CompanionSession {
    session: Arc<Session>,
    library: Arc<Library>,
    state: RwLock<ProbeState>,
}

impl CompanionSession {
    pub fn new(session: Arc<Session>, library: Arc<Library>) -> Self {
        Self {
            session,
            library,
            state: RwLock::new(ProbeState::default()),
        }
    }

    pub fn probe(&self, force: bool) -> Result<Option<CompanionInfo>, ApiError> {
        if !force {
            let state = self.read();
            if state.checked {
                return Ok(state.info);
            }
        }

        if !self.session.is_authenticated() {
            self.replace(ProbeState {
                checked: true,
                info: None,
                error: None,
            });
            return Ok(None);
        }

        let client = self.session.client()?;
        match client.companion_get_info_json::<CompanionInfo>("/MediaFlick/info") {
            Ok(info) => {
                let compatible = info.is_compatible();
                tracing::info!(
                    target: "companion",
                    plugin_version = %info.plugin_version,
                    api_version = info.api_version,
                    compatible,
                    "probed the MediaFlick Companion plugin"
                );
                self.replace(ProbeState {
                    checked: true,
                    error: (!compatible).then(|| {
                        format!(
                            "companion API {} is outside the supported range {}–{}",
                            info.api_version, MIN_API_VERSION, MAX_API_VERSION
                        )
                    }),
                    info: Some(info.clone()),
                });
                Ok(Some(info))
            }
            Err(ApiError::Status { status: 404 } | ApiError::Remote { status: 404, .. }) => {
                self.replace(ProbeState {
                    checked: true,
                    info: None,
                    error: None,
                });
                tracing::debug!(target: "companion", "the server has no companion plugin");
                Ok(None)
            }
            Err(error) => {
                // The capability/status object is browser-visible. The info
                // client already strips HTTP diagnostics; retain only a fixed
                // state label so logs and telemetry cannot reintroduce them.
                self.replace(ProbeState {
                    checked: true,
                    info: None,
                    error: Some("the MediaFlick Companion plugin could not be reached".to_string()),
                });
                Err(error)
            }
        }
    }

    pub fn clear(&self) {
        self.replace(ProbeState::default());
    }

    pub fn supports(&self, capability: &str) -> bool {
        self.read()
            .info
            .as_ref()
            .is_some_and(|info| info.supports(capability))
    }

    pub fn status(&self) -> Value {
        let state = self.read();
        let compatible = state
            .info
            .as_ref()
            .is_some_and(CompanionInfo::is_compatible);
        json!({
            "available": state.info.is_some(),
            "compatible": compatible,
            "checked": state.checked,
            "info": state.info,
            "error": state.error,
            "supportedApi": { "min": MIN_API_VERSION, "max": MAX_API_VERSION },
        })
    }

    /// Versioned ratings boundary. The plugin keeps any administrator-owned
    /// provider credential server-side and returns capabilities/data only.
    pub fn ratings_v1(&self, body: &Value) -> Result<Value, ApiError> {
        let _ = self.probe(false);
        if !self.supports("ratings-v1") {
            return Err(ApiError::NotConfigured);
        }
        self.client()?
            .companion_post_ratings_json_once("/MediaFlick/ratings/v1/batch", body)
    }

    pub fn calendar(&self, start: &str, end: &str) -> Result<Value, ApiError> {
        // Cached once probed; a probe failure just means the metadata
        // fallback below answers instead.
        let _ = self.probe(false);
        if self.supports("calendar") {
            let client = self.session.client()?;
            let mut value: Value = client.companion_get_json(
                "/MediaFlick/calendar",
                &[("start", start.to_string()), ("end", end.to_string())],
            )?;
            join_calendar(&self.library, &mut value);
            return Ok(value);
        }
        self.fallback_calendar(start, end)
    }

    /// TMDB movie collections derived from the library by the Companion
    /// plugin. There is no direct-Seerr fallback: only the plugin knows which
    /// movies the library actually contains, so without it the feature has no
    /// answer and degrades to `NotConfigured`.
    pub fn collections(&self) -> Result<Value, ApiError> {
        self.collections_endpoint("/MediaFlick/collections")
    }

    /// One collection's movie parts, joined against the local library so the
    /// UI can tell owned titles from discoverable ones.
    pub fn collection_detail(&self, collection_id: i64) -> Result<Value, ApiError> {
        let mut value =
            self.collections_endpoint(&format!("/MediaFlick/collections/{collection_id}"))?;
        join_seerr_rows(&self.library, &mut value, "parts");
        Ok(value)
    }

    /// The collection one TMDB movie belongs to, for detail-page links.
    pub fn movie_collection(&self, tmdb_id: i64) -> Result<Value, ApiError> {
        self.collections_endpoint(&format!("/MediaFlick/collections/movie/{tmdb_id}"))
    }

    /// Native BoxSet mirroring: the plugin writes TMDB collections into the
    /// server's own collections feature. Desktop then lists and browses them
    /// through ordinary `/Items` queries instead of derived summaries.
    pub fn native_collections(&self) -> bool {
        self.supports("collections-v2")
    }

    fn collections_endpoint(&self, path: &str) -> Result<Value, ApiError> {
        let _ = self.probe(false);
        if !self.supports("collections-v1") {
            return Err(ApiError::NotConfigured);
        }
        self.get_seerr(path, &[])
    }

    fn fallback_calendar(&self, start: &str, end: &str) -> Result<Value, ApiError> {
        let (client, user_id) = self.session.client_and_user()?;
        let response = items::fetch_upcoming(&client, &user_id, 500)?;
        let entries = response
            .items
            .iter()
            .filter_map(|item| {
                let date = item.premiere_date.as_deref()?.get(..10)?;
                if date < start || date > end {
                    return None;
                }
                let library_item_id = self
                    .library
                    .kind(&item.id)
                    .is_some()
                    .then(|| item.id.clone());
                let series_library_item_id = item.series_id.as_ref().and_then(|series_id| {
                    (self.library.kind(series_id).as_deref() == Some("Series"))
                        .then(|| series_id.clone())
                });
                Some(json!({
                    "kind": "episode",
                    "date": date,
                    "dateKind": "air",
                    "title": item.display_name(),
                    "seriesTitle": item.series_name,
                    "season": item.parent_index_number,
                    "episode": item.index_number,
                    "tmdbId": item.provider_id("Tmdb").and_then(|id| id.parse::<i64>().ok()),
                    "tvdbId": item.provider_id("Tvdb").and_then(|id| id.parse::<i64>().ok()),
                    "seriesTmdbId": Value::Null,
                    "seriesTvdbId": Value::Null,
                    "monitored": true,
                    "hasFile": library_item_id.is_some(),
                    "posterUrl": Value::Null,
                    "libraryItemId": library_item_id,
                    "seriesLibraryItemId": series_library_item_id,
                }))
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "entries": entries,
            "refreshedAt": Value::Null,
            "sources": {
                "jellyfin": {
                    "enabled": true,
                    "available": true,
                    "stale": false,
                    "refreshedAt": Value::Null,
                    "error": Value::Null,
                }
            },
            "windowStart": start,
            "windowEnd": end,
            "provider": "metadata",
        }))
    }

    fn read(&self) -> ProbeState {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn replace(&self, state: ProbeState) {
        *self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
    }

    fn client(&self) -> Result<JellyfinClient, ApiError> {
        self.session.client()
    }

    fn get_seerr(&self, path: &str, query: &[(&str, String)]) -> Result<Value, ApiError> {
        self.client()?.companion_get_json(path, query)
    }

    fn post_seerr(&self, path: &str, body: &Value) -> Result<Value, ApiError> {
        self.client()?.companion_post_json_once(path, body)
    }
}

#[derive(Debug)]
pub enum ProviderError {
    Companion(ApiError),
    Direct(SeerrError),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Companion(error) => error.fmt(formatter),
            Self::Direct(error) => error.fmt(formatter),
        }
    }
}

pub enum RequestsProvider {
    Companion(Arc<CompanionSession>),
    Direct(Arc<SeerrSession>),
}

impl RequestsProvider {
    pub fn select(companion: Arc<CompanionSession>, direct: Arc<SeerrSession>) -> Self {
        if companion.supports("seerr") {
            Self::Companion(companion)
        } else {
            Self::Direct(direct)
        }
    }

    pub fn status(&self) -> Result<Value, ProviderError> {
        match self {
            Self::Companion(companion) => companion
                .get_seerr("/MediaFlick/seerr/status", &[])
                .map_err(ProviderError::Companion),
            Self::Direct(direct) => Ok(direct.status()),
        }
    }

    pub fn search(&self, query: &str, page: i64) -> Result<Value, ProviderError> {
        match self {
            Self::Companion(companion) => {
                let mut value = companion
                    .get_seerr(
                        "/MediaFlick/seerr/search",
                        &[("query", query.to_string()), ("page", page.to_string())],
                    )
                    .map_err(ProviderError::Companion)?;
                join_seerr_results(&companion.library, &mut value);
                Ok(value)
            }
            Self::Direct(direct) => direct.search(query, page).map_err(ProviderError::Direct),
        }
    }

    pub fn person_credits(&self, tmdb_id: i64) -> Result<Value, ProviderError> {
        match self {
            Self::Companion(companion) => {
                if !companion.supports("seerr-person-discovery") {
                    return Err(ProviderError::Companion(ApiError::Remote {
                        status: 409,
                        message:
                            "the MediaFlick Companion plugin must be updated for cast discovery"
                                .to_string(),
                    }));
                }
                let mut value = companion
                    .get_seerr(&format!("/MediaFlick/seerr/person/{tmdb_id}/credits"), &[])
                    .map_err(ProviderError::Companion)?;
                join_seerr_results(&companion.library, &mut value);
                Ok(value)
            }
            Self::Direct(direct) => direct
                .person_credits(tmdb_id)
                .map_err(ProviderError::Direct),
        }
    }

    pub fn discover(
        &self,
        kind: DiscoverKind,
        page: i64,
        options: &DiscoverOptions,
    ) -> Result<Value, ProviderError> {
        match self {
            Self::Companion(companion) => {
                let query = options.companion_query_pairs(kind, page);
                let mut value = companion
                    .get_seerr(
                        &format!("/MediaFlick/seerr/discover/{}", kind.id()),
                        query.as_slice(),
                    )
                    .map_err(ProviderError::Companion)?;
                join_seerr_results(&companion.library, &mut value);
                Ok(value)
            }
            Self::Direct(direct) => direct
                .discover(kind, page, options)
                .map_err(ProviderError::Direct),
        }
    }

    pub fn genres(&self, media_type: &str) -> Result<Value, ProviderError> {
        match self {
            Self::Companion(companion) => companion
                .get_seerr(&format!("/MediaFlick/seerr/genres/{media_type}"), &[])
                .map_err(ProviderError::Companion),
            Self::Direct(direct) => direct.genres(media_type).map_err(ProviderError::Direct),
        }
    }

    pub fn media(&self, media_type: &str, tmdb_id: i64) -> Result<Value, ProviderError> {
        match self {
            Self::Companion(companion) => {
                let mut value = companion
                    .get_seerr(
                        &format!("/MediaFlick/seerr/media/{media_type}/{tmdb_id}"),
                        &[],
                    )
                    .map_err(ProviderError::Companion)?;
                join_seerr_item(&companion.library, &mut value);
                Ok(value)
            }
            Self::Direct(direct) => direct
                .media_detail(media_type, tmdb_id)
                .map_err(ProviderError::Direct),
        }
    }

    pub fn request_options(&self, media_type: &str, is_4k: bool) -> Result<Value, ProviderError> {
        match self {
            Self::Companion(companion) => companion
                .get_seerr(
                    &format!("/MediaFlick/seerr/request-options/{media_type}"),
                    &[("is4k", is_4k.to_string())],
                )
                .map_err(ProviderError::Companion),
            Self::Direct(direct) => direct
                .request_options(media_type, is_4k)
                .map_err(ProviderError::Direct),
        }
    }

    pub fn create(
        &self,
        media_type: &str,
        tmdb_id: i64,
        seasons: Option<Vec<i64>>,
        is_4k: bool,
        profile: Option<RequestProfileSelection>,
    ) -> Result<Value, ProviderError> {
        match self {
            Self::Companion(companion) => {
                let mut value = companion
                    .post_seerr(
                        "/MediaFlick/seerr/request",
                        &json!({
                            "mediaType": media_type,
                            "tmdbId": tmdb_id,
                            "seasons": seasons,
                            "is4k": is_4k,
                            "serverId": profile.map(|selection| selection.server_id),
                            "profileId": profile.map(|selection| selection.profile_id),
                        }),
                    )
                    .map_err(ProviderError::Companion)?;
                join_seerr_item(&companion.library, &mut value);
                Ok(value)
            }
            Self::Direct(direct) => direct
                .create_request(media_type, tmdb_id, seasons, is_4k, profile)
                .map_err(ProviderError::Direct),
        }
    }

    pub fn requests(&self, take: i64, skip: i64, filter: &str) -> Result<Value, ProviderError> {
        match self {
            Self::Companion(companion) => {
                let mut value = companion
                    .get_seerr(
                        "/MediaFlick/seerr/requests",
                        &[
                            ("take", take.to_string()),
                            ("skip", skip.to_string()),
                            ("filter", filter.to_string()),
                        ],
                    )
                    .map_err(ProviderError::Companion)?;
                join_seerr_results(&companion.library, &mut value);
                Ok(value)
            }
            Self::Direct(direct) => direct
                .requests(take, skip, filter)
                .map_err(ProviderError::Direct),
        }
    }

    pub fn cancel(&self, request_id: i64) -> Result<Value, ProviderError> {
        match self {
            Self::Companion(companion) => companion
                .client()
                .and_then(|client| {
                    client.companion_delete_once(&format!("/MediaFlick/seerr/request/{request_id}"))
                })
                .map(|()| json!({ "cancelled": true, "id": request_id }))
                .map_err(ProviderError::Companion),
            Self::Direct(direct) => direct
                .cancel_request(request_id)
                .map_err(ProviderError::Direct),
        }
    }
}

fn join_seerr_results(library: &Library, value: &mut Value) {
    join_seerr_rows(library, value, "results");
}

/// Joins one array of shaped Seerr rows against the local cache. `field` is
/// the array's name: discovery pages answer under `results`, collection
/// details under `parts`.
fn join_seerr_rows(library: &Library, value: &mut Value, field: &str) {
    let Some(results) = value.get_mut(field).and_then(Value::as_array_mut) else {
        return;
    };
    let movies = provider_ids(results, "movie");
    let series = provider_ids(results, "tv");
    let movie_ids = library.ids_by_tmdb("Movie", &movies).unwrap_or_default();
    let series_ids = library.ids_by_tmdb("Series", &series).unwrap_or_default();
    for item in results {
        let media_type = item["mediaType"].as_str().unwrap_or_default();
        let Some(tmdb_id) = item["tmdbId"].as_i64() else {
            continue;
        };
        let owned = if media_type == "movie" {
            &movie_ids
        } else {
            &series_ids
        };
        item["libraryItemId"] = owned
            .get(&tmdb_id.to_string())
            .map_or(Value::Null, |id| json!(id));
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

fn join_calendar(library: &Library, value: &mut Value) {
    let Some(entries) = value["entries"].as_array_mut() else {
        return;
    };
    let mut tmdb_by_kind: std::collections::HashMap<&str, Vec<String>> =
        std::collections::HashMap::new();
    let mut tvdb_by_kind: std::collections::HashMap<&str, Vec<String>> =
        std::collections::HashMap::new();
    for entry in entries.iter() {
        let kind = match entry["kind"].as_str() {
            Some("movie") => "Movie",
            Some("episode") => "Episode",
            _ => continue,
        };
        if let Some(id) = entry["tmdbId"].as_i64() {
            tmdb_by_kind.entry(kind).or_default().push(id.to_string());
        }
        if let Some(id) = entry["tvdbId"].as_i64() {
            tvdb_by_kind.entry(kind).or_default().push(id.to_string());
        }
        if kind == "Episode" {
            if let Some(id) = entry["seriesTmdbId"].as_i64() {
                tmdb_by_kind
                    .entry("Series")
                    .or_default()
                    .push(id.to_string());
            }
            if let Some(id) = entry["seriesTvdbId"].as_i64() {
                tvdb_by_kind
                    .entry("Series")
                    .or_default()
                    .push(id.to_string());
            }
        }
    }
    let mut resolved: std::collections::HashMap<(&str, String), String> =
        std::collections::HashMap::new();
    for kind in ["Movie", "Episode", "Series"] {
        for (id, item) in library
            .ids_by_tmdb(kind, tmdb_by_kind.get(kind).map_or(&[], Vec::as_slice))
            .unwrap_or_default()
        {
            resolved.insert((kind, format!("tmdb:{id}")), item);
        }
        for (id, item) in library
            .ids_by_tvdb(kind, tvdb_by_kind.get(kind).map_or(&[], Vec::as_slice))
            .unwrap_or_default()
        {
            resolved.insert((kind, format!("tvdb:{id}")), item);
        }
    }
    for entry in entries {
        let kind = if entry["kind"] == "movie" {
            "Movie"
        } else {
            "Episode"
        };
        let item = entry["tmdbId"]
            .as_i64()
            .and_then(|id| resolved.get(&(kind, format!("tmdb:{id}"))))
            .or_else(|| {
                entry["tvdbId"]
                    .as_i64()
                    .and_then(|id| resolved.get(&(kind, format!("tvdb:{id}"))))
            });
        entry["libraryItemId"] = item.map_or(Value::Null, |id| json!(id));
        let series_item = (kind == "Episode")
            .then(|| {
                entry["seriesTmdbId"]
                    .as_i64()
                    .and_then(|id| resolved.get(&("Series", format!("tmdb:{id}"))))
                    .or_else(|| {
                        entry["seriesTvdbId"]
                            .as_i64()
                            .and_then(|id| resolved.get(&("Series", format!("tvdb:{id}"))))
                    })
            })
            .flatten();
        entry["seriesLibraryItemId"] = series_item.map_or(Value::Null, |id| json!(id));
    }
}

#[cfg(test)]
mod tests {
    use super::{CompanionInfo, join_calendar};
    use crate::jellyfin::api::model::BaseItemDto;
    use crate::library::Library;
    use serde_json::json;

    #[test]
    fn only_supported_api_versions_enable_capabilities() {
        let current = CompanionInfo {
            api_version: 1,
            capabilities: vec!["calendar".to_string()],
            ..Default::default()
        };
        assert!(current.supports("calendar"));

        let future = CompanionInfo {
            api_version: 2,
            capabilities: vec!["calendar".to_string()],
            ..Default::default()
        };
        assert!(!future.supports("calendar"));
    }

    #[test]
    fn calendar_join_keeps_episode_and_series_library_identity() {
        let library = Library::open_in_memory().expect("library");
        let items = [
            serde_json::from_str::<BaseItemDto>(
                r#"{"Id":"series-1","Name":"Severance","Type":"Series","ProviderIds":{"Tvdb":"371980"}}"#,
            )
            .expect("series"),
            serde_json::from_str::<BaseItemDto>(
                r#"{"Id":"episode-1","Name":"The We We Are","Type":"Episode","SeriesId":"series-1","ProviderIds":{"Tvdb":"1234"}}"#,
            )
            .expect("episode"),
        ];
        library.upsert_page(&items).expect("seed");
        let mut calendar = json!({
            "entries": [{
                "kind": "episode",
                "tvdbId": 1234,
                "seriesTvdbId": 371980
            }]
        });

        join_calendar(&library, &mut calendar);

        assert_eq!(calendar["entries"][0]["libraryItemId"], "episode-1");
        assert_eq!(calendar["entries"][0]["seriesLibraryItemId"], "series-1");
    }
}
