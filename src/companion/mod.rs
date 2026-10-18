//! MediaFlick Companion discovery and provider selection.
//!
//! The server plugin is optional. Its API is probed with the authenticated
//! Jellyfin client and cached for the life of that login; every feature then
//! selects the plugin backend only when the advertised v1 capability exists.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::app::services::ShellBridge;
use crate::collections::{ProviderReadiness, ProviderResult};
use crate::jellyfin::api::items;
use crate::jellyfin::api::{ApiError, JellyfinClient};
use crate::jellyfin::session::{Live, Scoped, Session, SessionScope, Stale};
use crate::library::{CatalogKind, Library};

mod seerr;

const MIN_API_VERSION: i64 = 1;
const MAX_API_VERSION: i64 = 1;
const FAILED_PROBE_RETRY: Duration = Duration::from_secs(30);
const SUCCESSFUL_PROBE_REUSE: Duration = Duration::from_secs(5 * 60);
const FRANCHISE_MEMBERSHIPS_CAPABILITY: &str = "franchise-memberships-v1";

#[derive(Debug, Clone, Default, Deserialize, Serialize, ts_rs::TS)]
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

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CompanionStatus {
    pub available: bool,
    pub compatible: bool,
    pub checked: bool,
    pub info: Option<CompanionInfo>,
    pub error: Option<String>,
    pub supported_api: SupportedApi,
}

#[derive(Debug, Clone, Copy, Serialize, ts_rs::TS)]
pub struct SupportedApi {
    pub min: i64,
    pub max: i64,
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
    shell: Arc<ShellBridge>,
    state: RwLock<ProbeState>,
    probe_running: Mutex<bool>,
    probe_finished: Condvar,
}

impl CompanionSession {
    pub fn new(session: Arc<Session>, library: Arc<Library>, shell: Arc<ShellBridge>) -> Self {
        Self {
            session,
            library,
            shell,
            state: RwLock::new(ProbeState::default()),
            probe_running: Mutex::new(false),
            probe_finished: Condvar::new(),
        }
    }

