use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use crate::jellyfin::session::{Session, SessionScope};
use crate::library::RatingTarget;
use crate::preferences::AccountKey;

use super::RatingsError;

const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct InFlightKey {
    generation: u64,
    account: AccountKey,
    target: String,
}

impl InFlightKey {
    fn new(scope: &SessionScope, target: &RatingTarget) -> Self {
        Self {
            generation: scope.generation(),
            account: scope.account().clone(),
            target: format!(
                "{}:{}:{}",
                target.provider, target.media_type, target.provider_id
            ),
        }
    }
}

enum FlightState {
    Running,
    Finished { diagnostic: Option<String> },
}

enum WaitOutcome {
    Pending,
    Finished(Option<String>),
}

pub(super) struct Flight {
    id: u64,
    state: Mutex<FlightState>,
    finished: Condvar,
}

impl Flight {
    fn new(id: u64) -> Self {
        Self {
            id,
            state: Mutex::new(FlightState::Running),
            finished: Condvar::new(),
        }
    }

    fn complete(&self, diagnostic: Option<String>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        *state = FlightState::Finished { diagnostic };
        drop(state);
        self.finished.notify_all();
    }

    fn wait_step(&self) -> WaitOutcome {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let FlightState::Finished { diagnostic } = &*state {
            let diagnostic = diagnostic.clone();
            drop(state);
            return WaitOutcome::Finished(diagnostic);
        }
        state = self
            .finished
            .wait_timeout(state, WAIT_POLL_INTERVAL)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
        let outcome = match &*state {
            FlightState::Finished { diagnostic } => WaitOutcome::Finished(diagnostic.clone()),
            FlightState::Running => WaitOutcome::Pending,
        };
        drop(state);
        outcome
    }

    fn wait(
        &self,
        session: &Session,
        scope: &SessionScope,
        cancelled: Option<&AtomicBool>,
    ) -> Result<Option<String>, RatingsError> {
        loop {
            if is_cancelled(cancelled) {
                return Err(RatingsError::Cancelled);
            }
            if !session.scope_is_current(scope) {
                return Err(RatingsError::AccountChanged);
            }
            if let WaitOutcome::Finished(diagnostic) = self.wait_step() {
                return Ok(diagnostic);
            }
        }
    }
}

#[derive(Default)]
struct InFlightMap {
    next_id: u64,
    entries: HashMap<InFlightKey, Arc<Flight>>,
}

#[derive(Default)]
pub(super) struct InFlightCoordinator {
    active: Mutex<InFlightMap>,
}

pub(super) struct Claim<'a> {
    pub(super) owned: Vec<RatingTarget>,
    pub(super) guard: Option<FlightGuard<'a>>,
    pub(super) awaited: Vec<Arc<Flight>>,
}

pub(super) struct FlightGuard<'a> {
    coordinator: &'a InFlightCoordinator,
    keys: Vec<InFlightKey>,
    flight: Arc<Flight>,
    finished: bool,
}

impl FlightGuard<'_> {
    pub(super) fn finish(mut self, diagnostic: Option<String>) {
        self.release(diagnostic);
        self.finished = true;
    }

    fn release(&mut self, diagnostic: Option<String>) {
        let mut active = self
            .coordinator
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for key in &self.keys {
            if active
                .entries
                .get(key)
                .is_some_and(|entry| entry.id == self.flight.id)
            {
                active.entries.remove(key);
            }
        }
        drop(active);
        self.flight.complete(diagnostic);
    }
}

impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.release(None);
            self.finished = true;
        }
    }
}

