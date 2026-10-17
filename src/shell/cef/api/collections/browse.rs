use super::*;
use crate::collections::franchises::{sort_titles_by_release_date, visible_franchises};
use crate::collections::matching::{OwnershipPolicy, classify, local_item_map};
use crate::collections::snapshots::{RefreshState, SnapshotRepository};
use crate::collections::{ClassifiedTitle, CollectionProfile, NormalizedTitle, compare_titles};

use super::profiles::CollectionProfilesIndex;

/// Whether a collection's results can be shown yet.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
enum ResultsStatus {
    Updating,
    ResultsUnavailable,
    Ready,
}

/// `/api/collections/mine/{id}`: one profile and its classified results.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct CollectionProfileDetail {
    profile: CollectionProfile,
    status: ResultsStatus,
    owned: Vec<ClassifiedTitle>,
    missing: Vec<NormalizedTitle>,
    items: Vec<NormalizedTitle>,
    library_items: Vec<ItemSummary>,
    /// Sent only once results are ready.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    ownership_available: Option<bool>,
    refresh: RefreshState,
    /// Sent only once results are ready.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    overdue: Option<bool>,
}

/// `/api/collections/franchises`: the movie franchises the account sees.
#[derive(Serialize, ts_rs::TS)]
pub(in super::super) struct FranchiseCollectionsIndex {
    status: ResultsStatus,
    franchises: Vec<FranchiseCollectionSummary>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct FranchiseCollectionSummary {
    collection_id: u64,
    name: String,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    owned_count: usize,
    missing_count: usize,
    ownership_available: bool,
}

/// `/api/collections/franchises/{id}`: one movie franchise. Without library
/// ownership its titles are in `items`; with it, in `owned` and `missing`.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct FranchiseCollection {
    collection_id: u64,
    name: String,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    owned: Vec<ClassifiedTitle>,
    missing: Vec<NormalizedTitle>,
    items: Vec<NormalizedTitle>,
    library_items: Vec<ItemSummary>,
    ownership_available: bool,
}

/// `/api/collections/movie/{tmdbId}`: the franchise a movie belongs to.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct MovieCollection {
    tmdb_id: u64,
    collection: Option<MovieFranchise>,
}

#[derive(Serialize, ts_rs::TS)]
struct MovieFranchise {
    id: u64,
    name: String,
}

/// `/api/collections/title/{mediaType}/{tmdbId}`.
#[derive(Serialize, ts_rs::TS)]
pub(in super::super) struct CollectionTitle {
    item: NormalizedTitle,
}

pub(super) fn redirect_state(services: &Arc<Services>) -> Handled {
    super::profiles::settings(services, false)
}

pub(super) fn mine(services: &Arc<Services>) -> Handled {
    let account = active_account(services)?;
    let mut index = CollectionProfilesIndex::of(services, &account);
    index
        .profiles
        .sort_by(|left, right| compare_titles(&left.title, &right.title));
    Ok(ApiResponse::ok(index))
}

