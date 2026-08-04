//! Quota-aware MDBList ratings for desktop cards.
//!
//! Catalog queries never call this module. Mounted cards request IDs after they
//! render; those IDs are deduplicated, resolved to stable TMDB/IMDb identities,
//! served stale-while-revalidate from SQLite, and refreshed in bounded batches.
//! A versioned Companion boundary is the fallback only when no valid local key
//! exists, and never returns the plugin administrator's credential.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::app::build_info;
use crate::app::urls::{build_query, join_url};
use crate::companion::CompanionSession;
use crate::integrations::credentials::{CredentialStore, OsCredentialStore};
use crate::jellyfin::api::ApiError;
use crate::library::{CachedRatings, IntegrationState, Library, RatingTarget, now_unix};

const MDBLIST_BASE_URL: &str = "https://api.mdblist.com";
const MDBLIST_CREDENTIAL: &str = "mdblist-api-key";
const TMDB_CREDENTIAL: &str = "tmdb-api-key";
const CACHE_SCHEMA_VERSION: i64 = 1;
const MAX_BATCH_SIZE: usize = 100;
// MDBList's official reference documents a hard maximum of 200 IDs.
const _: () = assert!(MAX_BATCH_SIZE <= 200);
const MAX_REQUEST_IDS: usize = 500;
const FRESH_SECONDS: i64 = 7 * 24 * 60 * 60;
const NEGATIVE_FRESH_SECONDS: i64 = 24 * 60 * 60;
const EXPIRE_SECONDS: i64 = 30 * 24 * 60 * 60;
const PLUGIN_FRESH_SECONDS: i64 = 24 * 60 * 60;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Local,
    Plugin,
}

impl Origin {
    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local_mdblist",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Debug)]
pub struct RatingsError(String);

impl RatingsError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for RatingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RatingsError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Quota {
    limit: Option<i64>,
    remaining: Option<i64>,
    reset_at: Option<i64>,
    retry_after_secs: Option<i64>,
}

impl Quota {
    fn retry_at(&self, now: i64) -> Option<i64> {
        self.retry_after_secs
            .map(|seconds| now.saturating_add(seconds.max(1)))
            .or_else(|| {
                (self.remaining == Some(0))
                    .then_some(self.reset_at)
                    .flatten()
            })
    }
}

#[derive(Debug)]
struct MdbResponse {
    body: Value,
    quota: Quota,
}

#[derive(Debug)]
enum MdbError {
    Unauthorized(Quota),
    RateLimited(Quota),
    Transport,
    Decode,
    Remote { status: u16, quota: Quota },
}

/// Authentication is a transport concern rather than a query-shape concern.
/// This release constructs only `ApiKey`; a confirmed Public PKCE flow can
/// supply a bearer token without changing batching, cache, or precedence.
pub enum MdbAuth<'a> {
    ApiKey(&'a str),
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "reserved for the documented future Public PKCE route"
        )
    )]
    PublicPkceBearer(&'a str),
}

trait MdbTransport: Send + Sync {
    fn validate(&self, auth: MdbAuth<'_>) -> Result<MdbResponse, MdbError>;
    fn batch(
        &self,
        auth: MdbAuth<'_>,
        provider: &str,
        media_type: &str,
        ids: &[String],
    ) -> Result<MdbResponse, MdbError>;
}

struct HttpTransport {
    agent: ureq::Agent,
}

impl HttpTransport {
    fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .http_status_as_error(false)
            .user_agent(format!("mediaflick-desktop/{}", build_info::APP_VERSION))
            .build()
            .into();
        Self { agent }
    }

    fn finish(
        &self,
        mut response: ureq::http::Response<ureq::Body>,
    ) -> Result<MdbResponse, MdbError> {
        let status = response.status().as_u16();
        let headers = response.headers();
        let mut quota = Quota {
            limit: integer_header(headers, "x-ratelimit-limit"),
            remaining: integer_header(headers, "x-ratelimit-remaining"),
            reset_at: integer_header(headers, "x-ratelimit-reset"),
            retry_after_secs: integer_header(headers, "retry-after"),
        };
        let bytes = response
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_to_vec()
            .map_err(|_| MdbError::Transport)?;
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice::<Value>(&bytes).map_err(|_| MdbError::Decode)?
        };
        absorb_body_quota(&mut quota, &body);
        match status {
            200..=299 => Ok(MdbResponse { body, quota }),
            401 | 403 => Err(MdbError::Unauthorized(quota)),
            429 => Err(MdbError::RateLimited(quota)),
            _ => Err(MdbError::Remote { status, quota }),
        }
    }
}

impl MdbTransport for HttpTransport {
    fn validate(&self, auth: MdbAuth<'_>) -> Result<MdbResponse, MdbError> {
        let (url, bearer) = authenticated_url("/user", auth);
        let mut request = self.agent.get(url).header("Accept", "application/json");
        if let Some(bearer) = bearer {
            request = request.header("Authorization", bearer);
        }
        let response = request.call().map_err(|_| MdbError::Transport)?;
        self.finish(response)
    }

