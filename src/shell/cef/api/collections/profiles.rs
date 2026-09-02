use serde::Deserialize;

use super::*;
use crate::collections::profiles::{
    allocate_profile_id, allocate_revision_id, apply_normalized_mdblist_id,
    result_configuration_changed,
};
use crate::collections::snapshots::{RefreshState, SnapshotRepository};
use crate::collections::{
    CollectionMode, CollectionProfile, CollectionSnapshot, CollectionSource, MediaType,
    RefreshCadence, ResultLimit, TemplateReference,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileDraft {
    template: TemplateReference,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    custom_poster_id: Option<String>,
    source: CollectionSource,
    media_type: MediaType,
    #[serde(default)]
    limit: ResultLimit,
    #[serde(default)]
    cadence: RefreshCadence,
    #[serde(default)]
    available_on_home: bool,
}

impl ProfileDraft {
    fn into_profile(self, id: String, revision: String) -> std::io::Result<CollectionProfile> {
        let mut profile = CollectionProfile {
            id,
            revision,
            template: self.template,
            title: self.title.trim().to_string(),
            description: self.description.trim().to_string(),
            custom_poster_id: self.custom_poster_id,
            source: self.source,
            media_type: self.media_type,
            limit: self.limit,
            cadence: self.cadence,
            available_on_home: self.available_on_home,
        };
        apply_normalized_mdblist_id(&mut profile.source)?;
        profile
            .validate()
            .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
        Ok(profile)
    }
}

pub(super) fn settings(services: &Arc<Services>, force_probe: bool) -> ApiResponse {
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    let readiness = services.companion.collection_readiness(force_probe);
    let repository = SnapshotRepository::new(&services.library);
    let has_results = repository.has_account_results(&account).unwrap_or(false);
    let effective_mode = services
        .collections
        .effective_mode(&account, &readiness, has_results);
    let settings = services.collections.account(&account);
    let recovery = services.collections.take_recovery_notice();
    ApiResponse::ok(json!({
        "effectiveMode": effective_mode,
        "mediaFlickAvailable": readiness.tmdb || !settings.profiles.is_empty() || has_results,
        "modeSelection": settings.mode_selection,
        "franchises": settings.franchises,
        "recovery": recovery.map(|notice| json!({
            "damagedPath": notice.damaged_path,
            "restoredBackup": notice.restored_backup,
        })),
        "readiness": readiness,
    }))
}

pub(super) fn patch_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let scope = match active_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let account = scope.account().clone();
    let body = request.json();
    let mode = match body.get("modeSelection") {
        Some(mode) => match serde_json::from_value::<CollectionMode>(mode.clone()) {
            Ok(mode) => mode,
            Err(_) => return ApiResponse::error(400, "modeSelection is invalid"),
        },
        None => CollectionMode::default(),
    };
    let updates_mode = body.get("modeSelection").is_some();
    if updates_mode && mode == CollectionMode::MediaFlick {
        let readiness = services.companion.collection_readiness(false);
        let has_results = SnapshotRepository::new(&services.library)
            .has_account_results(&account)
            .unwrap_or(false);
        let has_configuration = !services.collections.account(&account).profiles.is_empty();
        if !readiness.tmdb && !has_configuration && !has_results {
            return ApiResponse::error(409, "MediaFlick collections are unavailable");
        }
    }
    let include_unreleased = body
        .get("includeUnreleased")
        .and_then(serde_json::Value::as_bool);
    let mutation = services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            if updates_mode {
                if let Err(error) = services.collections.set_mode(&account, mode) {
                    return Ok(Some(configuration_failure(&error)));
                }
                if mode == CollectionMode::MediaFlick {
                    crate::collections::scheduler::request_run(services.clone());
                }
            }
            if let Some(include) = include_unreleased
                && let Err(error) = services
                    .collections
                    .set_include_unreleased(&account, include)
            {
                return Ok(Some(configuration_failure(&error)));
            }
            Ok(None)
        });
    match mutation {
        Err(response) | Ok(Some(response)) => response,
        Ok(None) => settings(services, false),
    }
}

