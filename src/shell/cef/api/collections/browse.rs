use super::*;
use crate::collections::franchises::visible_franchises;
use crate::collections::matching::{OwnershipPolicy, classify, local_item_map};
use crate::collections::snapshots::SnapshotRepository;

pub(super) fn redirect_state(services: &Arc<Services>) -> ApiResponse {
    super::profiles::settings(services, false)
}

pub(super) fn mine(services: &Arc<Services>) -> ApiResponse {
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    ApiResponse::ok(json!({
        "profiles": services.collections.account(&account).profiles,
        "errors": services.collections.profile_errors(&account),
    }))
}

pub(super) fn profile_detail(services: &Arc<Services>, profile_id: &str) -> ApiResponse {
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    let Some(profile) = services
        .collections
        .account(&account)
        .profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
    else {
        return ApiResponse::error(404, "that collection does not exist");
    };
    let repository = SnapshotRepository::new(&services.library);
    let refresh = repository
        .refresh_state(&account, &profile.id)
        .unwrap_or_default();
    let snapshot = match repository.profile(&account, &profile.id, &profile.revision) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => {
            let unavailable = profile.validate().is_err() || refresh.latest_failure.is_some();
            return ApiResponse::ok(json!({
                "profile": profile,
                "status": if unavailable { "resultsUnavailable" } else { "updating" },
                "owned": [],
                "missing": [],
                "items": [],
                "refresh": refresh,
            }));
        }
        Err(error) => return storage_failure(&error),
    };
    crate::collections::scheduler::hydrate_identity_map(services);
    let classified = match classify(
        &services.library,
        &account,
        &snapshot.items,
        OwnershipPolicy {
            complete_sync: crate::library::sync::ownership_available(&services.library),
            restricted_user: services.session.user_restricted(),
        },
    ) {
        Ok(classified) => classified,
        Err(error) => return storage_failure(&error),
    };
    let overdue = crate::collections::scheduler::is_due(
        refresh.last_success,
        profile.cadence,
        crate::library::now_unix(),
    );
    ApiResponse::ok(json!({
        "profile": profile,
        "status": "ready",
        "owned": classified.owned,
        "missing": classified.missing,
        "items": classified.items,
        "ownershipAvailable": classified.ownership_available,
        "refresh": refresh,
        "overdue": overdue,
    }))
}

pub(super) fn franchises(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    let ownership_available = crate::library::sync::ownership_available(&services.library);
    let repository = SnapshotRepository::new(&services.library);
    let refresh = match repository.franchise_refresh_state(&account) {
        Ok(refresh) => refresh,
        Err(error) => return storage_failure(&error),
    };
    if ownership_available && !refresh.initialized {
        crate::collections::scheduler::request_run(services.clone());
    }
    let status = if refresh.initialized {
        "ready"
    } else if refresh.latest_failure.is_some() {
        "resultsUnavailable"
    } else {
        "updating"
    };
    crate::collections::scheduler::hydrate_identity_map(services);
    let snapshots = match repository.franchises(&account) {
        Ok(snapshots) => snapshots,
        Err(error) => return storage_failure(&error),
    };
    if !ownership_available {
        let restricted = services.session.user_restricted();
        let franchises = snapshots
            .into_iter()
            .map(|mut snapshot| {
                snapshot.items.sort_by_key(|item| item.source_order);
                snapshot.items.retain(|item| !item.adult);
                json!({
                    "collectionId": snapshot.collection_id,
                    "name": snapshot.name,
                    "posterPath": snapshot.poster_path,
                    "backdropPath": snapshot.backdrop_path,
                    "owned": [],
                    "missing": [],
                    "items": if restricted { Vec::new() } else { snapshot.items },
                    "ownershipAvailable": false,
                })
            })
            .collect::<Vec<_>>();
        return ApiResponse::ok(json!({
            "franchises": franchises,
            "status": status,
        }));
    }
    let provider_items = snapshots
        .iter()
        .flat_map(|snapshot| snapshot.items.iter().cloned())
        .collect::<Vec<_>>();
    let local = match local_item_map(&services.library, &provider_items) {
        Ok(local) => local,
        Err(error) => return storage_failure(&error),
    };
    let date = request
        .param("localDate")
        .unwrap_or_else(crate::collections::scheduler::current_utc_date);
    let include_unreleased = services
        .collections
        .account(&account)
        .franchises
        .include_unreleased;
    let mut franchises = visible_franchises(&snapshots, &local, include_unreleased, &date);
    if services.session.user_restricted() {
        for franchise in &mut franchises {
            franchise.missing.clear();
        }
        franchises.retain(|franchise| franchise.owned.len() >= 2);
    }
    ApiResponse::ok(json!({
        "status": status,
        "franchises": franchises.into_iter().map(|franchise| json!({
            "collectionId": franchise.collection_id,
            "name": franchise.name,
            "posterPath": franchise.poster_path,
            "backdropPath": franchise.backdrop_path,
            "owned": franchise.owned,
            "missing": franchise.missing,
            "items": [],
            "ownershipAvailable": true,
        })).collect::<Vec<_>>()
    }))
}

