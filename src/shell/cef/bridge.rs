use super::document::*;
use super::events::*;
use super::*;

/// The remaining native About and update dialogs talk to the shell over
/// `mediaflick-desktop://<action>` URLs. Settings-specific native operations
/// use the typed API and shell queue instead. Only first-party documents
/// holding this session's token may trigger these legacy actions.
pub(super) fn bridge_request_is_trusted(
    request_url: &str,
    browser: Option<&mut Browser>,
    frame: Option<&mut Frame>,
) -> bool {
    let document_url = browser
        .and_then(|browser| browser.main_frame())
        .map(|frame| CefString::from(&frame.url()).to_string())
        .or_else(|| frame.map(|frame| CefString::from(&frame.url()).to_string()))
        .unwrap_or_default();

    if !bridge_token_is_valid(request_url) {
        return false;
    }
    document_url.is_empty()
        || document_url.starts_with("data:")
        || document_url.starts_with("mediaflick-desktop://")
}

fn bridge_token_is_valid(request_url: &str) -> bool {
    let Some((_, query)) = request_url.split_once('?') else {
        return false;
    };
    let query = query.split('#').next().unwrap_or_default();
    urls::query_param(query, "token").is_some_and(|token| token == jellyfin_bridge::bridge_token())
}

pub(super) fn route_bridge_action(
    request_url: &str,
    browser: Option<&mut Browser>,
    frame: Option<&mut Frame>,
    state: &BrowserState,
) -> bool {
    use jellyfin_bridge::BridgeAction;

    let Some(action) = jellyfin_bridge::parse_bridge_action(request_url) else {
        return false;
    };
    match action {
        BridgeAction::About => show_about_dialog(browser, frame),
        BridgeAction::Exit => initiate_app_exit(browser, state),
        BridgeAction::DownloadUpdate(query) => start_update_download(query, state),
        BridgeAction::OpenUpdateRelease => open_update_release_page(),
    }
    true
}

/// Server administration is deliberately not rebuilt in the own UI; it opens in
/// the system browser instead.
pub(super) fn open_server_dashboard(state: &BrowserState) {
    let server_url = state
        .lock()
        .ok()
        .and_then(|state| state.settings.jellyfin_url.clone())
        .or_else(|| services::services().and_then(|services| services.session.server_url()));
    let Some(server_url) = server_url else {
        notify_error(
            state,
            "No server configured",
            "Sign in to a Jellyfin server first.",
        );
        return;
    };
    open_external_link(&format!(
        "{}/web/#/dashboard",
        server_url.trim_end_matches('/')
    ));
}

pub(super) fn open_external_link(url: &str) {
    if !is_safe_external_link(url) || !is_browser_openable_url(url) {
        return;
    }
    tracing::info!(target: "app", url, "opening link in default browser");
    if let Err(error) = open_url_in_default_browser(url) {
        tracing::warn!(target: "app", url, "failed to open link in default browser: {error}");
    }
}

fn open_update_release_page() {
    open_external_link(updater::GITHUB_LATEST_RELEASE_PAGE_URL);
}

fn is_safe_external_link(url: &str) -> bool {
    !url.is_empty() && !url.starts_with('-')
}

pub(super) fn is_browser_openable_url(url: &str) -> bool {
    url_scheme(url).is_some_and(|scheme| matches!(scheme.as_str(), "http" | "https" | "mailto"))
}

