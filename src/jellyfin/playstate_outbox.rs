use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, PoisonError, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::api::ApiError;
use super::session::Session;
use crate::library::Library;
use crate::preferences::AccountKey;

const MAX_STOPPED_SESSIONS: usize = 256;
const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(2);

static REGISTERED_OUTBOXES: Mutex<Vec<Weak<PlaystateOutbox>>> = Mutex::new(Vec::new());

#[derive(Debug, Clone)]
pub struct OutboxEntry {
    pub item_id: String,
    pub play_session_id: String,
    pub position_ticks: i64,
    pub is_paused: bool,
    pub failed: bool,
    pub play_method: String,
    pub device_id: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxKind {
    Start,
    Progress,
    Stopped,
    StopEncoding,
}

impl OutboxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Progress => "progress",
            Self::Stopped => "stopped",
            Self::StopEncoding => "stop_encoding",
        }
    }

    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Start => "Sessions/Playing",
            Self::Progress => "Sessions/Playing/Progress",
            Self::Stopped => "Sessions/Playing/Stopped",
            Self::StopEncoding => "Videos/ActiveEncodings",
        }
    }

    fn from_str(raw: &str) -> Option<Self> {
        match raw {
            "start" => Some(Self::Start),
            "progress" => Some(Self::Progress),
            "stopped" => Some(Self::Stopped),
            "stop_encoding" => Some(Self::StopEncoding),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutboxRow {
    pub(crate) id: i64,
    pub(crate) revision: i64,
    pub(crate) item_id: String,
    pub(crate) play_session_id: String,
    pub(crate) kind: OutboxKind,
    pub(crate) device_id: String,
    pub(crate) payload_json: String,
}

#[derive(Default)]
struct OutboxState {
    stopped: bool,
    wake_seq: u64,
    idle_seq: u64,
    paused_accounts: HashSet<AccountKey>,
    stopped_sessions: VecDeque<(AccountKey, String, String)>,
}

pub struct PlaystateOutbox {
    library: Arc<Library>,
    session: Arc<Session>,
    state: Arc<Mutex<OutboxState>>,
    cvar: Arc<Condvar>,
    has_worker: bool,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for PlaystateOutbox {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("PlaystateOutbox").finish()
    }
}

impl Drop for PlaystateOutbox {
    fn drop(&mut self) {
        self.signal_stop();
    }
}

impl PlaystateOutbox {
    pub fn new(library: Arc<Library>, session: Arc<Session>) -> Arc<Self> {
        let state = Arc::new(Mutex::new(OutboxState::default()));
        let cvar = Arc::new(Condvar::new());
        let outbox = Arc::new(Self {
            library,
            session,
            state,
            cvar,
            has_worker: true,
            worker: Mutex::new(None),
        });
        let worker_ref = Arc::downgrade(&outbox);
        if let Ok(handle) = thread::Builder::new()
            .name("jellyfin-playstate-outbox".to_string())
            .spawn(move || run_worker(&worker_ref))
        {
            let mut slot = outbox.worker.lock().unwrap_or_else(PoisonError::into_inner);
            *slot = Some(handle);
        }
        register_outbox(&outbox);
        outbox
    }

    pub fn enqueue_start(&self, account: &AccountKey, entry: &OutboxEntry) -> bool {
        if !entry.play_session_id.is_empty()
            && self.is_session_stopped(account, &entry.item_id, &entry.play_session_id)
        {
            return false;
        }
        let now = now_unix_secs();
        let payload = entry.payload.to_string();
        let result = self.library.with_connection(|connection| {
            if !entry.play_session_id.is_empty() {
                let stopped_exists: bool = connection.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM playstate_outbox
                         WHERE server_id = ?1 AND user_id = ?2 AND item_id = ?3
                           AND play_session_id = ?4 AND kind = 'stopped'
                     )",
                    params![
                        account.server_id(),
                        account.user_id(),
                        entry.item_id,
                        entry.play_session_id
                    ],
                    |row| row.get(0),
                )?;
                if stopped_exists {
                    return Ok(false);
                }
            }
            upsert_outbox_row(connection, account, entry, OutboxKind::Start, &payload, now)?;
            Ok(true)
        });
        match result {
            Ok(true) => {
                self.bump_and_notify();
                true
            }
            Ok(false) => false,
            Err(error) => {
                tracing::warn!(target: "jellyfin.playstate", "failed to persist start in outbox: {error}");
                false
            }
        }
    }

    pub fn enqueue_progress(&self, account: &AccountKey, entry: &OutboxEntry) -> bool {
        if !entry.play_session_id.is_empty()
            && self.is_session_stopped(account, &entry.item_id, &entry.play_session_id)
        {
            return false;
        }
        let now = now_unix_secs();
        let payload = entry.payload.to_string();
        let result = self.library.with_connection(|connection| {
            if !entry.play_session_id.is_empty() {
                let stopped_exists: bool = connection.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM playstate_outbox
                         WHERE server_id = ?1 AND user_id = ?2 AND item_id = ?3
                           AND play_session_id = ?4 AND kind = 'stopped'
                     )",
                    params![
                        account.server_id(),
                        account.user_id(),
                        entry.item_id,
                        entry.play_session_id
                    ],
                    |row| row.get(0),
                )?;
                if stopped_exists {
                    return Ok(false);
                }
            }
            connection.execute(
                "INSERT INTO playstate_outbox (
                     server_id, user_id, item_id, play_session_id, kind,
                     position_ticks, is_paused, failed, play_method, device_id,
                     payload_json, attempts, next_retry_at, revision, created_at, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, 'progress', ?5, ?6, ?7, ?8, ?9, ?10, 0, 0, 0, ?11, ?11)
                 ON CONFLICT (server_id, user_id, item_id, play_session_id, kind) DO UPDATE SET
                     position_ticks = excluded.position_ticks,
                     is_paused = excluded.is_paused,
                     failed = excluded.failed,
                     play_method = excluded.play_method,
                     device_id = excluded.device_id,
                     payload_json = excluded.payload_json,
                     attempts = 0,
                     next_retry_at = 0,
                     revision = playstate_outbox.revision + 1,
                     updated_at = excluded.updated_at",
                params![
                    account.server_id(),
                    account.user_id(),
                    entry.item_id,
                    entry.play_session_id,
                    entry.position_ticks.max(0),
                    i64::from(entry.is_paused),
                    i64::from(entry.failed),
                    entry.play_method,
                    entry.device_id,
                    payload,
                    now,
                ],
            )?;
            Ok(true)
        });
        match result {
            Ok(true) => {
                self.bump_and_notify();
                true
            }
            Ok(false) => false,
            Err(error) => {
                tracing::warn!(target: "jellyfin.playstate", "failed to persist progress in outbox: {error}");
                false
            }
        }
    }

    pub fn enqueue_stopped(&self, account: &AccountKey, entry: &OutboxEntry) -> bool {
        if !entry.play_session_id.is_empty() {
            self.mark_session_stopped(account, &entry.item_id, &entry.play_session_id);
        }
        let now = now_unix_secs();
        let payload = entry.payload.to_string();
        let result = self.library.with_transaction(|tx| {
            tx.execute(
                "DELETE FROM playstate_outbox
                 WHERE server_id = ?1 AND user_id = ?2 AND item_id = ?3
                   AND play_session_id = ?4 AND kind = 'progress'",
                params![
                    account.server_id(),
                    account.user_id(),
                    entry.item_id,
                    entry.play_session_id
                ],
            )?;
            upsert_outbox_row(tx, account, entry, OutboxKind::Stopped, &payload, now)?;
            if entry.play_method == "Transcode"
                && !entry.play_session_id.trim().is_empty()
                && !entry.device_id.trim().is_empty()
            {
                let stop_encoding = OutboxEntry {
                    item_id: entry.item_id.clone(),
                    play_session_id: entry.play_session_id.clone(),
                    position_ticks: 0,
                    is_paused: false,
                    failed: false,
                    play_method: entry.play_method.clone(),
                    device_id: entry.device_id.clone(),
                    payload: serde_json::json!({}),
                };
                upsert_outbox_row(
                    tx,
                    account,
                    &stop_encoding,
                    OutboxKind::StopEncoding,
                    "{}",
                    now,
                )?;
            }
            Ok(())
        });
        match result {
            Ok(()) => {
                self.bump_and_notify();
                true
            }
            Err(error) => {
                tracing::warn!(target: "jellyfin.playstate", "failed to persist stop in outbox: {error}");
                false
            }
        }
    }

    pub fn wake_account(&self, account: &AccountKey) {
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.paused_accounts.remove(account);
            state.wake_seq = state.wake_seq.saturating_add(1);
        }
        let _ = self.library.with_connection(|connection| {
            connection.execute(
                "UPDATE playstate_outbox SET next_retry_at = 0
                 WHERE server_id = ?1 AND user_id = ?2",
                params![account.server_id(), account.user_id()],
            )
        });
        self.cvar.notify_all();
    }

    pub fn flush_active(&self, timeout: Duration) -> bool {
        if !self.has_worker {
            return self.active_pending_count().is_ok_and(|count| count == 0);
        }
        let deadline = Instant::now() + timeout;
        let target_seq = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.stopped {
                drop(state);
                return self.active_pending_count().is_ok_and(|count| count == 0);
            }
            state.wake_seq = state.wake_seq.saturating_add(1);
            state.wake_seq
        };
        self.cvar.notify_all();

        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if state.stopped || state.idle_seq >= target_seq {
                break;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                drop(state);
                return self.active_pending_count().is_ok_and(|count| count == 0);
            }
            let (next_state, wait_result) = self
                .cvar
                .wait_timeout(state, remaining)
                .unwrap_or_else(PoisonError::into_inner);
            state = next_state;
            if wait_result.timed_out() && state.idle_seq < target_seq {
                drop(state);
                return self.active_pending_count().is_ok_and(|count| count == 0);
            }
        }
        drop(state);
        self.active_pending_count().is_ok_and(|count| count == 0)
    }

    pub fn delete_account(&self, account: &AccountKey) -> rusqlite::Result<()> {
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.paused_accounts.remove(account);
            state
                .stopped_sessions
                .retain(|(stored, _, _)| stored != account);
        }
        self.library.with_connection(|connection| {
            connection.execute(
                "DELETE FROM playstate_outbox WHERE server_id = ?1 AND user_id = ?2",
                params![account.server_id(), account.user_id()],
            )?;
            Ok(())
        })
    }

    pub fn shutdown(&self) {
        self.signal_stop();
        let handle = {
            let mut slot = self.worker.lock().unwrap_or_else(PoisonError::into_inner);
            slot.take()
        };
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    fn signal_stop(&self) {
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.stopped = true;
        }
        self.cvar.notify_all();
    }

    fn bump_and_notify(&self) {
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.wake_seq = state.wake_seq.saturating_add(1);
        }
        self.cvar.notify_all();
    }

    fn is_session_stopped(
        &self,
        account: &AccountKey,
        item_id: &str,
        play_session_id: &str,
    ) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .stopped_sessions
            .iter()
            .any(|(acc, item, ps)| acc == account && item == item_id && ps == play_session_id)
    }

    fn mark_session_stopped(&self, account: &AccountKey, item_id: &str, play_session_id: &str) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let key = (
            account.clone(),
            item_id.to_string(),
            play_session_id.to_string(),
        );
        if !state.stopped_sessions.contains(&key) {
            if state.stopped_sessions.len() >= MAX_STOPPED_SESSIONS {
                state.stopped_sessions.pop_front();
            }
            state.stopped_sessions.push_back(key);
        }
    }

    fn active_pending_count(&self) -> rusqlite::Result<i64> {
        let Some(account) = self.session.account_key() else {
            return Ok(0);
        };
        self.library.with_connection(|connection| {
            connection.query_row(
                "SELECT count(*) FROM playstate_outbox WHERE server_id = ?1 AND user_id = ?2",
                params![account.server_id(), account.user_id()],
                |row| row.get(0),
            )
        })
    }

    fn next_due_row(&self, account: &AccountKey, now: i64) -> rusqlite::Result<Option<OutboxRow>> {
        let paused = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.paused_accounts.contains(account)
        };
        if paused {
            return Ok(None);
        }
        self.library.with_connection(|connection| {
            let next = connection
                .query_row(
                    "SELECT o.id, o.revision, o.item_id, o.play_session_id, o.kind,
                            o.device_id, o.payload_json, o.next_retry_at
                     FROM playstate_outbox o
                     WHERE o.server_id = ?1 AND o.user_id = ?2
                     ORDER BY
                         CASE WHEN o.play_session_id = '' THEN o.id ELSE (
                             SELECT min(s.id)
                             FROM playstate_outbox s
                             WHERE s.server_id = o.server_id
                               AND s.user_id = o.user_id
                               AND s.item_id = o.item_id
                               AND s.play_session_id = o.play_session_id
                         ) END ASC,
                         CASE WHEN o.play_session_id = '' THEN 0 ELSE CASE o.kind
                             WHEN 'start' THEN 0
                             WHEN 'progress' THEN 1
                             WHEN 'stopped' THEN 2
                             ELSE 3
                         END END ASC,
                         o.id ASC
                     LIMIT 1",
                    params![account.server_id(), account.user_id()],
                    |row| {
                        let kind_str: String = row.get(4)?;
                        let kind = OutboxKind::from_str(&kind_str).ok_or_else(|| {
                            rusqlite::Error::FromSqlConversionFailure(
                                4,
                                rusqlite::types::Type::Text,
                                "invalid outbox kind".into(),
                            )
                        })?;
                        let retry_at: i64 = row.get(7)?;
                        Ok((
                            OutboxRow {
                                id: row.get(0)?,
                                revision: row.get(1)?,
                                item_id: row.get(2)?,
                                play_session_id: row.get(3)?,
                                kind,
                                device_id: row.get(5)?,
                                payload_json: row.get(6)?,
                            },
                            retry_at,
                        ))
                    },
                )
                .optional()?;
            Ok(next.and_then(|(row, retry_at)| (retry_at <= now).then_some(row)))
        })
    }

    fn record_outcome(
        &self,
        account: &AccountKey,
        row_id: i64,
        row_revision: i64,
        row_kind: OutboxKind,
        outcome: Result<(), ApiError>,
        now: i64,
    ) -> rusqlite::Result<()> {
        match outcome {
            Ok(()) => self.library.with_connection(|connection| {
                connection.execute(
                    "DELETE FROM playstate_outbox WHERE id = ?1 AND revision = ?2 AND kind = ?3",
                    params![row_id, row_revision, row_kind.as_str()],
                )?;
                Ok(())
            }),
            Err(ApiError::Unauthorized) => {
                self.state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .paused_accounts
                    .insert(account.clone());
                Ok(())
            }
            Err(ApiError::Status { status } | ApiError::Remote { status, .. })
                if (400..500).contains(&status) && !matches!(status, 408 | 429) =>
            {
                self.library.with_connection(|connection| {
                    connection.execute(
                        "DELETE FROM playstate_outbox WHERE id = ?1 AND revision = ?2 AND kind = ?3",
                        params![row_id, row_revision, row_kind.as_str()],
                    )?;
                    Ok(())
                })
            }
            Err(error) => {
                let retry_after = error
                    .retry_after()
                    .map(|duration| duration.as_secs().cast_signed());
                self.library.with_connection(|connection| {
                    let attempts: Option<i64> = connection
                        .query_row(
                            "SELECT attempts FROM playstate_outbox WHERE id = ?1 AND revision = ?2 AND kind = ?3",
                            params![row_id, row_revision, row_kind.as_str()],
                            |row| row.get(0),
                        )
                        .optional()?;
                    let Some(attempts) = attempts else {
                        return Ok(());
                    };
                    let exp_backoff = (2_i64 << attempts.clamp(0, 5)).min(60);
                    let delay = retry_after.map_or(exp_backoff, |secs| secs.max(exp_backoff));
                    connection.execute(
                        "UPDATE playstate_outbox
                         SET attempts = ?4, next_retry_at = ?5, updated_at = ?6
                         WHERE id = ?1 AND revision = ?2 AND kind = ?3",
                        params![
                            row_id,
                            row_revision,
                            row_kind.as_str(),
                            attempts + 1,
                            now + delay,
                            now
                        ],
                    )?;
                    Ok(())
                })
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn new_manual(library: Arc<Library>, session: Arc<Session>) -> Self {
        Self {
            library,
            session,
            state: Arc::new(Mutex::new(OutboxState::default())),
            cvar: Arc::new(Condvar::new()),
            has_worker: false,
            worker: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn is_account_paused(&self, account: &AccountKey) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.paused_accounts.contains(account)
    }

    #[cfg(test)]
    pub(crate) fn next_due_row_for_test(
        &self,
        account: &AccountKey,
        now: i64,
    ) -> rusqlite::Result<Option<OutboxRow>> {
        self.next_due_row(account, now)
    }

    #[cfg(test)]
    pub(crate) fn record_outcome_for_test(
        &self,
        account: &AccountKey,
        row_id: i64,
        row_revision: i64,
        row_kind: OutboxKind,
        outcome: Result<(), ApiError>,
        now: i64,
    ) -> rusqlite::Result<()> {
        self.record_outcome(account, row_id, row_revision, row_kind, outcome, now)
    }
}

pub(crate) fn flush_registered(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let live: Vec<Arc<PlaystateOutbox>> = {
        let mut guard = REGISTERED_OUTBOXES
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        guard.retain(|weak| weak.strong_count() > 0);
        guard.iter().filter_map(Weak::upgrade).collect()
    };
    let mut all_ok = true;
    for outbox in live {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !outbox.flush_active(remaining) {
            all_ok = false;
        }
    }
    all_ok
}

fn register_outbox(outbox: &Arc<PlaystateOutbox>) {
    let mut guard = REGISTERED_OUTBOXES
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    guard.retain(|weak| weak.strong_count() > 0);
    guard.push(Arc::downgrade(outbox));
}

fn upsert_outbox_row(
    connection: &Connection,
    account: &AccountKey,
    entry: &OutboxEntry,
    kind: OutboxKind,
    payload: &str,
    now: i64,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO playstate_outbox (
             server_id, user_id, item_id, play_session_id, kind,
             position_ticks, is_paused, failed, play_method, device_id,
             payload_json, attempts, next_retry_at, revision, created_at, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, 0, 0, ?12, ?12)
         ON CONFLICT (server_id, user_id, item_id, play_session_id, kind) DO UPDATE SET
             position_ticks = excluded.position_ticks,
             is_paused = excluded.is_paused,
             failed = excluded.failed,
             play_method = excluded.play_method,
             device_id = excluded.device_id,
             payload_json = excluded.payload_json,
             attempts = 0,
             next_retry_at = 0,
             revision = playstate_outbox.revision + 1,
             updated_at = excluded.updated_at",
        params![
            account.server_id(),
            account.user_id(),
            entry.item_id,
            entry.play_session_id,
            kind.as_str(),
            entry.position_ticks.max(0),
            i64::from(entry.is_paused),
            i64::from(entry.failed),
            entry.play_method,
            entry.device_id,
            payload,
            now,
        ],
    )?;
    Ok(())
}

fn run_worker(weak: &Weak<PlaystateOutbox>) {
    let mut observed_seq = 0_u64;
    loop {
        let Some(outbox) = weak.upgrade() else {
            return;
        };
        let target_seq = drain_cycle(&outbox);
        observed_seq = observed_seq.max(target_seq);
        let state = outbox.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.stopped {
            return;
        }
        let next_wait = if state.wake_seq > observed_seq {
            Duration::ZERO
        } else {
            IDLE_POLL_INTERVAL
        };
        if !next_wait.is_zero() {
            let _ = outbox.cvar.wait_timeout(state, next_wait);
        }
    }
}

fn drain_cycle(outbox: &PlaystateOutbox) -> u64 {
    let cycle_seq = {
        let state = outbox.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.stopped {
            return state.wake_seq;
        }
        state.wake_seq
    };
    while let Ok(scope) = outbox.session.scope() {
        if outbox
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stopped
        {
            break;
        }
        let now = now_unix_secs();
        let Ok(Some(row)) = outbox.next_due_row(scope.account(), now) else {
            break;
        };
        let endpoint = row.kind.endpoint();
        tracing::trace!(
            target: "jellyfin.playstate",
            endpoint,
            item_id = %row.item_id,
            play_session_id = %row.play_session_id,
            "sending queued Jellyfin playback state"
        );
        let outcome = match deliver_row(&scope, &row) {
            Ok(status) => {
                tracing::trace!(
                    target: "jellyfin.playstate",
                    endpoint,
                    item_id = %row.item_id,
                    status,
                    "sent Jellyfin playback state"
                );
                Ok(())
            }
            Err(error) => {
                tracing::warn!(
                    target: "jellyfin.playstate",
                    endpoint,
                    item_id = %row.item_id,
                    "failed to report Jellyfin playback state: {error}"
                );
                Err(error)
            }
        };
        if let Err(error) = outbox.record_outcome(
            scope.account(),
            row.id,
            row.revision,
            row.kind,
            outcome,
            now_unix_secs(),
        ) {
            tracing::warn!(target: "jellyfin.playstate", "failed to update playstate outbox row: {error}");
            break;
        }
    }
    {
        let mut state = outbox.state.lock().unwrap_or_else(PoisonError::into_inner);
        if cycle_seq > state.idle_seq {
            state.idle_seq = cycle_seq;
        }
    }
    outbox.cvar.notify_all();
    cycle_seq
}

fn deliver_row(scope: &super::session::SessionScope, row: &OutboxRow) -> Result<u16, ApiError> {
    let payload: Value = serde_json::from_str(&row.payload_json)
        .map_err(|error| ApiError::Decode(error.to_string()))?;
    match row.kind {
        OutboxKind::Start => scope
            .client()
            .post_playstate_once("/Sessions/Playing", &payload),
        OutboxKind::Progress => scope
            .client()
            .post_playstate_once("/Sessions/Playing/Progress", &payload),
        OutboxKind::Stopped => scope
            .client()
            .post_playstate_once("/Sessions/Playing/Stopped", &payload),
        OutboxKind::StopEncoding => scope.client().delete_playstate_once(
            "/Videos/ActiveEncodings",
            &[
                ("deviceId", row.device_id.clone()),
                ("playSessionId", row.play_session_id.clone()),
            ],
        ),
    }
}

fn now_unix_secs() -> i64 {
    i64::try_from(crate::app::clock::since_epoch().as_secs()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
