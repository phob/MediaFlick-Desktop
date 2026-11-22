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

const IDLE_INTERVAL: Duration = Duration::from_secs(30);
const STARTUP_HOLD_MAX: Duration = Duration::from_secs(15);
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
    let mut startup_hold = (session.is_authenticated() && super::bootstrap_progress(library).ready)
        .then(|| Instant::now() + STARTUP_HOLD_MAX);
    loop {
        if !session.is_authenticated() {
            startup_hold = None;
            handle.running.store(false, Ordering::Relaxed);
            match wait(handle, IDLE_INTERVAL) {
                Wake::Stopped => return,
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
            let generation = session.generation();
            let outcome = run_cycle_inner(library, session, trigger, Some(handle));
            trigger = Trigger::Scheduled;
            let delay = match outcome {
                Ok(report) => {
                    backoff = Duration::ZERO;
                    handle.clear_failure();
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
                Err(ApiError::Cancelled) => {
                    backoff = Duration::ZERO;
                    IDLE_INTERVAL
                }
                Err(ApiError::Unauthorized) => {
                    backoff = Duration::ZERO;
                    IDLE_INTERVAL
                }
                Err(error) => {
                    tracing::warn!(target: "library.sync", "library sync cycle failed: {error}");
                    backoff = next_retry_backoff(library, backoff);
                    let delay = retry_delay(&error, jittered(backoff));
                    handle.set_retry(&error, delay, generation);
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

fn startup_hold_deadline(until: Instant, window_ready: bool, now: Instant) -> Instant {
    if window_ready {
        until.min(now + STARTUP_HOLD_SETTLE)
    } else {
        until
    }
}

fn next_retry_backoff(library: &Library, previous: Duration) -> Duration {
    let initial_download = super::first_catalog_download(library).unwrap_or_else(|error| {
        tracing::warn!(
            target: "library.sync",
            "could not read catalog bootstrap state for retry backoff: {error}"
        );
        false
    });
    next_backoff(previous, initial_download)
}

fn next_backoff(previous: Duration, initial_download: bool) -> Duration {
    let (initial, maximum) = if initial_download {
        (Duration::from_secs(5), Duration::from_secs(60))
    } else {
        (SYNC_INTERVAL * 2, Duration::from_secs(30 * 60))
    };
    (previous * 2).max(initial).min(maximum)
}

fn retry_delay(error: &ApiError, fallback: Duration) -> Duration {
    error.retry_after().unwrap_or(fallback).max(fallback)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wake {
    Requested,
    Elapsed,
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
    if flags.requested {
        flags.requested = false;
        return Wake::Requested;
    }
    Wake::Elapsed
}

fn jittered(base: Duration) -> Duration {
    let entropy = u64::from_str_radix(&random_hex(2), 16).unwrap_or(0);
    let spread = base.as_secs().max(1) / 5;
    base + Duration::from_secs(entropy % spread.max(1))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::jellyfin::session::Live;
    use crate::library::Library;
    use crate::library::sync::{
        META_BOOTSTRAP_DONE, META_BOOTSTRAP_OFFSET, META_CATALOG_READY, META_LAST_BOOTSTRAP,
        META_LATEST_FAILURE, SYNC_INTERVAL,
    };

    #[test]
    fn a_server_retry_after_outlasts_the_local_backoff() {
        let limited = crate::jellyfin::api::ApiError::RateLimited {
            retry_after_secs: Some(300),
        };
        assert_eq!(
            super::retry_delay(&limited, Duration::from_secs(60)),
            Duration::from_secs(300)
        );
    }

    #[test]
    fn retry_backoff_uses_steady_state_schedule_during_daily_redownload() {
        let live = Live::for_test();
        let initial = Library::open_in_memory().expect("library");
        assert_eq!(
            super::next_retry_backoff(&initial, Duration::ZERO),
            Duration::from_secs(5)
        );
        assert_eq!(
            super::next_retry_backoff(&initial, Duration::from_secs(40)),
            Duration::from_secs(60)
        );

        initial
            .set_meta(&live, META_CATALOG_READY, "1")
            .expect("ready");
        assert_eq!(
            super::next_retry_backoff(&initial, Duration::ZERO),
            Duration::from_secs(5)
        );

        let redownload = Library::open_in_memory().expect("library");
        redownload
            .set_meta(&live, META_BOOTSTRAP_DONE, "0")
            .expect("done");
        redownload
            .set_meta(&live, META_LAST_BOOTSTRAP, "1000")
            .expect("last bootstrap");
        redownload
            .set_meta(&live, META_BOOTSTRAP_OFFSET, "200")
            .expect("offset");
        redownload
            .set_meta(&live, META_LATEST_FAILURE, "1")
            .expect("latest failure");
        assert_eq!(
            super::next_retry_backoff(&redownload, Duration::ZERO),
            SYNC_INTERVAL * 2
        );
        assert_eq!(
            super::next_retry_backoff(&redownload, SYNC_INTERVAL * 2),
            Duration::from_secs(30 * 60)
        );

        let settled = Library::open_in_memory().expect("library");
        settled
            .set_meta(&live, META_BOOTSTRAP_DONE, "1")
            .expect("done");
        settled
            .set_meta(&live, META_LAST_BOOTSTRAP, "1000")
            .expect("last bootstrap");
        assert_eq!(
            super::next_retry_backoff(&settled, Duration::ZERO),
            SYNC_INTERVAL * 2
        );

        settled
            .db
            .with_connection(|connection| connection.execute_batch("DROP TABLE meta"))
            .expect("drop meta");
        assert_eq!(
            super::next_retry_backoff(&settled, Duration::ZERO),
            SYNC_INTERVAL * 2
        );
    }
}
