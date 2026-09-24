use crate::app::threads::spawn_named;
use std::time::Instant;

use super::bridge::*;
use super::document::*;
use super::*;
use crate::playback::PlayerSnapshot;
use crate::preferences::FullscreenBehavior;

pub(super) fn warm_configured_player(playback: &PlaybackCoordinator, settings: &AppSettings) {
    playback.configure(settings.player_preferences());
    let Some(path) = crate::players::configured_player_path(settings) else {
        tracing::debug!(target: "mpv.ipc", "skipped player warmup because the selected runtime is unavailable");
        return;
    };
    playback.warm(path, player_warmup_mode(settings));
}

fn player_warmup_mode(settings: &AppSettings) -> FullscreenBehavior {
    if libmpv_overlay::is_configured(settings) {
        FullscreenBehavior::Windowed
    } else {
        settings.default_fullscreen
    }
}

/// Coalesces rapid playback state updates before forwarding them to the WebUI,
/// while tracking which meaningful state changes should be logged.
pub(super) fn start_playback_event_bridge(state: &BrowserState, rx: Receiver<PlaybackEvent>) {
    const STATE_PUSH_INTERVAL: Duration = Duration::from_millis(100);

    let state = Arc::downgrade(state);
    spawn_named("playback-event-bridge", move || {
        let mut last_logged_snapshot = None;
        let mut post = |event| {
            let Some(state) = state.upgrade() else {
                return false;
            };
            let log_event = playback_event_needs_log(&mut last_logged_snapshot, &event);
            let mut task = PlaybackEventTask::new(state, event, log_event);
            if post_task(ThreadId::UI, Some(&mut task)) == 0 {
                tracing::warn!(target: "bridge", "failed to post playback event to CEF UI thread");
            }
            true
        };
        let mut pending_state = None;
        let mut state_deadline: Option<Instant> = None;
        loop {
            let received = if let Some(deadline) = state_deadline {
                rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            } else {
                match rx.recv() {
                    Ok(event) => Ok(event),
                    Err(_) => Err(mpsc::RecvTimeoutError::Disconnected),
                }
            };
            match received {
                Ok(PlaybackEvent::StateChanged(snapshot)) => {
                    pending_state = Some(snapshot);
                    state_deadline.get_or_insert_with(|| Instant::now() + STATE_PUSH_INTERVAL);
                }
                Ok(event) => {
                    if let Some(snapshot) = pending_state.take()
                        && !post(PlaybackEvent::StateChanged(snapshot))
                    {
                        break;
                    }
                    state_deadline = None;
                    if !post(event) {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let Some(snapshot) = pending_state.take() else {
                        state_deadline = None;
                        continue;
                    };
                    state_deadline = None;
                    if !post(PlaybackEvent::StateChanged(snapshot)) {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if let Some(snapshot) = pending_state {
                        let _ = post(PlaybackEvent::StateChanged(snapshot));
                    }
                    break;
                }
            }
        }
    });
}

/// Timeline movement and diagnostic counters still reach the UI, but should
/// not repeat the full snapshot in the log on every progress update.
fn playback_event_needs_log(
    last_logged_snapshot: &mut Option<PlayerSnapshot>,
    event: &PlaybackEvent,
) -> bool {
    let PlaybackEvent::StateChanged(snapshot) = event else {
        *last_logged_snapshot = None;
        return true;
    };
    if last_logged_snapshot.as_ref().is_some_and(|previous| {
        previous.active == snapshot.active
            && previous.playback_id == snapshot.playback_id
            && previous.item_id == snapshot.item_id
            && previous.media_source_id == snapshot.media_source_id
            && previous.play_session_id == snapshot.play_session_id
            && previous.play_method == snapshot.play_method
            && previous.duration_ms == snapshot.duration_ms
            && previous.paused == snapshot.paused
            && previous.volume == snapshot.volume
            && previous.mute == snapshot.mute
            && previous.tracks == snapshot.tracks
            && previous.chapters == snapshot.chapters
            && previous.skip_segments == snapshot.skip_segments
            && previous.diagnostics.buffering == snapshot.diagnostics.buffering
            && previous.stop_reason == snapshot.stop_reason
    }) {
        return false;
    }
    *last_logged_snapshot = Some(snapshot.clone());
    true
}

/// Settings are persisted from an app-scheme background thread.  CEF's player
/// and frame APIs are UI-thread-only, so relay its apply plan before touching
/// either of them.
pub(super) fn start_preferences_event_bridge(state: &BrowserState) {
    let Some(services) = services::services() else {
        return;
    };
    let receiver = services.preferences.subscribe();
    let state = Arc::downgrade(state);
    spawn_named("preference-bridge", move || {
        while let Ok(change) = receiver.recv() {
            let Some(state) = state.upgrade() else {
                break;
            };
            let mut task = PreferencesChangeTask::new(state, change);
            if post_task(ThreadId::UI, Some(&mut task)) == 0 {
                tracing::warn!(target: "config", "failed to post preference change to CEF UI thread");
            }
        }
    });
}

pub(super) fn start_shell_request_bridge(state: &BrowserState) {
    let Some(services) = services::services() else {
        return;
    };
    let receiver = services.shell.subscribe();
    let state = Arc::downgrade(state);
    spawn_named("shell-request-bridge", move || {
        while let Ok(request) = receiver.recv() {
            let Some(state) = state.upgrade() else {
                break;
            };
            let mut task = ShellRequestTask::new(state, request);
            if post_task(ThreadId::UI, Some(&mut task)) == 0 {
                tracing::warn!(target: "shell", "failed to post shell request to CEF UI thread");
            }
        }
    });
}

pub(super) fn start_update_check_bridge(state: BrowserState) {
    spawn_named("update-check", move || match updater::check_for_update() {
        Ok(Some(release)) => post_update_event(state, UpdateEvent::Available(release)),
        Ok(None) => tracing::debug!(target: "updater", "no supported update available"),
        Err(error) => tracing::warn!(target: "updater", "failed to check for updates: {error}"),
    });
}

pub(super) fn post_update_event(state: BrowserState, event: UpdateEvent) {
    let mut task = UpdateEventTask::new(state, event);
    if post_task(ThreadId::UI, Some(&mut task)) == 0 {
        tracing::warn!(target: "updater", "failed to post update event to CEF UI thread");
    }
}

pub(super) fn post_mpv_setup_event(state: BrowserState, event: MpvSetupEvent) {
    let mut task = MpvSetupEventTask::new(state, event);
    if post_task(ThreadId::UI, Some(&mut task)) == 0 {
        tracing::warn!(target: "mpv.setup", "failed to post mpv setup event to CEF UI thread");
    }
}

#[derive(Debug, Clone)]
pub(super) enum UpdateEvent {
    Available(UpdateRelease),
    DownloadProgress { downloaded: u64, total: Option<u64> },
    DownloadReady(PathBuf),
    Error(String),
}

#[derive(Debug, Clone)]
pub(super) enum MpvSetupEvent {
    Progress {
        request_id: String,
        downloaded: u64,
        total: Option<u64>,
    },
    Extracting {
        request_id: String,
    },
    Ready {
        request_id: String,
        path: PathBuf,
    },
    Error {
        request_id: String,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybackCacheRefreshOutcome {
    Refreshed,
    Deferred,
}

/// Why a post-playback refresh did not reach the catalog cache.
enum PlaybackCacheWriteError {
    /// The account changed during the fetch; the row belongs to the previous
    /// account's cache.
    StaleSession,
    Storage(rusqlite::Error),
}

impl PlaybackCacheRefreshOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Refreshed => "refreshed",
            Self::Deferred => "deferred",
        }
    }
}

wrap_task! {
    struct PlaybackEventTask {
        state: BrowserState,
        event: PlaybackEvent,
        log_event: bool,
    }

    impl Task {
        fn execute(&self) {
            dispatch_playback_event(&self.state, &self.event, self.log_event);
        }
    }
}

wrap_task! {
    struct PlaybackCacheRefreshTask {
        state: BrowserState,
        item_id: String,
        outcome: PlaybackCacheRefreshOutcome,
    }

    impl Task {
        fn execute(&self) {
            dispatch_playback_cache_refreshed(&self.state, &self.item_id, self.outcome);
        }
    }
}

wrap_task! {
    struct UpdateEventTask {
        state: BrowserState,
        event: UpdateEvent,
    }

    impl Task {
        fn execute(&self) {
            handle_update_event(&self.state, self.event.clone());
        }
    }
}

wrap_task! {
    struct MpvSetupEventTask {
        state: BrowserState,
        event: MpvSetupEvent,
    }

    impl Task {
        fn execute(&self) {
            handle_mpv_setup_event(&self.state, self.event.clone());
        }
    }
}

wrap_task! {
    struct PreferencesChangeTask {
        state: BrowserState,
        change: SettingsChange,
    }

    impl Task {
        fn execute(&self) {
            apply_preference_change(&self.state, &self.change);
        }
    }
}

wrap_task! {
    struct ShellRequestTask {
        state: BrowserState,
        request: ShellRequest,
    }

    impl Task {
        fn execute(&self) {
            handle_shell_request(&self.state, self.request.clone());
        }
    }
}

wrap_task! {
    struct BridgeActionTask {
        request_url: String,
        state: BrowserState,
    }

    impl Task {
        fn execute(&self) {
            if !route_bridge_action(&self.request_url, &self.state) {
                tracing::warn!(
                    target: "bridge",
                    url = %logger::redact_url_secrets(&self.request_url),
                    "ignored unrecognized bridge resource request"
                );
            }
        }
    }
}

pub(super) fn post_bridge_action(request_url: String, state: BrowserState) {
    let mut task = BridgeActionTask::new(request_url, state);
    if post_task(ThreadId::UI, Some(&mut task)) == 0 {
        tracing::warn!(target: "bridge", "failed to post bridge action to CEF UI thread");
    }
}

wrap_task! {
    struct ErrorToastTask {
        state: BrowserState,
        title: String,
        body: String,
    }

    impl Task {
        fn execute(&self) {
            dispatch_error_toast(&self.state, &self.title, &self.body);
        }
    }
}

/// Delivers a playback event to every registered WebUI browser and emits
/// dispatch diagnostics when `log_event` is true.
fn dispatch_playback_event(state: &BrowserState, event: &PlaybackEvent, log_event: bool) {
    if let PlaybackEvent::Failed { message } = &event {
        tracing::warn!(target: "bridge", message, "player backend reported a playback failure");
        dispatch_error_toast(state, "Playback error", message);
        return;
    }

    if let PlaybackEvent::Stopped(snapshot) = &event {
        mirror_playback_progress(state, snapshot);
    }

    let frame_count = broadcast_script(
        state,
        &playback_event_script(event),
        "mediaflick-desktop://playback-event",
    );
    if log_event {
        tracing::debug!(
            target: "bridge",
            ?event,
            frame_count,
            "dispatched playback event to WebUI"
        );
    }
}

/// Runs `script` in the main frame of every registered browser, attributed to
/// `source_url` in DevTools, and returns how many frames received it. The
/// browser list is copied out so no lock is held while CEF runs the script.
pub(super) fn broadcast_script(state: &BrowserState, script: &str, source_url: &str) -> usize {
    let browsers = state
        .lock()
        .map(|state| state.browsers.clone())
        .unwrap_or_default();
    let mut frame_count = 0;
    for frame in browsers.iter().filter_map(Browser::main_frame) {
        frame.execute_java_script(
            Some(&CefString::from(script)),
            Some(&CefString::from(source_url)),
            1,
        );
        frame_count += 1;
    }
    frame_count
}

/// Refreshes the stopped item from Jellyfin after its final playstate report.
/// This lets the server — including any administrator-configured resume
/// thresholds — decide whether the item belongs in Continue Watching.
///
/// Runs off the UI thread because both network and database work can block.
fn mirror_playback_progress(state: &BrowserState, snapshot: &crate::playback::PlayerSnapshot) {
    let (Some(item_id), Some(services)) = (snapshot.item_id.clone(), services::services()) else {
        return;
    };
    let refresh_state = state.clone();
    let spawn_failure_item_id = item_id.clone();
    if let Err(error) = thread::Builder::new()
        .name("library-progress".to_string())
        .spawn(move || {
            let outcome = if !flush_playstate_reports(PLAYSTATE_CACHE_REFRESH_TIMEOUT) {
                tracing::warn!(
                    target: "jellyfin.playstate",
                    item_id,
                    "timed out sending final playback state; scheduled a library refresh"
                );
                services.sync.request();
                PlaybackCacheRefreshOutcome::Deferred
            } else {
                let result = (|| {
                    let scope = services.session.scope()?;
                    let item = items::fetch_item(scope.client(), scope.user_id(), &item_id)?;
                    Ok::<_, crate::jellyfin::api::ApiError>((scope, item))
                })();
                match result {
                    Ok((scope, Some(item))) => {
                        let cached = services.session.commit_if_current(
                            &scope,
                            || PlaybackCacheWriteError::StaleSession,
                            || {
                                services
                                    .library
                                    .upsert_page(&[item])
                                    .map_err(PlaybackCacheWriteError::Storage)
                            },
                        );
                        match cached {
                            Ok(_) => PlaybackCacheRefreshOutcome::Refreshed,
                            Err(PlaybackCacheWriteError::StaleSession) => {
                                tracing::debug!(
                                    target: "jellyfin.playstate",
                                    item_id,
                                    "dropped a playback-state refresh for a previous session"
                                );
                                PlaybackCacheRefreshOutcome::Deferred
                            }
                            Err(PlaybackCacheWriteError::Storage(error)) => {
                                tracing::warn!(
                                    target: "library.db",
                                    item_id,
                                    "failed to cache Jellyfin's resolved playback state: {error}"
                                );
                                services.sync.request();
                                PlaybackCacheRefreshOutcome::Deferred
                            }
                        }
                    }
                    Ok((_, None)) => {
                        tracing::debug!(
                            target: "jellyfin.playstate",
                            item_id,
                            "stopped item was absent from Jellyfin's playback-state refresh"
                        );
                        services.sync.request();
                        PlaybackCacheRefreshOutcome::Deferred
                    }
                    Err(error) => {
                        tracing::debug!(
                            target: "jellyfin.playstate",
                            item_id,
                            "could not refresh Jellyfin's resolved playback state: {error}"
                        );
                        services.sync.request();
                        PlaybackCacheRefreshOutcome::Deferred
                    }
                }
            };
            post_playback_cache_refreshed(refresh_state, item_id, outcome);
        })
    {
        tracing::warn!(target: "library.db", "failed to spawn the progress mirror thread: {error}");
        dispatch_playback_cache_refreshed(
            state,
            &spawn_failure_item_id,
            PlaybackCacheRefreshOutcome::Deferred,
        );
    }
}

fn post_playback_cache_refreshed(
    state: BrowserState,
    item_id: String,
    outcome: PlaybackCacheRefreshOutcome,
) {
    let mut task = PlaybackCacheRefreshTask::new(state, item_id, outcome);
    if post_task(ThreadId::UI, Some(&mut task)) == 0 {
        tracing::warn!(target: "library.db", "failed to post playback cache refresh completion to CEF UI thread");
    }
}

fn dispatch_playback_cache_refreshed(
    state: &BrowserState,
    item_id: &str,
    outcome: PlaybackCacheRefreshOutcome,
) {
    broadcast_script(
        state,
        &playback_cache_refresh_script(item_id, outcome),
        "mediaflick-desktop://playback-cache-refreshed",
    );
}

fn playback_cache_refresh_script(item_id: &str, outcome: PlaybackCacheRefreshOutcome) -> String {
    let payload = json!({ "itemId": item_id, "status": outcome.as_str() });
    format!(
        "window.__mediaFlickDesktopPlaybackCacheRefreshed&&window.__mediaFlickDesktopPlaybackCacheRefreshed({});",
        js_json(&payload)
    )
}

fn playback_event_script(event: &PlaybackEvent) -> String {
    match event {
        PlaybackEvent::StateChanged(snapshot) => format!(
            "window.__mediaFlickDesktopPlaybackStateChanged&&window.__mediaFlickDesktopPlaybackStateChanged({});",
            js_json(snapshot)
        ),
        PlaybackEvent::Stopped(snapshot) => format!(
            "window.__mediaFlickDesktopPlaybackStopped&&window.__mediaFlickDesktopPlaybackStopped({});",
            js_json(snapshot)
        ),
        PlaybackEvent::Failed { .. } => String::new(),
    }
}

fn handle_update_event(state: &BrowserState, event: UpdateEvent) {
    match event {
        UpdateEvent::Available(release) => {
            tracing::info!(
                target: "updater",
                version = %release.version,
                asset = release.asset.as_ref().map(|asset| asset.name.as_str()).unwrap_or("none"),
                "update available"
            );
            if let Ok(mut state) = state.lock() {
                state.update_available = Some(release.clone());
                state.update_download_started = false;
            }
            dispatch_update_available(state, &release);
        }
        UpdateEvent::DownloadProgress { downloaded, total } => {
            dispatch_update_progress(
                state,
                "downloading",
                &json!({ "downloaded": downloaded, "total": total }),
            );
        }
        UpdateEvent::DownloadReady(path) => {
            dispatch_update_progress(state, "installing", &json!({ "downloaded": 1, "total": 1 }));
            match updater::start_installer(&path) {
                Ok(()) => initiate_app_exit(state),
                Err(error) => {
                    if let Ok(mut state) = state.lock() {
                        state.update_download_started = false;
                    }
                    dispatch_update_progress(
                        state,
                        "error",
                        &json!({ "message": error.to_string() }),
                    );
                }
            }
        }
        UpdateEvent::Error(message) => {
            tracing::warn!(target: "updater", "update failed: {message}");
            if let Ok(mut state) = state.lock() {
                state.update_download_started = false;
            }
            dispatch_update_progress(state, "error", &json!({ "message": message }));
        }
    }
}

pub(super) fn dispatch_update_available(state: &BrowserState, release: &UpdateRelease) {
    broadcast_script(
        state,
        &updater::update_available_script(release),
        UPDATE_TOAST_SOURCE,
    );
}

pub(super) fn dispatch_update_progress(
    state: &BrowserState,
    status: &str,
    payload: &serde_json::Value,
) {
    broadcast_script(
        state,
        &updater::update_progress_script(status, payload),
        UPDATE_TOAST_SOURCE,
    );
}

fn handle_mpv_setup_event(state: &BrowserState, event: MpvSetupEvent) {
    let progress = |request_id: String, mut payload: serde_json::Value| {
        payload["requestId"] = json!(request_id);
        dispatch_shell_event(state, "mpv-install-progress", payload);
    };
    let finished = || {
        if let Ok(mut state) = state.lock() {
            state.mpv_setup_started = false;
        }
    };
    match event {
        MpvSetupEvent::Progress {
            request_id,
            downloaded,
            total,
        } => progress(
            request_id,
            json!({ "state": "downloading", "downloaded": downloaded, "total": total }),
        ),
        MpvSetupEvent::Extracting { request_id } => {
            progress(request_id, json!({ "state": "extracting" }));
        }
        MpvSetupEvent::Ready { request_id, path } => {
            let mpv_path = path.to_string_lossy().into_owned();
            tracing::info!(target: "mpv.setup", path = %mpv_path, "mpv installed");
            finished();
            let save_result: Result<(), String> = match services::services() {
                Some(services) => services
                    .preferences
                    .set_mpv_path(mpv_path.clone())
                    .map(|_| ())
                    .map_err(|error| error.to_string()),
                None => Err("preferences service is unavailable".to_string()),
            };
            match save_result {
                Ok(()) => progress(
                    request_id,
                    json!({ "state": "completed", "path": mpv_path }),
                ),
                Err(message) => {
                    tracing::warn!(target: "mpv.setup", "failed to save installed mpv path: {message}");
                    progress(request_id, json!({ "state": "failed", "message": message }));
                }
            }
        }
        MpvSetupEvent::Error {
            request_id,
            message,
        } => {
            tracing::warn!(target: "mpv.setup", "mpv setup failed: {message}");
            finished();
            progress(request_id, json!({ "state": "failed", "message": message }));
        }
    }
}

fn apply_preference_change(state: &BrowserState, change: &SettingsChange) {
    let (playback, event_tx, browsers) = match state.lock() {
        Ok(mut state) => {
            apply_settings_snapshot_preserving_live_window(
                &mut state.settings,
                change.settings.clone(),
            );
            (
                state.playback.clone(),
                state.playback_event_tx.clone(),
                state.browsers.clone(),
            )
        }
        Err(error) => {
            tracing::warn!(target: "config", "failed to apply settings to browser state: {error}");
            return;
        }
    };

    if change.plan.rebuild_player {
        tracing::info!(
            target: "playback",
            backend = change.settings.effective_backend().as_str(),
            "rebuilding player backend after settings change"
        );
        playback.replace(build_backend(&change.settings, event_tx));
        warm_configured_player(&playback, &change.settings);
    } else if change.plan.update_player_preferences {
        playback.configure(change.settings.player_preferences());
    }
    if change.plan.update_shell_css {
        for browser in browsers {
            if let Some(frame) = browser.main_frame() {
                apply_scrollbar_settings_to_frame(&frame, state);
            }
        }
    }
}

fn apply_settings_snapshot_preserving_live_window(
    current: &mut AppSettings,
    mut incoming: AppSettings,
) {
    // Bounds are intentionally persisted only on close/minimize. Until then the
    // BrowserState copy is newer than the preference service's disk snapshot,
    // so a settings PATCH must not replace it with the last launched size.
    incoming.webui_window = current.webui_window;
    *current = incoming;
}

fn handle_shell_request(state: &BrowserState, request: ShellRequest) {
    match request {
        ShellRequest::MainWindowReady => reveal_main_window(state),
        ShellRequest::FilePicker { request_id } => open_settings_file_dialog(state, request_id),
        ShellRequest::InstallMpv { request_id } => start_mpv_download(state, request_id),
        ShellRequest::LibraryChanged {
            item_ids,
            context_ids,
        } => {
            dispatch_shell_event(
                state,
                "library-changed",
                json!({ "itemIds": item_ids, "contextIds": context_ids }),
            );
        }
        ShellRequest::CatalogChanged {
            item_ids,
            context_ids,
        } => {
            dispatch_shell_event(
                state,
                "catalog-changed",
                json!({ "itemIds": item_ids, "contextIds": context_ids }),
            );
        }
        ShellRequest::CollectionsChanged => {
            dispatch_shell_event(state, "collections-changed", json!({}));
        }
        ShellRequest::SessionExpired => {
            dispatch_shell_event(state, "jellyfin-session-expired", json!({}));
        }
    }
}

pub(super) fn dispatch_shell_event(state: &BrowserState, kind: &str, payload: serde_json::Value) {
    let mut event = serde_json::Map::new();
    event.insert("type".to_string(), json!(kind));
    event.insert("payload".to_string(), payload);
    let event = serde_json::Value::Object(event);
    let script = format!(
        "window.dispatchEvent(new CustomEvent('mediaflick-desktop-shell', {{ detail: {} }}));",
        js_json(&event)
    );
    broadcast_script(state, &script, "mediaflick-desktop://shell-event");
}

pub(super) fn show_pending_update_to_frame(frame: &Frame, state: &BrowserState) {
    let pending_update = state.lock().ok().and_then(|state| {
        (!state.update_download_started)
            .then(|| state.update_available.clone())
            .flatten()
    });
    if let Some(release) = pending_update {
        let script = updater::update_available_script(&release);
        execute_update_script(frame, &script);
    }
}

pub(super) fn apply_scrollbar_settings_to_frame(frame: &Frame, state: &BrowserState) {
    let show_scrollbars = state
        .lock()
        .map(|state| state.settings.show_scrollbars)
        .unwrap_or(false);
    let script = format!(
        r#"(() => {{
  const id = '__mediaFlickDesktopScrollbarStyle';
  const existing = document.getElementById(id);
  if ({show_scrollbars}) {{ existing && existing.remove(); return; }}
  if (existing) return;
  const style = document.createElement('style');
  style.id = id;
  style.textContent = `
    html, body, * {{ scrollbar-width: none !important; -ms-overflow-style: none !important; }}
    *::-webkit-scrollbar {{ width: 0 !important; height: 0 !important; display: none !important; }}
  `;
  (document.head || document.documentElement).appendChild(style);
}})();"#,
        show_scrollbars = show_scrollbars
    );
    frame.execute_java_script(
        Some(&CefString::from(script.as_str())),
        Some(&CefString::from("mediaflick-desktop://scrollbars")),
        1,
    );
}