    pub fn probe(&self, force: bool) -> Result<Option<CompanionInfo>, ApiError> {
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
            .fetch(|client, _| client.companion_get_info_json::<CompanionInfo>("/MediaFlick/info"))
        {
            Ok(info) => {
                let info = self.replace_scoped(&scope, |live| {
                    let info = live.open(info).map_err(|Stale| ApiError::AccountChanged)?;
                    let compatible = info.is_compatible();
                    let state = ProbeState {
                        checked: true,
                        error: (!compatible).then(|| {
                            format!(
                                "companion API {} is outside the supported range {}–{}",
                                info.api_version, MIN_API_VERSION, MAX_API_VERSION
                            )
                        }),
                        info: Some(info.clone()),
                        checked_at: Some(Instant::now()),
                    };
                    Ok((state, info))
                })?;
                tracing::info!(
                    target: "companion",
                    plugin_version = %info.plugin_version,
                    api_version = info.api_version,
                    compatible = info.is_compatible(),
                    "probed the MediaFlick Companion plugin"
                );
                Ok(Some(info))
            }
            Err(ApiError::Status { status: 404 } | ApiError::Remote { status: 404, .. }) => {
                self.replace_scoped(&scope, |_live| {
                    let state = ProbeState {
                        checked: true,
                        info: None,
                        error: None,
                        checked_at: Some(Instant::now()),
                    };
                    Ok((state, ()))
                })?;
                tracing::debug!(target: "companion", "the server has no companion plugin");
                Ok(None)
            }
            Err(error) => {
                // The capability/status object is browser-visible. The info
                // client already strips HTTP diagnostics; retain only a fixed
                // state label so logs and telemetry cannot reintroduce them.
                let previous = self.read();
                self.replace_scoped(&scope, |_live| {
                    let state = ProbeState {
                        checked: true,
                        info: previous.info,
                        error: Some(
                            "the MediaFlick Companion plugin could not be reached".to_string(),
                        ),
                        checked_at: Some(Instant::now()),
                    };
                    Ok((state, ()))
                })?;
                Err(self.session.scoped_error(&scope, error))
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

    pub fn status(&self) -> CompanionStatus {
        let state = self.read();
        CompanionStatus {
            available: state.info.is_some(),
            compatible: state
                .info
                .as_ref()
                .is_some_and(CompanionInfo::is_compatible),
            checked: state.checked,
            info: state.info,
            error: state.error,
            supported_api: SupportedApi {
                min: MIN_API_VERSION,
                max: MAX_API_VERSION,
            },
        }
    }

    /// Asks for server ratings as `scope`. The answer stays sealed so the
    /// ratings cache can open it under the commit that stores it.
    pub fn ratings_v1(
        &self,
        scope: &SessionScope,
        body: &Value,
    ) -> Result<Scoped<Value>, ApiError> {
        let _ = self.probe(false);
        if !self.supports("ratings-v1") {
            return Err(ApiError::NotConfigured);
        }
        scope.fetch(|client, _| {
            client.companion_post_ratings_json_once("/MediaFlick/ratings/v1/batch", body)
        })
    }

    pub fn calendar(
        &self,
        scope: &SessionScope,
        start: &str,
        end: &str,
    ) -> Result<Value, ApiError> {
        let _ = self.probe(false);
        if self.supports("calendar") {
            let mut value: Value = self.fetch(scope, |client, _| {
                client.companion_get_json(
                    "/MediaFlick/calendar",
                    &[("start", start.to_string()), ("end", end.to_string())],
                )
            })?;
            join_calendar(&self.library, &mut value);
            return Ok(value);
        }
        self.fallback_calendar(scope, start, end)
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

    pub fn preview_collection(
        &self,
        scope: &SessionScope,
        body: &Value,
    ) -> Result<ProviderResult, ApiError> {
        self.collection_operation(scope, "preview", body)
    }

    pub fn refresh_collection(
        &self,
        scope: &SessionScope,
        body: &Value,
    ) -> Result<ProviderResult, ApiError> {
        self.collection_operation(scope, "results", body)
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
        self.fetch(scope, |client, _| {
            client.companion_post_json_once(
                "/MediaFlick/collection-experience/v1/franchises",
                &json!({ "tmdbIds": tmdb_ids, "collectionIds": collection_ids }),
            )
        })
    }

    pub fn search_public_lists(
        &self,
        scope: &SessionScope,
        query: &str,
    ) -> Result<Value, ApiError> {
        self.collection_json_operation(scope, "mdblist/search", &json!({ "query": query }))
    }

    pub fn validate_public_list(
        &self,
        scope: &SessionScope,
        selector: &str,
    ) -> Result<Value, ApiError> {
        self.collection_json_operation(scope, "mdblist/validate", &json!({ "selector": selector }))
    }

    pub fn resolve_collection_identities(
        &self,
        scope: &SessionScope,
        items: &Value,
    ) -> Result<Value, ApiError> {
        self.collection_json_operation(scope, "identities", &json!({ "items": items }))
    }

    pub fn collection_artwork(
        &self,
        scope: &SessionScope,
        size: &str,
        path: &str,
    ) -> Result<(Vec<u8>, String), ApiError> {
        self.require_collection_experience()?;
        self.fetch(scope, |client, _| {
            client.companion_get_bytes(
                "/MediaFlick/collection-experience/v1/artwork",
                &[("size", size.to_string()), ("path", path.to_string())],
            )
        })
    }

    fn collection_operation(
        &self,
        scope: &SessionScope,
        operation: &str,
        body: &Value,
    ) -> Result<ProviderResult, ApiError> {
        let value = self.collection_json_operation(scope, operation, body)?;
        serde_json::from_value(value).map_err(|error| {
            ApiError::Decode(format!("invalid collection provider response: {error}"))
        })
    }

    fn collection_json_operation(
        &self,
        scope: &SessionScope,
        operation: &str,
        body: &Value,
    ) -> Result<Value, ApiError> {
        self.require_collection_experience()?;
        self.fetch(scope, |client, _| {
            client.companion_post_json_once(
                &format!("/MediaFlick/collection-experience/v1/{operation}"),
                body,
            )
        })
    }

    fn require_collection_experience(&self) -> Result<(), ApiError> {
        self.probe(false)?;
        if self.supports("collection-experience-v1") {
            Ok(())
        } else {
            Err(ApiError::NotConfigured)
        }
    }

    fn fallback_calendar(
        &self,
        scope: &SessionScope,
        start: &str,
        end: &str,
    ) -> Result<Value, ApiError> {
        let response = self.fetch(scope, |client, user_id| {
            items::fetch_upcoming(client, user_id, 500)
        })?;
        let cached_kind = |item_id: &str| {
            self.library.catalog_kind(item_id).unwrap_or_else(|error| {
                tracing::warn!(target: "companion", item_id, "could not read a cached item kind: {error}");
                None
            })
        };
        let entries = response
            .items
            .iter()
            .filter_map(|item| {
                let date = item.premiere_date.as_deref()?.get(..10)?;
                if date < start || date > end {
                    return None;
                }
                let library_item_id = cached_kind(&item.id).is_some().then(|| item.id.clone());
                let series_library_item_id = item.series_id.as_ref().and_then(|series_id| {
                    matches!(cached_kind(series_id), Some(CatalogKind::Series))
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
                    "posterPath": Value::Null,
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

    /// Stores the probe state `build` makes, while `scope` is still the
    /// signed-in account, and returns what `build` returned with it.
    fn replace_scoped<T>(
        &self,
        scope: &SessionScope,
        build: impl FnOnce(&Live) -> Result<(ProbeState, T), ApiError>,
    ) -> Result<T, ApiError> {
        let (changed, answer) = self.session.commit_if_current(
            scope,
            || ApiError::AccountChanged,
            |live| {
                let (state, answer) = build(live)?;
                let previous = self.cached_collection_readiness();
                self.replace(state);
                Ok((previous != self.cached_collection_readiness(), answer))
            },
        )?;
        if changed {
            self.shell.collections_changed();
        }
        Ok(answer)
    }

    /// Calls the Companion as `scope` and opens the answer for the caller.
    /// An answer or failure for an account that is no longer signed in is
    /// [`ApiError::AccountChanged`].
    fn fetch<T>(
        &self,
        scope: &SessionScope,
        call: impl FnOnce(&JellyfinClient, &str) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        self.session.fetch_current(scope, call)
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
}

fn join_calendar(library: &Library, value: &mut Value) {
    let Some(entries) = value["entries"].as_array_mut() else {
        return;
    };
    let kinds = entries.iter().map(calendar_kind).collect::<Vec<_>>();
    let mut tmdb_by_kind: HashMap<CatalogKind, Vec<String>> = HashMap::new();
    let mut tvdb_by_kind: HashMap<CatalogKind, Vec<String>> = HashMap::new();
    for (entry, kind) in entries.iter().zip(&kinds) {
        let Some(kind) = *kind else {
            continue;
        };
        if let Some(id) = entry["tmdbId"].as_i64() {
            tmdb_by_kind.entry(kind).or_default().push(id.to_string());
        }
        if let Some(id) = entry["tvdbId"].as_i64() {
            tvdb_by_kind.entry(kind).or_default().push(id.to_string());
        }
        if kind == CatalogKind::Episode {
            if let Some(id) = entry["seriesTmdbId"].as_i64() {
                tmdb_by_kind
                    .entry(CatalogKind::Series)
                    .or_default()
                    .push(id.to_string());
            }
            if let Some(id) = entry["seriesTvdbId"].as_i64() {
                tvdb_by_kind
                    .entry(CatalogKind::Series)
                    .or_default()
                    .push(id.to_string());
            }
        }
    }
    let mut resolved: HashMap<(CatalogKind, String), String> = HashMap::new();
    for kind in [
        CatalogKind::Movie,
        CatalogKind::Episode,
        CatalogKind::Series,
    ] {
        for (id, item) in library
            .ids_by_tmdb(kind, tmdb_by_kind.get(&kind).map_or(&[], Vec::as_slice))
            .unwrap_or_default()
        {
            resolved.insert((kind, format!("tmdb:{id}")), item);
        }
        for (id, item) in library
            .ids_by_tvdb(kind, tvdb_by_kind.get(&kind).map_or(&[], Vec::as_slice))
            .unwrap_or_default()
        {
            resolved.insert((kind, format!("tvdb:{id}")), item);
        }
    }
    let find = |kind: CatalogKind, tmdb: &Value, tvdb: &Value| {
        tmdb.as_i64()
            .and_then(|id| resolved.get(&(kind, format!("tmdb:{id}"))))
            .or_else(|| {
                tvdb.as_i64()
                    .and_then(|id| resolved.get(&(kind, format!("tvdb:{id}"))))
            })
            .cloned()
    };
    for (entry, kind) in entries.iter_mut().zip(kinds) {
        let item = kind.and_then(|kind| find(kind, &entry["tmdbId"], &entry["tvdbId"]));
        let series_item = (kind == Some(CatalogKind::Episode))
            .then(|| {
                find(
                    CatalogKind::Series,
                    &entry["seriesTmdbId"],
                    &entry["seriesTvdbId"],
                )
            })
            .flatten();
        entry["libraryItemId"] = item.map_or(Value::Null, Value::String);
        entry["seriesLibraryItemId"] = series_item.map_or(Value::Null, Value::String);
    }
}

/// The Companion calendar lists movies and episodes.
fn calendar_kind(entry: &Value) -> Option<CatalogKind> {
    match entry["kind"].as_str()? {
        "movie" => Some(CatalogKind::Movie),
        "episode" => Some(CatalogKind::Episode),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{CompanionInfo, CompanionSession, ProbeState, join_calendar};
    use crate::jellyfin::api::model::BaseItemDto;
    use crate::jellyfin::session::Session;
    use crate::library::Library;
    use serde_json::json;

    fn companion_at(server_url: String) -> (Arc<Session>, CompanionSession) {
        let library = Arc::new(Library::open_in_memory().expect("library"));
        let mut credentials = library.credentials();
        credentials.server_url = Some(server_url);
        credentials.server_id = Some("server".to_string());
        credentials.user_id = Some("user".to_string());
        credentials.token = Some("token".to_string());
        library.save_credentials(&credentials).expect("credentials");
        let session = Arc::new(Session::restore(library.clone(), Arc::default()));
        let companion = CompanionSession::new(session.clone(), library, Arc::default());
        (session, companion)
    }

    struct ScriptedServer {
        url: String,
        response: Arc<Mutex<(u16, &'static str)>>,
        requests: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
    }

    impl ScriptedServer {
        fn start(status: u16, body: &'static str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
            listener
                .set_nonblocking(true)
                .expect("nonblocking listener");
            let server = Self {
                url: format!("http://{}", listener.local_addr().expect("address")),
                response: Arc::new(Mutex::new((status, body))),
                requests: Arc::default(),
                stop: Arc::default(),
            };
            let (response, requests, stop) = (
                server.response.clone(),
                server.requests.clone(),
                server.stop.clone(),
            );
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let mut reader = BufReader::new(&mut stream);
                    let mut line = String::new();
                    while reader.read_line(&mut line).is_ok_and(|read| read > 0) && line != "\r\n" {
                        line.clear();
                    }
                    drop(reader);
                    requests.fetch_add(1, Ordering::SeqCst);
                    let (status, body) = *response.lock().expect("scripted response");
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} Scripted\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
            });
            server
        }

        fn respond(&self, status: u16, body: &'static str) {
            *self.response.lock().expect("scripted response") = (status, body);
        }

        fn requests(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }
    }

    impl Drop for ScriptedServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    fn age_last_probe(companion: &CompanionSession, by: Duration) {
        let state = companion.read();
        companion.replace(ProbeState {
            checked_at: state.checked_at.and_then(|at| at.checked_sub(by)),
            ..state
        });
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
    fn failed_probes_are_retried_and_successes_refreshed_after_a_while() {
        // Failure modes: a failure stays cached until logout, so Companion
        // features remain off after a plugin restart; a failure or success is
        // re-probed on every read; a transient failure signs the user out; a
        // success is never refreshed, so a removed plugin still looks present.
        const MUCH_LATER: Duration = Duration::from_secs(10 * 60);
        let server = ScriptedServer::start(503, "");
        let (session, companion) = companion_at(server.url.clone());

        assert!(companion.probe(false).is_err());
        let requests = server.requests();
        assert!(requests > 0);
        let _ = companion.probe(false);
        assert_eq!(server.requests(), requests);
        assert!(session.is_authenticated());

        server.respond(200, r#"{"apiVersion":1,"capabilities":["calendar"]}"#);
        age_last_probe(&companion, MUCH_LATER);
        assert!(companion.probe(false).expect("retried probe").is_some());
        assert!(companion.supports("calendar"));
        let requests = server.requests();
        assert!(companion.probe(false).expect("reused probe").is_some());
        assert_eq!(server.requests(), requests);

        server.respond(404, "");
        age_last_probe(&companion, MUCH_LATER);
        assert!(companion.probe(false).expect("refreshed probe").is_none());
        assert!(!companion.supports("calendar"));
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
        library
            .upsert_page(&crate::jellyfin::session::Live::for_test(), &items)
            .expect("seed");
        library
            .apply_user_data(
                &crate::jellyfin::session::Live::for_test(),
                &[crate::library::UserDataRecord {
                    jellyfin_id: "m1".to_string(),
                    played: true,
                    ..Default::default()
                }],
            )
            .expect("user data");
        let mut detail = json!({
            "parts": [
                { "mediaType": "movie", "tmdbId": 603 },
                { "mediaType": "movie", "tmdbId": 348 },
                { "mediaType": "movie", "tmdbId": 624834 },
                { "mediaType": "tv", "tmdbId": 603 }
            ]
        });

        super::seerr::join_seerr_rows(
            &library,
            &mut detail,
            "parts",
            super::seerr::OtherKinds::Keep,
        );

        assert_eq!(detail["parts"][0]["libraryItemId"], "m1");
        assert_eq!(detail["parts"][0]["played"], true);
        assert_eq!(detail["parts"][1]["libraryItemId"], "m2");
        assert_eq!(detail["parts"][1]["played"], false);
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
        library
            .upsert_page(&crate::jellyfin::session::Live::for_test(), &items)
            .expect("seed");
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