impl InFlightCoordinator {
    pub(super) fn claim(&self, scope: &SessionScope, targets: &[RatingTarget]) -> Claim<'_> {
        if targets.is_empty() {
            return Claim {
                owned: Vec::new(),
                guard: None,
                awaited: Vec::new(),
            };
        }
        let mut owned = Vec::new();
        let mut owned_keys = Vec::new();
        let mut awaited = Vec::<Arc<Flight>>::new();
        let mut mine: Option<Arc<Flight>> = None;

        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        for target in targets {
            let key = InFlightKey::new(scope, target);
            if let Some(existing) = active.entries.get(&key) {
                let is_mine = mine.as_ref().is_some_and(|flight| flight.id == existing.id);
                if !is_mine && !awaited.iter().any(|flight| flight.id == existing.id) {
                    awaited.push(Arc::clone(existing));
                }
            } else {
                let flight = mine.get_or_insert_with(|| {
                    active.next_id = active.next_id.wrapping_add(1);
                    Arc::new(Flight::new(active.next_id))
                });
                active.entries.insert(key.clone(), Arc::clone(flight));
                owned.push(target.clone());
                owned_keys.push(key);
            }
        }
        drop(active);

        let guard = mine.map(|flight| FlightGuard {
            coordinator: self,
            keys: owned_keys,
            flight,
            finished: false,
        });
        Claim {
            owned,
            guard,
            awaited,
        }
    }
}

pub(super) fn wait_for_flights(
    session: &Session,
    scope: &SessionScope,
    awaited: &[Arc<Flight>],
    cancelled: Option<&AtomicBool>,
) -> Result<Option<String>, RatingsError> {
    let mut diagnostic = None;
    for flight in awaited {
        let flight_diagnostic = flight.wait(session, scope, cancelled)?;
        if diagnostic.is_none() {
            diagnostic = flight_diagnostic;
        }
    }
    Ok(diagnostic)
}

pub(super) fn is_cancelled(cancelled: Option<&AtomicBool>) -> bool {
    cancelled.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::mpsc;
    use std::thread;

    use crate::app::services::test_support::TestServices;

    use super::*;

    fn target(id: &str) -> RatingTarget {
        RatingTarget {
            item_id: format!("tmdb:movie:{id}"),
            kind: "Movie".to_string(),
            media_type: "movie".to_string(),
            provider: "tmdb".to_string(),
            provider_id: id.to_string(),
        }
    }

    #[test]
    fn leader_panic_releases_inflight_entry_and_wakes_waiter() {
        let fixture = TestServices::signed_in("http://127.0.0.1:1");
        let scope = fixture.services.session.scope().expect("scope");
        let coordinator = Arc::new(InFlightCoordinator::default());
        let items = vec![target("603")];

        let (claimed_tx, claimed_rx) = mpsc::channel();
        let (panic_tx, panic_rx) = mpsc::channel();
        let leader_coordinator = Arc::clone(&coordinator);
        let leader_scope = scope.clone();
        let leader_items = items.clone();
        let leader = thread::spawn(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let claim = leader_coordinator.claim(&leader_scope, &leader_items);
                assert_eq!(claim.owned.len(), 1);
                let _guard = claim.guard.expect("leader guard");
                claimed_tx.send(()).expect("signal claimed");
                panic_rx
                    .recv_timeout(Duration::from_secs(3))
                    .expect("trigger panic");
                panic!("simulated leader failure");
            }));
            assert!(outcome.is_err());
        });

        claimed_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("leader claimed");

        let waiter_claim = coordinator.claim(&scope, &items);
        assert!(waiter_claim.owned.is_empty());
        assert_eq!(waiter_claim.awaited.len(), 1);
        let awaited = waiter_claim.awaited;

        let waiter_session = Arc::clone(&fixture.services.session);
        let waiter_scope = scope.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let waiter = thread::spawn(move || {
            let result = wait_for_flights(&waiter_session, &waiter_scope, &awaited, None);
            let _ = done_tx.send(result);
        });

        panic_tx.send(()).expect("let leader panic");
        leader.join().expect("leader join");

        let waiter_result = done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("waiter woke after leader panic");
        waiter.join().expect("waiter join");
        assert!(matches!(waiter_result, Ok(None)));

        let next_claim = coordinator.claim(&scope, &items);
        assert_eq!(next_claim.owned.len(), 1);
        assert!(next_claim.awaited.is_empty());
        next_claim.guard.expect("next guard").finish(None);
    }
}