const UPDATE_TOAST_SOURCE: &str = "mediaflick-desktop://update-toast";

fn execute_update_script(frame: &Frame, script: &str) {
    frame.execute_java_script(
        Some(&CefString::from(script)),
        Some(&CefString::from(UPDATE_TOAST_SOURCE)),
        1,
    );
}

pub(super) fn notify_error(state: &BrowserState, title: &str, body: &str) {
    let mut task = ErrorToastTask::new(state.clone(), title.to_string(), body.to_string());
    if post_task(ThreadId::UI, Some(&mut task)) == 0 {
        tracing::warn!(target: "bridge", title, "failed to post error toast to CEF UI thread");
    }
}

fn dispatch_error_toast(state: &BrowserState, title: &str, body: &str) {
    let script = error_toast::error_toast_script(title, body);
    if broadcast_script(state, &script, "mediaflick-desktop://error-toast") == 0 {
        tracing::warn!(
            target: "bridge",
            title,
            "skipped error toast because no WebUI browsers are registered"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::{PlaybackDiagnostics, PlayerChapter, PlayerSnapshot};
    use crate::preferences::{AppSettings, AppearanceAccent, WebUiWindowSettings};

    #[test]
    fn playback_dispatch_logging_ignores_progress_and_diagnostic_counters() {
        let mut last_logged = None;
        let mut snapshot = PlayerSnapshot {
            active: true,
            playback_id: Some(1),
            ..PlayerSnapshot::default()
        };
        assert!(playback_event_needs_log(
            &mut last_logged,
            &PlaybackEvent::StateChanged(snapshot.clone())
        ));
        for tick in 0..100 {
            snapshot.position_ms = f64::from(tick) * 100.0;
            snapshot.diagnostics.buffered_until_ms = Some(snapshot.position_ms + 10_000.0);
            snapshot.diagnostics.dropped_frames = Some(i64::from(tick));
            snapshot.diagnostics.frame_rate = Some(24.0 + f64::from(tick) / 1000.0);
            let event = PlaybackEvent::StateChanged(snapshot.clone());
            assert!(!playback_event_needs_log(&mut last_logged, &event));
        }
    }

    #[test]
    fn settings_snapshots_do_not_roll_back_live_window_geometry() {
        let mut live = AppSettings {
            webui_window: WebUiWindowSettings {
                width: 1536,
                height: 864,
                position: None,
                maximized: false,
            },
            ..AppSettings::default()
        };
        let mut stale_preference_snapshot = AppSettings::default();
        stale_preference_snapshot.appearance.accent = AppearanceAccent::Violet;

        apply_settings_snapshot_preserving_live_window(&mut live, stale_preference_snapshot);

        // This is the snapshot the close lifecycle persists after resize ->
        // unrelated settings change -> close.
        assert_eq!(live.webui_window.size(), (1536, 864));
        assert_eq!(live.appearance.accent, AppearanceAccent::Violet);
    }

    #[test]
    fn playback_snapshot_script_carries_timeline_and_diagnostics() {
        let script = playback_event_script(&PlaybackEvent::StateChanged(PlayerSnapshot {
            chapters: vec![PlayerChapter {
                title: "Opening".to_string(),
                start_ms: 30_000.0,
            }],
            diagnostics: PlaybackDiagnostics {
                buffered_until_ms: Some(60_000.0),
                buffering: true,
                dropped_frames: Some(2),
                frame_rate: Some(23.976),
            },
            ..PlayerSnapshot::default()
        }));
        let payload = script
            .split_once("StateChanged(")
            .and_then(|(_, call)| call.strip_suffix(");"))
            .expect("snapshot argument");
        let payload: serde_json::Value = serde_json::from_str(payload).expect("snapshot json");

        // The field names `PlayerState` in ui/src/lib/api.ts reads.
        let mut keys = payload
            .as_object()
            .expect("snapshot object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "active",
                "chapters",
                "diagnostics",
                "durationMs",
                "itemId",
                "mediaSourceId",
                "mute",
                "paused",
                "playMethod",
                "playSessionId",
                "playbackId",
                "positionMs",
                "skipSegments",
                "stopReason",
                "tracks",
                "volume",
            ]
        );
        assert_eq!(payload["chapters"][0]["title"], "Opening");
        assert_eq!(payload["diagnostics"]["bufferedUntilMs"], 60_000.0);
        assert_eq!(payload["diagnostics"]["buffering"], true);
        assert_eq!(payload["diagnostics"]["droppedFrames"], 2);
    }

    #[test]
    fn playback_cache_completion_script_carries_item_and_outcome() {
        let script = playback_cache_refresh_script(
            "item-after-a-slow-refresh",
            PlaybackCacheRefreshOutcome::Refreshed,
        );

        assert!(script.contains("__mediaFlickDesktopPlaybackCacheRefreshed"));
        assert!(script.contains("item-after-a-slow-refresh"));
        assert!(script.contains("refreshed"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn integrated_shell_warms_windowed_before_playback() {
        let settings = AppSettings {
            player_backend: Some(crate::preferences::PlayerBackend::Libmpv),
            default_fullscreen: FullscreenBehavior::Fullscreen,
            ..AppSettings::default()
        };

        assert_eq!(player_warmup_mode(&settings), FullscreenBehavior::Windowed);
    }
}