pub(super) fn franchise_detail(
    services: &Arc<Services>,
    collection_id: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let id = match collection_id.parse::<u64>().ok().filter(|id| *id > 0) {
        Some(id) => id,
        None => return ApiResponse::error(400, "that is not a TMDB collection id"),
    };
    let response = franchises(services, request);
    if response.status != 200 {
        return response;
    }
    let value: Value = serde_json::from_slice(&response.body).unwrap_or_default();
    match value["franchises"].as_array().and_then(|rows| {
        rows.iter()
            .find(|row| row["collectionId"].as_u64() == Some(id))
    }) {
        Some(franchise) => ApiResponse::ok(franchise.clone()),
        None => ApiResponse::error(404, "that movie franchise is not visible"),
    }
}

pub(super) fn movie_franchise(services: &Arc<Services>, tmdb_id: &str) -> ApiResponse {
    let tmdb_id = match tmdb_id.parse::<u64>().ok().filter(|id| *id > 0) {
        Some(id) => id,
        None => return ApiResponse::error(400, "that is not a TMDB movie id"),
    };
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    let repository = SnapshotRepository::new(&services.library);
    let collection = repository
        .franchises(&account)
        .unwrap_or_default()
        .into_iter()
        .find(|franchise| {
            franchise
                .items
                .iter()
                .any(|item| item.identity.tmdb_id == tmdb_id)
        });
    ApiResponse::ok(json!({
        "tmdbId": tmdb_id,
        "collection": collection.map(|item| json!({
            "id": item.collection_id,
            "name": item.name,
        })),
    }))
}

pub(super) fn title(services: &Arc<Services>, media_type: &str, tmdb_id: &str) -> ApiResponse {
    let account = match active_account(services) {
        Ok(account) => account,
        Err(response) => return response,
    };
    if services.session.user_restricted() {
        return ApiResponse::error(404, "that collection title is not available");
    }
    let media_type = match media_type {
        "movie" => crate::collections::MediaType::Movie,
        "series" | "tv" => crate::collections::MediaType::Series,
        _ => return ApiResponse::error(400, "that is not a collection media type"),
    };
    let tmdb_id = match tmdb_id.parse::<u64>().ok().filter(|id| *id > 0) {
        Some(id) => id,
        None => return ApiResponse::error(400, "that is not a TMDB title id"),
    };
    match SnapshotRepository::new(&services.library).title(&account, media_type, tmdb_id) {
        Ok(Some(item)) => ApiResponse::ok(json!({ "item": item })),
        Ok(None) => ApiResponse::error(404, "that collection title is not available"),
        Err(error) => storage_failure(&error),
    }
}
