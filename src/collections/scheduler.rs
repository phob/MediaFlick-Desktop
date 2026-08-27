use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use crate::app::services::{Services, ShellRequest};
use crate::preferences::AccountKey;

use super::franchises::FranchiseSnapshot;
use super::matching::{
    ResolvedIdentityMapping, owned_tmdb_ids, save_identity_mappings,
    unresolved_secondary_identities,
};
use super::snapshots::{RefreshState, SnapshotRepository};
use super::{CollectionMode, CollectionProfile, CollectionSnapshot, MediaType, RefreshCadence};

const SCHEDULER_INTERVAL: Duration = Duration::from_secs(60 * 60);

pub fn start(services: Arc<Services>) {
    std::thread::spawn(move || {
        loop {
            refresh_due_work(&services);
            std::thread::sleep(SCHEDULER_INTERVAL);
        }
    });
}

pub fn request_run(services: Arc<Services>) {
    std::thread::spawn(move || refresh_due_work(&services));
}

/// A complete local-library update can change franchise membership and title
/// ownership without changing a provider snapshot.
pub fn request_after_library_sync(services: Arc<Services>) {
    std::thread::spawn(move || {
        let Some(account) = active_mediaflick_account(&services) else {
            return;
        };
        let mut changed = refresh_franchises(&services, &account);
        changed |= refresh_due_profiles_for_account(&services, &account);
        if changed {
            let _ = services.shell.request(ShellRequest::CollectionsChanged);
        }
    });
}

fn refresh_due_work(services: &Arc<Services>) {
    let Some(account) = active_mediaflick_account(services) else {
        return;
    };
    let repository = SnapshotRepository::new(&services.library);
    let franchise_state = repository
        .franchise_refresh_state(&account)
        .unwrap_or_default();
    let mut changed =
        franchise_refresh_due(&franchise_state) && refresh_franchises(services, &account);
    changed |= refresh_due_profiles_for_account(services, &account);
    if changed {
        let _ = services.shell.request(ShellRequest::CollectionsChanged);
    }
}

fn franchise_refresh_due(state: &RefreshState) -> bool {
    !state.initialized
}

fn active_mediaflick_account(services: &Services) -> Option<AccountKey> {
    let account = services.session.account_key()?;
    let ownership_available = crate::library::sync::ownership_available(&services.library);
    let repository = SnapshotRepository::new(&services.library);
    let has_results = repository.has_account_results(&account).unwrap_or(false);
    let readiness = services.companion.collection_readiness(false);
    let mode = services
        .collections
        .effective_mode(&account, &readiness, has_results);
    automatic_work_allowed(mode, ownership_available).then_some(account)
}

fn automatic_work_allowed(mode: CollectionMode, ownership_available: bool) -> bool {
    mode == CollectionMode::MediaFlick && ownership_available
}

fn refresh_due_profiles_for_account(services: &Services, account: &AccountKey) -> bool {
    let now = crate::library::now_unix();
    let repository = SnapshotRepository::new(&services.library);
    let profiles = services.collections.account(account).profiles;
    let mut changed = false;
    for profile in profiles {
        if profile.validate().is_err() {
            continue;
        }
        let state = repository
            .refresh_state(account, &profile.id)
            .unwrap_or_default();
        if !is_due(state.last_success, profile.cadence, now) {
            continue;
        }
        changed |= refresh_profile(services, &repository, account, &profile, now);
    }
    changed
}

pub fn refresh_franchises(services: &Services, account: &AccountKey) -> bool {
    let attempted_at = crate::library::now_unix();
    let repository = SnapshotRepository::new(&services.library);
    hydrate_identity_map(services);
    let ids = owned_tmdb_ids(&services.library, MediaType::Movie).unwrap_or_default();
    let snapshots = if ids.is_empty() {
        Vec::new()
    } else {
        let value = match services.companion.resolve_franchises(&ids) {
            Ok(value) => value,
            Err(error) => {
                save_failed_franchise_refresh(&repository, account, attempted_at);
                tracing::debug!(
                    target: "collections",
                    "automatic movie franchise refresh failed: {error}"
                );
                return true;
            }
        };
        let Some(rows) = value.get("franchises").cloned() else {
            save_failed_franchise_refresh(&repository, account, attempted_at);
            tracing::warn!(target: "collections", "movie franchise response has no results");
            return true;
        };
        match serde_json::from_value::<Vec<FranchiseSnapshot>>(rows) {
            Ok(snapshots) => snapshots,
            Err(error) => {
                save_failed_franchise_refresh(&repository, account, attempted_at);
                tracing::warn!(
                    target: "collections",
                    "could not read movie franchise results: {error}"
                );
                return true;
            }
        }
    };
    match repository.commit_franchises(account, &snapshots, attempted_at) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(target: "collections", "could not save movie franchises: {error}");
            false
        }
    }
}

fn save_failed_franchise_refresh(
    repository: &SnapshotRepository<'_>,
    account: &AccountKey,
    attempted_at: i64,
) {
    if let Err(error) = repository.save_franchise_refresh_failure(account, attempted_at) {
        tracing::warn!(
            target: "collections",
            "could not save movie franchise refresh failure: {error}"
        );
    }
}

