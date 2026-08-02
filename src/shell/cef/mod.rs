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
use crate::app::{logger, urls};
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

pub fn run(config: AppConfig) -> i32 {
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
        let resources_dir_path = app_dir.clone();

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

wrap_app! {
    pub struct JellyfinApp {
        config: AppConfig,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefStringUtf16>,
            command_line: Option<&mut CommandLine>,
        ) {
            let Some(command_line) = command_line else {
                return;
            };

            // Same spirit as upstream jellyfin-desktop: avoid Google background
            // services and permit media playback without a browser gesture.
            for switch in [
                "disable-background-networking",
                "disable-client-side-phishing-detection",
                "disable-component-update",
                "disable-default-apps",
                "disable-domain-reliability",
                "disable-extensions",
                "disable-notifications",
                "disable-pings",
                "disable-sync",
                "disable-translate",
                "no-first-run",
                "no-pings",
            ] {
                command_line.append_switch(Some(&CefString::from(switch)));
            }

            if !self.config.settings.show_scrollbars {
                command_line.append_switch(Some(&CefString::from("hide-scrollbars")));
            }

            for (name, value) in [
                ("autoplay-policy", "no-user-gesture-required"),
                ("password-store", "basic"),
            ] {
                command_line.append_switch_with_value(
                    Some(&CefString::from(name)),
                    Some(&CefString::from(value)),
                );
            }

            #[cfg(target_os = "windows")]
            {
                // In this windowed Views shell CEF 148 starts the separate GPU
                // process with GL disabled, which loops through STATUS_BREAKPOINT
                // exits. Keeping the GPU service in-process avoids that crash loop.
                command_line.append_switch(Some(&CefString::from("in-process-gpu")));
                command_line.append_switch_with_value(
                    Some(&CefString::from("use-angle")),
                    Some(&CefString::from("d3d11")),
                );
            }
        }

        fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
            let Some(registrar) = registrar else {
                return;
            };
            let scheme = CefString::from("mediaflick-desktop");
            let scheme_options = SchemeOptions::STANDARD.get_raw()
                | SchemeOptions::SECURE.get_raw()
                | SchemeOptions::CORS_ENABLED.get_raw()
                | SchemeOptions::FETCH_ENABLED.get_raw();
            registrar.add_custom_scheme(Some(&scheme), cef_i32(scheme_options));
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(JellyfinBrowserProcessHandler::new(
                RefCell::new(None),
                self.config.clone(),
            ))
        }
    }
}

wrap_browser_process_handler! {
    struct JellyfinBrowserProcessHandler {
        client: RefCell<Option<Client>>,
        config: AppConfig,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);

            // The own UI is the only UI: it is served from our own scheme and
            // talks to the library cache and the player through /api/*.
            app_scheme::register();
            services::init_with_settings(self.config.settings.clone());

            let handler_state = new_browser_state(
                self.config.title.clone(),
                services::services()
                    .map(|services| services.preferences.snapshot())
                    .unwrap_or_else(|| self.config.settings.clone()),
            );
            {
                let mut client = self.client.borrow_mut();
                *client = Some(JellyfinClient::new(handler_state.clone()));
            }

            let settings = BrowserSettings::default();
            let url = CefString::from(app_scheme::APP_URL);
            let runtime_style = RuntimeStyle::ALLOY;

            let mut client = self.default_client();
            let mut browser_delegate = JellyfinBrowserViewDelegate::new(runtime_style);
            let browser_view = browser_view_create(
                client.as_mut(),
                Some(&url),
                Some(&settings),
                None,
                None,
                Some(&mut browser_delegate),
            );

            let Some(browser_view) = browser_view else {
                eprintln!("Failed to create CEF BrowserView");
                quit_message_loop();
                return;
            };

            let show_state = if self.config.hidden {
                ShowState::HIDDEN
            } else if self.config.settings.webui_window.maximized {
                ShowState::MAXIMIZED
            } else {
                ShowState::NORMAL
            };
            let mut window_delegate = JellyfinWindowDelegate::new(
                RefCell::new(Some(browser_view)),
                runtime_style,
                show_state,
                self.config.title.clone(),
                self.config.settings.webui_window,
                Some(handler_state),
            );
            window_create_top_level(Some(&mut window_delegate));
        }

        fn default_client(&self) -> Option<Client> {
            self.client.borrow().clone()
        }
    }
}

wrap_browser_view_delegate! {
    struct JellyfinBrowserViewDelegate {
        runtime_style: RuntimeStyle,
    }

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        fn on_popup_browser_view_created(
            &self,
            _browser_view: Option<&mut BrowserView>,
                popup_browser_view: Option<&mut BrowserView>,
                _is_devtools: i32,
        ) -> i32 {
            let mut window_delegate = JellyfinWindowDelegate::new(
                RefCell::new(popup_browser_view.cloned()),
                self.runtime_style,
                ShowState::NORMAL,
                "MediaFlick Desktop".to_string(),
                WebUiWindowSettings::default(),
                None,
            );
            window_create_top_level(Some(&mut window_delegate));
            1
        }

        fn browser_runtime_style(&self) -> RuntimeStyle {
            self.runtime_style
        }
    }
}