pub(super) fn profile_detail(services: &Arc<Services>, profile_id: &str) -> Handled {
    let account = active_account(services)?;
    let Some(profile) = services
        .collections
        .account(&account)
        .profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
    else {
        return Err(ApiResponse::error(404, "that collection does not exist"));
    };
    let repository = SnapshotRepository::new(&services.library);
    let refresh = repository
        .refresh_state(&account, &profile.id)
        .unwrap_or_default();
    let snapshot = match repository.profile(&account, &profile.id, &profile.revision) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => {
            let unavailable = profile.validate().is_err() || refresh.latest_failure.is_some();
            return Ok(ApiResponse::ok(CollectionProfileDetail {
                profile,
                status: if unavailable {
                    ResultsStatus::ResultsUnavailable
                } else {
                    ResultsStatus::Updating
                },
                owned: Vec::new(),
                missing: Vec::new(),
                items: Vec::new(),
                library_items: Vec::new(),
                ownership_available: None,
                refresh,
                overdue: None,
            }));
        }
        Err(error) => return Err(storage_failure(&error)),
    };
    let mut classified = match classify(
        &services.library,
        &account,
        &snapshot.items,
        OwnershipPolicy {
            complete_sync: crate::library::sync::ownership_available(&services.library),
            restricted_user: services.session.user_restricted(),
        },
    ) {
        Ok(classified) => classified,
        Err(error) => return Err(storage_failure(&error)),
    };
    classified
        .owned
        .sort_by(|left, right| compare_titles(&left.title.title, &right.title.title));
    classified
        .missing
        .sort_by(|left, right| compare_titles(&left.title, &right.title));
    classified
        .items
        .sort_by(|left, right| compare_titles(&left.title, &right.title));
    let library_items = match primary_library_items(services, &classified.owned) {
        Ok(items) => items,
        Err(error) => return Err(storage_failure(&error)),
    };
    let overdue = crate::collections::scheduler::is_due(
        refresh.last_success,
        profile.cadence,
        crate::library::now_unix(),
    );
    Ok(ApiResponse::ok(CollectionProfileDetail {
        profile,
        status: ResultsStatus::Ready,
        owned: classified.owned,
        missing: classified.missing,
        items: classified.items,
        library_items,
        ownership_available: Some(classified.ownership_available),
        refresh,
        overdue: Some(overdue),
    }))
}

pub(super) fn franchises(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let account = active_account(services)?;
    let ownership_available = crate::library::sync::ownership_available(&services.library);
    let repository = SnapshotRepository::new(&services.library);
    let refresh = match repository.franchise_refresh_state(&account) {
        Ok(refresh) => refresh,
        Err(error) => return Err(storage_failure(&error)),
    };
    if ownership_available && !refresh.initialized {
        crate::collections::scheduler::request_run(services.clone());
    }
    let status = if refresh.initialized {
        ResultsStatus::Ready
    } else if refresh.latest_failure.is_some() {
        ResultsStatus::ResultsUnavailable
    } else {
        ResultsStatus::Updating
    };
    let snapshots = match repository.franchises(&account) {
        Ok(snapshots) => snapshots,
        Err(error) => return Err(storage_failure(&error)),
    };
    if !ownership_available {
        let mut snapshots = snapshots;
        snapshots.sort_by(|left, right| {
            compare_titles(&left.name, &right.name)
                .then_with(|| left.collection_id.cmp(&right.collection_id))
        });
        let franchises = snapshots
            .into_iter()
            .map(|snapshot| FranchiseCollectionSummary {
                collection_id: snapshot.collection_id,
                name: snapshot.name,
                poster_path: snapshot.poster_path,
                backdrop_path: snapshot.backdrop_path,
                owned_count: 0,
                missing_count: 0,
                ownership_available: false,
            })
            .collect();
        return Ok(ApiResponse::ok(FranchiseCollectionsIndex {
            status,
            franchises,
        }));
    }
    let provider_items = snapshots
        .iter()
        .flat_map(|snapshot| snapshot.items.iter().cloned())
        .collect::<Vec<_>>();
    let local = match local_item_map(&services.library, &provider_items) {
        Ok(local) => local,
        Err(error) => return Err(storage_failure(&error)),
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
    Ok(ApiResponse::ok(FranchiseCollectionsIndex {
        status,
        franchises: franchises
            .into_iter()
            .map(|franchise| FranchiseCollectionSummary {
                collection_id: franchise.collection_id,
                name: franchise.name,
                poster_path: franchise.poster_path,
                backdrop_path: franchise.backdrop_path,
                owned_count: franchise.owned.len(),
                missing_count: franchise.missing.len(),
                ownership_available: true,
            })
            .collect(),
    }))
}

pub(super) fn franchise_detail(
    services: &Arc<Services>,
    collection_id: &str,
    request: &ApiRequest,
) -> Handled {
    let id = match collection_id.parse::<u64>().ok().filter(|id| *id > 0) {
        Some(id) => id,
        None => return Err(ApiResponse::error(400, "that is not a TMDB collection id")),
    };
    let account = active_account(services)?;
    let repository = SnapshotRepository::new(&services.library);
    let snapshot = match repository.franchise(&account, id) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => {
            return Err(ApiResponse::error(
                404,
                "that movie franchise does not exist",
            ));
        }
        Err(error) => return Err(storage_failure(&error)),
    };
    let ownership_available = crate::library::sync::ownership_available(&services.library);
    if !ownership_available {
        let mut items = snapshot.items;
        sort_titles_by_release_date(&mut items);
        items.retain(|item| !item.adult);
        if services.session.user_restricted() {
            items.clear();
        }
        return Ok(ApiResponse::ok(FranchiseCollection {
            collection_id: snapshot.collection_id,
            name: snapshot.name,
            poster_path: snapshot.poster_path,
            backdrop_path: snapshot.backdrop_path,
            owned: Vec::new(),
            missing: Vec::new(),
            items,
            library_items: Vec::new(),
            ownership_available: false,
        }));
    }
    let local = match local_item_map(&services.library, &snapshot.items) {
        Ok(local) => local,
        Err(error) => return Err(storage_failure(&error)),
    };
    let date = request
        .param("localDate")
        .unwrap_or_else(crate::collections::scheduler::current_utc_date);
    let include_unreleased = services
        .collections
        .account(&account)
        .franchises
        .include_unreleased;
    let Some(mut franchise) = visible_franchises(
        std::slice::from_ref(&snapshot),
        &local,
        include_unreleased,
        &date,
    )
    .pop() else {
        return Err(ApiResponse::error(
            404,
            "that movie franchise is not visible",
        ));
    };
    if services.session.user_restricted() {
        franchise.missing.clear();
        if franchise.owned.len() < 2 {
            return Err(ApiResponse::error(
                404,
                "that movie franchise is not visible",
            ));
        }
    }
    let library_items = match primary_library_items(services, &franchise.owned) {
        Ok(items) => items,
        Err(error) => return Err(storage_failure(&error)),
    };
    Ok(ApiResponse::ok(FranchiseCollection {
        collection_id: franchise.collection_id,
        name: franchise.name,
        poster_path: franchise.poster_path,
        backdrop_path: franchise.backdrop_path,
        owned: franchise.owned,
        missing: franchise.missing,
        items: Vec::new(),
        library_items,
        ownership_available: true,
    }))
}

