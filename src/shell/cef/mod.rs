use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cef::*;
use serde_json::json;

use crate::app::paths::app_data_dir;
use crate::app::services::{self, ShellFilePickerTarget, ShellRequest};
use crate::app::{build_info, logger, urls};
use crate::jellyfin::api::items;
use crate::jellyfin::bridge as jellyfin_bridge;
use crate::jellyfin::playback_reporter::flush_playstate_reports;
use crate::maintenance::player_setup::{self as mpv_setup, MpvSetupPhase};
use crate::maintenance::updater::{self, UpdateRelease};
use crate::playback::{PlaybackCoordinator, PlaybackEvent};
use crate::players::build_backend;
use crate::preferences::{AppSettings, CloseBehavior, SettingsChange, WebUiWindowSettings};
use crate::shell::ui::{about, error_toast};
use crate::windows::set_window_icon;

pub mod api;
pub mod app_scheme;

/// Enough time to send the final Jellyfin playstate request before reading the
/// server's resolved user data back into the local cache.
const PLAYSTATE_CACHE_REFRESH_TIMEOUT: Duration = Duration::from_secs(11);

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub settings: AppSettings,
    pub title: String,
    pub remote_debugging_port: i32,
    pub hidden: bool,
}

pub fn run(config: &AppConfig) -> i32 {
    // CEF requires this API hash initialization before most other API calls.
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = args::Args::new();
    let Some(command_line) = args.as_cmd_line() else {
        eprintln!("Failed to parse CEF command line");
        return 1;
    };

    let type_switch = CefString::from("type");
    let is_browser_process = command_line.has_switch(Some(&type_switch)) != 1;
    let mut app = JellyfinApp::new(config.clone());

    if !is_browser_process {
        let exit_code = execute_process(
            Some(args.as_main_args()),
            Some(&mut app),
            std::ptr::null_mut(),
        );
        return exit_code.max(0);
    }

    let exit_code = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if exit_code >= 0 {
        return exit_code;
    }

    let paths = RuntimePaths::new();
    if let Err(error) = paths.create() {
        eprintln!("Failed to create CEF data directories: {error}");
        return 1;
    }

    let cache_path = paths.cache_dir.to_string_lossy();
    let log_file = paths.log_file.to_string_lossy();
    let product = format!("mediaflick-desktop/{}", env!("CARGO_PKG_VERSION"));
    let windowless_rendering_enabled = i32::from(prototype_osr::is_configured(&config.settings));
    let settings = Settings {
        no_sandbox: 1,
        browser_subprocess_path: cef_string_from_path(paths.browser_subprocess_path.as_ref()),
        cache_path: CefString::from(cache_path.as_ref()),
        root_cache_path: CefString::from(cache_path.as_ref()),
        persist_session_cookies: 1,
        user_agent_product: CefString::from(product.as_str()),
        locale: CefString::from("en-US"),
        log_file: CefString::from(log_file.as_ref()),
        log_severity: LogSeverity::INFO,
        resources_dir_path: cef_string_from_path(paths.resources_dir_path.as_ref()),
        locales_dir_path: cef_string_from_path(paths.locales_dir_path.as_ref()),
        framework_dir_path: cef_string_from_path(paths.framework_dir_path.as_ref()),
        remote_debugging_port: config.remote_debugging_port,
        disable_signal_handlers: 1,
        use_views_default_popup: 1,
        windowless_rendering_enabled,
        ..Default::default()
    };

    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) != 1
    {
        eprintln!("CEF initialization failed");
        return 1;
    }

    run_message_loop();
    shutdown();
    0
}

struct RuntimePaths {
    cache_dir: PathBuf,
    log_file: PathBuf,
    browser_subprocess_path: Option<PathBuf>,
    resources_dir_path: Option<PathBuf>,
    locales_dir_path: Option<PathBuf>,
    framework_dir_path: Option<PathBuf>,
}