wrap_window_delegate! {
    struct JellyfinWindowDelegate {
        browser_view: RefCell<Option<BrowserView>>,
        runtime_style: RuntimeStyle,
        initial_show_state: ShowState,
        title: String,
        window_settings: WebUiWindowSettings,
        state: Option<BrowserState>,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            let (width, height) = self.window_settings.size();
            Size {
                width,
                height,
            }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let Some(window) = window else {
                return;
            };
            window.set_title(Some(&CefString::from(self.title.as_str())));
            set_window_icon(window);

            let browser_view = self.browser_view.borrow();
            let Some(browser_view) = browser_view.as_ref() else {
                return;
            };

            let mut view = View::from(browser_view);
            window.add_child_view(Some(&mut view));

            if self.initial_show_state == ShowState::MAXIMIZED {
                window.maximize();
            }
            if self.initial_show_state != ShowState::HIDDEN {
                window.show();
            }
        }

        fn on_window_closing(&self, window: Option<&mut Window>) {
            update_webui_window_from_window(self.state.as_ref(), window);
            save_webui_window_settings(self.state.as_ref());
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            *self.browser_view.borrow_mut() = None;
        }

        fn on_window_bounds_changed(
            &self,
            window: Option<&mut Window>,
            new_bounds: Option<&Rect>,
        ) {
            update_webui_window_settings(self.state.as_ref(), window, new_bounds);
        }

        fn can_resize(&self, _window: Option<&mut Window>) -> i32 {
            1
        }

        fn can_maximize(&self, _window: Option<&mut Window>) -> i32 {
            1
        }

        fn can_minimize(&self, _window: Option<&mut Window>) -> i32 {
            1
        }

        fn can_close(&self, window: Option<&mut Window>) -> i32 {
            let mut window = window;
            if should_minimize_instead_of_close(self.state.as_ref()) {
                update_webui_window_from_window(self.state.as_ref(), window.as_deref_mut());
                save_webui_window_settings(self.state.as_ref());
                if let Some(window) = window {
                    window.minimize();
                }
                return 0;
            }

            let browser_view = self.browser_view.borrow();
            let Some(browser_view) = browser_view.as_ref() else {
                return 1;
            };
            let Some(browser) = browser_view.browser() else {
                return 1;
            };
            let Some(browser_host) = browser.host() else {
                return 1;
            };
            browser_host.try_close_browser()
        }

        fn initial_show_state(&self, _window: Option<&mut Window>) -> ShowState {
            self.initial_show_state
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            self.runtime_style
        }
    }
}

fn update_webui_window_from_window(state: Option<&BrowserState>, window: Option<&mut Window>) {
    let bounds = window.as_ref().map(|window| window.bounds());
    update_webui_window_settings(state, window, bounds.as_ref());
}

fn update_webui_window_settings(
    state: Option<&BrowserState>,
    window: Option<&mut Window>,
    bounds: Option<&Rect>,
) {
    let Some(state) = state else {
        return;
    };
    let Some(bounds) = bounds else {
        return;
    };
    let maximized = window
        .as_ref()
        .is_some_and(|window| window.is_maximized() != 0);
    match state.lock() {
        Ok(mut state) => {
            state
                .settings
                .webui_window
                .record_bounds(bounds.width, bounds.height, maximized);
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
    playback: Arc<PlaybackCoordinator>,
    playback_event_tx: mpsc::Sender<PlaybackEvent>,
    update_available: Option<UpdateRelease>,
    update_download_started: bool,
    mpv_setup_started: bool,
    force_close_requested: bool,
}

type BrowserState = Arc<Mutex<BrowserStateInner>>;

fn new_browser_state(title: String, settings: AppSettings) -> BrowserState {
    let (playback_event_tx, playback_event_rx) = mpsc::channel();
    let playback = Arc::new(PlaybackCoordinator::new(build_backend(
        &settings,
        playback_event_tx.clone(),
    )));
    warm_configured_player(&playback, &settings);
    // The app-scheme API starts playback from a background thread, so it needs
    // the coordinator without reaching into this UI-thread state.
    if let Some(services) = services::services() {
        services.attach_playback(playback.clone());
    }
    let state = Arc::new(Mutex::new(BrowserStateInner {
        title,
        settings,
        browsers: Vec::new(),
        playback,
        playback_event_tx,
        update_available: None,
        update_download_started: false,
        mpv_setup_started: false,
        force_close_requested: false,
    }));
    start_playback_event_bridge(state.clone(), playback_event_rx);
    start_preferences_event_bridge(state.clone());
    start_shell_request_bridge(state.clone());
    start_update_check_bridge(state.clone());
    state
}

fn warm_configured_player(playback: &PlaybackCoordinator, settings: &AppSettings) {
    playback.configure_segments(settings.segment_skip_config());
    let Some(path) = settings.player_path() else {
        tracing::debug!(target: "mpv.ipc", "skipped player warmup because no executable is configured");
        return;
    };
    playback.warm(path.to_string(), settings.default_fullscreen);
}

fn start_playback_event_bridge(state: BrowserState, rx: Receiver<PlaybackEvent>) {
    let state = Arc::downgrade(&state);
    thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            let Some(state) = state.upgrade() else {
                break;
            };
            let mut task = PlaybackEventTask::new(state, event);
            if post_task(ThreadId::UI, Some(&mut task)) == 0 {
                tracing::warn!(target: "bridge", "failed to post playback event to CEF UI thread");
            }
        }
    });
}