fn primary_library_items(
    services: &Services,
    owned: &[ClassifiedTitle],
) -> rusqlite::Result<Vec<ItemSummary>> {
    let ids = owned
        .iter()
        .filter_map(|title| title.local_items.first())
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    services.library.items_by_ids(&ids)
}

pub(super) fn movie_franchise(services: &Arc<Services>, tmdb_id: &str) -> Handled {
    let tmdb_id = match tmdb_id.parse::<u64>().ok().filter(|id| *id > 0) {
        Some(id) => id,
        None => return Err(ApiResponse::error(400, "that is not a TMDB movie id")),
    };
    let account = active_account(services)?;
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
    Ok(ApiResponse::ok(MovieCollection {
        tmdb_id,
        collection: collection.map(|item| MovieFranchise {
            id: item.collection_id,
            name: item.name,
        }),
    }))
}

pub(super) fn title(services: &Arc<Services>, media_type: &str, tmdb_id: &str) -> Handled {
    let account = active_account(services)?;
    if services.session.user_restricted() {
        return Err(ApiResponse::error(
            404,
            "that collection title is not available",
        ));
    }
    let media_type = match media_type {
        "movie" => crate::collections::MediaType::Movie,
        "series" | "tv" => crate::collections::MediaType::Series,
        _ => {
            return Err(ApiResponse::error(
                400,
                "that is not a collection media type",
            ));
        }
    };
    let tmdb_id = match tmdb_id.parse::<u64>().ok().filter(|id| *id > 0) {
        Some(id) => id,
        None => return Err(ApiResponse::error(400, "that is not a TMDB title id")),
    };
    match SnapshotRepository::new(&services.library).title(&account, media_type, tmdb_id) {
        Ok(Some(item)) => Ok(ApiResponse::ok(CollectionTitle { item })),
        Ok(None) => Err(ApiResponse::error(
            404,
            "that collection title is not available",
        )),
        Err(error) => Err(storage_failure(&error)),
    }
}
