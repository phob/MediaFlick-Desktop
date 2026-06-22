use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cef::*;
use serde_json::json;

use crate::app::about;
use crate::app::client_settings;
use crate::app::logger;
use crate::app::mpv_setup::{self, MpvSetupPhase};
use crate::app::settings::{
    AppSettings, CloseBehavior, MpvFullscreenBehavior, PlayerBackend as PlayerBackendKind,
    SegmentSkipMode, WebUiWindowSettings, normalize_server_url,
};
use crate::app::updater::{self, UpdateRelease};
use crate::jellyfin::bridge::{self as jellyfin_bridge, PlaybackContext};
use crate::mpv::input::MpvInputBindings;
use crate::mpv::{HttpHeader, MpvControlCommand, MpvLaunch, MpvPlaybackEvent};
use crate::player::{PlayerBackend, build_backend};
use crate::windows::set_window_icon;

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
        let base = platform_data_dir().join("mediaflick-desktop");
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

fn platform_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(value) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(value);
        }
        if let Some(value) = std::env::var_os("APPDATA") {
            return PathBuf::from(value);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support");
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(value) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(value);
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local").join("share");
        }
    }

    std::env::temp_dir()
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

        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(JellyfinRenderProcessHandler::new())
        }
    }
}

wrap_render_process_handler! {
    struct JellyfinRenderProcessHandler;

    impl RenderProcessHandler {
        fn on_context_created(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _context: Option<&mut V8Context>,
        ) {
            let Some(frame) = frame else {
                return;
            };
            if frame.is_main() == 0 {
                return;
            }
            let frame_url = CefString::from(&frame.url()).to_string();
            if frame_url.starts_with("data:") || frame_url.starts_with("mediaflick-desktop://") {
                return;
            }
            let script = jellyfin_bridge::bridge_script();
            frame.execute_java_script(
                Some(&CefString::from(script.as_str())),
                Some(&CefString::from("mediaflick-desktop://bridge.js")),
                0,
            );
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

            let handler_state = new_browser_state(
                self.config.title.clone(),
                self.config.settings.clone(),
            );
            {
                let mut client = self.client.borrow_mut();
                *client = Some(JellyfinClient::new(handler_state.clone()));
            }

            let settings = BrowserSettings::default();
            let initial_url = if self.config.settings.is_complete() {
                self.config
                    .settings
                    .jellyfin_url
                    .clone()
                    .unwrap_or_else(|| welcome_page_url(&self.config.settings))
            } else {
                welcome_page_url(&self.config.settings)
            };
            let url = CefString::from(initial_url.as_str());
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
    if let Err(error) = settings.save() {
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
    playback_contexts: Vec<PendingPlaybackContext>,
    player: Box<dyn PlayerBackend>,
    playback_event_tx: mpsc::Sender<MpvPlaybackEvent>,
    update_available: Option<UpdateRelease>,
    update_download_started: bool,
    mpv_setup_started: bool,
    force_close_requested: bool,
}

struct PendingPlaybackContext {
    context: PlaybackContext,
    seen_at: Instant,
}

type BrowserState = Arc<Mutex<BrowserStateInner>>;

fn new_browser_state(title: String, settings: AppSettings) -> BrowserState {
    let (playback_event_tx, playback_event_rx) = mpsc::channel();
    let player = build_backend(&settings, playback_event_tx.clone());
    warm_configured_player(player.as_ref(), &settings);
    let state = Arc::new(Mutex::new(BrowserStateInner {
        title,
        settings,
        browsers: Vec::new(),
        playback_contexts: Vec::new(),
        player,
        playback_event_tx,
        update_available: None,
        update_download_started: false,
        mpv_setup_started: false,
        force_close_requested: false,
    }));
    start_playback_event_bridge(state.clone(), playback_event_rx);
    start_update_check_bridge(state.clone());
    state
}

fn warm_configured_player(player: &dyn PlayerBackend, settings: &AppSettings) {
    player.set_segment_skip_config(settings.segment_skip_config());
    let Some(path) = settings.player_path() else {
        tracing::debug!(target: "mpv.ipc", "skipped player warmup because no executable is configured");
        return;
    };
    player.warm(path.to_string(), settings.default_fullscreen);
}

fn start_playback_event_bridge(state: BrowserState, rx: Receiver<MpvPlaybackEvent>) {
    thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            let mut task = PlaybackEventTask::new(state.clone(), event);
            if post_task(ThreadId::UI, Some(&mut task)) == 0 {
                tracing::warn!(target: "bridge", "failed to post playback event to CEF UI thread");
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
    Progress { downloaded: u64, total: Option<u64> },
    Extracting,
    Ready(PathBuf),
    Error(String),
}

wrap_task! {
    struct PlaybackEventTask {
        state: BrowserState,
        event: MpvPlaybackEvent,
    }

    impl Task {
        fn execute(&self) {
            dispatch_playback_event(&self.state, self.event.clone());
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

fn dispatch_playback_event(state: &BrowserState, event: MpvPlaybackEvent) {
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

fn playback_event_script(event: &MpvPlaybackEvent) -> String {
    match event {
        MpvPlaybackEvent::Stopped(snapshot) => {
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
        MpvSetupEvent::Progress { downloaded, total } => {
            dispatch_mpv_setup(
                state,
                "downloading",
                json!({ "downloaded": downloaded, "total": total }),
            );
        }
        MpvSetupEvent::Extracting => {
            dispatch_mpv_setup(state, "extracting", json!({}));
        }
        MpvSetupEvent::Ready(path) => {
            let mpv_path = path.to_string_lossy().into_owned();
            tracing::info!(target: "mpv.setup", path = %mpv_path, "mpv installed");
            if let Ok(mut state) = state.lock() {
                state.mpv_setup_started = false;
                state.settings.mpv_path = Some(mpv_path.clone());
                state.settings.sanitize();
                if let Err(error) = state.settings.save() {
                    tracing::warn!(target: "mpv.setup", "failed to save mpv path: {error}");
                }
                warm_configured_player(state.player.as_ref(), &state.settings);
            }
            dispatch_mpv_setup(state, "done", json!({ "path": mpv_path }));
        }
        MpvSetupEvent::Error(message) => {
            tracing::warn!(target: "mpv.setup", "mpv setup failed: {message}");
            if let Ok(mut state) = state.lock() {
                state.mpv_setup_started = false;
            }
            dispatch_mpv_setup(state, "error", json!({ "message": message }));
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
const MENU_ID_ABOUT: i32 = MENU_ID_CLIENT_SETTINGS + 1;

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
            model.add_item(MENU_ID_CLIENT_SETTINGS, Some(&CefString::from("Client Settings")));
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
                MENU_ID_CLIENT_SETTINGS => show_client_settings_dialog(browser, frame, &self.state),
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
                if let Ok(state) = self.state.lock() {
                    state.player.shutdown();
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
            let frame_url = CefString::from(&frame.url()).to_string();
            if frame_url.starts_with("mediaflick-desktop://") {
                return;
            }
            if !frame_url.starts_with("data:") {
                let script = jellyfin_bridge::bridge_script();
                frame.execute_java_script(
                    Some(&CefString::from(script.as_str())),
                    Some(&CefString::from("mediaflick-desktop://bridge.js")),
                    1,
                );
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
            if !request_url.starts_with("mediaflick-desktop://") {
                if should_open_navigation_externally(
                    &request_url,
                    frame.as_deref_mut(),
                    user_gesture,
                    &self.state,
                ) {
                    open_external_link(&request_url);
                    return 1;
                }
                return 0;
            }

            if !bridge_request_is_trusted(
                &request_url,
                browser.as_deref_mut(),
                frame.as_deref_mut(),
                &self.state,
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
            if request_url.starts_with("mediaflick-desktop://") {
                let mut browser = browser;
                let mut frame = frame;
                if bridge_request_is_trusted(
                    &request_url,
                    browser.as_deref_mut(),
                    frame.as_deref_mut(),
                    &self.state,
                ) {
                    if !route_bridge_action(&request_url, browser, frame, &self.state) {
                        tracing::warn!(
                            target: "bridge",
                            url = %request_url,
                            "ignored unrecognized bridge resource request"
                        );
                    }
                } else {
                    tracing::warn!(
                        target: "bridge",
                        url = %request_url,
                        "rejected bridge resource request from untrusted frame"
                    );
                }
                return ReturnValue::CANCEL;
            }

            let Some(mut launch) = jellyfin_bridge::launch_from_stream_url(
                &request_url,
                request_headers(request),
            ) else {
                return ReturnValue::CONTINUE;
            };

            tracing::debug!(
                target: "bridge",
                launch = %logger::launch_summary(&launch),
                "captured direct stream resource for mpv handoff"
            );
            merge_recent_playback_context(&self.state, &mut launch);
            if hand_off_to_mpv(&self.state, launch) {
                ReturnValue::CANCEL
            } else {
                ReturnValue::CONTINUE
            }
        }
    }
}

fn bridge_request_is_trusted(
    request_url: &str,
    browser: Option<&mut Browser>,
    frame: Option<&mut Frame>,
    state: &BrowserState,
) -> bool {
    let document_url = browser
        .and_then(|browser| browser.main_frame())
        .map(|frame| CefString::from(&frame.url()).to_string())
        .or_else(|| frame.map(|frame| CefString::from(&frame.url()).to_string()))
        .unwrap_or_default();

    if document_url.is_empty()
        || document_url.starts_with("data:")
        || document_url.starts_with("mediaflick-desktop://")
    {
        return true;
    }
    let server_url = state
        .lock()
        .ok()
        .and_then(|state| state.settings.jellyfin_url.clone());
    if !server_url.is_some_and(|server_url| same_web_origin(&document_url, &server_url)) {
        return false;
    }
    bridge_token_is_valid(request_url)
}

fn bridge_token_is_valid(request_url: &str) -> bool {
    let Some((_, query)) = request_url.split_once('?') else {
        return false;
    };
    let query = query.split('#').next().unwrap_or_default();
    query_param(query, "token").is_some_and(|token| token == jellyfin_bridge::bridge_token())
}

fn route_bridge_action(
    request_url: &str,
    browser: Option<&mut Browser>,
    frame: Option<&mut Frame>,
    state: &BrowserState,
) -> bool {
    if request_url.starts_with("mediaflick-desktop://select-mpv") {
        open_mpv_dialog(browser, frame, state);
        return true;
    }

    if request_url.starts_with("mediaflick-desktop://select-mpchc") {
        open_mpchc_dialog(browser, frame, state);
        return true;
    }

    if request_url.starts_with("mediaflick-desktop://app-about") {
        show_about_dialog(browser, frame);
        return true;
    }

    if request_url.starts_with("mediaflick-desktop://mpv-download") {
        start_mpv_download(state);
        return true;
    }

    if request_url.starts_with("mediaflick-desktop://mpv-help") {
        open_external_link(mpv_setup::MPV_HELP_URL);
        return true;
    }

    if is_client_settings_request(request_url) {
        show_client_settings_dialog(browser, frame, state);
        return true;
    }

    if request_url.starts_with("mediaflick-desktop://app-exit") {
        initiate_app_exit(browser, state);
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "update-download") {
        start_update_download(query, state);
        return true;
    }

    if request_url.starts_with("mediaflick-desktop://update-release") {
        open_update_release_page();
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "save") {
        save_settings_and_open(query, frame, state);
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "client-settings-save") {
        save_client_settings(query, browser, frame, state);
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "play-context") {
        remember_playback_context(query, state);
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "play") {
        spawn_mpv_from_bridge_payload(query, state);
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "player-state") {
        respond_player_state(browser, frame, query, state);
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "playback-stop-ack") {
        log_playback_stop_ack(query);
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "player-command") {
        handle_player_command(query, state);
        return true;
    }

    false
}

fn should_open_navigation_externally(
    request_url: &str,
    frame: Option<&mut Frame>,
    user_gesture: i32,
    state: &BrowserState,
) -> bool {
    if user_gesture == 0 || !is_browser_openable_url(request_url) {
        return false;
    }

    let current_url = if let Some(frame) = frame {
        if frame.is_main() == 0 {
            return false;
        }
        CefString::from(&frame.url()).to_string()
    } else {
        String::new()
    };

    if same_web_origin(request_url, &current_url) {
        return false;
    }

    let server_url = state
        .lock()
        .ok()
        .and_then(|state| state.settings.jellyfin_url.clone());
    if server_url
        .as_deref()
        .is_some_and(|server_url| same_web_origin(request_url, server_url))
    {
        return false;
    }

    true
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

#[derive(Debug, PartialEq, Eq)]
struct UrlOrigin {
    scheme: String,
    host: String,
    port: Option<u16>,
}

fn same_web_origin(left: &str, right: &str) -> bool {
    let Some(left) = parse_web_origin(left) else {
        return false;
    };
    parse_web_origin(right).is_some_and(|right| left == right)
}

fn parse_web_origin(url: &str) -> Option<UrlOrigin> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        return None;
    }

    let authority = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?;
    if authority.is_empty() {
        return None;
    }

    let (host, port) = parse_host_port(authority)?;
    let default_port = match scheme.as_str() {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    };
    let port = if port == default_port { None } else { port };

    Some(UrlOrigin { scheme, host, port })
}

fn parse_host_port(authority: &str) -> Option<(String, Option<u16>)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, suffix) = rest.split_once(']')?;
        let port = suffix
            .strip_prefix(':')
            .filter(|value| !value.is_empty())
            .and_then(|value| value.parse().ok());
        return Some((format!("[{host}]"), port));
    }

    let (host, port) = authority
        .rsplit_once(':')
        .and_then(|(host, port)| Some((host, port.parse().ok()?)))
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    (!host.is_empty()).then(|| (host.to_ascii_lowercase(), port))
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

fn show_client_settings_dialog(
    browser: Option<&mut Browser>,
    frame: Option<&mut Frame>,
    state: &BrowserState,
) {
    let settings = state
        .lock()
        .map(|state| state.settings.clone())
        .unwrap_or_default();
    let bindings = MpvInputBindings::load();
    let script = client_settings::dialog_script(&settings, &bindings);
    let target_frame = browser
        .and_then(|browser| browser.main_frame())
        .or_else(|| frame.map(|frame| frame.clone()));
    if let Some(frame) = target_frame {
        frame.execute_java_script(
            Some(&CefString::from(script.as_str())),
            Some(&CefString::from("mediaflick-desktop://client-settings")),
            1,
        );
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
        if let Ok(state) = state.lock() {
            state.player.shutdown();
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

fn start_mpv_download(state: &BrowserState) {
    if !mpv_setup::supported() {
        dispatch_mpv_setup(
            state,
            "error",
            json!({ "message": "Automatic mpv download is only available on Windows." }),
        );
        return;
    }

    match state.lock() {
        Ok(mut state) => {
            if state.mpv_setup_started {
                tracing::debug!(target: "mpv.setup", "ignored duplicate mpv download request");
                return;
            }
            state.mpv_setup_started = true;
        }
        Err(error) => {
            tracing::warn!(target: "mpv.setup", "failed to lock browser state for mpv download: {error}");
            return;
        }
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
        let result = mpv_setup::download_and_install(move |phase| match phase {
            MpvSetupPhase::Downloading { downloaded, total } => post_mpv_setup_event(
                progress_state.clone(),
                MpvSetupEvent::Progress { downloaded, total },
            ),
            MpvSetupPhase::Extracting => {
                post_mpv_setup_event(progress_state.clone(), MpvSetupEvent::Extracting)
            }
        });
        match result {
            Ok(path) => post_mpv_setup_event(state_for_thread, MpvSetupEvent::Ready(path)),
            Err(error) => {
                post_mpv_setup_event(state_for_thread, MpvSetupEvent::Error(error.to_string()))
            }
        }
    });
}

fn is_client_settings_request(request_url: &str) -> bool {
    request_url == "mediaflick-desktop://client-settings"
        || request_url.starts_with("mediaflick-desktop://client-settings?")
        || request_url.starts_with("mediaflick-desktop://client-settings/")
}

fn bridge_action_query<'a>(request_url: &'a str, action: &str) -> Option<&'a str> {
    let after_scheme = request_url.strip_prefix("mediaflick-desktop://")?;
    let query = after_scheme
        .strip_prefix(action)?
        .strip_prefix('/')
        .unwrap_or_else(|| &after_scheme[action.len()..])
        .strip_prefix('?')?;
    Some(query)
}

wrap_run_file_dialog_callback! {
    struct MpvFileDialogCallback {
        frame: Option<Frame>,
    }

    impl RunFileDialogCallback {
        fn on_file_dialog_dismissed(&self, file_paths: Option<&mut CefStringList>) {
            let Some(frame) = &self.frame else {
                return;
            };
            let Some(path) = file_paths.and_then(|paths| std::mem::take(paths).into_iter().next()) else {
                execute_welcome_js(frame, "window.__mediaFlickDesktopSetBusy(false);");
                return;
            };
            execute_welcome_js(
                frame,
                &format!(
                    "window.__mediaFlickDesktopSetMpvPath({});",
                    js_string_literal(&path)
                ),
            );
        }
    }
}

wrap_run_file_dialog_callback! {
    struct MpchcFileDialogCallback {
        frame: Option<Frame>,
    }

    impl RunFileDialogCallback {
        fn on_file_dialog_dismissed(&self, file_paths: Option<&mut CefStringList>) {
            let Some(frame) = &self.frame else {
                return;
            };
            let Some(path) = file_paths.and_then(|paths| std::mem::take(paths).into_iter().next()) else {
                execute_welcome_js(frame, "window.__mediaFlickDesktopSetBusy(false);");
                return;
            };
            execute_welcome_js(
                frame,
                &format!(
                    "window.__mediaFlickDesktopSetMpchcPath({});",
                    js_string_literal(&path)
                ),
            );
        }
    }
}

const PLAYBACK_CONTEXT_TTL: Duration = Duration::from_secs(15 * 60);
fn remember_playback_context(query: &str, state: &BrowserState) {
    let context = match jellyfin_bridge::parse_context_payload(query) {
        Ok(context) => context,
        Err(error) => {
            tracing::warn!(target: "bridge", "failed to parse playback context payload: {error}");
            return;
        }
    };
    tracing::debug!(
        target: "bridge",
        context = %playback_context_summary(&context),
        "remembering playback context from bridge"
    );
    let Ok(mut state) = state.lock() else {
        tracing::warn!(target: "bridge", "failed to lock browser state while remembering playback context");
        return;
    };
    prune_playback_state(&mut state);
    state.player.update_playback_context(context.clone());
    state.playback_contexts.push(PendingPlaybackContext {
        context,
        seen_at: Instant::now(),
    });
}

fn spawn_mpv_from_bridge_payload(query: &str, state: &BrowserState) {
    let mut launch = match jellyfin_bridge::parse_launch_payload(query) {
        Ok(launch) => launch,
        Err(error) => {
            tracing::warn!(target: "bridge", "failed to parse mpv launch payload: {error}");
            return;
        }
    };
    if launch.media_url.trim().is_empty() {
        tracing::warn!(target: "bridge", "ignored mpv launch payload with empty media URL");
        return;
    }
    tracing::debug!(
        target: "bridge",
        launch = %logger::launch_summary(&launch),
        "received mpv launch payload from bridge"
    );
    let merge_score = merge_recent_playback_context(state, &mut launch);
    tracing::debug!(
        target: "bridge",
        merge_score = ?merge_score,
        launch = %logger::launch_summary(&launch),
        "launch payload ready for mpv handoff"
    );
    let _ = hand_off_to_mpv(state, launch);
}

fn handle_player_command(query: &str, state: &BrowserState) {
    let payload = match jellyfin_bridge::parse_player_command_payload(query) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(target: "bridge", "failed to parse player command payload: {error}");
            return;
        }
    };
    let Some(command) = player_command_from_payload(&payload) else {
        tracing::debug!(
            target: "bridge",
            command = %payload.command,
            "ignored unsupported player command payload"
        );
        return;
    };
    let Ok(state) = state.lock() else {
        tracing::warn!(target: "bridge", "failed to lock browser state while handling player command");
        return;
    };
    tracing::debug!(target: "bridge", ?command, "forwarding web player command to mpv");
    state.player.control(command);
}

fn log_playback_stop_ack(query: &str) {
    let payload = match jellyfin_bridge::parse_playback_stop_ack_payload(query) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(target: "bridge", "failed to parse playback stop ack payload: {error}");
            return;
        }
    };
    tracing::debug!(
        target: "bridge",
        active = ?payload.active,
        playback_id = ?payload.playback_id,
        item_id = %payload.item_id.as_deref().unwrap_or("unknown"),
        media_source_id = %payload.media_source_id.as_deref().unwrap_or("unknown"),
        play_session_id = %payload.play_session_id.as_deref().unwrap_or("unknown"),
        position_ms = ?payload.position_ms,
        stop_reason = %payload.stop_reason.as_deref().unwrap_or("unknown"),
        handled_players = payload.handled_players,
        handled_synthetic = payload.handled_synthetic,
        ignored_players = payload.ignored_players,
        ignored_synthetic = payload.ignored_synthetic,
        active_players = payload.active_players,
        "WebUI acknowledged mpv playback stopped"
    );
}

fn respond_player_state(
    browser: Option<&mut Browser>,
    frame: Option<&mut Frame>,
    query: &str,
    state: &BrowserState,
) {
    let (snapshot, capabilities) = state
        .lock()
        .map(|state| (state.player.snapshot(), state.player.capabilities()))
        .unwrap_or_default();
    let response = json!({
        "requestId": query_param(query, "requestId").unwrap_or_default(),
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
        "capabilities": {
            "chapterMarkers": capabilities.chapter_markers,
            "externalSubtitles": capabilities.external_subtitles,
            "injectedHotkeys": capabilities.injected_hotkeys,
            "absoluteVolume": capabilities.absolute_volume,
            "pushesPosition": capabilities.pushes_position,
        },
    });
    let script = format!(
        "window.__mediaFlickDesktopReceivePlayerState&&window.__mediaFlickDesktopReceivePlayerState({});",
        js_json(&response)
    );

    let target_frame = browser
        .and_then(|browser| browser.main_frame())
        .or_else(|| frame.map(|frame| frame.clone()));
    if let Some(frame) = target_frame {
        frame.execute_java_script(
            Some(&CefString::from(script.as_str())),
            Some(&CefString::from("mediaflick-desktop://player-state")),
            1,
        );
    }
}

fn player_command_from_payload(
    payload: &jellyfin_bridge::PlayerCommandPayload,
) -> Option<MpvControlCommand> {
    match payload.command.as_str() {
        "set-pause" => payload.pause.map(MpvControlCommand::SetPause),
        "seek" => payload
            .position_ms
            .filter(|value| value.is_finite())
            .map(MpvControlCommand::SeekMilliseconds),
        "set-volume" => payload
            .volume
            .filter(|value| value.is_finite())
            .map(MpvControlCommand::SetVolume),
        "set-mute" => payload.mute.map(MpvControlCommand::SetMute),
        "set-playback-rate" => payload
            .rate
            .filter(|value| value.is_finite())
            .map(MpvControlCommand::SetPlaybackRate),
        "set-audio-stream" => payload
            .audio_mpv_id
            .filter(|id| *id > 0)
            .map(MpvControlCommand::SetAudioTrack),
        "set-subtitle-stream" => payload
            .subtitle_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(|url| MpvControlCommand::AddSubtitle(url.to_string()))
            .or(Some(MpvControlCommand::SetSubtitleTrack(
                payload.subtitle_mpv_id,
            ))),
        "stop" => Some(MpvControlCommand::Stop),
        _ => None,
    }
}

fn merge_recent_playback_context(state: &BrowserState, launch: &mut MpvLaunch) -> Option<u8> {
    let Ok(mut state) = state.lock() else {
        tracing::warn!(target: "bridge", "failed to lock browser state while merging playback context");
        return None;
    };
    prune_playback_state(&mut state);
    let best_context = state
        .playback_contexts
        .iter()
        .enumerate()
        .filter_map(|(index, pending)| {
            let score = pending.context.match_score(launch);
            (score > 0).then_some((score, pending.seen_at, index))
        })
        .max_by_key(|(score, seen_at, _index)| (*score, *seen_at))
        .and_then(|(score, _seen_at, index)| {
            state
                .playback_contexts
                .get(index)
                .map(|pending| (score, pending.context.clone()))
        });
    drop(state);

    if let Some((score, context)) = best_context {
        tracing::debug!(
            target: "bridge",
            score,
            context = %playback_context_summary(&context),
            "merged recent playback context into launch"
        );
        context.merge_into_launch(launch);
        Some(score)
    } else {
        tracing::trace!(
            target: "bridge",
            launch = %logger::launch_summary(launch),
            "no recent playback context matched launch"
        );
        None
    }
}

fn hand_off_to_mpv(state: &BrowserState, launch: MpvLaunch) -> bool {
    let Ok(mut state) = state.lock() else {
        tracing::warn!(target: "bridge", "failed to lock browser state while handing playback to mpv");
        return false;
    };
    prune_playback_state(&mut state);
    let Some(path) = state.settings.player_path().map(str::to_string) else {
        tracing::warn!(
            target: "bridge",
            launch = %logger::launch_summary(&launch),
            "cannot hand playback to the player because no executable is configured"
        );
        return false;
    };
    let fullscreen = state.settings.default_fullscreen;
    tracing::info!(
        target: "bridge",
        player_path = %path,
        launch = %logger::launch_summary(&launch),
        "handing playback to player controller"
    );
    state.player.load(path, fullscreen, launch);
    true
}

fn prune_playback_state(state: &mut BrowserStateInner) {
    let now = Instant::now();
    state
        .playback_contexts
        .retain(|context| now.saturating_duration_since(context.seen_at) <= PLAYBACK_CONTEXT_TTL);
}

fn playback_context_summary(context: &PlaybackContext) -> String {
    format!(
        "item={} media_source={} play_session={} start={} url={}",
        display_opt(context.item_id.as_deref()),
        display_opt(context.media_source_id.as_deref()),
        display_opt(context.play_session_id.as_deref()),
        context
            .start_time_ticks
            .map(|ticks| ticks.to_string())
            .unwrap_or_else(|| "none".to_string()),
        context
            .media_url
            .as_deref()
            .map(logger::redact_url_secrets)
            .unwrap_or_else(|| "unknown".to_string())
    )
}

fn display_opt(value: Option<&str>) -> &str {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
}

fn request_headers(request: &Request) -> Vec<HttpHeader> {
    let mut map = CefStringMultimap::new();
    request.header_map(Some(&mut map));
    let headers = map
        .into_iter()
        .flat_map(|(name, values)| {
            values.into_iter().map(move |value| HttpHeader {
                name: name.clone(),
                value,
            })
        })
        .collect::<Vec<_>>();
    tracing::trace!(
        target: "bridge",
        headers = %logger::redacted_header_summary(&headers),
        "captured direct stream request headers"
    );
    headers
}

fn open_mpv_dialog(browser: Option<&mut Browser>, frame: Option<&mut Frame>, state: &BrowserState) {
    let Some(browser) = browser else {
        return;
    };
    let Some(host) = browser.host() else {
        return;
    };
    let frame = frame.map(|frame| frame.clone());

    let default_path = state
        .lock()
        .ok()
        .and_then(|state| state.settings.mpv_path.clone())
        .map(|path| CefString::from(path.as_str()));
    let mut filters = CefStringList::new();
    #[cfg(target_os = "windows")]
    filters.append(".exe");
    let filters = if cfg!(target_os = "windows") {
        Some(&mut filters)
    } else {
        None
    };
    let title = CefString::from("Select mpv executable");
    let mut callback = MpvFileDialogCallback::new(frame);
    host.run_file_dialog(
        FileDialogMode::OPEN,
        Some(&title),
        default_path.as_ref(),
        filters,
        Some(&mut callback),
    );
}

fn open_mpchc_dialog(
    browser: Option<&mut Browser>,
    frame: Option<&mut Frame>,
    state: &BrowserState,
) {
    let Some(browser) = browser else {
        return;
    };
    let Some(host) = browser.host() else {
        return;
    };
    let frame = frame.map(|frame| frame.clone());

    let default_path = state
        .lock()
        .ok()
        .and_then(|state| state.settings.mpchc_path.clone())
        .map(|path| CefString::from(path.as_str()));
    let mut filters = CefStringList::new();
    #[cfg(target_os = "windows")]
    filters.append(".exe");
    let filters = if cfg!(target_os = "windows") {
        Some(&mut filters)
    } else {
        None
    };
    let title = CefString::from("Select MPC-HC executable");
    let mut callback = MpchcFileDialogCallback::new(frame);
    host.run_file_dialog(
        FileDialogMode::OPEN,
        Some(&title),
        default_path.as_ref(),
        filters,
        Some(&mut callback),
    );
}

fn save_settings_and_open(query: &str, frame: Option<&mut Frame>, state: &BrowserState) {
    let Some(frame) = frame else {
        return;
    };
    let server = query_param(query, "server").and_then(|value| normalize_server_url(&value));
    let mpv_path = query_param(query, "mpv")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let (Some(jellyfin_url), Some(mpv_path)) = (server, mpv_path) else {
        notify_save_error(
            frame,
            "Enter a Jellyfin server URL and choose an mpv executable.",
        );
        return;
    };

    let mut settings = state
        .lock()
        .map(|state| state.settings.clone())
        .unwrap_or_default();
    settings.jellyfin_url = Some(jellyfin_url.clone());
    settings.mpv_path = Some(mpv_path);
    settings.sanitize();

    if let Err(error) = settings.save() {
        notify_save_error(frame, &format!("Could not save config: {error}"));
        return;
    }

    if let Ok(mut state) = state.lock() {
        state.settings = settings;
        warm_configured_player(state.player.as_ref(), &state.settings);
    }

    frame.load_url(Some(&CefString::from(jellyfin_url.as_str())));
}

fn save_client_settings(
    query: &str,
    browser: Option<&mut Browser>,
    frame: Option<&mut Frame>,
    state: &BrowserState,
) {
    let target_frame = browser
        .and_then(|browser| browser.main_frame())
        .or_else(|| frame.map(|frame| frame.clone()));
    let Some(frame) = target_frame else {
        return;
    };

    let mut settings = state
        .lock()
        .map(|state| state.settings.clone())
        .unwrap_or_default();
    settings.player_backend = query_param(query, "playerBackend")
        .as_deref()
        .and_then(PlayerBackendKind::from_id)
        .unwrap_or(settings.player_backend);
    if let Some(mpv_path) = query_param(query, "mpv")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        settings.mpv_path = Some(mpv_path);
    }
    if let Some(mpchc_path) = query_param(query, "mpchc")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        settings.mpchc_path = Some(mpchc_path);
    }
    if settings.player_path().is_none() {
        let message = match settings.effective_backend() {
            PlayerBackendKind::Mpchc => "Choose an MPC-HC executable.",
            PlayerBackendKind::Mpv => "Choose an mpv executable.",
        };
        notify_client_settings_error(&frame, message);
        return;
    }
    if let Some(log_level) = query_param(query, "logLevel")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        settings.log_level = log_level;
    }
    settings.default_fullscreen = query_param(query, "defaultFullscreen")
        .as_deref()
        .and_then(parse_fullscreen_behavior)
        .unwrap_or(settings.default_fullscreen);
    settings.close_behavior = query_param(query, "closeBehavior")
        .as_deref()
        .and_then(parse_close_behavior)
        .unwrap_or(settings.close_behavior);
    settings.show_scrollbars = query_param(query, "scrollbars")
        .as_deref()
        .map(|value| value == "visible")
        .unwrap_or(settings.show_scrollbars);
    settings.skip_intro = query_param(query, "skipIntro")
        .as_deref()
        .and_then(parse_segment_skip_mode)
        .unwrap_or(settings.skip_intro);
    settings.skip_credits = query_param(query, "skipCredits")
        .as_deref()
        .and_then(parse_segment_skip_mode)
        .unwrap_or(settings.skip_credits);
    settings.skip_recap = query_param(query, "skipRecap")
        .as_deref()
        .and_then(parse_segment_skip_mode)
        .unwrap_or(settings.skip_recap);
    settings.skip_commercial = query_param(query, "skipCommercial")
        .as_deref()
        .and_then(parse_segment_skip_mode)
        .unwrap_or(settings.skip_commercial);
    settings.sanitize();

    let bindings = MpvInputBindings {
        mark_watched_next: query_param(query, "markWatchedNext")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    };

    if let Err(error) = settings.save() {
        notify_client_settings_error(&frame, &format!("Could not save config: {error}"));
        return;
    }
    if let Err(error) = bindings.save() {
        notify_client_settings_error(&frame, &format!("Could not save input bindings: {error}"));
        return;
    }

    if let Ok(mut state) = state.lock() {
        let backend_changed = state.settings.effective_backend() != settings.effective_backend();
        state.settings = settings;
        if backend_changed {
            tracing::info!(
                target: "playback",
                backend = state.settings.effective_backend().as_str(),
                "rebuilding player backend after settings change"
            );
            state.player.shutdown();
            let event_tx = state.playback_event_tx.clone();
            let player = build_backend(&state.settings, event_tx);
            state.player = player;
        }
        warm_configured_player(state.player.as_ref(), &state.settings);
    }
    apply_scrollbar_settings_to_frame(&frame, state);
    execute_client_settings_js(
        &frame,
        "window.__mediaFlickDesktopClientSettingsSaved&&window.__mediaFlickDesktopClientSettingsSaved();",
    );
}

fn parse_fullscreen_behavior(value: &str) -> Option<MpvFullscreenBehavior> {
    match value {
        "fullscreen" => Some(MpvFullscreenBehavior::Fullscreen),
        "windowed" => Some(MpvFullscreenBehavior::Windowed),
        _ => None,
    }
}

fn parse_close_behavior(value: &str) -> Option<CloseBehavior> {
    match value {
        "exit_app" => Some(CloseBehavior::ExitApp),
        "minimize_window" => Some(CloseBehavior::MinimizeWindow),
        _ => None,
    }
}

fn parse_segment_skip_mode(value: &str) -> Option<SegmentSkipMode> {
    match value {
        "disabled" => Some(SegmentSkipMode::Disabled),
        "prompt" => Some(SegmentSkipMode::Prompt),
        "always" => Some(SegmentSkipMode::Always),
        _ => None,
    }
}

fn notify_save_error(frame: &Frame, message: &str) {
    execute_welcome_js(
        frame,
        &format!(
            "window.__mediaFlickDesktopSaveFailed({});",
            js_string_literal(message)
        ),
    );
}

fn execute_welcome_js(frame: &Frame, script: &str) {
    frame.execute_java_script(
        Some(&CefString::from(script)),
        Some(&CefString::from("mediaflick-desktop://welcome")),
        1,
    );
}

fn notify_client_settings_error(frame: &Frame, message: &str) {
    execute_client_settings_js(
        frame,
        &format!(
            "window.__mediaFlickDesktopClientSettingsSaveFailed&&window.__mediaFlickDesktopClientSettingsSaveFailed({});",
            js_string_literal(message)
        ),
    );
}

fn execute_client_settings_js(frame: &Frame, script: &str) {
    frame.execute_java_script(
        Some(&CefString::from(script)),
        Some(&CefString::from("mediaflick-desktop://client-settings")),
        1,
    );
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        (percent_decode(raw_key) == key).then(|| percent_decode(raw_value))
    })
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn js_string_literal(value: &str) -> String {
    escape_js_line_separators(&serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string()))
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

fn welcome_page_url(settings: &AppSettings) -> String {
    data_uri(welcome_html(settings).as_bytes(), "text/html")
}

fn welcome_html(settings: &AppSettings) -> String {
    let connect_disabled = if settings.jellyfin_url.is_some() && settings.mpv_path.is_some() {
        ""
    } else {
        "disabled"
    };

    include_str!("../ui/welcome.html")
        .replace(
            "{{saved_url}}",
            &html_escape(settings.jellyfin_url.as_deref().unwrap_or_default()),
        )
        .replace(
            "{{saved_mpv}}",
            &html_escape(settings.mpv_path.as_deref().unwrap_or_default()),
        )
        .replace("{{mpv_placeholder}}", &html_escape(mpv_placeholder()))
        .replace("{{mpv_setup_config}}", &mpv_setup::ui_config_json())
        .replace("{{app_version}}", about::APP_VERSION)
        .replace("{{connect_disabled}}", connect_disabled)
}

fn load_error_html(title: &str, failed_url: &str, error_text: &str, error_code: i32) -> String {
    include_str!("../ui/load_error.html")
        .replace("{{title}}", &html_escape(title))
        .replace("{{failed_url}}", &html_escape(failed_url))
        .replace("{{error_text}}", &html_escape(error_text))
        .replace("{{error_code}}", &error_code.to_string())
}

fn mpv_placeholder() -> &'static str {
    "mpv"
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
        bridge_token_is_valid, escape_js_line_separators, html_escape, is_browser_openable_url,
        is_safe_external_link, js_string_literal, percent_decode, same_web_origin, url_scheme,
    };

    #[test]
    fn same_origin_matches_identical_urls() {
        assert!(same_web_origin(
            "http://192.168.1.10:8096/web/index.html",
            "http://192.168.1.10:8096"
        ));
    }

    #[test]
    fn same_origin_ignores_scheme_and_host_case() {
        assert!(same_web_origin(
            "HTTPS://Jellyfin.Example.COM/web/",
            "https://jellyfin.example.com"
        ));
    }

    #[test]
    fn same_origin_treats_default_ports_as_implicit() {
        assert!(same_web_origin("http://host:80/web", "http://host"));
        assert!(same_web_origin("https://host:443/web", "https://host"));
    }

    #[test]
    fn same_origin_distinguishes_explicit_ports() {
        assert!(!same_web_origin("http://host:8096", "http://host:9096"));
        assert!(!same_web_origin("http://host:8443", "http://host"));
    }

    #[test]
    fn same_origin_distinguishes_scheme() {
        assert!(!same_web_origin("http://host", "https://host"));
    }

    #[test]
    fn same_origin_distinguishes_host() {
        assert!(!same_web_origin(
            "http://jellyfin.example.com",
            "http://attacker.example.com"
        ));
    }

    #[test]
    fn same_origin_strips_userinfo() {
        assert!(same_web_origin(
            "http://user:pass@jellyfin.example.com/web",
            "http://jellyfin.example.com"
        ));
    }

    #[test]
    fn same_origin_matches_ipv6_with_default_port() {
        assert!(same_web_origin("http://[::1]:80/web", "http://[::1]"));
        assert!(!same_web_origin("http://[::1]:8096", "http://[::1]:9096"));
    }

    #[test]
    fn same_origin_rejects_non_web_schemes() {
        assert!(!same_web_origin("file:///etc/passwd", "file:///etc/passwd"));
        assert!(!same_web_origin(
            "mediaflick-desktop://bridge",
            "mediaflick-desktop://bridge"
        ));
        assert!(!same_web_origin(
            "data:text/html,evil",
            "data:text/html,evil"
        ));
    }

    #[test]
    fn same_origin_rejects_unparseable_input() {
        assert!(!same_web_origin("not-a-url", "http://host"));
        assert!(!same_web_origin("http://", "http://"));
    }

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
    fn js_string_literal_escapes_quotes_and_terminators() {
        let literal = js_string_literal("say \"hi\"\u{2028}now");
        assert_eq!(literal, "\"say \\\"hi\\\"\\u2028now\"");
    }

    #[test]
    fn html_escape_encodes_all_markup_characters() {
        assert_eq!(
            html_escape("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn bridge_token_is_valid_accepts_the_session_token() {
        let token = crate::jellyfin::bridge::bridge_token();
        let url = format!("mediaflick-desktop://play?token={token}&payload=%7B%7D");
        assert!(bridge_token_is_valid(&url));
    }

    #[test]
    fn bridge_token_is_valid_rejects_wrong_or_missing_token() {
        assert!(!bridge_token_is_valid(
            "mediaflick-desktop://play?token=deadbeef&payload=%7B%7D"
        ));
        assert!(!bridge_token_is_valid(
            "mediaflick-desktop://play?payload=%7B%7D"
        ));
        assert!(!bridge_token_is_valid("mediaflick-desktop://app-exit"));
    }

    #[test]
    fn percent_decode_handles_escapes_plus_and_passthrough() {
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        assert_eq!(percent_decode("%2Fpath%2F"), "/path/");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