/// Settings are persisted from an app-scheme background thread.  CEF's player
/// and frame APIs are UI-thread-only, so relay its apply plan before touching
/// either of them.
fn start_preferences_event_bridge(state: BrowserState) {
    let Some(services) = services::services() else {
        return;
    };
    let receiver = services.preferences.subscribe();
    let state = Arc::downgrade(&state);
    thread::spawn(move || {
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

fn start_shell_request_bridge(state: BrowserState) {
    let Some(services) = services::services() else {
        return;
    };
    let receiver = services.shell.subscribe();
    let state = Arc::downgrade(&state);
    thread::spawn(move || {
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

fn start_update_check_bridge(state: BrowserState) {
    thread::spawn(move || match updater::check_for_update() {
        Ok(Some(release)) => post_update_event(state, UpdateEvent::Available(release)),
        Ok(None) => tracing::debug!(target: "updater", "no supported update available"),
        Err(error) => tracing::warn!(target: "updater", "failed to check for updates: {error}"),
    });
}

fn post_update_event(state: BrowserState, event: UpdateEvent) {
    let mut task = UpdateEventTask::new(state, event);
    if post_task(ThreadId::UI, Some(&mut task)) == 0 {
        tracing::warn!(target: "updater", "failed to post update event to CEF UI thread");
    }
}

fn post_mpv_setup_event(state: BrowserState, event: MpvSetupEvent) {
    let mut task = MpvSetupEventTask::new(state, event);
    if post_task(ThreadId::UI, Some(&mut task)) == 0 {
        tracing::warn!(target: "mpv.setup", "failed to post mpv setup event to CEF UI thread");
    }
}

#[derive(Debug, Clone)]
enum UpdateEvent {
    Available(UpdateRelease),
    DownloadProgress { downloaded: u64, total: Option<u64> },
    DownloadReady(PathBuf),
    Error(String),
}

#[derive(Debug, Clone)]
enum MpvSetupEvent {
    Progress {
        request_id: Option<String>,
        downloaded: u64,
        total: Option<u64>,
    },
    Extracting {
        request_id: Option<String>,
    },
    Ready {
        request_id: Option<String>,
        path: PathBuf,
    },
    Error {
        request_id: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybackCacheRefreshOutcome {
    Refreshed,
    Deferred,
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
    }

    impl Task {
        fn execute(&self) {
            dispatch_playback_event(&self.state, self.event.clone());
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
            apply_preference_change(&self.state, self.change.clone());
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
        browser: Option<Browser>,
        frame: Option<Frame>,
        state: BrowserState,
    }

    impl Task {
        fn execute(&self) {
            if !route_bridge_action(
                &self.request_url,
                self.browser.clone().as_mut(),
                self.frame.clone().as_mut(),
                &self.state,
            ) {
                tracing::warn!(
                    target: "bridge",
                    url = %logger::redact_url_secrets(&self.request_url),
                    "ignored unrecognized bridge resource request"
                );
            }
        }
    }
}

fn post_bridge_action(
    request_url: String,
    browser: Option<Browser>,
    frame: Option<Frame>,
    state: BrowserState,
) {
    let mut task = BridgeActionTask::new(request_url, browser, frame, state);
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

fn dispatch_playback_event(state: &BrowserState, event: PlaybackEvent) {
    if let PlaybackEvent::Failed { message } = &event {
        tracing::warn!(target: "bridge", message, "player backend reported a playback failure");
        dispatch_error_toast(state, "Playback error", message);
        return;
    }

    if let PlaybackEvent::Stopped(snapshot) = &event {
        mirror_playback_progress(state, snapshot);
    }

    let browsers = state
        .lock()
        .map(|state| state.browsers.clone())
        .unwrap_or_default();
    if browsers.is_empty() {
        tracing::debug!(
            target: "bridge",
            ?event,
            "skipped playback event dispatch because no WebUI browsers are registered"
        );
        return;
    }

    let script = playback_event_script(&event);
    let browser_count = browsers.len();
    let mut frame_count = 0usize;
    for browser in browsers {
        if let Some(frame) = browser.main_frame() {
            frame_count += 1;
            frame.execute_java_script(
                Some(&CefString::from(script.as_str())),
                Some(&CefString::from("mediaflick-desktop://playback-event")),
                1,
            );
        }
    }
    tracing::debug!(
        target: "bridge",
        ?event,
        browser_count,
        frame_count,
        "dispatched playback event to WebUI"
    );
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
                    let (client, user_id) = services.session.client_and_user()?;
                    items::fetch_item(&client, &user_id, &item_id)
                })();
                match result {
                    Ok(Some(item)) => {
                        if let Err(error) = services.library.upsert_page(&[item]) {
                            tracing::warn!(
                                target: "library.db",
                                item_id,
                                "failed to cache Jellyfin's resolved playback state: {error}"
                            );
                            services.sync.request();
                            PlaybackCacheRefreshOutcome::Deferred
                        } else {
                            PlaybackCacheRefreshOutcome::Refreshed
                        }
                    }
                    Ok(None) => {
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
    let browsers = state
        .lock()
        .map(|state| state.browsers.clone())
        .unwrap_or_default();
    let script = playback_cache_refresh_script(item_id, outcome);
    for browser in browsers {
        if let Some(frame) = browser.main_frame() {
            frame.execute_java_script(
                Some(&CefString::from(script.as_str())),
                Some(&CefString::from(
                    "mediaflick-desktop://playback-cache-refreshed",
                )),
                1,
            );
        }
    }
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
        PlaybackEvent::Stopped(snapshot) => {
            let payload = json!({
                "active": snapshot.active,
                "playbackId": snapshot.playback_id,
                "itemId": snapshot.item_id,
                "mediaSourceId": snapshot.media_source_id,
                "playSessionId": snapshot.play_session_id,
                "positionMs": snapshot.position_ms,
                "durationMs": snapshot.duration_ms,
                "paused": snapshot.paused,
                "volume": snapshot.volume,
                "mute": snapshot.mute,
                "stopReason": snapshot.stop_reason,
            });
            format!(
                "window.__mediaFlickDesktopPlaybackStopped&&window.__mediaFlickDesktopPlaybackStopped({});",
                js_json(&payload)
            )
        }
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
                json!({ "downloaded": downloaded, "total": total }),
            );
        }
        UpdateEvent::DownloadReady(path) => {
            dispatch_update_progress(state, "installing", json!({ "downloaded": 1, "total": 1 }));
            match updater::start_installer(&path) {
                Ok(()) => initiate_app_exit(None, state),
                Err(error) => {
                    if let Ok(mut state) = state.lock() {
                        state.update_download_started = false;
                    }
                    dispatch_update_progress(
                        state,
                        "error",
                        json!({ "message": error.to_string() }),
                    );
                }
            }
        }
        UpdateEvent::Error(message) => {
            tracing::warn!(target: "updater", "update failed: {message}");
            if let Ok(mut state) = state.lock() {
                state.update_download_started = false;
            }
            dispatch_update_progress(state, "error", json!({ "message": message }));
        }
    }
}

fn dispatch_update_available(state: &BrowserState, release: &UpdateRelease) {
    let browsers = state
        .lock()
        .map(|state| state.browsers.clone())
        .unwrap_or_default();
    let script = updater::update_available_script(release);
    for browser in browsers {
        if let Some(frame) = browser.main_frame() {
            execute_update_script(&frame, &script);
        }
    }
}

fn dispatch_update_progress(state: &BrowserState, status: &str, payload: serde_json::Value) {
    let browsers = state
        .lock()
        .map(|state| state.browsers.clone())
        .unwrap_or_default();
    let script = updater::update_progress_script(status, payload);
    for browser in browsers {
        if let Some(frame) = browser.main_frame() {
            execute_update_script(&frame, &script);
        }
    }
}

fn handle_mpv_setup_event(state: &BrowserState, event: MpvSetupEvent) {
    match event {
        MpvSetupEvent::Progress {
            request_id,
            downloaded,
            total,
        } => {
            dispatch_mpv_setup(
                state,
                "downloading",
                json!({ "downloaded": downloaded, "total": total }),
            );
            if let Some(request_id) = request_id {
                dispatch_shell_event(
                    state,
                    "mpv-install-progress",
                    json!({
                        "requestId": request_id, "state": "downloading", "downloaded": downloaded, "total": total,
                    }),
                );
            }
        }
        MpvSetupEvent::Extracting { request_id } => {
            dispatch_mpv_setup(state, "extracting", json!({}));
            if let Some(request_id) = request_id {
                dispatch_shell_event(
                    state,
                    "mpv-install-progress",
                    json!({
                        "requestId": request_id, "state": "extracting",
                    }),
                );
            }
        }
        MpvSetupEvent::Ready { request_id, path } => {
            let mpv_path = path.to_string_lossy().into_owned();
            tracing::info!(target: "mpv.setup", path = %mpv_path, "mpv installed");
            if let Ok(mut state) = state.lock() {
                state.mpv_setup_started = false;
            }
            let save_result: Result<(), String> = match services::services() {
                Some(services) => services
                    .preferences
                    .set_mpv_path(mpv_path.clone())
                    .map(|_| ())
                    .map_err(|error| error.to_string()),
                None => Err("preferences service is unavailable".to_string()),
            };
            match save_result {
                Ok(()) => {
                    dispatch_mpv_setup(state, "done", json!({ "path": mpv_path }));
                    if let Some(request_id) = request_id {
                        dispatch_shell_event(
                            state,
                            "mpv-install-progress",
                            json!({
                                "requestId": request_id, "state": "completed", "path": mpv_path,
                            }),
                        );
                    }
                }
                Err(message) => {
                    tracing::warn!(target: "mpv.setup", "failed to save installed mpv path: {message}");
                    dispatch_mpv_setup(state, "error", json!({ "message": message }));
                    if let Some(request_id) = request_id {
                        dispatch_shell_event(
                            state,
                            "mpv-install-progress",
                            json!({
                                "requestId": request_id, "state": "failed", "message": message,
                            }),
                        );
                    }
                }
            }
        }
        MpvSetupEvent::Error {
            request_id,
            message,
        } => {
            tracing::warn!(target: "mpv.setup", "mpv setup failed: {message}");
            if let Ok(mut state) = state.lock() {
                state.mpv_setup_started = false;
            }
            dispatch_mpv_setup(state, "error", json!({ "message": message }));
            if let Some(request_id) = request_id {
                dispatch_shell_event(
                    state,
                    "mpv-install-progress",
                    json!({
                        "requestId": request_id, "state": "failed", "message": message,
                    }),
                );
            }
        }
    }
}

fn dispatch_mpv_setup(state: &BrowserState, status: &str, payload: serde_json::Value) {
    let browsers = state
        .lock()
        .map(|state| state.browsers.clone())
        .unwrap_or_default();
    let script = mpv_setup::setup_script(status, payload);
    for browser in browsers {
        if let Some(frame) = browser.main_frame() {
            execute_mpv_setup_script(&frame, &script);
        }
    }
}

fn apply_preference_change(state: &BrowserState, change: SettingsChange) {
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
    } else {
        if change.plan.update_input_bindings {
            playback.refresh_input_bindings();
        }
        if change.plan.update_segment_policy {
            playback.configure_segments(change.settings.segment_skip_config());
        }
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
        ShellRequest::FilePicker { request_id, target } => {
            open_settings_file_dialog(state, request_id, target);
        }
        ShellRequest::InstallMpv { request_id } => {
            start_mpv_download_for_settings(state, request_id)
        }
        ShellRequest::MetadataRepaired { item_ids } => {
            dispatch_shell_event(
                state,
                "library-metadata-repaired",
                json!({ "itemIds": item_ids }),
            );
        }
        ShellRequest::SessionExpired => {
            dispatch_shell_event(state, "jellyfin-session-expired", json!({}));
        }
    }
}

fn dispatch_shell_event(state: &BrowserState, kind: &str, payload: serde_json::Value) {
    let browsers = state
        .lock()
        .map(|state| state.browsers.clone())
        .unwrap_or_default();
    let event = json!({ "type": kind, "payload": payload });
    let script = format!(
        "window.dispatchEvent(new CustomEvent('mediaflick-desktop-shell', {{ detail: {} }}));",
        js_json(&event)
    );
    for browser in browsers {
        if let Some(frame) = browser.main_frame() {
            frame.execute_java_script(
                Some(&CefString::from(script.as_str())),
                Some(&CefString::from("mediaflick-desktop://shell-event")),
                1,
            );
        }
    }
}

fn show_pending_update_to_frame(frame: &Frame, state: &BrowserState) {
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

fn apply_scrollbar_settings_to_frame(frame: &Frame, state: &BrowserState) {
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

fn execute_update_script(frame: &Frame, script: &str) {
    frame.execute_java_script(
        Some(&CefString::from(script)),
        Some(&CefString::from("mediaflick-desktop://update-toast")),
        1,
    );
}

fn notify_error(state: &BrowserState, title: &str, body: &str) {
    let mut task = ErrorToastTask::new(state.clone(), title.to_string(), body.to_string());
    if post_task(ThreadId::UI, Some(&mut task)) == 0 {
        tracing::warn!(target: "bridge", title, "failed to post error toast to CEF UI thread");
    }
}

fn dispatch_error_toast(state: &BrowserState, title: &str, body: &str) {
    let browsers = state
        .lock()
        .map(|state| state.browsers.clone())
        .unwrap_or_default();
    if browsers.is_empty() {
        tracing::warn!(
            target: "bridge",
            title,
            "skipped error toast because no WebUI browsers are registered"
        );
        return;
    }
    let script = error_toast::error_toast_script(title, body);
    for browser in browsers {
        if let Some(frame) = browser.main_frame() {
            execute_error_script(&frame, &script);
        }
    }
}

fn execute_error_script(frame: &Frame, script: &str) {
    frame.execute_java_script(
        Some(&CefString::from(script)),
        Some(&CefString::from("mediaflick-desktop://error-toast")),
        1,
    );
}

fn execute_mpv_setup_script(frame: &Frame, script: &str) {
    frame.execute_java_script(
        Some(&CefString::from(script)),
        Some(&CefString::from("mediaflick-desktop://mpv-setup")),
        1,
    );
}

wrap_client! {
    struct JellyfinClient {
        state: BrowserState,
    }

    impl Client {
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(JellyfinContextMenuHandler::new(self.state.clone()))
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(JellyfinDisplayHandler::new(self.state.clone()))
        }

        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(JellyfinKeyboardHandler::new())
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(JellyfinLifeSpanHandler::new(self.state.clone()))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(JellyfinLoadHandler::new(self.state.clone()))
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(JellyfinRequestHandler::new(self.state.clone()))
        }
    }
}

const MENU_ID_FULLSCREEN: i32 = sys::cef_menu_id_t::MENU_ID_USER_FIRST as i32;
const MENU_ID_CLIENT_SETTINGS: i32 = MENU_ID_FULLSCREEN + 1;
const MENU_ID_DASHBOARD: i32 = MENU_ID_CLIENT_SETTINGS + 1;
const MENU_ID_ABOUT: i32 = MENU_ID_DASHBOARD + 1;

fn cef_i32<T>(value: T) -> i32
where
    T: TryInto<i32>,
    T::Error: std::fmt::Debug,
{
    value.try_into().expect("CEF enum value fits in i32")
}

fn remove_trailing_separator(model: &MenuModel) {
    let count = model.count();
    if count > 0 && model.type_at(count - 1) == MenuItemType::SEPARATOR {
        model.remove_at(count - 1);
    }
}

wrap_context_menu_handler! {
    struct JellyfinContextMenuHandler {
        state: BrowserState,
    }

    impl ContextMenuHandler {
        fn on_before_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            let Some(model) = model else {
                return;
            };
            model.remove(cef_i32(MenuId::PRINT.get_raw()));
            model.remove(cef_i32(MenuId::VIEW_SOURCE.get_raw()));
            remove_trailing_separator(model);
            if model.count() > 0 {
                model.add_separator();
            }
            model.add_item(MENU_ID_FULLSCREEN, Some(&CefString::from("Fullscreen")));
            model.add_item(MENU_ID_CLIENT_SETTINGS, Some(&CefString::from("Settings")));
            model.add_item(
                MENU_ID_DASHBOARD,
                Some(&CefString::from("Open Jellyfin dashboard")),
            );
            model.add_item(MENU_ID_ABOUT, Some(&CefString::from("About")));
        }

        fn on_context_menu_command(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            command_id: i32,
            _event_flags: EventFlags,
        ) -> i32 {
            match command_id {
                MENU_ID_FULLSCREEN => toggle_browser_fullscreen(browser),
                MENU_ID_CLIENT_SETTINGS => open_settings_page(browser, frame),
                MENU_ID_DASHBOARD => open_server_dashboard(&self.state),
                MENU_ID_ABOUT => show_about_dialog(browser, frame),
                _ => return 0,
            }
            1
        }
    }
}

wrap_keyboard_handler! {
    struct JellyfinKeyboardHandler;

    impl KeyboardHandler {
        #[cfg(target_os = "windows")]
        fn on_pre_key_event(
            &self,
            browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            _os_event: Option<&mut sys::MSG>,
            _is_keyboard_shortcut: Option<&mut i32>,
        ) -> i32 {
            handle_pre_key_event(browser, event)
        }

        #[cfg(target_os = "linux")]
        fn on_pre_key_event(
            &self,
            browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            _os_event: Option<&mut sys::XEvent>,
            _is_keyboard_shortcut: Option<&mut i32>,
        ) -> i32 {
            handle_pre_key_event(browser, event)
        }

        #[cfg(target_os = "macos")]
        fn on_pre_key_event(
            &self,
            browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            _os_event: *mut u8,
            _is_keyboard_shortcut: Option<&mut i32>,
        ) -> i32 {
            handle_pre_key_event(browser, event)
        }
    }
}

const VK_F11: i32 = 0x7A;

fn handle_pre_key_event(browser: Option<&mut Browser>, event: Option<&KeyEvent>) -> i32 {
    let Some(event) = event else {
        return 0;
    };
    if event.windows_key_code == VK_F11 && is_key_down_event(event) {
        toggle_browser_fullscreen(browser);
        return 1;
    }
    0
}

fn is_key_down_event(event: &KeyEvent) -> bool {
    let event_type = event.type_.get_raw();
    event_type == KeyEventType::RAWKEYDOWN.get_raw()
        || event_type == KeyEventType::KEYDOWN.get_raw()
}

wrap_display_handler! {
    struct JellyfinDisplayHandler {
        state: BrowserState,
    }

    impl DisplayHandler {
        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            let fallback_title = self
                .state
                .lock()
                .map(|state| state.title.clone())
                .unwrap_or_else(|_| "MediaFlick Desktop".to_string());
            let title_string = title
                .map(CefString::to_string)
                .filter(|value| !value.is_empty())
                .unwrap_or(fallback_title);
            let title = CefString::from(title_string.as_str());

            let mut browser = browser.cloned();
            if let Some(browser_view) = browser_view_get_for_browser(browser.as_mut())
                && let Some(window) = browser_view.window()
            {
                window.set_title(Some(&title));
            }
        }
    }
}

wrap_life_span_handler! {
    struct JellyfinLifeSpanHandler {
        state: BrowserState,
    }

    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut std::os::raw::c_int>,
        ) -> std::os::raw::c_int {
            let url = target_url.map(CefString::to_string).unwrap_or_default();
            open_external_link(&url);
            1
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser.cloned() else {
                return;
            };
            let pending_update = if let Ok(mut state) = self.state.lock() {
                state.browsers.push(browser);
                state.update_available.clone()
            } else {
                None
            };
            if let Some(release) = pending_update {
                dispatch_update_available(&self.state, &release);
            }
        }

        fn do_close(&self, _browser: Option<&mut Browser>) -> i32 {
            0
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            let Some(mut closing_browser) = browser.cloned() else {
                quit_message_loop();
                return;
            };

            let should_quit = if let Ok(mut state) = self.state.lock() {
                if let Some(index) = state
                    .browsers
                    .iter()
                    .position(|browser| browser.is_same(Some(&mut closing_browser)) != 0)
                {
                    state.browsers.remove(index);
                }
                state.browsers.is_empty()
            } else {
                true
            };

            if should_quit {
                let playback = self
                    .state
                    .lock()
                    .ok()
                    .map(|state| state.playback.clone());
                if let Some(playback) = playback {
                    playback.shutdown();
                }
                if let Some(services) = services::services() {
                    services.sync.stop();
                }
                quit_message_loop();
            }
        }
    }
}

wrap_load_handler! {
    struct JellyfinLoadHandler {
        state: BrowserState,
    }

    impl LoadHandler {
        fn on_load_end(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _http_status_code: i32,
        ) {
            let Some(frame) = frame else {
                return;
            };
            if frame.is_main() == 0 {
                return;
            }
            apply_scrollbar_settings_to_frame(frame, &self.state);
            show_pending_update_to_frame(frame, &self.state);
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            let Some(frame) = frame else {
                return;
            };
            if frame.is_main() == 0 {
                return;
            }

            let raw_error = sys::cef_errorcode_t::from(error_code);
            if raw_error == sys::cef_errorcode_t::ERR_ABORTED {
                return;
            }

            let title = self
                .state
                .lock()
                .map(|state| state.title.clone())
                .unwrap_or_else(|_| "MediaFlick Desktop".to_string());
            let html = load_error_html(
                &title,
                &failed_url.map(CefString::to_string).unwrap_or_default(),
                &error_text.map(CefString::to_string).unwrap_or_default(),
                raw_error as i32,
            );
            let uri = data_uri(html.as_bytes(), "text/html");
            frame.load_url(Some(&CefString::from(uri.as_str())));
        }
    }
}

wrap_request_handler! {
    struct JellyfinRequestHandler {
        state: BrowserState,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            user_gesture: i32,
            _is_redirect: i32,
        ) -> i32 {
            let Some(request) = request else {
                return 0;
            };
            let request_url = CefString::from(&request.url()).to_string();
            let mut browser = browser;
            let mut frame = frame;
            if request_url.starts_with("https://www.youtube-nocookie.com/embed/") {
                request.set_referrer(
                    Some(&CefString::from("http://localhost/")),
                    ReferrerPolicy::NEVER_CLEAR_REFERRER,
                );
                return 0;
            }
            // Our own UI is served by the app-scheme handler; let it through.
            if app_scheme::is_app_url(&request_url) {
                return 0;
            }
            if !request_url.starts_with("mediaflick-desktop://") {
                // Nothing outside the app bundle is ever rendered in-window.
                if user_gesture != 0 && is_browser_openable_url(&request_url) {
                    open_external_link(&request_url);
                    return 1;
                }
                return 0;
            }

            if !bridge_request_is_trusted(
                &request_url,
                browser.as_deref_mut(),
                frame.as_deref_mut(),
            ) {
                tracing::warn!(
                    target: "bridge",
                    url = %request_url,
                    "rejected bridge navigation from untrusted frame"
                );
                return 1;
            }
            if !route_bridge_action(&request_url, browser, frame, &self.state) {
                tracing::warn!(
                    target: "bridge",
                    url = %request_url,
                    "ignored unrecognized bridge navigation"
                );
            }
            1
        }

        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: i32,
            _is_download: i32,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut i32>,
        ) -> Option<ResourceRequestHandler> {
            Some(JellyfinResourceRequestHandler::new(self.state.clone()))
        }
    }
}

