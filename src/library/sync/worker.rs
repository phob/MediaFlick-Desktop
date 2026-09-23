use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use crate::app::ids::random_hex;
use crate::app::services::ShellBridge;
use crate::jellyfin::api::ApiError;
use crate::jellyfin::session::Session;
use crate::library::Library;

use super::cycle::run_cycle_inner;
use super::{SYNC_INTERVAL, SyncHandle, Trigger};

/// Delay used while waiting for the user to sign in.
const IDLE_INTERVAL: Duration = Duration::from_secs(30);
/// The longest a warm start holds its first cycle for the window to paint.
const STARTUP_HOLD_MAX: Duration = Duration::from_secs(15);
/// How long the first cycle waits after the window paints, so Home's live
/// follow-up requests finish before the cycle's change notifications arrive.
const STARTUP_HOLD_SETTLE: Duration = Duration::from_secs(3);

pub fn spawn(library: Arc<Library>, session: Arc<Session>, shell: Arc<ShellBridge>) -> SyncHandle {
    let handle = SyncHandle::new(shell);
    let worker = handle.clone();
    if let Err(error) = thread::Builder::new()
        .name("library-sync".to_string())
        .spawn(move || run(&library, &session, &worker))
    {
        tracing::warn!(target: "library.sync", "failed to start the library sync thread: {error}");
    }
    handle
}

fn run(library: &Arc<Library>, session: &Arc<Session>, handle: &SyncHandle) {
    let mut backoff = Duration::ZERO;
    let mut normal_deadline = Instant::now();
    let mut trigger = Trigger::Scheduled;
    // A restored session with a usable catalog paints Home from SQLite. A cycle
    // started beside it would commit pages and send change notifications that
    // refetch Home while it is still loading, so the first cycle waits for the
    // window instead. A catalog that is not ready yet gates the window itself
    // and is never held.
    let mut startup_hold = (session.is_authenticated() && super::bootstrap_progress(library).ready)
        .then(|| Instant::now() + STARTUP_HOLD_MAX);
    loop {
        if !session.is_authenticated() {
            startup_hold = None;
            handle.running.store(false, Ordering::Relaxed);
            match wait(handle, IDLE_INTERVAL) {
                Wake::Stopped => return,
                // Held until a cycle can actually consume it: this is the
                // sign-in nudge arriving just before the session goes live.
                Wake::Requested => trigger = Trigger::Requested,
                Wake::Elapsed => {}
            }
            continue;
        }

        if let Some(until) = startup_hold {
            let until = startup_hold_deadline(until, handle.window_ready(), Instant::now());
            let remaining = until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                startup_hold = None;
            } else {
                startup_hold = Some(until);
                // A request made meanwhile, such as the event socket's
                // reconnect nudge, runs as this first cycle once it is due.
                match wait(handle, remaining) {
                    Wake::Stopped => return,
                    Wake::Requested => trigger = Trigger::Requested,
                    Wake::Elapsed => {}
                }
                continue;
            }
        }

        let normal_due = trigger == Trigger::Requested || Instant::now() >= normal_deadline;
        handle.running.store(normal_due, Ordering::Relaxed);

        if normal_due {
            let outcome = run_cycle_inner(library, session, trigger, Some(handle));
            trigger = Trigger::Scheduled;
            let delay = match outcome {
                Ok(report) => {
                    backoff = Duration::ZERO;
                    if report.changed() {
                        tracing::info!(
                            target: "library.sync",
                            catalogued = report.bootstrapped,
                            updated = report.updated,
                            deleted = report.deleted,
                            elapsed_ms = report.elapsed_ms,
                            "library sync cycle finished"
                        );
                    }
                    jittered(SYNC_INTERVAL)
                }
                Err(ApiError::Cancelled) if handle.is_stopped() => return,
                // A sign-out, account deletion, or account switch invalidates
                // the old generation. It is an expected handoff, not a
                // failing sync that should poison the next account's backoff.
                Err(ApiError::Cancelled) => {
                    backoff = Duration::ZERO;
                    IDLE_INTERVAL
                }
                Err(ApiError::Unauthorized) => {
                    // The cycle already reported the 401 against the account it
                    // ran for, so a late rejection of a previous account's
                    // token cannot expire the account that replaced it.
                    backoff = Duration::ZERO;
                    IDLE_INTERVAL
                }
                Err(error) => {
                    tracing::warn!(target: "library.sync", "library sync cycle failed: {error}");
                    backoff = next_backoff(
                        backoff,
                        !matches!(
                            library.meta(super::META_BOOTSTRAP_DONE),
                            Ok(Some(done)) if done == "1"
                        ),
                    );
                    let delay = retry_delay(&error, jittered(backoff));
                    handle.set_retry(&error, delay);
                    delay
                }
            };
            normal_deadline = Instant::now() + delay;
        }
        handle.running.store(false, Ordering::Relaxed);

        let delay = normal_deadline.saturating_duration_since(Instant::now());
        match wait(handle, delay) {
            Wake::Stopped => return,
            Wake::Requested => trigger = Trigger::Requested,
            Wake::Elapsed => {}
        }
    }
}