pub(super) fn templates(services: &Arc<Services>) -> ApiResponse {
    let readiness = services.companion.collection_readiness(false);
    let templates = crate::collections::templates::catalog()
        .into_iter()
        .map(|template| {
            let available = match template.source.provider() {
                crate::collections::Provider::Tmdb => readiness.tmdb,
                crate::collections::Provider::MdbList => readiness.mdblist,
            };
            json!({ "template": template, "available": available })
        })
        .collect::<Vec<_>>();
    ApiResponse::ok(json!({
        "categories": crate::collections::templates::TemplateCategory::ORDER,
        "templates": templates,
        "readiness": readiness,
    }))
}

pub(super) fn preview(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    let draft = match parse_draft(request) {
        Ok(draft) => draft,
        Err(response) => return response,
    };
    let profile = match draft.into_profile(allocate_profile_id(), allocate_revision_id()) {
        Ok(profile) => profile,
        Err(error) => return configuration_failure(&error),
    };
    crate::collections::scheduler::hydrate_identity_map(services);
    if services.session.user_restricted() {
        if !crate::library::sync::ownership_available(&services.library) {
            return ApiResponse::error(409, "Preview is unavailable until the library is ready");
        }
        let result = match services
            .companion
            .refresh_collection(&provider_request(services, &profile))
        {
            Ok(result) => result,
            Err(error) => return ApiResponse::from_api_error(&error),
        };
        let classified = match crate::collections::matching::classify(
            &services.library,
            &account,
            &result.items,
            crate::collections::matching::OwnershipPolicy {
                complete_sync: true,
                restricted_user: true,
            },
        ) {
            Ok(classified) => classified,
            Err(error) => return storage_failure(&error),
        };
        let mut items = classified
            .owned
            .into_iter()
            .map(|item| item.title)
            .collect::<Vec<_>>();
        let total = u32::try_from(items.len()).unwrap_or(u32::MAX);
        let movies = u32::try_from(
            items
                .iter()
                .filter(|item| item.identity.media_type == MediaType::Movie)
                .count(),
        )
        .unwrap_or(u32::MAX);
        let series = total.saturating_sub(movies);
        items.truncate(24);
        return ApiResponse::ok(json!(crate::collections::ProviderResult {
            items,
            total,
            movies,
            series,
            source_identity: result.source_identity,
        }));
    }
    match services
        .companion
        .preview_collection(&provider_request(services, &profile))
    {
        Ok(result) => ApiResponse::ok(json!(result)),
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

pub(super) fn list(services: &Arc<Services>) -> ApiResponse {
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    let settings = services.collections.account(&account);
    let errors = services.collections.profile_errors(&account);
    ApiResponse::ok(json!({ "profiles": settings.profiles, "errors": errors }))
}

pub(super) fn read(services: &Arc<Services>, profile_id: &str) -> ApiResponse {
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    match profile(services, &account, profile_id) {
        Some(profile) => ApiResponse::ok(json!(profile)),
        None => ApiResponse::error(404, "that collection does not exist"),
    }
}

pub(super) fn create(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    let draft = match parse_draft(request) {
        Ok(draft) => draft,
        Err(response) => return response,
    };
    let profile = match draft.into_profile(allocate_profile_id(), allocate_revision_id()) {
        Ok(profile) => profile,
        Err(error) => return configuration_failure(&error),
    };
    let staged_artwork = profile.custom_poster_id.clone();
    let response = commit_result_profile(services, &account, profile, true);
    if response.status >= 400 {
        remove_unreferenced_artwork(services, &account, staged_artwork.as_deref());
    }
    response
}

pub(super) fn edit(
    services: &Arc<Services>,
    profile_id: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let scope = match active_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let account = scope.account().clone();
    let Some(previous) = profile(services, &account, profile_id) else {
        return ApiResponse::error(404, "that collection does not exist");
    };
    let draft = match parse_draft(request) {
        Ok(draft) => draft,
        Err(response) => return response,
    };
    let next = match draft.into_profile(previous.id.clone(), previous.revision.clone()) {
        Ok(profile) => profile,
        Err(error) => return configuration_failure(&error),
    };
    let staged_artwork = (next.custom_poster_id != previous.custom_poster_id)
        .then(|| next.custom_poster_id.clone())
        .flatten();
    let forget_home = previous.available_on_home && !next.available_on_home;
    let response = if result_configuration_changed(&previous, &next) {
        let response = commit_result_profile(
            services,
            &account,
            CollectionProfile {
                revision: allocate_revision_id(),
                ..next
            },
            true,
        );
        if response.status >= 400 {
            remove_unreferenced_artwork(services, &account, staged_artwork.as_deref());
        }
        response
    } else {
        services
            .session
            .commit_if_current(&scope, stale_account_response, || {
                Ok(match services.collections.save_profile(&account, next) {
                    Ok(profile) => {
                        update_next_due(services, &account, &profile);
                        ApiResponse::ok(json!(profile))
                    }
                    Err(error) => {
                        remove_unreferenced_artwork(services, &account, staged_artwork.as_deref());
                        configuration_failure(&error)
                    }
                })
            })
            .unwrap_or_else(|response| response)
    };
    if response.status < 400
        && forget_home
        && let Err(error) = services
            .accounts
            .forget_home_collection(&account, profile_id)
    {
        return configuration_failure(&error);
    }
    response
}

fn update_next_due(services: &Services, account: &AccountKey, profile: &CollectionProfile) {
    let repository = SnapshotRepository::new(&services.library);
    let Ok(mut state) = repository.refresh_state(account, &profile.id) else {
        return;
    };
    state.next_due = state.last_success.and_then(|last_success| {
        crate::collections::scheduler::next_due_at(last_success, profile.cadence)
    });
    if let Err(error) = repository.save_refresh_state(account, &profile.id, &state) {
        tracing::warn!(target: "collections", "could not update collection cadence: {error}");
    }
}

pub(super) fn refresh(services: &Arc<Services>, profile_id: &str) -> ApiResponse {
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    let Some(profile) = profile(services, &account, profile_id) else {
        return ApiResponse::error(404, "that collection does not exist");
    };
    if let Err(error) = profile.validate() {
        return ApiResponse::error(409, error);
    }
    commit_result_profile(services, &account, profile, false)
}

pub(super) fn delete(services: &Arc<Services>, profile_id: &str) -> ApiResponse {
    let scope = match active_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let account = scope.account().clone();
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(match services.collections.remove_profile(&account, profile_id) {
                Ok(Some(_)) => {
                    let repository = SnapshotRepository::new(&services.library);
                    if let Err(error) = repository.remove_profile(&account, profile_id) {
                        tracing::warn!(target: "collections", "could not remove collection cache: {error}");
                    }
                    match services.accounts.forget_home_collection(&account, profile_id) {
                        Ok(()) => ApiResponse::ok(json!({ "deleted": true })),
                        Err(error) => configuration_failure(&error),
                    }
                }
                Ok(None) => ApiResponse::error(404, "that collection does not exist"),
                Err(error) => configuration_failure(&error),
            })
        })
        .unwrap_or_else(|response| response)
}