impl RuntimePaths {
    fn new() -> Self {
        let base = app_data_dir();
        let browser_subprocess_path = current_exe_path();
        let app_dir = browser_subprocess_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(PathBuf::from);

        #[cfg(target_os = "macos")]
        let bundle_contents_dir = browser_subprocess_path
            .as_deref()
            .and_then(macos_bundle_contents_dir);

        #[cfg(target_os = "macos")]
        let resources_dir_path = bundle_contents_dir
            .as_ref()
            .map(|path| path.join("Resources"))
            .or_else(|| app_dir.clone());
        #[cfg(not(target_os = "macos"))]
        let resources_dir_path = app_dir;

        let locales_dir_path = resources_dir_path.as_ref().map(|path| path.join("locales"));

        #[cfg(target_os = "macos")]
        let framework_dir_path = bundle_contents_dir.map(|path| {
            path.join("Frameworks")
                .join("Chromium Embedded Framework.framework")
        });
        #[cfg(not(target_os = "macos"))]
        let framework_dir_path = None;

        Self {
            cache_dir: base.join("cef-cache"),
            log_file: base.join("cef.log"),
            browser_subprocess_path,
            resources_dir_path,
            locales_dir_path,
            framework_dir_path,
        }
    }

    fn create(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.cache_dir)?;
        if let Some(parent) = self.log_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn macos_bundle_contents_dir(exe_path: &std::path::Path) -> Option<PathBuf> {
    let macos_dir = exe_path.parent()?;
    if macos_dir.file_name().and_then(|name| name.to_str()) != Some("MacOS") {
        return None;
    }

    let contents_dir = macos_dir.parent()?;
    (contents_dir.file_name().and_then(|name| name.to_str()) == Some("Contents"))
        .then(|| contents_dir.to_path_buf())
}

fn current_exe_path() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?;
    Some(std::fs::canonicalize(&path).unwrap_or(path))
}

fn cef_string_from_path(path: Option<&PathBuf>) -> CefString {
    path.map(|path| CefString::from(path.to_string_lossy().as_ref()))
        .unwrap_or_default()
}

fn linux_desktop_id() -> CefString {
    // The generated CEF wrapper writes only borrowed string structs back to the
    // native callback output. Keep the allocated string alive and let CEF own
    // the raw value it receives for this one-shot window creation callback.
    let owned = CefString::from(build_info::APP_DESKTOP_ID);
    let borrowed = owned.clone();
    std::mem::forget(owned);
    borrowed
}

mod bridge;
mod document;
mod events;
mod handlers;
mod prototype_osr;
mod runtime;

use events::{
    start_playback_event_bridge, start_preferences_event_bridge, start_shell_request_bridge,
    start_update_check_bridge, warm_configured_player,
};
use runtime::JellyfinApp;

fn update_webui_window_from_window(state: Option<&BrowserState>, window: Option<&Window>) {
    let bounds = window.map(Window::bounds);
    update_webui_window_settings(state, window, bounds.as_ref());
}

fn update_webui_window_settings(
    state: Option<&BrowserState>,
    window: Option<&Window>,
    bounds: Option<&Rect>,
) {
    let Some(state) = state else {
        return;
    };
    let Some(bounds) = bounds else {
        return;
    };
    if window.is_some_and(|window| window.is_minimized() != 0 || window.is_fullscreen() != 0) {
        return;
    }
    let maximized = window.is_some_and(|window| window.is_maximized() != 0);
    match state.lock() {
        Ok(mut state) => {
            state.settings.webui_window.record_bounds(
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                maximized,
            );
        }
        Err(error) => {
            tracing::warn!(target: "config", "failed to update WebUI window settings: {error}");
        }
    }
}

fn save_webui_window_settings(state: Option<&BrowserState>) {
    let Some(state) = state else {
        return;
    };
    let settings = match state.lock() {
        Ok(state) => state.settings.clone(),
        Err(error) => {
            tracing::warn!(target: "config", "failed to read WebUI window settings: {error}");
            return;
        }
    };
    let Some(services) = services::services() else {
        return;
    };
    if let Err(error) = services.preferences.record_window(settings.webui_window) {
        tracing::warn!(target: "config", "failed to save mediaflick-desktop config on window close: {error}");
    }
}