/// The held first cycle's start: the cap, or a short settle after the window
/// has painted, whichever comes first.
fn startup_hold_deadline(until: Instant, window_ready: bool, now: Instant) -> Instant {
    if window_ready {
        until.min(now + STARTUP_HOLD_SETTLE)
    } else {
        until
    }
}

fn next_backoff(previous: Duration, incomplete: bool) -> Duration {
    let (initial, maximum) = if incomplete {
        (Duration::from_secs(5), Duration::from_secs(60))
    } else {
        (SYNC_INTERVAL * 2, Duration::from_secs(30 * 60))
    };
    (previous * 2).max(initial).min(maximum)
}

fn retry_delay(error: &ApiError, fallback: Duration) -> Duration {
    error.retry_after().unwrap_or(fallback).max(fallback)
}

/// Why the sync thread stopped waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wake {
    /// Someone called [`SyncHandle::request`].
    Requested,
    /// The timeout elapsed on its own.
    Elapsed,
    /// The thread should exit.
    Stopped,
}

fn wait(handle: &SyncHandle, timeout: Duration) -> Wake {
    let Ok(mut flags) = handle.signal.flags.lock() else {
        return Wake::Stopped;
    };
    if flags.stopped {
        return Wake::Stopped;
    }
    if flags.requested {
        flags.requested = false;
        return Wake::Requested;
    }
    let (mut flags, _) = handle
        .signal
        .condvar
        .wait_timeout(flags, timeout)
        .unwrap_or_else(|error| error.into_inner());
    if flags.stopped {
        return Wake::Stopped;
    }
    // A request that landed during the wait still counts as one: the condvar
    // cannot distinguish it from a plain timeout, but the flag can.
    if flags.requested {
        flags.requested = false;
        return Wake::Requested;
    }
    Wake::Elapsed
}

/// Spreads restarts across clients so a server is not hit by a thundering herd.
fn jittered(base: Duration) -> Duration {
    let entropy = u64::from_str_radix(&random_hex(2), 16).unwrap_or(0);
    let spread = base.as_secs().max(1) / 5;
    base + Duration::from_secs(entropy % spread.max(1))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{STARTUP_HOLD_SETTLE, Wake, jittered, startup_hold_deadline, wait};
    use crate::library::sync::{SYNC_INTERVAL, SyncHandle};

    #[test]
    fn incomplete_catalog_retries_quickly_with_a_bounded_backoff() {
        let mut delay = Duration::ZERO;
        for seconds in [5, 10, 20, 40, 60, 60] {
            delay = super::next_backoff(delay, true);
            assert_eq!(delay, Duration::from_secs(seconds));
        }
        let limited = crate::jellyfin::api::ApiError::RateLimited {
            retry_after_secs: Some(300),
        };
        assert_eq!(
            super::retry_delay(&limited, delay),
            Duration::from_secs(300)
        );
        assert_eq!(
            super::next_backoff(Duration::ZERO, false),
            SYNC_INTERVAL * 2
        );
    }

    #[test]
    fn a_painted_window_shortens_the_startup_hold_to_its_settle() {
        let now = std::time::Instant::now();
        let cap = now + Duration::from_secs(15);
        assert_eq!(startup_hold_deadline(cap, false, now), cap);
        assert_eq!(
            startup_hold_deadline(cap, true, now),
            now + STARTUP_HOLD_SETTLE
        );
        // Re-evaluating later never pushes the start past the settle already set.
        let settled = startup_hold_deadline(cap, true, now);
        assert_eq!(
            startup_hold_deadline(settled, true, now + Duration::from_secs(1)),
            settled
        );
        // Nor past the cap.
        let late = cap - Duration::from_secs(1);
        assert_eq!(startup_hold_deadline(cap, true, late), cap);
    }

    #[test]
    fn a_window_ready_report_wakes_a_held_worker_without_requesting_a_cycle() {
        let handle = SyncHandle::new(Arc::default());
        assert!(!handle.window_ready());

        handle.release_startup_hold();
        assert!(handle.window_ready());
        assert_eq!(wait(&handle, Duration::from_millis(1)), Wake::Elapsed);
    }

    #[test]
    fn jitter_only_ever_delays_the_next_cycle() {
        for _ in 0..20 {
            let delay = jittered(SYNC_INTERVAL);
            assert!(delay >= SYNC_INTERVAL);
            assert!(delay < SYNC_INTERVAL + SYNC_INTERVAL / 5 + Duration::from_secs(1));
        }
    }

    /// The refresh button is only a "reconcile now" lever if the request
    /// survives the wait it interrupts — a request that reads back as a plain
    /// timeout would silently fall back to the hourly gate.
    #[test]
    fn a_request_is_distinguishable_from_a_timeout_and_is_consumed_once() {
        let handle = SyncHandle::new(Arc::default());

        handle.request();
        assert_eq!(wait(&handle, Duration::ZERO), Wake::Requested);
        // Consumed: the next wait is an ordinary scheduled one.
        assert_eq!(wait(&handle, Duration::from_millis(1)), Wake::Elapsed);

        handle.stop();
        assert_eq!(wait(&handle, Duration::ZERO), Wake::Stopped);
    }
}