pub(super) fn reorder(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let scope = match active_scope(services) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let account = scope.account().clone();
    let ids = request
        .json()
        .get("profileIds")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(
                match services.collections.reorder_profiles(&account, &ids) {
                    Ok(()) => {
                        let settings = services.collections.account(&account);
                        let errors = services.collections.profile_errors(&account);
                        ApiResponse::ok(json!({
                            "profiles": settings.profiles,
                            "errors": errors,
                        }))
                    }
                    Err(error) => configuration_failure(&error),
                },
            )
        })
        .unwrap_or_else(|response| response)
}

fn commit_result_profile(
    services: &Arc<Services>,
    account: &AccountKey,
    mut profile: CollectionProfile,
    save_configuration: bool,
) -> ApiResponse {
    let scope = match services.session.scope() {
        Ok(scope) if scope.account() == account => scope,
        _ => return stale_account_response(),
    };
    let repository = SnapshotRepository::new(&services.library);
    let attempt = crate::library::now_unix();
    let result = match services
        .companion
        .refresh_collection(&provider_request(services, &profile))
    {
        Ok(result) => result,
        Err(error) => {
            if let Err(response) =
                services
                    .session
                    .commit_if_current(&scope, stale_account_response, || {
                        save_failed_refresh(&repository, account, &profile.id, attempt);
                        Ok(())
                    })
            {
                return response;
            }
            return ApiResponse::from_api_error(&error);
        }
    };
    if let (CollectionSource::MdbListPublicList { list_id, .. }, Some(source_identity)) =
        (&mut profile.source, result.source_identity.as_deref())
    {
        *list_id = source_identity.to_string();
    }
    let snapshot = CollectionSnapshot {
        profile_id: profile.id.clone(),
        revision: profile.revision.clone(),
        committed_at: attempt,
        items: result.items,
    };
    let visible_total = if services.session.user_restricted() {
        crate::collections::matching::classify(
            &services.library,
            account,
            &snapshot.items,
            crate::collections::matching::OwnershipPolicy {
                complete_sync: crate::library::sync::ownership_available(&services.library),
                restricted_user: true,
            },
        )
        .map(|classified| u32::try_from(classified.owned.len()).unwrap_or(u32::MAX))
        .unwrap_or(0)
    } else {
        result.total
    };
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            if let Err(error) = repository.commit_profile(account, &snapshot) {
                return Ok(storage_failure(&error));
            }
            if save_configuration
                && let Err(error) = services.collections.save_profile(account, profile.clone())
            {
                if let Err(cleanup_error) =
                    repository.remove_revision(account, &profile.id, &profile.revision)
                {
                    tracing::warn!(
                        target: "collections",
                        "could not roll back an unreferenced collection revision: {cleanup_error}"
                    );
                }
                return Ok(configuration_failure(&error));
            }
            if save_configuration {
                let active = services
                    .collections
                    .account(account)
                    .profiles
                    .into_iter()
                    .map(|stored| (stored.id, stored.revision))
                    .collect();
                let _ = repository.remove_unreferenced_revisions(account, &active);
            }
            let next_due = crate::collections::scheduler::next_due_at(attempt, profile.cadence);
            let _ = repository.save_refresh_state(
                account,
                &profile.id,
                &RefreshState {
                    last_attempt: Some(attempt),
                    last_success: Some(attempt),
                    latest_failure: None,
                    next_due,
                    initialized: true,
                },
            );
            Ok(ApiResponse::ok(
                json!({ "profile": profile, "total": visible_total }),
            ))
        })
        .unwrap_or_else(|response| response)
}

