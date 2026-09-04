//! MediaFlick Companion discovery and provider selection.
//!
//! The server plugin is optional. Its API is probed with the authenticated
//! Jellyfin client and cached for the life of that login; every feature then
//! selects the plugin backend only when the advertised v1 capability exists.

use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::collections::{ProviderReadiness, ProviderResult};
use crate::jellyfin::api::items;
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::session::{Session, SessionScope};
use crate::library::Library;
use crate::seerr::{DiscoverKind, DiscoverOptions, RequestProfileSelection};

const MIN_API_VERSION: i64 = 1;
const MAX_API_VERSION: i64 = 1;
const FAILED_PROBE_RETRY: Duration = Duration::from_secs(30);
const SUCCESSFUL_PROBE_REUSE: Duration = Duration::from_secs(5 * 60);
const FRANCHISE_MEMBERSHIPS_CAPABILITY: &str = "franchise-memberships-v1";

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
    checked_at: Option<Instant>,
}

impl ProbeState {
    fn reusable(&self) -> bool {
        if !self.checked {
            return false;
        }
        let lifetime = if self.error.is_some() {
            FAILED_PROBE_RETRY
        } else {
            SUCCESSFUL_PROBE_REUSE
        };
        self.checked_at
            .is_some_and(|checked_at| checked_at.elapsed() < lifetime)
    }
}

pub struct CompanionSession {
    session: Arc<Session>,
    library: Arc<Library>,
    state: RwLock<ProbeState>,
    probe_running: Mutex<bool>,
    probe_finished: Condvar,
}

impl CompanionSession {
    pub fn new(session: Arc<Session>, library: Arc<Library>) -> Self {
        Self {
            session,
            library,
            state: RwLock::new(ProbeState::default()),
            probe_running: Mutex::new(false),
            probe_finished: Condvar::new(),
        }
    }