fn url_scheme(url: &str) -> Option<String> {
    let scheme = url.split_once(':')?.0;
    if scheme.is_empty()
        || !scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
    {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

#[cfg(target_os = "windows")]
fn open_url_in_default_browser(url: &str) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    let file: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut::<std::ffi::c_void>() as HWND,
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if (result as isize) <= 32 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn open_url_in_default_browser(url: &str) -> std::io::Result<()> {
    std::process::Command::new("open").arg(url).spawn()?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_url_in_default_browser(url: &str) -> std::io::Result<()> {
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    Ok(())
}

pub(super) fn toggle_browser_fullscreen(browser: Option<&mut Browser>) {
    let Some(mut browser) = browser.cloned() else {
        return;
    };
    let Some(browser_view) = browser_view_get_for_browser(Some(&mut browser)) else {
        return;
    };
    let Some(window) = browser_view.window() else {
        return;
    };
    let fullscreen = i32::from(window.is_fullscreen() == 0);
    window.set_fullscreen(fullscreen);
}

pub(super) fn show_about_dialog(browser: Option<&mut Browser>, frame: Option<&mut Frame>) {
    let script = about::dialog_script();
    let target_frame = browser
        .and_then(|browser| browser.main_frame())
        .or_else(|| frame.map(|frame| frame.clone()));
    if let Some(frame) = target_frame {
        frame.execute_java_script(
            Some(&CefString::from(script.as_str())),
            Some(&CefString::from("mediaflick-desktop://app-about")),
            1,
        );
    }
}

/// Kept as a bridge target for an older update-toast document, but it now
/// performs an in-app navigation rather than injecting a native modal.
pub(super) fn open_settings_page(browser: Option<&mut Browser>, frame: Option<&mut Frame>) {
    let target_frame = browser
        .and_then(|browser| browser.main_frame())
        .or_else(|| frame.map(|frame| frame.clone()));
    if let Some(frame) = target_frame {
        frame.load_url(Some(&CefString::from(
            "mediaflick-desktop://app/settings/client/player",
        )));
    }
}

pub(super) fn initiate_app_exit(browser: Option<&mut Browser>, state: &BrowserState) {
    tracing::info!(target: "app", "exit requested from Jellyfin Web user menu");

    let mut browsers = state
        .lock()
        .map(|mut state| {
            state.force_close_requested = true;
            state.browsers.clone()
        })
        .unwrap_or_default();
    if browsers.is_empty()
        && let Some(browser) = browser.cloned()
    {
        browsers.push(browser);
    }

    let mut close_requests = 0usize;
    for browser in browsers {
        if let Some(host) = browser.host() {
            host.close_browser(1);
            close_requests += 1;
        }
    }

    if close_requests == 0 {
        let playback = state.lock().ok().map(|state| state.playback.clone());
        if let Some(playback) = playback {
            playback.shutdown();
        }
        quit_message_loop();
    }
}

fn start_update_download(_query: &str, state: &BrowserState) {
    let release = match state.lock() {
        Ok(mut state) => {
            if state.update_download_started {
                tracing::debug!(target: "updater", "ignored duplicate update download request");
                return;
            }
            let Some(release) = state.update_available.clone() else {
                tracing::warn!(target: "updater", "ignored update download request without an available update");
                return;
            };
            if !release.automatic_install || release.asset.is_none() {
                tracing::debug!(target: "updater", "ignored update download request for a release without automatic installation");
                return;
            }
            state.update_download_started = true;
            release
        }
        Err(error) => {
            tracing::warn!(target: "updater", "failed to lock browser state for update download: {error}");
            return;
        }
    };

    tracing::info!(
        target: "updater",
        version = %release.version,
        asset = release.asset.as_ref().map(|asset| asset.name.as_str()).unwrap_or("none"),
        "starting update download"
    );
    dispatch_update_progress(
        state,
        "downloading",
        &json!({
            "downloaded": 0,
            "total": release.asset.as_ref().and_then(|asset| asset.size),
        }),
    );

    let state_for_thread = state.clone();
    thread::spawn(move || {
        let progress_state = state_for_thread.clone();
        let result = updater::download_update(&release, move |downloaded, total| {
            post_update_event(
                progress_state.clone(),
                UpdateEvent::DownloadProgress { downloaded, total },
            );
        });
        match result {
            Ok(path) => post_update_event(state_for_thread, UpdateEvent::DownloadReady(path)),
            Err(error) => {
                post_update_event(state_for_thread, UpdateEvent::Error(error.to_string()))
            }
        }
    });
}

fn start_mpv_download(state: &BrowserState, request_id: Option<String>) {
    if !mpv_setup::supported() {
        dispatch_mpv_setup(
            state,
            "error",
            &json!({ "message": "Automatic mpv download is only available on Windows." }),
        );
        if let Some(request_id) = request_id {
            dispatch_shell_event(
                state,
                "mpv-install-progress",
                json!({
                    "requestId": request_id,
                    "state": "failed",
                    "message": "Automatic mpv download is only available on Windows.",
                }),
            );
        }
        return;
    }

    let already_running = match state.lock() {
        Ok(mut state) => {
            if state.mpv_setup_started {
                true
            } else {
                state.mpv_setup_started = true;
                false
            }
        }
        Err(error) => {
            tracing::warn!(target: "mpv.setup", "failed to lock browser state for mpv download: {error}");
            return;
        }
    };
    if already_running {
        tracing::debug!(target: "mpv.setup", "ignored duplicate mpv download request");
        if let Some(request_id) = request_id.as_ref() {
            dispatch_shell_event(
                state,
                "mpv-install-progress",
                json!({
                    "requestId": request_id,
                    "state": "failed",
                    "message": "An mpv installation is already running.",
                }),
            );
        }
        return;
    }

    tracing::info!(target: "mpv.setup", "starting mpv download");
    dispatch_mpv_setup(
        state,
        "downloading",
        &json!({ "downloaded": 0, "total": null }),
    );

    let state_for_thread = state.clone();
    thread::spawn(move || {
        let progress_state = state_for_thread.clone();
        let progress_request_id = request_id.clone();
        let result = mpv_setup::download_and_install(move |phase| match phase {
            MpvSetupPhase::Downloading { downloaded, total } => post_mpv_setup_event(
                progress_state.clone(),
                MpvSetupEvent::Progress {
                    request_id: progress_request_id.clone(),
                    downloaded,
                    total,
                },
            ),
            MpvSetupPhase::Extracting => post_mpv_setup_event(
                progress_state.clone(),
                MpvSetupEvent::Extracting {
                    request_id: progress_request_id.clone(),
                },
            ),
        });
        match result {
            Ok(path) => {
                post_mpv_setup_event(state_for_thread, MpvSetupEvent::Ready { request_id, path })
            }
            Err(error) => post_mpv_setup_event(
                state_for_thread,
                MpvSetupEvent::Error {
                    request_id,
                    message: error.to_string(),
                },
            ),
        }
    });
}

pub(super) fn start_mpv_download_for_settings(state: &BrowserState, request_id: String) {
    start_mpv_download(state, Some(request_id));
}

wrap_run_file_dialog_callback! {
    struct SettingsFileDialogCallback {
        frame: Option<Frame>,
        request_id: String,
        target: ShellFilePickerTarget,
    }

    impl RunFileDialogCallback {
        fn on_file_dialog_dismissed(&self, file_paths: Option<&mut CefStringList>) {
            let Some(frame) = &self.frame else { return; };
            let path = file_paths
                .and_then(|paths| std::mem::take(paths).into_iter().next());
            dispatch_shell_event_to_frame(
                frame,
                "file-picker-completed",
                file_picker_completion_payload(
                    &self.request_id,
                    self.target,
                    path.as_deref(),
                    None,
                ),
            );
        }
    }
}

fn file_picker_target_id(target: ShellFilePickerTarget) -> &'static str {
    match target {
        ShellFilePickerTarget::Mpv => "mpv",
        ShellFilePickerTarget::Mpchc => "mpchc",
    }
}

fn file_picker_completion_payload(
    request_id: &str,
    target: ShellFilePickerTarget,
    path: Option<&str>,
    error: Option<&str>,
) -> serde_json::Value {
    json!({
        "requestId": request_id,
        "target": file_picker_target_id(target),
        "path": path,
        "error": error,
    })
}

pub(super) fn open_settings_file_dialog(
    state: &BrowserState,
    request_id: String,
    target: ShellFilePickerTarget,
) {
    let browser = state
        .lock()
        .ok()
        .and_then(|state| state.browsers.first().cloned());
    let Some(browser) = browser else {
        dispatch_shell_event(
            state,
            "file-picker-completed",
            file_picker_completion_payload(
                &request_id,
                target,
                None,
                Some("The browser is not ready."),
            ),
        );
        return;
    };
    let Some(host) = browser.host() else {
        dispatch_shell_event(
            state,
            "file-picker-completed",
            file_picker_completion_payload(
                &request_id,
                target,
                None,
                Some("The browser is unavailable."),
            ),
        );
        return;
    };
    let settings = services::services()
        .map(|services| services.preferences.snapshot())
        .unwrap_or_default();
    let (title, initial_path) = match target {
        ShellFilePickerTarget::Mpv => ("Select mpv executable", settings.mpv_path),
        ShellFilePickerTarget::Mpchc => ("Select MPC-HC executable", settings.mpchc_path),
    };
    let mut filters = CefStringList::new();
    #[cfg(target_os = "windows")]
    filters.append(".exe");
    let filters = if cfg!(target_os = "windows") {
        Some(&mut filters)
    } else {
        None
    };
    let Some(frame) = browser.main_frame() else {
        dispatch_shell_event(
            state,
            "file-picker-completed",
            file_picker_completion_payload(
                &request_id,
                target,
                None,
                Some("The settings page is not ready."),
            ),
        );
        return;
    };
    let default_path = initial_path.as_deref().map(CefString::from);
    let mut callback = SettingsFileDialogCallback::new(Some(frame), request_id, target);
    host.run_file_dialog(
        FileDialogMode::OPEN,
        Some(&CefString::from(title)),
        default_path.as_ref(),
        filters,
        Some(&mut callback),
    );
}

fn dispatch_shell_event_to_frame(frame: &Frame, kind: &str, payload: serde_json::Value) {
    let mut event = serde_json::Map::new();
    event.insert("type".to_string(), json!(kind));
    event.insert("payload".to_string(), payload);
    let event = serde_json::Value::Object(event);
    let script = format!(
        "window.dispatchEvent(new CustomEvent('mediaflick-desktop-shell', {{ detail: {} }}));",
        js_json(&event)
    );
    frame.execute_java_script(
        Some(&CefString::from(script.as_str())),
        Some(&CefString::from("mediaflick-desktop://shell-event")),
        1,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_scheme_lowercases_known_schemes() {
        assert_eq!(url_scheme("HTTPS://host").as_deref(), Some("https"));
        assert_eq!(
            url_scheme("MailTo:user@example.com").as_deref(),
            Some("mailto")
        );
    }

    #[test]
    fn url_scheme_rejects_invalid_input() {
        assert_eq!(url_scheme("no-scheme-here"), None);
        assert_eq!(url_scheme(":missing"), None);
        assert_eq!(url_scheme("has space:rest"), None);
    }

    #[test]
    fn browser_openable_allows_only_safe_schemes() {
        assert!(is_browser_openable_url("https://example.com"));
        assert!(is_browser_openable_url("http://example.com"));
        assert!(is_browser_openable_url("mailto:user@example.com"));
        assert!(!is_browser_openable_url("javascript:alert(1)"));
        assert!(!is_browser_openable_url("file:///etc/passwd"));
        assert!(!is_browser_openable_url("data:text/html,evil"));
    }

    #[test]
    fn safe_external_link_rejects_empty_and_flag_like() {
        assert!(is_safe_external_link("https://example.com"));
        assert!(!is_safe_external_link(""));
        assert!(!is_safe_external_link("--malicious-flag"));
    }

    #[test]
    fn file_picker_completion_keeps_cancellation_and_errors_correlatable() {
        let cancelled =
            file_picker_completion_payload("request-one", ShellFilePickerTarget::Mpv, None, None);
        assert_eq!(cancelled["requestId"], "request-one");
        assert_eq!(cancelled["target"], "mpv");
        assert!(cancelled["path"].is_null());
        assert!(cancelled["error"].is_null());

        let failed = file_picker_completion_payload(
            "request-two",
            ShellFilePickerTarget::Mpchc,
            None,
            Some("dialog failed"),
        );
        assert_eq!(failed["requestId"], "request-two");
        assert_eq!(failed["target"], "mpchc");
        assert_eq!(failed["error"], "dialog failed");
    }

    #[test]
    fn dialog_requests_must_carry_this_session_token() {
        let token = crate::jellyfin::bridge::bridge_token();
        let url = format!("mediaflick-desktop://update-download?token={token}&version=1");
        assert!(bridge_token_is_valid(&url));
        assert!(!bridge_token_is_valid(
            "mediaflick-desktop://update-download?token=deadbeef&version=1"
        ));
        assert!(!bridge_token_is_valid(
            "mediaflick-desktop://update-download?version=1"
        ));
        assert!(!bridge_token_is_valid("mediaflick-desktop://app-exit"));
    }
}