fn stale_account_response() -> ApiResponse {
    ApiResponse::error(
        409,
        "the Jellyfin account changed while the request was running",
    )
}

fn save_failed_refresh(
    repository: &SnapshotRepository<'_>,
    account: &AccountKey,
    profile_id: &str,
    attempt: i64,
) {
    let mut state = repository
        .refresh_state(account, profile_id)
        .unwrap_or_default();
    state.last_attempt = Some(attempt);
    state.latest_failure = Some("Results unavailable".to_string());
    let _ = repository.save_refresh_state(account, profile_id, &state);
}

fn profile(
    services: &Arc<Services>,
    account: &AccountKey,
    profile_id: &str,
) -> Option<CollectionProfile> {
    services
        .collections
        .account(account)
        .profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
}

fn remove_unreferenced_artwork(
    services: &Arc<Services>,
    account: &AccountKey,
    artwork_id: Option<&str>,
) {
    let Some(artwork_id) = artwork_id else {
        return;
    };
    let referenced = services
        .collections
        .account(account)
        .profiles
        .iter()
        .any(|profile| profile.custom_poster_id.as_deref() == Some(artwork_id));
    if !referenced && let Err(error) = services.artwork.remove(artwork_id) {
        tracing::warn!(target: "collections", "could not roll back custom artwork: {error}");
    }
}

fn provider_request(services: &Services, profile: &CollectionProfile) -> Value {
    let owned_tmdb_ids = matches!(profile.source, CollectionSource::TmdbCollection { .. })
        .then(|| {
            crate::collections::matching::owned_tmdb_ids(
                &services.library,
                crate::collections::MediaType::Movie,
            )
            .unwrap_or_default()
        })
        .unwrap_or_default();
    json!({
        "source": profile.source,
        "mediaType": profile.media_type,
        "limit": profile.limit,
        "ownedTmdbIds": owned_tmdb_ids,
    })
}

fn parse_draft(request: &ApiRequest) -> Result<ProfileDraft, ApiResponse> {
    serde_json::from_value(request.json())
        .map_err(|error| ApiResponse::error(400, format!("invalid collection profile: {error}")))
}