wrap_resource_request_handler! {
    struct JellyfinResourceRequestHandler {
        state: BrowserState,
    }

    impl ResourceRequestHandler {
        fn on_before_resource_load(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            let Some(request) = request else {
                return ReturnValue::CONTINUE;
            };

            let request_url = CefString::from(&request.url()).to_string();
            // The app scheme is served by our resource handler, not here.
            if app_scheme::is_app_url(&request_url) {
                return ReturnValue::CONTINUE;
            }
            // YouTube rejects embeds from custom schemes with player error 153
            // because Chromium has no HTTP origin to send as the referrer.
            // This one validated frame is the app's only remote embed; give its
            // initial request a conventional local-app HTTP origin so YouTube
            // receives the client identity it requires. Nested player requests
            // then carry their normal web referrer.
            if request_url.starts_with("https://www.youtube-nocookie.com/embed/") {
                request.set_referrer(
                    Some(&CefString::from("http://localhost/")),
                    ReferrerPolicy::NEVER_CLEAR_REFERRER,
                );
                return ReturnValue::CONTINUE;
            }
            if !request_url.starts_with("mediaflick-desktop://") {
                return ReturnValue::CONTINUE;
            }

            let mut browser = browser;
            let mut frame = frame;
            if bridge_request_is_trusted(
                &request_url,
                browser.as_deref_mut(),
                frame.as_deref_mut(),
            ) {
                post_bridge_action(
                    request_url.clone(),
                    browser.as_deref().cloned(),
                    frame.as_deref().cloned(),
                    self.state.clone(),
                );
            } else {
                tracing::warn!(
                    target: "bridge",
                    url = %logger::redact_url_secrets(&request_url),
                    "rejected native dialog request from untrusted frame"
                );
            }
            ReturnValue::CANCEL
        }
    }
}

