use crate::app::threads::spawn_named;
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::json;

use crate::app::services::{Services, ShellRequest};
use crate::preferences::AccountKey;

use super::franchises::{FranchiseMembership, FranchiseSnapshot};
use super::matching::{
    ResolvedIdentityMapping, owned_tmdb_ids, provider_request, save_identity_mappings,
    unresolved_secondary_identities,
};
use super::snapshots::{RefreshState, SnapshotRepository};
use super::{CollectionMode, CollectionProfile, CollectionSnapshot, MediaType, RefreshCadence};

const SCHEDULER_INTERVAL: Duration = Duration::from_secs(60 * 60);
const FRANCHISE_MEMBERSHIP_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
pub(crate) const FAILURE_BACKOFF_SECONDS: i64 = 5 * 60;
static FRANCHISE_FLIGHTS: OnceLock<Mutex<HashSet<AccountKey>>> = OnceLock::new();
static PROFILE_FLIGHTS: OnceLock<Mutex<HashSet<(AccountKey, String)>>> = OnceLock::new();

struct FlightGuard<K: Eq + Hash + 'static> {
    set: &'static OnceLock<Mutex<HashSet<K>>>,
    key: K,
}

impl<K: Eq + Hash + Clone + 'static> FlightGuard<K> {
    fn acquire(set: &'static OnceLock<Mutex<HashSet<K>>>, key: K) -> Option<Self> {
        let mut flights = set
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        flights.insert(key.clone()).then(|| Self { set, key })
    }
}

impl<K: Eq + Hash + 'static> Drop for FlightGuard<K> {
    fn drop(&mut self) {
        let mut flights = self
            .set
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        flights.remove(&self.key);
    }
}

pub fn start(services: Arc<Services>) {
    spawn_named("collections-scheduler", move || {
        loop {
            refresh_due_work(&services);
            std::thread::sleep(SCHEDULER_INTERVAL);
        }
    });
}

pub fn request_run(services: Arc<Services>) {
    spawn_named("collections-refresh", move || refresh_due_work(&services));
}

pub fn request_after_library_sync(services: Arc<Services>) {
    spawn_named("collections-after-sync", move || {
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

pub(crate) fn refresh_due_work(services: &Arc<Services>) {
    let Some(account) = active_mediaflick_account(services) else {
        return;
    };
    let mut changed = refresh_franchises(services, &account);
    changed |= refresh_due_profiles_for_account(services, &account);
    if changed {
        let _ = services.shell.request(ShellRequest::CollectionsChanged);
    }
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
        let state = match repository.refresh_state(account, &profile.id) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(
                    target: "collections",
                    profile_id = profile.id,
                    "could not read the collection refresh state; skipping it: {error}"
                );
                continue;
            }
        };
        if !is_due(&state, profile.cadence, now) || in_failure_backoff(&state, now) {
            continue;
        }
        changed |= refresh_profile(services, &repository, account, &profile, now);
    }
    changed
}

pub fn refresh_franchises(services: &Services, account: &AccountKey) -> bool {
    let Some(_flight) = FlightGuard::acquire(&FRANCHISE_FLIGHTS, account.clone()) else {
        return false;
    };
    let scope = match services.session.scope() {
        Ok(scope) if scope.account() == account => scope,
        _ => return false,
    };
    let attempted_at = crate::library::now_unix();
    let repository = SnapshotRepository::new(&services.library);
    let refresh = match repository.franchise_refresh_state(account) {
        Ok(refresh) => refresh,
        Err(error) => {
            tracing::warn!(
                target: "collections",
                "could not read the movie franchise refresh state: {error}"
            );
            return false;
        }
    };
    if in_failure_backoff(&refresh, attempted_at) {
        return false;
    }
    hydrate_identity_map(services);
    if !services.session.scope_is_current(&scope) {
        return false;
    }
    let ids = match owned_tmdb_ids(&services.library, MediaType::Movie) {
        Ok(ids) => ids,
        Err(error) => {
            tracing::warn!(target: "collections", "could not read the owned movies for franchises: {error}");
            return false;
        }
    };
    let plan = match repository.franchise_resolution_plan(
        account,
        &ids,
        attempted_at.saturating_sub(FRANCHISE_MEMBERSHIP_TTL_SECONDS),
    ) {
        Ok(plan) => plan,
        Err(error) => {
            tracing::warn!(target: "collections", "could not plan movie franchise refresh: {error}");
            return false;
        }
    };
    if !ids.is_empty() && plan.stale_movie_ids.is_empty() {
        return false;
    }
    let (snapshots, memberships) = if ids.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        let Some(resolved) = fetch_franchise_resolution(services, &scope, &plan) else {
            return save_failed_franchise_refresh(
                services,
                &scope,
                &repository,
                account,
                attempted_at,
            );
        };
        resolved
    };
    commit_franchise_refresh(
        services,
        &scope,
        &repository,
        account,
        &snapshots,
        &memberships,
        attempted_at,
    )
}