fn should_minimize_instead_of_close(state: Option<&BrowserState>) -> bool {
    state
        .and_then(|state| state.lock().ok())
        .is_some_and(|state| {
            !state.force_close_requested
                && state.settings.close_behavior == CloseBehavior::MinimizeWindow
        })
}

struct BrowserStateInner {
    title: String,
    settings: AppSettings,
    browsers: Vec<Browser>,
    main_window: Option<Window>,
    initial_show_state: ShowState,
    main_window_reveal_requested: bool,
    main_window_revealed: bool,
    playback: Arc<PlaybackCoordinator>,
    playback_event_tx: mpsc::Sender<PlaybackEvent>,
    update_available: Option<UpdateRelease>,
    update_download_started: bool,
    mpv_setup_started: bool,
    force_close_requested: bool,
    player_warmed: bool,
}

type BrowserState = Arc<Mutex<BrowserStateInner>>;

fn register_main_window(state: &BrowserState, window: &Window) {
    let reveal_requested = state.lock().is_ok_and(|mut state| {
        state.main_window = Some(window.clone());
        state.main_window_reveal_requested
    });
    if reveal_requested {
        reveal_main_window(state);
    }
}

fn clear_main_window(state: &BrowserState) {
    if let Ok(mut state) = state.lock() {
        state.main_window = None;
    }
}

fn reveal_main_window(state: &BrowserState) {
    debug_assert_ne!(currently_on(ThreadId::UI), 0);

    let (window, show_state) = {
        let Ok(mut state) = state.lock() else {
            tracing::warn!(target: "shell", "failed to read main window state before reveal");
            return;
        };
        state.main_window_reveal_requested = true;
        if state.main_window_revealed || state.initial_show_state == ShowState::HIDDEN {
            return;
        }
        let Some(window) = state.main_window.clone() else {
            return;
        };
        state.main_window_revealed = true;
        (window, state.initial_show_state)
    };

    if show_state == ShowState::MAXIMIZED {
        window.maximize();
    }
    // Show alone displays the window at its current Z-order position without
    // making it active. Because the reveal happens after UI readiness, the
    // process no longer holds launch-time foreground rights, so the window
    // would stay behind whatever currently has focus. Activate brings it to
    // the front and gives it keyboard focus.
    window.show();
    window.activate();
}

fn new_browser_state(
    title: String,
    settings: AppSettings,
    initial_show_state: ShowState,
) -> BrowserState {
    let (playback_event_tx, playback_event_rx) = mpsc::channel();
    let playback = Arc::new(PlaybackCoordinator::new(build_backend(
        &settings,
        playback_event_tx.clone(),
    )));
    // The app-scheme API starts playback from a background thread, so it needs
    // the coordinator without reaching into this UI-thread state.
    if let Some(services) = services::services() {
        services.attach_playback(playback.clone());
    }
    let state = Arc::new(Mutex::new(BrowserStateInner {
        title,
        settings,
        browsers: Vec::new(),
        main_window: None,
        initial_show_state,
        main_window_reveal_requested: false,
        main_window_revealed: false,
        playback,
        playback_event_tx,
        update_available: None,
        update_download_started: false,
        mpv_setup_started: false,
        force_close_requested: false,
        player_warmed: false,
    }));
    start_playback_event_bridge(&state, playback_event_rx);
    start_preferences_event_bridge(&state);
    start_shell_request_bridge(&state);
    start_update_check_bridge(state.clone());
    state
}

fn prepare_player_for_window(state: &BrowserState) {
    let prepared = state.lock().ok().and_then(|mut state| {
        if state.player_warmed {
            return None;
        }
        state.player_warmed = true;
        Some((state.playback.clone(), state.settings.clone()))
    });
    let Some((playback, settings)) = prepared else {
        return;
    };
    warm_configured_player(&playback, &settings);
}

fn configured_player_native_window(
    state: &BrowserState,
    timeout: Duration,
) -> Option<crate::playback::NativeWindowHandle> {
    let playback = state.lock().ok().map(|state| state.playback.clone())?;
    playback.native_window(timeout)
}