pub fn hydrate_identity_map(services: &Services) {
    let requests = match unresolved_secondary_identities(&services.library) {
        Ok(requests) => requests,
        Err(error) => {
            tracing::warn!(target: "collections", "could not inspect secondary provider identities: {error}");
            return;
        }
    };
    if requests.is_empty() {
        return;
    }
    let Ok(value) = services
        .companion
        .resolve_collection_identities(&json!(requests))
    else {
        return;
    };
    let mappings = value
        .get("mappings")
        .cloned()
        .and_then(|rows| serde_json::from_value::<Vec<ResolvedIdentityMapping>>(rows).ok())
        .unwrap_or_default();
    if let Err(error) = save_identity_mappings(&services.library, &mappings) {
        tracing::warn!(target: "collections", "could not save secondary provider identities: {error}");
    }
}

fn refresh_profile(
    services: &Services,
    repository: &SnapshotRepository<'_>,
    account: &AccountKey,
    profile: &CollectionProfile,
    attempt: i64,
) -> bool {
    let owned_tmdb_ids = matches!(
        profile.source,
        super::CollectionSource::TmdbCollection { .. }
    )
    .then(|| {
        super::matching::owned_tmdb_ids(&services.library, super::MediaType::Movie)
            .unwrap_or_default()
    })
    .unwrap_or_default();
    let result = match services.companion.refresh_collection(&json!({
        "source": profile.source,
        "mediaType": profile.media_type,
        "limit": profile.limit,
        "ordering": profile.ordering,
        "ownedTmdbIds": owned_tmdb_ids,
    })) {
        Ok(result) => result,
        Err(error) => {
            save_failed_refresh(repository, account, &profile.id, attempt);
            tracing::debug!(
                target: "collections",
                profile_id = profile.id,
                "automatic collection refresh failed: {error}"
            );
            return true;
        }
    };
    if !profile_is_current(services, account, profile) {
        return false;
    }
    let snapshot = CollectionSnapshot {
        profile_id: profile.id.clone(),
        revision: profile.revision.clone(),
        committed_at: attempt,
        items: result.items,
    };
    if let Err(error) = repository.commit_profile(account, &snapshot) {
        save_failed_refresh(repository, account, &profile.id, attempt);
        tracing::warn!(
            target: "collections",
            profile_id = profile.id,
            "could not save automatic collection refresh: {error}"
        );
        return true;
    }
    let state = RefreshState {
        last_attempt: Some(attempt),
        last_success: Some(attempt),
        latest_failure: None,
        next_due: next_due_at(attempt, profile.cadence),
        initialized: true,
    };
    if let Err(error) = repository.save_refresh_state(account, &profile.id, &state) {
        tracing::warn!(
            target: "collections",
            profile_id = profile.id,
            "could not save automatic collection refresh state: {error}"
        );
    }
    true
}

fn profile_is_current(
    services: &Services,
    account: &AccountKey,
    expected: &CollectionProfile,
) -> bool {
    services
        .collections
        .account(account)
        .profiles
        .into_iter()
        .any(|profile| profile.id == expected.id && profile.revision == expected.revision)
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

pub fn next_due_at(last_success: i64, cadence: RefreshCadence) -> Option<i64> {
    match cadence {
        RefreshCadence::Manual => None,
        RefreshCadence::Daily => Some(last_success.saturating_add(86_400)),
        RefreshCadence::Weekly => Some(last_success.saturating_add(7 * 86_400)),
        RefreshCadence::Monthly => Some(next_month(last_success)),
    }
}

pub fn is_due(last_success: Option<i64>, cadence: RefreshCadence, now: i64) -> bool {
    match last_success {
        None => cadence != RefreshCadence::Manual,
        Some(last_success) => next_due_at(last_success, cadence).is_some_and(|due| due <= now),
    }
}

pub fn current_utc_date() -> String {
    let timestamp = crate::library::now_unix();
    let (year, month, day) = civil_from_days(timestamp.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

fn next_month(timestamp: i64) -> i64 {
    let days = timestamp.div_euclid(86_400);
    let seconds = timestamp.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let day = day.min(days_in_month(next_year, next_month));
    days_from_civil(next_year, next_month, day)
        .saturating_mul(86_400)
        .saturating_add(seconds)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.rem_euclid(4) == 0
            && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0) =>
        {
            29
        }
        2 => 28,
        _ => 30,
    }
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monthly_cadence_clamps_to_the_last_day() {
        let january_31 = days_from_civil(2025, 1, 31) * 86_400 + 3_600;
        let february_28 = days_from_civil(2025, 2, 28) * 86_400 + 3_600;
        assert_eq!(
            next_due_at(january_31, RefreshCadence::Monthly),
            Some(february_28)
        );
    }

    #[test]
    fn manual_profiles_never_become_due() {
        assert!(!is_due(None, RefreshCadence::Manual, i64::MAX));
        assert_eq!(next_due_at(0, RefreshCadence::Manual), None);
    }

    #[test]
    fn automatic_work_requires_mediaflick_mode_and_complete_ownership() {
        assert!(automatic_work_allowed(CollectionMode::MediaFlick, true));
        assert!(!automatic_work_allowed(CollectionMode::MediaFlick, false));
        assert!(!automatic_work_allowed(CollectionMode::Jellyfin, true));
    }

    #[test]
    fn a_persisted_franchise_result_is_not_due_at_startup() {
        assert!(franchise_refresh_due(&RefreshState::default()));
        assert!(!franchise_refresh_due(&RefreshState {
            initialized: true,
            last_success: Some(100),
            ..RefreshState::default()
        }));
    }
}