    fn batch(
        &self,
        auth: MdbAuth<'_>,
        provider: &str,
        media_type: &str,
        ids: &[String],
    ) -> Result<MdbResponse, MdbError> {
        let path = format!("/{provider}/{media_type}/");
        let (url, bearer) = authenticated_url(&path, auth);
        let body_ids = ids
            .iter()
            .map(|id| {
                if provider == "tmdb" {
                    id.parse::<i64>().map_or_else(|_| json!(id), Value::from)
                } else {
                    json!(id)
                }
            })
            .collect::<Vec<_>>();
        let mut request = self.agent.post(url).header("Accept", "application/json");
        if let Some(bearer) = bearer {
            request = request.header("Authorization", bearer);
        }
        let response = request
            .send_json(json!({ "ids": body_ids }))
            .map_err(|_| MdbError::Transport)?;
        self.finish(response)
    }
}

fn authenticated_url(path: &str, auth: MdbAuth<'_>) -> (String, Option<String>) {
    let base = join_url(MDBLIST_BASE_URL, path);
    match auth {
        MdbAuth::ApiKey(key) => {
            let query = build_query(&[("apikey", key.to_string())]);
            (format!("{base}?{query}"), None)
        }
        MdbAuth::PublicPkceBearer(token) => (base, Some(format!("Bearer {token}"))),
    }
}

fn integer_header(headers: &ureq::http::HeaderMap, name: &str) -> Option<i64> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok())
}

fn integer_at(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
    })
}

fn absorb_body_quota(quota: &mut Quota, body: &Value) {
    let body_quota = body.get("quota").unwrap_or(body);
    quota.limit = quota
        .limit
        .or_else(|| integer_at(body_quota, &["limit", "api_requests", "apiRequests"]));
    quota.remaining = quota.remaining.or_else(|| {
        integer_at(
            body_quota,
            &[
                "remaining",
                "api_requests_remaining",
                "apiRequestsRemaining",
            ],
        )
    });
    if quota.remaining.is_none()
        && let (Some(limit), Some(used)) = (
            quota.limit,
            integer_at(
                body_quota,
                &["api_requests_count", "apiRequestsCount", "used"],
            ),
        )
    {
        quota.remaining = Some(limit.saturating_sub(used).max(0));
    }
    quota.reset_at = quota
        .reset_at
        .or_else(|| integer_at(body_quota, &["reset", "reset_at", "resetAt"]));
}

pub struct RatingsService {
    library: Arc<Library>,
    companion: Arc<CompanionSession>,
    credentials: Arc<dyn CredentialStore>,
    transport: Arc<dyn MdbTransport>,
    in_flight: Mutex<HashSet<String>>,
}

impl RatingsService {
    pub fn new(library: Arc<Library>, companion: Arc<CompanionSession>) -> Self {
        Self {
            library,
            companion,
            credentials: Arc::new(OsCredentialStore),
            transport: Arc::new(HttpTransport::new()),
            in_flight: Mutex::new(HashSet::new()),
        }
    }

    pub fn status(&self, selected_sources: &[String]) -> Value {
        let _ = self.companion.probe(false);
        let mdblist = self.credential_status(MDBLIST_CREDENTIAL);
        let tmdb = self.credential_status(TMDB_CREDENTIAL);
        let plugin = self.companion.supports("ratings-v1");
        let origin = effective_origin(
            mdblist["configured"].as_bool().unwrap_or(false),
            mdblist["valid"].as_bool().unwrap_or(false),
            plugin,
        );
        let mut sources = known_source_definitions();
        let known = sources
            .iter()
            .filter_map(|source| source["id"].as_str().map(str::to_string))
            .collect::<HashSet<_>>();
        for source in self.library.observed_rating_sources().unwrap_or_default() {
            if known.contains(&source) {
                continue;
            }
            sources.push(unknown_source_definition(&source));
        }
        json!({
            "boundaryVersion": 1,
            "auth": {
                "currentMode": "api_key",
                "supportedModes": ["api_key"],
                "futureModes": ["public_pkce"],
            },
            "credentialPrecedence": ["local", "plugin", "none"],
            "effectiveOrigin": origin.map(Origin::as_str).unwrap_or("none"),
            "available": origin.is_some(),
            "selectionEnabled": origin.is_some(),
            "local": { "mdblist": mdblist, "tmdb": tmdb },
            "plugin": {
                "available": plugin,
                "capability": "ratings-v1",
                "boundaryVersion": 1,
                "detail": if plugin {
                    "Compatible server rating capability detected. No administrator credential is exposed."
                } else {
                    "The optional ratings-v1 server capability is not available; local MDBList remains independent."
                },
            },
            "sources": sources,
            "selectedSources": selected_sources,
        })
    }

