use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::app::ids::random_hex;
use crate::jellyfin::api::ApiError;
use crate::jellyfin::session::Session;
use crate::library::Library;

use super::cycle::run_cycle_inner;
use super::{Flags, SYNC_INTERVAL, Signal, SyncHandle, Trigger, WorkerState};

/// Delay used while waiting for the user to sign in.
const IDLE_INTERVAL: Duration = Duration::from_secs(30);

pub fn spawn(library: Arc<Library>, session: Arc<Session>) -> SyncHandle {
    let handle = SyncHandle {
        signal: Arc::new(Signal {
            flags: Mutex::new(Flags::default()),
            condvar: Condvar::new(),
        }),
        running: Arc::new(AtomicBool::new(false)),
        state: Arc::new(Mutex::new(WorkerState::default())),
    };
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
    let mut backoff = SYNC_INTERVAL;
    let mut normal_deadline = Instant::now();
    let mut trigger = Trigger::Scheduled;
    loop {
        if !session.is_authenticated() {
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

        let normal_due = trigger == Trigger::Requested || Instant::now() >= normal_deadline;
        handle.running.store(normal_due, Ordering::Relaxed);

        if normal_due {
            let outcome = run_cycle_inner(library, session, trigger, Some(handle));
            trigger = Trigger::Scheduled;
            let delay = match outcome {
                Ok(report) => {
                    backoff = SYNC_INTERVAL;
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
                Err(ApiError::Unauthorized) => {
                    // `mark_expired` itself notifies the UI, exactly once.
                    session.mark_expired();
                    IDLE_INTERVAL
                }
                Err(error) => {
                    tracing::warn!(target: "library.sync", "library sync cycle failed: {error}");
                    backoff = (backoff * 2).min(Duration::from_secs(30 * 60));
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
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    use super::{Wake, jittered, wait};
    use crate::library::sync::{Flags, SYNC_INTERVAL, Signal, SyncHandle, WorkerState};

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
        let handle = SyncHandle {
            signal: Arc::new(Signal {
                flags: Mutex::new(Flags::default()),
                condvar: Condvar::new(),
            }),
            running: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(WorkerState::default())),
        };

        handle.request();
        assert_eq!(wait(&handle, Duration::ZERO), Wake::Requested);
        // Consumed: the next wait is an ordinary scheduled one.
        assert_eq!(wait(&handle, Duration::from_millis(1)), Wake::Elapsed);

        handle.stop();
        assert_eq!(wait(&handle, Duration::ZERO), Wake::Stopped);
    }
}