/// The remaining native About and update dialogs talk to the shell over
/// `mediaflick-desktop://<action>` URLs. Settings-specific native operations
/// use the typed API and shell queue instead. Only first-party documents
/// holding this session's token may trigger these legacy actions.
fn bridge_request_is_trusted(
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

fn route_bridge_action(
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
fn open_server_dashboard(state: &BrowserState) {
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

fn open_external_link(url: &str) {
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

fn is_browser_openable_url(url: &str) -> bool {
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

fn toggle_browser_fullscreen(browser: Option<&mut Browser>) {
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

fn show_about_dialog(browser: Option<&mut Browser>, frame: Option<&mut Frame>) {
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
fn open_settings_page(browser: Option<&mut Browser>, frame: Option<&mut Frame>) {
    let target_frame = browser
        .and_then(|browser| browser.main_frame())
        .or_else(|| frame.map(|frame| frame.clone()));
    if let Some(frame) = target_frame {
        frame.load_url(Some(&CefString::from(
            "mediaflick-desktop://app/settings/client/player",
        )));
    }
}

fn initiate_app_exit(browser: Option<&mut Browser>, state: &BrowserState) {
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
        json!({
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
            json!({ "message": "Automatic mpv download is only available on Windows." }),
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
        json!({ "downloaded": 0, "total": null }),
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

fn start_mpv_download_for_settings(state: &BrowserState, request_id: String) {
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
                    path,
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
    path: Option<String>,
    error: Option<&str>,
) -> serde_json::Value {
    json!({
        "requestId": request_id,
        "target": file_picker_target_id(target),
        "path": path,
        "error": error,
    })
}

fn open_settings_file_dialog(
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
    let event = json!({ "type": kind, "payload": payload });
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

fn js_json(value: &serde_json::Value) -> String {
    escape_js_line_separators(&value.to_string())
}

fn escape_js_line_separators(json: &str) -> String {
    if json.contains('\u{2028}') || json.contains('\u{2029}') {
        json.replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029")
    } else {
        json.to_string()
    }
}

fn load_error_html(title: &str, failed_url: &str, error_text: &str, error_code: i32) -> String {
    include_str!("../ui/load_error.html")
        .replace("{{title}}", &html_escape(title))
        .replace("{{failed_url}}", &html_escape(failed_url))
        .replace("{{error_text}}", &html_escape(error_text))
        .replace("{{error_code}}", &error_code.to_string())
}

fn data_uri(data: &[u8], mime_type: &str) -> String {
    let data = CefString::from(&base64_encode(Some(data)));
    let uri = CefString::from(&uriencode(Some(&data), 0)).to_string();
    format!("data:{mime_type};base64,{uri}")
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::{
        PlaybackCacheRefreshOutcome, ShellFilePickerTarget,
        apply_settings_snapshot_preserving_live_window, bridge_token_is_valid,
        escape_js_line_separators, file_picker_completion_payload, html_escape,
        is_browser_openable_url, is_safe_external_link, playback_cache_refresh_script, url_scheme,
    };
    use crate::preferences::{AppSettings, AppearanceTheme, WebUiWindowSettings};

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
    fn escape_js_line_separators_neutralizes_unicode_terminators() {
        let input = "value\u{2028}with\u{2029}terminators";
        let escaped = escape_js_line_separators(input);
        assert_eq!(escaped, "value\\u2028with\\u2029terminators");
        assert!(!escaped.contains('\u{2028}'));
        assert!(!escaped.contains('\u{2029}'));
    }

    #[test]
    fn escape_js_line_separators_leaves_plain_text_untouched() {
        assert_eq!(escape_js_line_separators("plain text"), "plain text");
    }

    #[test]
    fn html_escape_encodes_all_markup_characters() {
        assert_eq!(
            html_escape("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn settings_snapshots_do_not_roll_back_live_window_geometry() {
        let mut live = AppSettings {
            webui_window: WebUiWindowSettings {
                width: 1536,
                height: 864,
                maximized: false,
            },
            ..AppSettings::default()
        };
        let mut stale_preference_snapshot = AppSettings::default();
        stale_preference_snapshot.appearance.theme = AppearanceTheme::Light;

        apply_settings_snapshot_preserving_live_window(&mut live, stale_preference_snapshot);

        // This is the snapshot the close lifecycle persists after resize ->
        // unrelated settings change -> close.
        assert_eq!(live.webui_window.size(), (1536, 864));
        assert_eq!(live.appearance.theme, AppearanceTheme::Light);
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
