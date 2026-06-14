use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cef::*;
use serde_json::json;

use crate::app::about;
use crate::app::logger;
use crate::app::settings::{AppSettings, WebUiWindowSettings, normalize_server_url};
use crate::jellyfin::bridge::{self as jellyfin_bridge, PlaybackContext};
use crate::mpv::{HttpHeader, MpvControlCommand, MpvController, MpvLaunch, MpvPlaybackEvent};
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
    let product = format!("jellyfin-mpv/{}", env!("CARGO_PKG_VERSION"));
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
}

impl RuntimePaths {
    fn new() -> Self {
        let base = platform_data_dir().join("jellyfin-mpv");
        let browser_subprocess_path = current_exe_path();
        let resources_dir_path = browser_subprocess_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(PathBuf::from);
        let locales_dir_path = resources_dir_path.as_ref().map(|path| path.join("locales"));
        Self {
            cache_dir: base.join("cef-cache"),
            log_file: base.join("cef.log"),
            browser_subprocess_path,
            resources_dir_path,
            locales_dir_path,
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
            let scheme = CefString::from("jellyfin-mpv");
            registrar.add_custom_scheme(
                Some(&scheme),
                SchemeOptions::STANDARD.get_raw()
                    | SchemeOptions::SECURE.get_raw()
                    | SchemeOptions::CORS_ENABLED.get_raw()
                    | SchemeOptions::FETCH_ENABLED.get_raw(),
            );
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
            if frame_url.starts_with("data:") || frame_url.starts_with("jellyfin-mpv://") {
                return;
            }
            let script = jellyfin_bridge::bridge_script();
            frame.execute_java_script(
                Some(&CefString::from(script.as_str())),
                Some(&CefString::from("jellyfin-mpv://bridge.js")),
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
                "jellyfin-mpv".to_string(),
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

        fn can_close(&self, _window: Option<&mut Window>) -> i32 {
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
        tracing::warn!(target: "config", "failed to save jellyfin-mpv config on window close: {error}");
    }
}

struct BrowserStateInner {
    title: String,
    settings: AppSettings,
    browsers: Vec<Browser>,
    playback_contexts: Vec<PendingPlaybackContext>,
    mpv_controller: MpvController,
}

struct PendingPlaybackContext {
    context: PlaybackContext,
    seen_at: Instant,
}

type BrowserState = Arc<Mutex<BrowserStateInner>>;

fn new_browser_state(title: String, settings: AppSettings) -> BrowserState {
    let (playback_event_tx, playback_event_rx) = mpsc::channel();
    let state = Arc::new(Mutex::new(BrowserStateInner {
        title,
        settings,
        browsers: Vec::new(),
        playback_contexts: Vec::new(),
        mpv_controller: MpvController::new(Some(playback_event_tx)),
    }));
    start_playback_event_bridge(state.clone(), playback_event_rx);
    state
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

wrap_task! {
    struct PlaybackEventTask {
        state: BrowserState,
        event: MpvPlaybackEvent,
    }

    impl Task {
        fn execute(&self) {
            dispatch_playback_event(&self.state, self.event);
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

    let script = playback_event_script(event);
    let browser_count = browsers.len();
    let mut frame_count = 0usize;
    for browser in browsers {
        if let Some(frame) = browser.main_frame() {
            frame_count += 1;
            frame.execute_java_script(
                Some(&CefString::from(script.as_str())),
                Some(&CefString::from("jellyfin-mpv://playback-event")),
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

fn playback_event_script(event: MpvPlaybackEvent) -> String {
    match event {
        MpvPlaybackEvent::Stopped(snapshot) => {
            let payload = json!({
                "active": snapshot.active,
                "positionMs": snapshot.position_ms,
                "durationMs": snapshot.duration_ms,
                "paused": snapshot.paused,
                "volume": snapshot.volume,
                "mute": snapshot.mute,
                "stopReason": snapshot.stop_reason,
            });
            format!(
                "window.__jellyfinMpvPlaybackStopped&&window.__jellyfinMpvPlaybackStopped({payload});"
            )
        }
    }
}

wrap_client! {
    struct JellyfinClient {
        state: BrowserState,
    }

    impl Client {
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(JellyfinContextMenuHandler::new())
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(JellyfinDisplayHandler::new(self.state.clone()))
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

const MENU_ID_ABOUT: i32 = sys::cef_menu_id_t::MENU_ID_USER_FIRST as i32;

wrap_context_menu_handler! {
    struct JellyfinContextMenuHandler;

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
            if model.count() > 0 {
                model.add_separator();
            }
            model.add_item(
                MENU_ID_ABOUT,
                Some(&CefString::from("About jellyfin-mpv")),
            );
        }

        fn on_context_menu_command(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            command_id: i32,
            _event_flags: EventFlags,
        ) -> i32 {
            if command_id != MENU_ID_ABOUT {
                return 0;
            }
            show_about_dialog(browser, frame);
            1
        }
    }
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
                .unwrap_or_else(|_| "jellyfin-mpv".to_string());
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
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser.cloned() else {
                return;
            };
            if let Ok(mut state) = self.state.lock() {
                state.browsers.push(browser);
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
                    state.mpv_controller.shutdown();
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
            if frame_url.starts_with("data:") || frame_url.starts_with("jellyfin-mpv://") {
                return;
            }
            let script = jellyfin_bridge::bridge_script();
            frame.execute_java_script(
                Some(&CefString::from(script.as_str())),
                Some(&CefString::from("jellyfin-mpv://bridge.js")),
                1,
            );
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
                .unwrap_or_else(|_| "jellyfin-mpv".to_string());
            let failed_url = html_escape(&failed_url.map(CefString::to_string).unwrap_or_default());
            let error_text = html_escape(&error_text.map(CefString::to_string).unwrap_or_default());
            let error_code = raw_error as i32;
            let html = format!(
                r#"<!doctype html>
<html>
<head><meta charset="utf-8"><title>{title}</title></head>
<body style="margin:40px;font:16px system-ui;background:#101010;color:#f4f4f4">
  <h1>Could not load Jellyfin</h1>
  <p><strong>URL:</strong> {failed_url}</p>
  <p><strong>Error:</strong> {error_text} ({error_code})</p>
  <p>Pass a different server with <code>--url http://localhost:8096</code>.</p>
</body>
</html>"#,
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
            _user_gesture: i32,
            _is_redirect: i32,
        ) -> i32 {
            let Some(request) = request else {
                return 0;
            };
            let request_url = CefString::from(&request.url()).to_string();
            if !request_url.starts_with("jellyfin-mpv://") {
                return 0;
            }

            if request_url.starts_with("jellyfin-mpv://select-mpv") {
                open_mpv_dialog(browser, frame, &self.state);
                return 1;
            }

            if request_url.starts_with("jellyfin-mpv://app-about") {
                show_about_dialog(browser, frame);
                return 1;
            }

            if request_url.starts_with("jellyfin-mpv://app-exit") {
                initiate_app_exit(browser, &self.state);
                return 1;
            }

            if let Some(query) = bridge_action_query(&request_url, "save") {
                save_settings_and_open(query, frame, &self.state);
                return 1;
            }

            if let Some(query) = bridge_action_query(&request_url, "play-context") {
                remember_playback_context(query, &self.state);
                return 1;
            }

            if let Some(query) = bridge_action_query(&request_url, "play") {
                spawn_mpv_from_bridge_payload(query, &self.state);
                return 1;
            }

            if let Some(query) = bridge_action_query(&request_url, "player-state") {
                respond_player_state(browser, frame, query, &self.state);
                return 1;
            }

            if let Some(query) = bridge_action_query(&request_url, "playback-stop-ack") {
                log_playback_stop_ack(query);
                return 1;
            }

            if let Some(query) = bridge_action_query(&request_url, "player-command") {
                handle_player_command(query, &self.state);
                return 1;
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
            if handle_bridge_resource_request(&request_url, browser, frame, &self.state) {
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

fn handle_bridge_resource_request(
    request_url: &str,
    browser: Option<&mut Browser>,
    frame: Option<&mut Frame>,
    state: &BrowserState,
) -> bool {
    if let Some(query) = bridge_action_query(request_url, "play-context") {
        tracing::trace!(target: "bridge", "handling play-context bridge resource request");
        remember_playback_context(query, state);
        return true;
    }

    if request_url.starts_with("jellyfin-mpv://app-about") {
        tracing::trace!(target: "bridge", "handling app-about bridge resource request");
        show_about_dialog(browser, frame);
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "play") {
        tracing::trace!(target: "bridge", "handling play bridge resource request");
        spawn_mpv_from_bridge_payload(query, state);
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "player-state") {
        tracing::trace!(target: "bridge", "handling player-state bridge resource request");
        respond_player_state(browser, frame, query, state);
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "playback-stop-ack") {
        tracing::trace!(target: "bridge", "handling playback-stop-ack bridge resource request");
        log_playback_stop_ack(query);
        return true;
    }

    if let Some(query) = bridge_action_query(request_url, "player-command") {
        tracing::trace!(target: "bridge", "handling player-command bridge resource request");
        handle_player_command(query, state);
        return true;
    }

    false
}

fn show_about_dialog(browser: Option<&mut Browser>, frame: Option<&mut Frame>) {
    let script = about::dialog_script();
    let target_frame = browser
        .and_then(|browser| browser.main_frame())
        .or_else(|| frame.map(|frame| frame.clone()));
    if let Some(frame) = target_frame {
        frame.execute_java_script(
            Some(&CefString::from(script.as_str())),
            Some(&CefString::from("jellyfin-mpv://app-about")),
            1,
        );
    }
}

fn initiate_app_exit(browser: Option<&mut Browser>, state: &BrowserState) {
    tracing::info!(target: "app", "exit requested from Jellyfin Web user menu");

    let mut browsers = state
        .lock()
        .map(|state| state.browsers.clone())
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
            state.mpv_controller.shutdown();
        }
        quit_message_loop();
    }
}

fn bridge_action_query<'a>(request_url: &'a str, action: &str) -> Option<&'a str> {
    let after_scheme = request_url.strip_prefix("jellyfin-mpv://")?;
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
                execute_welcome_js(frame, "window.__jellyfinMpvSetBusy(false);");
                return;
            };
            execute_welcome_js(
                frame,
                &format!(
                    "window.__jellyfinMpvSetMpvPath({});",
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
    state.mpv_controller.control(command);
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
        position_ms = ?payload.position_ms,
        stop_reason = %payload.stop_reason.as_deref().unwrap_or("unknown"),
        handled_players = payload.handled_players,
        handled_synthetic = payload.handled_synthetic,
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
    let snapshot = state
        .lock()
        .map(|state| state.mpv_controller.snapshot())
        .unwrap_or_default();
    let response = json!({
        "requestId": query_param(query, "requestId").unwrap_or_default(),
        "active": snapshot.active,
        "positionMs": snapshot.position_ms,
        "durationMs": snapshot.duration_ms,
        "paused": snapshot.paused,
        "volume": snapshot.volume,
        "mute": snapshot.mute,
        "stopReason": snapshot.stop_reason,
    });
    let script = format!(
        "window.__jellyfinMpvReceivePlayerState&&window.__jellyfinMpvReceivePlayerState({});",
        response
    );

    let target_frame = browser
        .and_then(|browser| browser.main_frame())
        .or_else(|| frame.map(|frame| frame.clone()));
    if let Some(frame) = target_frame {
        frame.execute_java_script(
            Some(&CefString::from(script.as_str())),
            Some(&CefString::from("jellyfin-mpv://player-state")),
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
    let Some(mpv_path) = state.settings.mpv_path.clone() else {
        tracing::warn!(
            target: "bridge",
            launch = %logger::launch_summary(&launch),
            "cannot hand playback to mpv because mpv path is not configured"
        );
        return false;
    };
    tracing::info!(
        target: "bridge",
        mpv_path = %mpv_path,
        launch = %logger::launch_summary(&launch),
        "handing playback to mpv controller"
    );
    state.mpv_controller.load(mpv_path, launch);
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
    filters.append(".exe");
    let title = CefString::from("Select mpv.exe");
    let mut callback = MpvFileDialogCallback::new(frame);
    host.run_file_dialog(
        FileDialogMode::OPEN,
        Some(&title),
        default_path.as_ref(),
        Some(&mut filters),
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
        notify_save_error(frame, "Enter a Jellyfin server URL and select mpv.exe.");
        return;
    };

    let webui_window = state
        .lock()
        .map(|state| state.settings.webui_window)
        .unwrap_or_default();
    let settings = AppSettings {
        jellyfin_url: Some(jellyfin_url.clone()),
        mpv_path: Some(mpv_path),
        webui_window,
    };

    if let Err(error) = settings.save() {
        notify_save_error(frame, &format!("Could not save config: {error}"));
        return;
    }

    if let Ok(mut state) = state.lock() {
        state.settings = settings;
    }

    frame.load_url(Some(&CefString::from(jellyfin_url.as_str())));
}

fn notify_save_error(frame: &Frame, message: &str) {
    execute_welcome_js(
        frame,
        &format!(
            "window.__jellyfinMpvSaveFailed({});",
            js_string_literal(message)
        ),
    );
}

fn execute_welcome_js(frame: &Frame, script: &str) {
    frame.execute_java_script(
        Some(&CefString::from(script)),
        Some(&CefString::from("jellyfin-mpv://welcome")),
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
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
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
        .replace("{{app_version}}", about::APP_VERSION)
        .replace("{{connect_disabled}}", connect_disabled)
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