fn fetch_franchise_resolution(
    services: &Services,
    scope: &crate::jellyfin::session::SessionScope,
    plan: &super::snapshots::FranchiseResolutionPlan,
) -> Option<(Vec<FranchiseSnapshot>, Vec<FranchiseMembership>)> {
    let value = match services.companion.resolve_franchises(
        scope,
        &plan.stale_movie_ids,
        &plan.known_collection_ids,
    ) {
        Ok(value) => value,
        Err(error) => {
            services.session.note_scoped_error(scope, &error);
            tracing::debug!(
                target: "collections",
                "automatic movie franchise refresh failed: {error}"
            );
            return None;
        }
    };
    let Some(rows) = value.get("franchises").cloned() else {
        tracing::warn!(target: "collections", "movie franchise response has no results");
        return None;
    };
    let memberships = value
        .get("memberships")
        .cloned()
        .and_then(|rows| serde_json::from_value::<Vec<FranchiseMembership>>(rows).ok())
        .unwrap_or_default();
    let expected = plan.stale_movie_ids.iter().copied().collect::<HashSet<_>>();
    let resolved = memberships
        .iter()
        .map(|membership| membership.tmdb_id)
        .collect::<HashSet<_>>();
    if resolved != expected || memberships.len() != expected.len() {
        tracing::warn!(target: "collections", "movie franchise response has incomplete membership results");
        return None;
    }
    match serde_json::from_value::<Vec<FranchiseSnapshot>>(rows) {
        Ok(snapshots) => Some((snapshots, memberships)),
        Err(error) => {
            tracing::warn!(
                target: "collections",
                "could not read movie franchise results: {error}"
            );
            None
        }
    }
}

fn commit_franchise_refresh(
    services: &Services,
    scope: &crate::jellyfin::session::SessionScope,
    repository: &SnapshotRepository<'_>,
    account: &AccountKey,
    snapshots: &[FranchiseSnapshot],
    memberships: &[FranchiseMembership],
    attempted_at: i64,
) -> bool {
    services
        .session
        .commit_if_current(scope, || false, |_live| {
            Ok(match repository.commit_franchise_resolution(
                account,
                snapshots,
                memberships,
                attempted_at,
            ) {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(target: "collections", "could not save movie franchises: {error}");
                    false
                }
            })
        })
        .unwrap_or(false)
}

fn save_failed_franchise_refresh(
    services: &Services,
    scope: &crate::jellyfin::session::SessionScope,
    repository: &SnapshotRepository<'_>,
    account: &AccountKey,
    attempted_at: i64,
) -> bool {
    services
        .session
        .commit_if_current(
            scope,
            || false,
            |_live| match repository.save_franchise_refresh_failure(account, attempted_at) {
                Ok(changed) => Ok(changed),
                Err(error) => {
                    tracing::warn!(
                        target: "collections",
                        "could not save movie franchise refresh failure: {error}"
                    );
                    Ok(false)
                }
            },
        )
        .unwrap_or(false)
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
    let Ok(scope) = services.session.scope() else {
        return;
    };
    let Ok(value) = services
        .companion
        .resolve_collection_identities(&scope, &json!(requests))
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
    let Some(_flight) =
        FlightGuard::acquire(&PROFILE_FLIGHTS, (account.clone(), profile.id.clone()))
    else {
        return false;
    };
    let scope = match services.session.scope() {
        Ok(scope) if scope.account() == account => scope,
        _ => return false,
    };
    let request = match provider_request(&services.library, profile) {
        Ok(request) => request,
        Err(error) => {
            tracing::warn!(
                target: "collections",
                profile_id = profile.id,
                "could not build the automatic collection refresh: {error}"
            );
            return false;
        }
    };
    let result = match services.companion.refresh_collection(&scope, &request) {
        Ok(result) => result,
        Err(error) => {
            tracing::debug!(
                target: "collections",
                profile_id = profile.id,
                "automatic collection refresh failed: {error}"
            );
            return services
                .session
                .commit_if_current(
                    &scope,
                    || false,
                    |_live| {
                        if !profile_is_current(services, account, profile) {
                            return Ok(false);
                        }
                        Ok(save_failed_refresh(
                            repository,
                            account,
                            &profile.id,
                            attempt,
                        ))
                    },
                )
                .unwrap_or(false);
        }
    };
    let snapshot = CollectionSnapshot {
        profile_id: profile.id.clone(),
        revision: profile.revision.clone(),
        committed_at: attempt,
        items: result.items,
    };
    commit_profile_refresh(
        services, &scope, repository, account, profile, &snapshot, attempt,
    )
}