    pub fn save_credential(
        &self,
        provider: &str,
        secret: &str,
        selected_sources: &[String],
    ) -> Result<Value, RatingsError> {
        let provider = credential_name(provider)?;
        let secret = secret.trim();
        if secret.is_empty()
            || secret.len() > 2048
            || secret.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(RatingsError::new("enter a valid API key"));
        }
        if provider == TMDB_CREDENTIAL && !valid_tmdb_key_shape(secret) {
            self.save_state(IntegrationState {
                service: provider.to_string(),
                validation: "invalid".to_string(),
                detail: Some(
                    "TMDB keys are normally a 32-character v3 key or a v4 JWT token.".to_string(),
                ),
                updated_at: now_unix(),
                ..IntegrationState::default()
            })?;
            return Ok(self.status(selected_sources));
        }
        self.credentials
            .set(provider, secret)
            .map_err(|error| RatingsError::new(error.to_string()))?;
        if provider == MDBLIST_CREDENTIAL {
            self.validate_key(secret, false)?;
        } else {
            self.save_state(IntegrationState {
                service: provider.to_string(),
                validation: "saved".to_string(),
                valid: true,
                detail: Some(
                    "Saved securely for future TMDB features. Ratings do not use this key."
                        .to_string(),
                ),
                updated_at: now_unix(),
                ..IntegrationState::default()
            })?;
        }
        Ok(self.status(selected_sources))
    }

    pub fn validate_credential(
        &self,
        provider: &str,
        selected_sources: &[String],
    ) -> Result<Value, RatingsError> {
        let provider = credential_name(provider)?;
        let key = self
            .credentials
            .get(provider)
            .map_err(|error| RatingsError::new(error.to_string()))?
            .ok_or_else(|| RatingsError::new("no credential is saved"))?;
        if provider == MDBLIST_CREDENTIAL {
            self.validate_key(&key, true)?;
        } else {
            self.save_state(IntegrationState {
                service: provider.to_string(),
                validation: "saved".to_string(),
                valid: valid_tmdb_key_shape(&key),
                detail: Some(
                    "Saved securely for future TMDB features. It is not sent by rating retrieval."
                        .to_string(),
                ),
                updated_at: now_unix(),
                ..IntegrationState::default()
            })?;
        }
        Ok(self.status(selected_sources))
    }

    pub fn reveal_credential(&self, provider: &str) -> Result<String, RatingsError> {
        let provider = credential_name(provider)?;
        self.credentials
            .get(provider)
            .map_err(|error| RatingsError::new(error.to_string()))?
            .ok_or_else(|| RatingsError::new("no credential is saved"))
    }

    pub fn remove_credential(
        &self,
        provider: &str,
        selected_sources: &[String],
    ) -> Result<Value, RatingsError> {
        let provider = credential_name(provider)?;
        self.credentials
            .remove(provider)
            .map_err(|error| RatingsError::new(error.to_string()))?;
        self.library
            .clear_integration_state(provider)
            .map_err(storage_error)?;
        Ok(self.status(selected_sources))
    }

    fn credential_status(&self, name: &str) -> Value {
        let read = self.credentials.get(name);
        let (configured, store_error) = match read {
            Ok(value) => (value.is_some(), None),
            Err(error) => (false, Some(error.to_string())),
        };
        let state = self
            .library
            .integration_state(name)
            .ok()
            .flatten()
            .unwrap_or_else(|| IntegrationState {
                service: name.to_string(),
                validation: "unchecked".to_string(),
                ..IntegrationState::default()
            });
        json!({
            "configured": configured,
            "valid": configured && state.valid,
            "validation": if configured { state.validation.as_str() } else { "absent" },
            "detail": store_error.or(state.detail),
            "quota": {
                "limit": state.quota_limit,
                "remaining": state.quota_remaining,
                "resetAt": state.quota_reset_at,
            },
            "retryAt": state.retry_at,
            "lastCheckedAt": (state.updated_at > 0).then_some(state.updated_at),
            "storage": "os_credential_vault",
            "usedForRatings": name == MDBLIST_CREDENTIAL,
        })
    }

    fn validate_key(&self, key: &str, preserve_valid_on_offline: bool) -> Result<(), RatingsError> {
        let previous = self
            .library
            .integration_state(MDBLIST_CREDENTIAL)
            .map_err(storage_error)?
            .unwrap_or_default();
        let now = now_unix();
        let state = match self.transport.validate(MdbAuth::ApiKey(key)) {
            Ok(response) => state_with_quota(
                "valid",
                true,
                "Valid MDBList credential.",
                response.quota,
                0,
                now,
            ),
            Err(MdbError::Unauthorized(quota)) => state_with_quota(
                "invalid",
                false,
                "MDBList rejected this API key (401).",
                quota,
                0,
                now,
            ),
            Err(MdbError::RateLimited(quota)) => state_with_quota(
                "rate_limited",
                true,
                "MDBList accepted the credential but its request quota is exhausted.",
                quota,
                previous.failure_count,
                now,
            ),
            Err(MdbError::Transport | MdbError::Decode) => offline_state(
                previous,
                preserve_valid_on_offline,
                "MDBList could not be reached. The key is saved; retry when online.",
                now,
            ),
            Err(MdbError::Remote { status, quota }) => state_with_quota(
                "unavailable",
                preserve_valid_on_offline && previous.valid,
                &format!("MDBList returned HTTP {status}; retry later."),
                quota,
                previous.failure_count.saturating_add(1),
                now,
            ),
        };
        self.save_state(state)
    }

    fn save_state(&self, state: IntegrationState) -> Result<(), RatingsError> {
        self.library
            .save_integration_state(&state)
            .map_err(storage_error)
    }

    /// Returns cached data immediately where possible and performs at most the
    /// bounded refresh work owned by this call. The catalog endpoints never
    /// invoke this method, so rating latency cannot delay progressive sync.
    pub fn batch(&self, item_ids: &[String]) -> Result<Value, RatingsError> {
        let mut item_ids = item_ids
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .collect::<Vec<_>>();
        item_ids.sort();
        item_ids.dedup();
        item_ids.truncate(MAX_REQUEST_IDS);

        // A platform without a local vault can still consume the plugin's
        // capability/data boundary; a vault failure must not suppress it.
        let configured = self.credentials.get(MDBLIST_CREDENTIAL).unwrap_or(None);
        let state = self
            .library
            .integration_state(MDBLIST_CREDENTIAL)
            .map_err(storage_error)?
            .unwrap_or_default();
        let _ = self.companion.probe(false);
        let origin = effective_origin(
            configured.is_some(),
            state.valid,
            self.companion.supports("ratings-v1"),
        );
        let Some(origin) = origin else {
            return Ok(json!({
                "available": false,
                "effectiveOrigin": "none",
                "items": [],
                "retryAt": state.retry_at,
                "diagnostic": "No valid local MDBList key or compatible plugin rating capability is available.",
            }));
        };

        let targets = self
            .library
            .rating_targets(&item_ids)
            .map_err(storage_error)?;
        let now = now_unix();
        let mut cache = self
            .library
            .cached_ratings(&targets, origin.as_str())
            .map_err(storage_error)?;
        let mut refresh = targets
            .iter()
            .filter(|target| {
                cache.get(&target.item_id).is_none_or(|cached| {
                    cached.schema_version != CACHE_SCHEMA_VERSION || cached.stale_at <= now
                })
            })
            .cloned()
            .collect::<Vec<_>>();

        let backed_off = origin == Origin::Local && state.retry_at.is_some_and(|retry| retry > now);
        if backed_off {
            refresh.clear();
        }
        let owned = self.claim(origin, &refresh);
        let mut diagnostic = None;
        if !owned.is_empty() {
            let refreshed = match origin {
                Origin::Local => self.refresh_local(
                    configured
                        .as_deref()
                        .expect("local origin requires a credential"),
                    &owned,
                ),
                Origin::Plugin => self.refresh_plugin(&owned),
            };
            if let Err(error) = refreshed {
                diagnostic = Some(error);
            }
            self.release(origin, &owned);
            cache = self
                .library
                .cached_ratings(&targets, origin.as_str())
                .map_err(storage_error)?;
        }

        let items = targets
            .iter()
            .filter_map(|target| {
                let cached = cache.get(&target.item_id)?;
                (cached.expires_at > now).then(|| {
                    json!({
                        "id": target.item_id,
                        "ratings": cached.ratings,
                        "origin": cached.origin,
                        "fetchedAt": cached.fetched_at,
                        "sourceUpdatedAt": cached.source_updated_at,
                        "stale": cached.stale_at <= now,
                        "schemaVersion": cached.schema_version,
                    })
                })
            })
            .collect::<Vec<_>>();
        let latest_state = self
            .library
            .integration_state(MDBLIST_CREDENTIAL)
            .map_err(storage_error)?
            .unwrap_or(state);
        Ok(json!({
            "available": true,
            "effectiveOrigin": origin.as_str(),
            "items": items,
            "retryAt": latest_state.retry_at,
            "quota": {
                "limit": latest_state.quota_limit,
                "remaining": latest_state.quota_remaining,
                "resetAt": latest_state.quota_reset_at,
            },
            "diagnostic": diagnostic,
        }))
    }

    fn claim(&self, origin: Origin, targets: &[RatingTarget]) -> Vec<RatingTarget> {
        let Ok(mut in_flight) = self.in_flight.lock() else {
            return Vec::new();
        };
        targets
            .iter()
            .filter(|target| in_flight.insert(target_key(origin, target)))
            .cloned()
            .collect()
    }

    fn release(&self, origin: Origin, targets: &[RatingTarget]) {
        if let Ok(mut in_flight) = self.in_flight.lock() {
            for target in targets {
                in_flight.remove(&target_key(origin, target));
            }
        }
    }

    fn refresh_local(&self, key: &str, targets: &[RatingTarget]) -> Result<(), String> {
        let mut groups = BTreeMap::<(String, String), Vec<RatingTarget>>::new();
        for target in targets {
            groups
                .entry((target.provider.clone(), target.media_type.clone()))
                .or_default()
                .push(target.clone());
        }
        for ((provider, media_type), group) in groups {
            for chunk in group.chunks(MAX_BATCH_SIZE) {
                let ids = chunk
                    .iter()
                    .map(|target| target.provider_id.clone())
                    .collect::<Vec<_>>();
                match self
                    .transport
                    .batch(MdbAuth::ApiKey(key), &provider, &media_type, &ids)
                {
                    Ok(response) => {
                        self.note_quota(response.quota.clone())
                            .map_err(|error| error.to_string())?;
                        let entries = normalize_batch(
                            chunk,
                            &provider,
                            response.body,
                            Origin::Local,
                            now_unix(),
                        );
                        self.library
                            .save_rating_cache(&entries)
                            .map_err(|error| format!("could not cache MDBList ratings: {error}"))?;
                    }
                    Err(error) => {
                        self.note_fetch_error(&error)
                            .map_err(|state_error| state_error.to_string())?;
                        return Err(fetch_error_message(&error));
                    }
                }
            }
        }
        Ok(())
    }

    fn refresh_plugin(&self, targets: &[RatingTarget]) -> Result<(), String> {
        let body = json!({
            "boundaryVersion": 1,
            "items": targets,
        });
        let response = self.companion.ratings_v1(&body).map_err(|error| match error {
            ApiError::NotConfigured | ApiError::Status { status: 404 } | ApiError::Remote { status: 404, .. } => {
                "The server rating capability is not available yet; cached plugin ratings remain usable.".to_string()
            }
            _ => format!("The server rating capability is temporarily unavailable: {error}"),
        })?;
        let entries = normalize_plugin_batch(targets, response, now_unix());
        self.library
            .save_rating_cache(&entries)
            .map_err(|error| format!("could not cache plugin ratings: {error}"))
    }

    fn note_quota(&self, quota: Quota) -> Result<(), RatingsError> {
        let previous = self
            .library
            .integration_state(MDBLIST_CREDENTIAL)
            .map_err(storage_error)?
            .unwrap_or_default();
        let now = now_unix();
        self.save_state(IntegrationState {
            service: MDBLIST_CREDENTIAL.to_string(),
            validation: "valid".to_string(),
            valid: true,
            detail: Some("Valid MDBList credential.".to_string()),
            quota_limit: quota.limit.or(previous.quota_limit),
            quota_remaining: quota.remaining.or(previous.quota_remaining),
            quota_reset_at: quota.reset_at.or(previous.quota_reset_at),
            retry_at: quota.retry_at(now),
            failure_count: 0,
            updated_at: now,
        })
    }

    fn note_fetch_error(&self, error: &MdbError) -> Result<(), RatingsError> {
        let previous = self
            .library
            .integration_state(MDBLIST_CREDENTIAL)
            .map_err(storage_error)?
            .unwrap_or_default();
        let now = now_unix();
        let state = match error {
            MdbError::Unauthorized(quota) => state_with_quota(
                "invalid",
                false,
                "MDBList rejected the saved key (401). Validate or replace it.",
                quota.clone(),
                0,
                now,
            ),
            MdbError::RateLimited(quota) => state_with_quota(
                "rate_limited",
                true,
                "MDBList quota exhausted; cached ratings remain visible.",
                quota.clone(),
                previous.failure_count,
                now,
            ),
            MdbError::Remote { status, quota } => state_with_quota(
                "unavailable",
                previous.valid,
                &format!("MDBList returned HTTP {status}; cached ratings remain visible."),
                quota.clone(),
                previous.failure_count.saturating_add(1),
                now,
            ),
            MdbError::Transport | MdbError::Decode => offline_state(
                previous,
                true,
                "MDBList is offline or returned an unreadable response; cached ratings remain visible.",
                now,
            ),
        };
        self.save_state(state)
    }
}