    pub fn probe(&self, force: bool) -> Result<Option<CompanionInfo>, ApiError> {
        // Waiters recheck the cache after discovery finishes. No cache or
        // synchronization lock is held while the authenticated request runs.
        let mut running = self
            .probe_running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *running {
            running = self
                .probe_finished
                .wait(running)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *running = true;
        drop(running);
        let result = self.probe_inner(force);
        *self
            .probe_running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
        self.probe_finished.notify_all();
        result
    }

    fn probe_inner(&self, force: bool) -> Result<Option<CompanionInfo>, ApiError> {
        if !force {
            let state = self.read();
            if state.reusable() {
                return Ok(state.info);
            }
        }

        if !self.session.is_authenticated() {
            self.replace(ProbeState {
                checked: true,
                info: None,
                error: None,
                checked_at: Some(Instant::now()),
            });
            return Ok(None);
        }

        let scope = self.session.scope()?;
        match scope
            .client()
            .companion_get_info_json::<CompanionInfo>("/MediaFlick/info")
        {
            Ok(info) => {
                let compatible = info.is_compatible();
                tracing::info!(
                    target: "companion",
                    plugin_version = %info.plugin_version,
                    api_version = info.api_version,
                    compatible,
                    "probed the MediaFlick Companion plugin"
                );
                self.replace_scoped(
                    &scope,
                    ProbeState {
                        checked: true,
                        error: (!compatible).then(|| {
                            format!(
                                "companion API {} is outside the supported range {}–{}",
                                info.api_version, MIN_API_VERSION, MAX_API_VERSION
                            )
                        }),
                        info: Some(info.clone()),
                        checked_at: Some(Instant::now()),
                    },
                )?;
                Ok(Some(info))
            }
            Err(ApiError::Status { status: 404 } | ApiError::Remote { status: 404, .. }) => {
                self.replace_scoped(
                    &scope,
                    ProbeState {
                        checked: true,
                        info: None,
                        error: None,
                        checked_at: Some(Instant::now()),
                    },
                )?;
                tracing::debug!(target: "companion", "the server has no companion plugin");
                Ok(None)
            }
            Err(error) => {
                // The capability/status object is browser-visible. The info
                // client already strips HTTP diagnostics; retain only a fixed
                // state label so logs and telemetry cannot reintroduce them.
                let previous = self.read();
                self.replace_scoped(
                    &scope,
                    ProbeState {
                        checked: true,
                        info: previous.info,
                        error: Some(
                            "the MediaFlick Companion plugin could not be reached".to_string(),
                        ),
                        checked_at: Some(Instant::now()),
                    },
                )?;
                self.session.note_scoped_error(&scope, &error);
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

    pub fn collection_readiness(&self, force: bool) -> ProviderReadiness {
        if self.probe(force).is_err() {
            return ProviderReadiness::default();
        }
        self.cached_collection_readiness()
    }

    pub fn cached_collection_readiness(&self) -> ProviderReadiness {
        let info = self.read().info;
        let Some(info) = info.filter(|info| info.supports("collection-experience-v1")) else {
            return ProviderReadiness::default();
        };
        ProviderReadiness {
            tmdb: info.services.get("tmdb").copied().unwrap_or(false),
            mdblist: info.services.get("mdblist").copied().unwrap_or(false),
        }
    }

    pub fn preview_collection(&self, body: &Value) -> Result<ProviderResult, ApiError> {
        self.collection_operation("preview", body)
    }

    pub fn refresh_collection(&self, body: &Value) -> Result<ProviderResult, ApiError> {
        self.collection_operation("results", body)
    }

    pub fn resolve_franchises(
        &self,
        scope: &SessionScope,
        tmdb_ids: &[u64],
        collection_ids: &[u64],
    ) -> Result<Value, ApiError> {
        self.require_collection_experience()?;
        self.require_capability(
            FRANCHISE_MEMBERSHIPS_CAPABILITY,
            "MediaFlick Companion does not provide movie franchise membership refresh",
        )?;
        if !self.session.scope_is_current(scope) {
            return Err(ApiError::Cancelled);
        }
        scope.client().companion_post_json_once(
            "/MediaFlick/collection-experience/v1/franchises",
            &json!({ "tmdbIds": tmdb_ids, "collectionIds": collection_ids }),
        )
    }

    pub fn search_public_lists(&self, query: &str) -> Result<Value, ApiError> {
        self.collection_json_operation("mdblist/search", &json!({ "query": query }))
    }

    pub fn validate_public_list(&self, selector: &str) -> Result<Value, ApiError> {
        self.collection_json_operation("mdblist/validate", &json!({ "selector": selector }))
    }

    pub fn resolve_collection_identities(&self, items: &Value) -> Result<Value, ApiError> {
        self.collection_json_operation("identities", &json!({ "items": items }))
    }

    pub fn collection_artwork(
        &self,
        size: &str,
        path: &str,
    ) -> Result<(Vec<u8>, String), ApiError> {
        self.require_collection_experience()?;
        self.client()?.companion_get_bytes(
            "/MediaFlick/collection-experience/v1/artwork",
            &[("size", size.to_string()), ("path", path.to_string())],
        )
    }

    fn collection_operation(
        &self,
        operation: &str,
        body: &Value,
    ) -> Result<ProviderResult, ApiError> {
        self.require_collection_experience()?;
        let value = self.client()?.companion_post_json_once(
            &format!("/MediaFlick/collection-experience/v1/{operation}"),
            body,
        )?;
        serde_json::from_value(value).map_err(|error| {
            ApiError::Decode(format!("invalid collection provider response: {error}"))
        })
    }

    fn collection_json_operation(&self, operation: &str, body: &Value) -> Result<Value, ApiError> {
        self.require_collection_experience()?;
        self.client()?.companion_post_json_once(
            &format!("/MediaFlick/collection-experience/v1/{operation}"),
            body,
        )
    }

    fn require_collection_experience(&self) -> Result<(), ApiError> {
        self.probe(false)?;
        if self.supports("collection-experience-v1") {
            Ok(())
        } else {
            Err(ApiError::NotConfigured)
        }
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

    fn replace_scoped(&self, scope: &SessionScope, state: ProbeState) -> Result<(), ApiError> {
        self.session.commit_if_current(
            scope,
            || ApiError::Cancelled,
            || {
                let previous = self.cached_collection_readiness();
                self.replace(state);
                if previous != self.cached_collection_readiness() {
                    crate::app::services::notify_collections_changed();
                }
                Ok(())
            },
        )
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
    fn require_capability(&self, capability: &str, message: &str) -> Result<(), ApiError> {
        self.probe(false)?;
        if self.supports(capability) {
            return Ok(());
        }
        Err(ApiError::Remote {
            status: 409,
            message: message.to_string(),
        })
    }

    pub fn seerr_status(&self) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.get_seerr("/MediaFlick/seerr/status", &[])
    }

    pub fn seerr_search(&self, query: &str, page: i64) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.get_seerr(
            "/MediaFlick/seerr/search",
            &[("query", query.to_string()), ("page", page.to_string())],
        )?;
        join_seerr_results(&self.library, &mut value);
        Ok(value)
    }

    pub fn seerr_person_credits(&self, tmdb_id: i64) -> Result<Value, ApiError> {
        self.require_capability(
            "seerr-person-discovery",
            "the MediaFlick Companion plugin must be updated for cast discovery",
        )?;
        let mut value =
            self.get_seerr(&format!("/MediaFlick/seerr/person/{tmdb_id}/credits"), &[])?;
        join_seerr_results(&self.library, &mut value);
        Ok(value)
    }

    pub fn seerr_discover(
        &self,
        kind: DiscoverKind,
        page: i64,
        options: &DiscoverOptions,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let query = options.query_pairs(kind, page);
        let mut value = self.get_seerr(
            &format!("/MediaFlick/seerr/discover/{}", kind.id()),
            query.as_slice(),
        )?;
        join_seerr_results(&self.library, &mut value);
        Ok(value)
    }

    pub fn seerr_genres(&self, media_type: &str) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.get_seerr(&format!("/MediaFlick/seerr/genres/{media_type}"), &[])
    }

    pub fn seerr_media(&self, media_type: &str, tmdb_id: i64) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.get_seerr(
            &format!("/MediaFlick/seerr/media/{media_type}/{tmdb_id}"),
            &[],
        )?;
        join_seerr_item(&self.library, &mut value);
        Ok(value)
    }

    pub fn seerr_request_options(&self, media_type: &str, is_4k: bool) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.get_seerr(
            &format!("/MediaFlick/seerr/request-options/{media_type}"),
            &[("is4k", is_4k.to_string())],
        )
    }

    pub fn seerr_create_request(
        &self,
        media_type: &str,
        tmdb_id: i64,
        seasons: Option<&[i64]>,
        is_4k: bool,
        profile: Option<RequestProfileSelection>,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.post_seerr(
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

    pub fn seerr_requests(&self, take: i64, skip: i64, filter: &str) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.get_seerr(
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

    pub fn seerr_cancel_request(&self, request_id: i64) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.client()?
            .companion_delete_once(&format!("/MediaFlick/seerr/request/{request_id}"))?;
        Ok(json!({ "cancelled": true, "id": request_id }))
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
    // Watched state rides along so collection pages can sort and filter on
    // it without hydrating every card first.
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
    use super::{
        CompanionInfo, FAILED_PROBE_RETRY, ProbeState, SUCCESSFUL_PROBE_REUSE, join_calendar,
    };
    use crate::jellyfin::api::model::BaseItemDto;
    use crate::library::Library;
    use serde_json::json;

    #[test]
    fn concurrent_discovery_reuses_one_authenticated_response() {
        use std::io::{BufRead, BufReader, Write};
        use std::sync::{Arc, Barrier, mpsc};
        use std::time::Duration;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let mut credentials = library.credentials();
        credentials.server_url = Some(format!(
            "http://{}",
            listener.local_addr().expect("address")
        ));
        credentials.server_id = Some("server".to_string());
        credentials.user_id = Some("user".to_string());
        credentials.token = Some("token".to_string());
        library.save_credentials(&credentials).expect("credentials");
        let session = Arc::new(crate::jellyfin::session::Session::restore(library.clone()));
        let companion = Arc::new(super::CompanionSession::new(session, library));
        let barrier = Arc::new(Barrier::new(3));
        let (sender, receiver) = mpsc::channel();
        for _ in 0..2 {
            let companion = companion.clone();
            let barrier = barrier.clone();
            let sender = sender.clone();
            std::thread::spawn(move || {
                barrier.wait();
                sender.send(companion.probe(false)).expect("probe result");
            });
        }
        barrier.wait();
        let (mut stream, _) = listener.accept().expect("request");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");
        let mut reader = BufReader::new(&mut stream);
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("request headers");
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        drop(reader);
        // Cached Home reads remain available while the discovery response is pending.
        assert_eq!(companion.cached_collection_readiness(), Default::default());
        let body = r#"{"apiVersion":1,"capabilities":["collection-experience-v1"],"services":{"tmdb":true}}"#;
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("response");
        for _ in 0..2 {
            assert!(
                receiver
                    .recv_timeout(Duration::from_secs(5))
                    .expect("completed probe")
                    .expect("successful probe")
                    .is_some()
            );
        }
        listener.set_nonblocking(true).expect("nonblocking");
        assert_eq!(
            listener.accept().expect_err("one request").kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(companion.cached_collection_readiness().tmdb);
    }

    #[test]
    fn cached_collection_readiness_never_probes_and_clear_drops_capabilities() {
        use std::sync::Arc;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        listener.set_nonblocking(true).expect("nonblocking");
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let mut credentials = library.credentials();
        credentials.server_url = Some(format!(
            "http://{}",
            listener.local_addr().expect("address")
        ));
        credentials.server_id = Some("server".to_string());
        credentials.user_id = Some("user".to_string());
        credentials.token = Some("token".to_string());
        library.save_credentials(&credentials).expect("credentials");
        let session = Arc::new(crate::jellyfin::session::Session::restore(library.clone()));
        let companion = super::CompanionSession::new(session, library);
        assert_eq!(companion.cached_collection_readiness(), Default::default());
        companion.replace(ProbeState {
            info: Some(CompanionInfo {
                api_version: 1,
                capabilities: vec!["collection-experience-v1".to_string()],
                services: [("tmdb".to_string(), true)].into(),
                ..Default::default()
            }),
            ..Default::default()
        });
        assert!(companion.cached_collection_readiness().tmdb);
        companion.clear();
        assert_eq!(companion.cached_collection_readiness(), Default::default());
        assert_eq!(
            listener.accept().expect_err("no network calls").kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

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
    fn transient_probe_failures_are_retried_without_logging_out() {
        let recent_failure = ProbeState {
            checked: true,
            error: Some("temporarily unavailable".to_string()),
            checked_at: Some(std::time::Instant::now()),
            ..ProbeState::default()
        };
        assert!(recent_failure.reusable());

        let expired_failure = ProbeState {
            checked_at: Some(std::time::Instant::now() - FAILED_PROBE_RETRY),
            ..recent_failure
        };
        assert!(!expired_failure.reusable());
    }

    #[test]
    fn successful_probe_results_are_refreshed_periodically() {
        let recent_success = ProbeState {
            checked: true,
            checked_at: Some(std::time::Instant::now()),
            ..ProbeState::default()
        };
        assert!(recent_success.reusable());

        let expired_success = ProbeState {
            checked_at: Some(std::time::Instant::now() - SUCCESSFUL_PROBE_REUSE),
            ..recent_success
        };
        assert!(!expired_success.reusable());
    }

    #[test]
    fn collection_parts_carry_ownership_and_watched_state() {
        let library = Library::open_in_memory().expect("library");
        let items = [
            serde_json::from_str::<BaseItemDto>(
                r#"{"Id":"m1","Name":"The Matrix","Type":"Movie","ProviderIds":{"Tmdb":"603"}}"#,
            )
            .expect("movie"),
            serde_json::from_str::<BaseItemDto>(
                r#"{"Id":"m2","Name":"Alien","Type":"Movie","ProviderIds":{"Tmdb":"348"}}"#,
            )
            .expect("movie"),
            serde_json::from_str::<BaseItemDto>(
                r#"{"Id":"s1","Name":"The 603","Type":"Series","ProviderIds":{"Tmdb":"603"}}"#,
            )
            .expect("series"),
        ];
        library.upsert_page(&items).expect("seed");
        library
            .upsert_user_data(&[crate::library::UserDataRecord {
                jellyfin_id: "m1".to_string(),
                played: true,
                ..Default::default()
            }])
            .expect("user data");
        let mut detail = json!({
            "parts": [
                { "mediaType": "movie", "tmdbId": 603 },
                { "mediaType": "movie", "tmdbId": 348 },
                { "mediaType": "movie", "tmdbId": 624834 },
                { "mediaType": "tv", "tmdbId": 603 }
            ]
        });

        super::join_seerr_rows(&library, &mut detail, "parts");

        assert_eq!(detail["parts"][0]["libraryItemId"], "m1");
        assert_eq!(detail["parts"][0]["played"], true);
        assert_eq!(detail["parts"][1]["libraryItemId"], "m2");
        assert_eq!(detail["parts"][1]["played"], false);
        // Unowned parts are neither joined nor watched.
        assert!(detail["parts"][2]["libraryItemId"].is_null());
        assert_eq!(detail["parts"][2]["played"], false);
        // TMDB numeric ids can overlap across movie and TV namespaces.
        assert_eq!(detail["parts"][3]["libraryItemId"], "s1");
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