fn commit_profile_refresh(
    services: &Services,
    scope: &crate::jellyfin::session::SessionScope,
    repository: &SnapshotRepository<'_>,
    account: &AccountKey,
    profile: &CollectionProfile,
    snapshot: &CollectionSnapshot,
    attempt: i64,
) -> bool {
    let state = RefreshState {
        last_attempt: Some(attempt),
        last_success: Some(attempt),
        latest_failure: None,
        next_due: next_due_at(attempt, profile.cadence),
        initialized: true,
    };
    enum ProfileCommit {
        Skipped,
        Committed {
            state_error: Option<rusqlite::Error>,
        },
        Failed {
            error: rusqlite::Error,
            changed: bool,
        },
    }
    let outcome = match services.session.commit_if_current(
        scope,
        || ProfileCommit::Skipped,
        |_live| {
            if !profile_is_current(services, account, profile) {
                return Ok(ProfileCommit::Skipped);
            }
            if let Err(error) = repository.commit_profile(account, snapshot) {
                let changed = save_failed_refresh(repository, account, &profile.id, attempt);
                return Ok(ProfileCommit::Failed { error, changed });
            }
            let state_error = repository
                .save_refresh_state(account, &profile.id, &state)
                .err();
            Ok(ProfileCommit::Committed { state_error })
        },
    ) {
        Ok(outcome) | Err(outcome) => outcome,
    };
    match outcome {
        ProfileCommit::Skipped => false,
        ProfileCommit::Failed { error, changed } => {
            tracing::warn!(
                target: "collections",
                profile_id = profile.id,
                "could not save automatic collection refresh: {error}"
            );
            changed
        }
        ProfileCommit::Committed { state_error } => {
            if let Some(error) = state_error {
                tracing::warn!(
                    target: "collections",
                    profile_id = profile.id,
                    "could not save automatic collection refresh state: {error}"
                );
            }
            true
        }
    }
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

pub(crate) fn save_failed_refresh(
    repository: &SnapshotRepository<'_>,
    account: &AccountKey,
    profile_id: &str,
    attempt: i64,
) -> bool {
    const FAILURE: &str = "Results unavailable";
    let recorded = repository
        .refresh_state(account, profile_id)
        .and_then(|mut state| {
            let changed = state.latest_failure.as_deref() != Some(FAILURE);
            state.last_attempt = Some(attempt);
            state.latest_failure = Some(FAILURE.to_string());
            repository.save_refresh_state(account, profile_id, &state)?;
            Ok(changed)
        });
    match recorded {
        Ok(changed) => changed,
        Err(error) => {
            tracing::warn!(
                target: "collections",
                profile_id,
                "could not record the failed collection refresh: {error}"
            );
            false
        }
    }
}

pub(crate) fn in_failure_backoff(state: &RefreshState, now: i64) -> bool {
    state.latest_failure.is_some()
        && state.last_attempt.is_some_and(|attempt| {
            attempt <= now && now.saturating_sub(attempt) < FAILURE_BACKOFF_SECONDS
        })
}

pub fn next_due_at(last_success: i64, cadence: RefreshCadence) -> Option<i64> {
    match cadence {
        RefreshCadence::Manual => None,
        RefreshCadence::Daily => Some(last_success.saturating_add(86_400)),
        RefreshCadence::Weekly => Some(last_success.saturating_add(7 * 86_400)),
        RefreshCadence::Monthly => Some(next_month(last_success)),
    }
}

pub fn is_due(state: &RefreshState, cadence: RefreshCadence, now: i64) -> bool {
    if !state.initialized {
        return true;
    }
    match state.last_success {
        None => cadence != RefreshCadence::Manual,
        Some(last_success) => next_due_at(last_success, cadence).is_some_and(|due| due <= now),
    }
}

pub fn current_utc_date() -> String {
    let timestamp = crate::library::now_unix();
    let (year, month, day) = crate::library::civil_from_days(timestamp.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

fn next_month(timestamp: i64) -> i64 {
    let days = timestamp.div_euclid(86_400);
    let seconds = timestamp.rem_euclid(86_400);
    let (year, month, day) = crate::library::civil_from_days(days);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monthly_cadence_clamps_to_the_last_day() {
        assert_eq!(
            next_due_at(1_738_285_200, RefreshCadence::Monthly),
            Some(1_740_704_400)
        );
        assert_eq!(
            next_due_at(1_706_662_800, RefreshCadence::Monthly),
            Some(1_709_168_400)
        );
    }

    #[test]
    fn manual_profiles_are_due_until_initialized_and_never_due_after() {
        assert!(is_due(&RefreshState::default(), RefreshCadence::Manual, 0));
        let initialized = RefreshState {
            last_attempt: Some(100),
            last_success: Some(100),
            latest_failure: None,
            next_due: None,
            initialized: true,
        };
        assert!(!is_due(&initialized, RefreshCadence::Manual, i64::MAX));
        assert_eq!(next_due_at(0, RefreshCadence::Manual), None);
    }

    #[test]
    fn flight_guard_isolates_accounts_and_releases_on_drop_and_panic() {
        static TEST_FLIGHTS: OnceLock<Mutex<HashSet<(AccountKey, String)>>> = OnceLock::new();
        let account_a = AccountKey::new("server", "user-a").expect("account a");
        let account_b = AccountKey::new("server", "user-b").expect("account b");

        let guard_a = FlightGuard::acquire(&TEST_FLIGHTS, (account_a.clone(), "p1".to_string()));
        assert!(guard_a.is_some());
        assert!(
            FlightGuard::acquire(&TEST_FLIGHTS, (account_a.clone(), "p1".to_string())).is_none()
        );
        let guard_other_profile =
            FlightGuard::acquire(&TEST_FLIGHTS, (account_a.clone(), "p2".to_string()));
        assert!(guard_other_profile.is_some());
        let guard_other_account =
            FlightGuard::acquire(&TEST_FLIGHTS, (account_b.clone(), "p1".to_string()));
        assert!(guard_other_account.is_some());

        drop(guard_a);
        assert!(FlightGuard::acquire(&TEST_FLIGHTS, (account_a, "p1".to_string())).is_some());

        let panicked = std::panic::catch_unwind(|| {
            let _guard = FlightGuard::acquire(&TEST_FLIGHTS, (account_b.clone(), "p3".to_string()))
                .expect("acquire before panic");
            panic!("simulated refresh panic");
        });
        assert!(panicked.is_err());
        assert!(FlightGuard::acquire(&TEST_FLIGHTS, (account_b, "p3".to_string())).is_some());
    }

    #[test]
    fn failure_backoff_respects_boundaries_and_clears_without_attempt_or_failure() {
        let base = 10_000;
        let failed = RefreshState {
            last_attempt: Some(base),
            last_success: None,
            latest_failure: Some("Results unavailable".to_string()),
            next_due: None,
            initialized: false,
        };
        assert!(in_failure_backoff(&failed, base));
        assert!(in_failure_backoff(
            &failed,
            base + FAILURE_BACKOFF_SECONDS - 1
        ));
        assert!(!in_failure_backoff(&failed, base + FAILURE_BACKOFF_SECONDS));
        assert!(!in_failure_backoff(&failed, base - 1));

        let cleared = RefreshState {
            last_attempt: None,
            ..failed.clone()
        };
        assert!(!in_failure_backoff(&cleared, base));

        let succeeded = RefreshState {
            latest_failure: None,
            ..failed
        };
        assert!(!in_failure_backoff(&succeeded, base));
    }
}