fn credential_name(provider: &str) -> Result<&'static str, RatingsError> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "mdblist" => Ok(MDBLIST_CREDENTIAL),
        "tmdb" => Ok(TMDB_CREDENTIAL),
        _ => Err(RatingsError::new("unsupported credential provider")),
    }
}

fn valid_tmdb_key_shape(value: &str) -> bool {
    (value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        || (value.starts_with("eyJ") && value.matches('.').count() == 2 && value.len() <= 2048)
}

fn effective_origin(local_configured: bool, local_valid: bool, plugin: bool) -> Option<Origin> {
    if local_configured && local_valid {
        Some(Origin::Local)
    } else if plugin {
        Some(Origin::Plugin)
    } else {
        None
    }
}

fn state_with_quota(
    validation: &str,
    valid: bool,
    detail: &str,
    quota: Quota,
    failure_count: i64,
    now: i64,
) -> IntegrationState {
    IntegrationState {
        service: MDBLIST_CREDENTIAL.to_string(),
        validation: validation.to_string(),
        valid,
        detail: Some(detail.to_string()),
        quota_limit: quota.limit,
        quota_remaining: quota.remaining,
        quota_reset_at: quota.reset_at,
        retry_at: quota.retry_at(now),
        failure_count,
        updated_at: now,
    }
}

fn offline_state(
    previous: IntegrationState,
    preserve_valid: bool,
    detail: &str,
    now: i64,
) -> IntegrationState {
    let failures = previous.failure_count.saturating_add(1).min(10);
    let delay = 30_i64.saturating_mul(1_i64 << failures.min(8));
    IntegrationState {
        service: MDBLIST_CREDENTIAL.to_string(),
        validation: "offline".to_string(),
        valid: preserve_valid && previous.valid,
        detail: Some(detail.to_string()),
        retry_at: Some(now.saturating_add(delay.min(6 * 60 * 60))),
        failure_count: failures,
        updated_at: now,
        ..previous
    }
}

fn storage_error(error: rusqlite::Error) -> RatingsError {
    RatingsError::new(format!("ratings storage is unavailable: {error}"))
}

fn fetch_error_message(error: &MdbError) -> String {
    match error {
        MdbError::Unauthorized(_) => "MDBList rejected the saved credential.".to_string(),
        MdbError::RateLimited(_) => {
            "MDBList quota is exhausted; retry timing is being respected.".to_string()
        }
        MdbError::Transport => "MDBList is unreachable; stale ratings are being used.".to_string(),
        MdbError::Decode => {
            "MDBList returned an unreadable response; stale ratings are being used.".to_string()
        }
        MdbError::Remote { status, .. } => {
            format!("MDBList returned HTTP {status}; stale ratings are being used.")
        }
    }
}

fn target_key(origin: Origin, target: &RatingTarget) -> String {
    format!(
        "{}:{}:{}:{}",
        origin.as_str(),
        target.provider,
        target.media_type,
        target.provider_id
    )
}

fn normalize_batch(
    targets: &[RatingTarget],
    provider: &str,
    body: Value,
    origin: Origin,
    now: i64,
) -> Vec<(RatingTarget, CachedRatings)> {
    let media = body.as_array().cloned().unwrap_or_default();
    let mut by_id = HashMap::<String, (Value, Option<String>)>::new();
    for item in media {
        let Some(provider_id) = item
            .get("ids")
            .and_then(|ids| ids.get(provider))
            .and_then(value_id)
        else {
            continue;
        };
        let source_updated_at = item
            .get("updated")
            .and_then(Value::as_str)
            .map(str::to_string);
        by_id.insert(provider_id, (normalize_media(&item), source_updated_at));
    }
    targets
        .iter()
        .cloned()
        .map(|target| {
            let (ratings, source_updated_at) = by_id
                .get(&target.provider_id)
                .cloned()
                .unwrap_or_else(|| (json!([]), None));
            let fresh = if ratings.as_array().is_some_and(Vec::is_empty) {
                NEGATIVE_FRESH_SECONDS
            } else {
                FRESH_SECONDS
            };
            (
                target,
                CachedRatings {
                    ratings,
                    source_updated_at,
                    fetched_at: now,
                    stale_at: now.saturating_add(fresh),
                    expires_at: now.saturating_add(EXPIRE_SECONDS),
                    schema_version: CACHE_SCHEMA_VERSION,
                    origin: origin.as_str().to_string(),
                },
            )
        })
        .collect()
}

fn normalize_plugin_batch(
    targets: &[RatingTarget],
    body: Value,
    now: i64,
) -> Vec<(RatingTarget, CachedRatings)> {
    if body.get("boundaryVersion").and_then(Value::as_i64) != Some(1) {
        return Vec::new();
    }
    let mut by_item = HashMap::<String, (Value, Option<String>)>::new();
    for item in body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let Some(item_id) = item
            .get("itemId")
            .or_else(|| item.get("id"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let ratings = normalize_rating_array(item.get("ratings").unwrap_or(&Value::Null));
        let source_updated_at = item
            .get("sourceUpdatedAt")
            .and_then(Value::as_str)
            .map(str::to_string);
        by_item.insert(item_id.to_string(), (ratings, source_updated_at));
    }
    targets
        .iter()
        .filter_map(|target| {
            let (ratings, source_updated_at) = by_item.get(&target.item_id)?.clone();
            Some((
                target.clone(),
                CachedRatings {
                    ratings,
                    source_updated_at,
                    fetched_at: now,
                    stale_at: now.saturating_add(PLUGIN_FRESH_SECONDS),
                    expires_at: now.saturating_add(EXPIRE_SECONDS),
                    schema_version: CACHE_SCHEMA_VERSION,
                    origin: Origin::Plugin.as_str().to_string(),
                },
            ))
        })
        .collect()
}

fn value_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

fn normalize_media(item: &Value) -> Value {
    let mut ratings = normalize_rating_array(item.get("ratings").unwrap_or(&Value::Null))
        .as_array()
        .cloned()
        .unwrap_or_default();
    if let Some(score) = number(item.get("score"))
        && score >= 0.0
    {
        ratings.push(normalized_rating(
            "mdblist_score",
            "score",
            score,
            Some(score),
            None,
        ));
    }
    if let Some(score) = number(
        item.get("score_average")
            .or_else(|| item.get("scoreAverage")),
    ) && score >= 0.0
    {
        ratings.push(normalized_rating(
            "mdblist_score_average",
            "score_average",
            score,
            Some(score),
            None,
        ));
    }
    deduplicate_ratings(ratings)
}

fn normalize_rating_array(value: &Value) -> Value {
    let mut ratings = Vec::new();
    for rating in value.as_array().into_iter().flatten() {
        let Some(raw_source) = rating
            .get("source")
            .or_else(|| rating.get("sourceId"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let source = canonical_source(raw_source);
        let raw_value = number(rating.get("value").or_else(|| rating.get("rating")));
        let score = number(rating.get("score"));
        let Some(value) = native_value(&source, raw_value, score) else {
            continue;
        };
        let votes = rating.get("votes").and_then(|votes| {
            votes
                .as_i64()
                .or_else(|| votes.as_str()?.parse::<i64>().ok())
                .filter(|votes| *votes >= 0)
        });
        ratings.push(normalized_rating(&source, raw_source, value, score, votes));
    }
    deduplicate_ratings(ratings)
}

fn deduplicate_ratings(ratings: Vec<Value>) -> Value {
    let mut by_source = BTreeMap::<String, Value>::new();
    for rating in ratings {
        if let Some(source) = rating.get("sourceId").and_then(Value::as_str) {
            by_source.insert(source.to_string(), rating);
        }
    }
    Value::Array(by_source.into_values().collect())
}

fn normalized_rating(
    source: &str,
    raw_source: &str,
    value: f64,
    score: Option<f64>,
    votes: Option<i64>,
) -> Value {
    json!({
        "sourceId": source,
        "rawSource": raw_source,
        "value": value,
        "score": score,
        "votes": votes,
        "scaleMax": scale_max(source),
    })
}

fn number(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    let number = value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())?;
    number.is_finite().then_some(number)
}

fn native_value(source: &str, raw: Option<f64>, score: Option<f64>) -> Option<f64> {
    let maximum = scale_max(source);
    if let Some(raw) = raw
        && raw >= 0.0
        && raw <= maximum
    {
        return Some(raw);
    }
    score
        .filter(|score| (0.0..=100.0).contains(score))
        .map(|score| score * maximum / 100.0)
}

fn canonical_source(source: &str) -> String {
    let compact = source.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    match compact.as_str() {
        "mal" => "myanimelist".to_string(),
        "audience" | "popcorn" | "tomatoesaudience" | "tomatoes_audience" | "rtaudience"
        | "rt_audience" => "popcorn".to_string(),
        "tomato" | "tomatometer" | "rtomatoes" | "rt_critic" => "tomatoes".to_string(),
        "score" | "mdblist" | "mdblist_score" => "mdblist_score".to_string(),
        "scoreaverage" | "score_average" | "mdblist_score_average" => {
            "mdblist_score_average".to_string()
        }
        _ => safe_source_id(&compact),
    }
}

fn safe_source_id(source: &str) -> String {
    let safe = source
        .chars()
        .filter(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-')
        })
        .take(64)
        .collect::<String>();
    if safe.is_empty() {
        "unknown".to_string()
    } else {
        safe
    }
}

fn scale_max(source: &str) -> f64 {
    match source {
        "imdb" | "tmdb" | "metacriticuser" | "myanimelist" => 10.0,
        "letterboxd" => 5.0,
        "rogerebert" => 4.0,
        _ => 100.0,
    }
}

fn known_source_definitions() -> Vec<Value> {
    vec![
        source_definition("mdblist_score", "MDBList Score", "MDB", 100.0, "percent"),
        source_definition(
            "mdblist_score_average",
            "MDBList Score Average",
            "AVG",
            100.0,
            "percent",
        ),
        source_definition("imdb", "IMDb", "IMDb", 10.0, "decimal"),
        source_definition("trakt", "Trakt", "Trakt", 100.0, "percent"),
        source_definition("tmdb", "TMDB", "TMDB", 10.0, "decimal"),
        source_definition("letterboxd", "Letterboxd", "LB", 5.0, "stars"),
        source_definition(
            "tomatoes",
            "Rotten Tomatoes Critics",
            "RT",
            100.0,
            "percent",
        ),
        source_definition(
            "popcorn",
            "Rotten Tomatoes Audience",
            "RT A",
            100.0,
            "percent",
        ),
        source_definition("metacritic", "Metacritic Critics", "MC", 100.0, "integer"),
        source_definition(
            "metacriticuser",
            "Metacritic Users",
            "MC U",
            10.0,
            "decimal",
        ),
        source_definition("rogerebert", "Roger Ebert", "Ebert", 4.0, "stars"),
        source_definition("myanimelist", "MyAnimeList", "MAL", 10.0, "decimal"),
    ]
}

fn source_definition(id: &str, label: &str, short_label: &str, scale: f64, format: &str) -> Value {
    json!({
        "id": id,
        "label": label,
        "shortLabel": short_label,
        "scaleMax": scale,
        "format": format,
        "known": true,
    })
}

fn unknown_source_definition(id: &str) -> Value {
    let label = id
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + characters.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ");
    json!({
        "id": safe_source_id(id),
        "label": if label.is_empty() { "New MDBList source" } else { &label },
        "shortLabel": safe_source_id(id).chars().take(6).collect::<String>().to_ascii_uppercase(),
        "scaleMax": 100,
        "format": "decimal",
        "known": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_sources_cover_current_official_fields_and_aliases() {
        assert_eq!(canonical_source("letterboxd"), "letterboxd");
        assert_eq!(canonical_source("tomatoes"), "tomatoes");
        assert_eq!(canonical_source("audience"), "popcorn");
        assert_eq!(canonical_source("tomatoesaudience"), "popcorn");
        assert_eq!(canonical_source("mal"), "myanimelist");
        assert_eq!(canonical_source("score_average"), "mdblist_score_average");
        let ids = known_source_definitions()
            .into_iter()
            .map(|definition| definition["id"].as_str().expect("id").to_string())
            .collect::<HashSet<_>>();
        for required in [
            "letterboxd",
            "tomatoes",
            "popcorn",
            "metacritic",
            "metacriticuser",
            "rogerebert",
            "myanimelist",
        ] {
            assert!(ids.contains(required), "missing {required}");
        }
    }

    #[test]
    fn normalization_uses_native_scales_and_preserves_unknown_sources() {
        let ratings = normalize_media(&json!({
            "score": 84,
            "score_average": 81,
            "ratings": [
                { "source": "imdb", "value": 8.1, "score": 81, "votes": 10 },
                { "source": "letterboxd", "value": 8, "score": 80 },
                { "source": "tomatoes", "value": 97, "score": 97 },
                { "source": "audience", "value": 91, "score": 91 },
                { "source": "future-meter!", "value": 7.25, "score": 73 }
            ]
        }));
        let by_source = ratings
            .as_array()
            .expect("ratings")
            .iter()
            .map(|rating| (rating["sourceId"].as_str().expect("source"), rating))
            .collect::<HashMap<_, _>>();
        assert_eq!(by_source["letterboxd"]["value"], 4.0);
        assert_eq!(by_source["letterboxd"]["scaleMax"], 5.0);
        assert_eq!(by_source["popcorn"]["value"], 91.0);
        assert_eq!(by_source["future_meter"]["rawSource"], "future-meter!");
        assert!(by_source.contains_key("mdblist_score"));
        assert!(by_source.contains_key("mdblist_score_average"));
    }

    #[test]
    fn batch_normalization_keeps_partial_and_missing_results_independent() {
        let targets = [
            RatingTarget {
                item_id: "a".to_string(),
                kind: "Movie".to_string(),
                media_type: "movie".to_string(),
                provider: "tmdb".to_string(),
                provider_id: "603".to_string(),
            },
            RatingTarget {
                item_id: "b".to_string(),
                kind: "Movie".to_string(),
                media_type: "movie".to_string(),
                provider: "tmdb".to_string(),
                provider_id: "604".to_string(),
            },
        ];
        let entries = normalize_batch(
            &targets,
            "tmdb",
            json!([{
                "ids": { "tmdb": 603 },
                "updated": "2026-08-04T20:00:00Z",
                "ratings": [{ "source": "imdb", "value": 8.7, "score": 87 }]
            }]),
            Origin::Local,
            100,
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1.ratings[0]["sourceId"], "imdb");
        assert_eq!(
            entries[0].1.source_updated_at.as_deref(),
            Some("2026-08-04T20:00:00Z")
        );
        assert_eq!(entries[1].1.ratings, json!([]));
        assert_eq!(entries[1].1.stale_at, 100 + NEGATIVE_FRESH_SECONDS);
    }

    #[test]
    fn credential_precedence_is_local_then_plugin_then_none() {
        assert_eq!(effective_origin(true, true, true), Some(Origin::Local));
        assert_eq!(effective_origin(false, false, true), Some(Origin::Plugin));
        assert_eq!(effective_origin(true, false, true), Some(Origin::Plugin));
        assert_eq!(effective_origin(true, false, false), None);
    }

    #[test]
    fn auth_modes_share_transport_without_putting_bearer_tokens_in_urls() {
        let (api_key_url, api_key_header) = authenticated_url("/user", MdbAuth::ApiKey("a b"));
        assert!(api_key_url.ends_with("/user?apikey=a%20b"));
        assert_eq!(api_key_header, None);
        let (pkce_url, pkce_header) =
            authenticated_url("/user", MdbAuth::PublicPkceBearer("token"));
        assert!(pkce_url.ends_with("/user"));
        assert!(!pkce_url.contains("token"));
        assert_eq!(pkce_header.as_deref(), Some("Bearer token"));
    }

    #[test]
    fn quota_body_and_retry_after_are_normalized() {
        let mut quota = Quota {
            retry_after_secs: Some(120),
            ..Quota::default()
        };
        absorb_body_quota(
            &mut quota,
            &json!({ "api_requests": 1000, "api_requests_count": 22 }),
        );
        assert_eq!(quota.limit, Some(1000));
        assert_eq!(quota.remaining, Some(978));
        assert_eq!(quota.retry_at(100), Some(220));
    }

    #[test]
    fn tmdb_validation_is_format_only_and_explicit() {
        assert!(valid_tmdb_key_shape("0123456789abcdef0123456789abcdef"));
        assert!(valid_tmdb_key_shape("eyJabc.payload.signature"));
        assert!(!valid_tmdb_key_shape("not-a-tmdb-key"));
    }
}
