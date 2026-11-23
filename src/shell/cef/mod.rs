use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cef::*;

use crate::app::paths::app_data_dir;
use crate::app::services::{Services, ShellRequest};
use crate::app::{build_info, logger, urls};
use crate::jellyfin::api::items;
use crate::jellyfin::bridge as jellyfin_bridge;
use crate::jellyfin::playback_reporter::flush_playstate_reports;
use crate::maintenance::player_setup::{self as mpv_setup, MpvSetupPhase};
use crate::maintenance::updater::{self, UpdateRelease};
use crate::playback::{PlaybackCoordinator, PlaybackEvent};
use crate::players::build_backend;
use crate::preferences::{AppSettings, CloseBehavior, SettingsChange, WebUiWindowSettings};
use crate::shell::locator;
use crate::shell::ui::{about, error_toast};
use crate::windows::set_window_icon;

pub mod api;
pub mod app_scheme;

const PLAYSTATE_CACHE_REFRESH_TIMEOUT: Duration = Duration::from_secs(11);

static CEF_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PostOutcome {
    Posted,
    ShuttingDown,
    Failed,
}

fn guarded_post(shutting_down: &AtomicBool, post: impl FnOnce() -> bool) -> PostOutcome {
    if shutting_down.load(Ordering::Acquire) {
        return PostOutcome::ShuttingDown;
    }
    if post() {
        PostOutcome::Posted
    } else if shutting_down.load(Ordering::Acquire) {
        PostOutcome::ShuttingDown
    } else {
        PostOutcome::Failed
    }
}

pub(super) fn mark_cef_shutting_down() {
    CEF_SHUTTING_DOWN.store(true, Ordering::SeqCst);
}

pub(super) fn post_ui_task(task: &mut Task) -> PostOutcome {
    guarded_post(&CEF_SHUTTING_DOWN, || {
        post_task(ThreadId::UI, Some(task)) != 0
    })
}

pub(super) fn post_delayed_ui_task(task: &mut Task, delay_ms: i64) -> PostOutcome {
    guarded_post(&CEF_SHUTTING_DOWN, || {
        post_delayed_task(ThreadId::UI, Some(task), delay_ms) != 0
    })
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub settings: AppSettings,
    pub title: String,
    pub remote_debugging_port: i32,
    pub hidden: bool,
}

pub fn run(config: &AppConfig) -> i32 {
    #[cfg(target_os = "macos")]
    let _framework = match current_exe_path()
        .ok_or_else(|| "could not resolve the executable path".to_string())
        .and_then(|exe| mac::load_framework(&exe))
    {
        Ok(framework) => framework,
        Err(error) => {
            eprintln!("Failed to load CEF: {error}");
            return 1;
        }
    };

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = args::Args::new();
    let Some(command_line) = args.as_cmd_line() else {
        eprintln!("Failed to parse CEF command line");
        return 1;
    };

    let type_switch = CefString::from("type");
    let is_browser_process = command_line.has_switch(Some(&type_switch)) != 1;
    let mut app = ShellApp::new(config.clone());

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

    #[cfg(target_os = "macos")]
    if let Err(error) = mac::install_application() {
        eprintln!("Failed to start CEF: {error}");
        return 1;
    }

    let paths = RuntimePaths::new();
    if let Err(error) = paths.create() {
        eprintln!("Failed to create CEF data directories: {error}");
        return 1;
    }

    let cache_path = paths.cache_dir.to_string_lossy();
    let log_file = paths.log_file.to_string_lossy();
    let product = format!("mediaflick-desktop/{}", env!("CARGO_PKG_VERSION"));
    let windowless_rendering_enabled = i32::from(libmpv_overlay::is_configured(&config.settings));
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

    let service_settings = config.settings.clone();
    if let Err(error) = thread::Builder::new()
        .name("services-init".to_string())
        .spawn(move || {
            locator::init_with_settings(service_settings);
        })
    {
        tracing::warn!(target: "main", "could not start services early: {error}");
    }

    CEF_SHUTTING_DOWN.store(false, Ordering::Release);
    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) != 1
    {
        mark_cef_shutting_down();
        eprintln!("CEF initialization failed");
        return 1;
    }

    run_message_loop();
    mark_cef_shutting_down();
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
        let exe = current_exe_path();
        let app_dir = exe
            .as_ref()
            .and_then(|path| path.parent())
            .map(PathBuf::from);

        #[cfg(target_os = "macos")]
        let bundle_contents_dir = exe.as_deref().and_then(macos_bundle_contents_dir);

        #[cfg(target_os = "macos")]
        let browser_subprocess_path = bundle_contents_dir
            .as_deref()
            .map(mac::helper_executable)
            .or(exe);
        #[cfg(not(target_os = "macos"))]
        let browser_subprocess_path = exe;

        #[cfg(target_os = "macos")]
        let resources_dir_path = bundle_contents_dir
            .as_ref()
            .map(|path| path.join("Resources"))
            .or_else(|| app_dir.clone());
        #[cfg(not(target_os = "macos"))]
        let resources_dir_path = app_dir;

        let locales_dir_path = resources_dir_path.as_ref().map(|path| path.join("locales"));

        #[cfg(target_os = "macos")]
        let framework_dir_path = current_exe_path().as_deref().and_then(mac::framework_dir);
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
    let owned = CefString::from(build_info::APP_DESKTOP_ID);
    let borrowed = owned.clone();
    std::mem::forget(owned);
    borrowed
}

mod bridge;
mod document;
mod events;
mod handlers;
mod libmpv_overlay;
#[cfg(target_os = "macos")]
mod mac;
mod recovery;
mod runtime;
mod ui_events;

use events::{
    start_playback_event_bridge, start_preferences_event_bridge, start_shell_request_bridge,
    start_update_check_bridge, warm_configured_player,
};
use runtime::ShellApp;
#[cfg(not(target_os = "windows"))]
pub use runtime::notify_already_running;

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
    let Some(services) = locator::services() else {
        return;
    };
    if let Err(error) = services.preferences.record_window(settings.webui_window) {
        tracing::warn!(target: "config", "failed to save mediaflick-desktop config on window close: {error}");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BeforeUnloadDecision {
    pub(super) proceed: bool,
    pub(super) notify_window_close_blocked: bool,
}

#[derive(Debug, Default)]
pub(super) struct WindowCloseState {
    force_close_requested: bool,
    close_pending: bool,
    shutting_down: bool,
}

impl WindowCloseState {
    pub(super) fn should_minimize(&self, behavior: CloseBehavior) -> bool {
        !self.force_close_requested && behavior == CloseBehavior::MinimizeWindow
    }

    pub(super) fn request_force_close(&mut self) {
        self.force_close_requested = true;
        self.close_pending = false;
        self.shutting_down = true;
    }

    pub(super) fn begin_close(&mut self) {
        if !self.force_close_requested {
            self.close_pending = true;
        }
    }

    pub(super) fn finish_close_attempt(&mut self, closed: bool) {
        if closed {
            self.close_pending = false;
            self.shutting_down = true;
        }
    }

    pub(super) fn mark_shutting_down(&mut self) {
        self.shutting_down = true;
    }

    pub(super) const fn is_exiting(&self) -> bool {
        self.force_close_requested || self.shutting_down
    }

    pub(super) fn on_before_unload(&mut self, is_reload: bool) -> BeforeUnloadDecision {
        if self.force_close_requested {
            self.close_pending = false;
            return BeforeUnloadDecision {
                proceed: true,
                notify_window_close_blocked: false,
            };
        }
        if is_reload {
            return BeforeUnloadDecision {
                proceed: false,
                notify_window_close_blocked: false,
            };
        }
        let notify_window_close_blocked = std::mem::replace(&mut self.close_pending, false);
        BeforeUnloadDecision {
            proceed: false,
            notify_window_close_blocked,
        }
    }
}

fn request_main_window_close_with(
    state: Option<&BrowserState>,
    try_close: impl FnOnce() -> bool,
    minimize: impl FnOnce(),
) -> bool {
    let minimize_instead = state
        .and_then(|state| state.lock().ok())
        .is_some_and(|state| state.close.should_minimize(state.settings.close_behavior));
    if minimize_instead {
        minimize();
        tracing::info!(target: "app", "close request minimized the window");
        return false;
    }
    if let Some(state) = state
        && let Ok(mut state) = state.lock()
    {
        state.close.begin_close();
    }
    let closed = try_close();
    if let Some(state) = state
        && let Ok(mut state) = state.lock()
    {
        state.close.finish_close_attempt(closed);
    }
    closed
}

fn request_main_window_close(
    state: Option<&BrowserState>,
    browser: Option<Browser>,
    minimize: impl FnOnce(),
) -> bool {
    request_main_window_close_with(
        state,
        || {
            browser
                .and_then(|browser| browser.host())
                .is_none_or(|host| host.try_close_browser() != 0)
        },
        minimize,
    )
}

struct BrowserStateInner {
    title: String,
    settings: AppSettings,
    browsers: Vec<Browser>,
    main_window: Option<Window>,
    initial_show_state: ShowState,
    reveal: recovery::WindowRevealState,
    renderer_recovery: recovery::RendererRecoveryState,
    playback: Arc<PlaybackCoordinator>,
    update_available: Option<UpdateRelease>,
    update_download_started: bool,
    mpv_setup_started: bool,
    close: WindowCloseState,
    player_warmed: bool,
}

type BrowserState = Arc<Mutex<BrowserStateInner>>;

fn show_views_window(window: &Window, show_state: ShowState) {
    if show_state == ShowState::MAXIMIZED {
        window.maximize();
    }
    window.show();
    window.activate();
}

fn register_main_window(state: &BrowserState, window: &Window) {
    let show_state = {
        let Ok(mut state) = state.lock() else {
            return;
        };
        state.main_window = Some(window.clone());
        let exiting = state.close.is_exiting();
        if state.reveal.on_target_ready(exiting) != recovery::RevealAction::RevealNow {
            return;
        }
        state.initial_show_state
    };
    show_views_window(window, show_state);
}

fn clear_main_window(state: &BrowserState) {
    if let Ok(mut state) = state.lock() {
        state.main_window = None;
    }
}

fn focus_main_window(state: &BrowserState) {
    debug_assert_ne!(currently_on(ThreadId::UI), 0);

    let overlay_active = libmpv_overlay::is_active();
    let window = match state.lock() {
        Ok(state) => {
            if state.close.is_exiting() {
                return;
            }
            if overlay_active {
                None
            } else {
                state.main_window.clone()
            }
        }
        Err(_) => return,
    };

    #[cfg(target_os = "linux")]
    if overlay_active {
        libmpv_overlay::reveal();
        return;
    }

    #[cfg(target_os = "windows")]
    if overlay_active {
        libmpv_overlay::focus();
        return;
    }

    if let Some(window) = window {
        if window.is_minimized() != 0 {
            window.restore();
        }
        window.show();
        window.activate();
    }
}

fn reveal_main_window(state: &BrowserState) {
    debug_assert_ne!(currently_on(ThreadId::UI), 0);

    let overlay_active = libmpv_overlay::is_active();
    let views_target = {
        let Ok(mut state) = state.lock() else {
            tracing::warn!(target: "shell", "failed to read main window state before reveal");
            return;
        };
        let exiting = state.close.is_exiting();
        let target_available = overlay_active || state.main_window.is_some();
        if state.reveal.request_reveal(target_available, exiting)
            != recovery::RevealAction::RevealNow
        {
            return;
        }
        if overlay_active {
            None
        } else {
            state
                .main_window
                .clone()
                .map(|window| (window, state.initial_show_state))
        }
    };

    #[cfg(target_os = "linux")]
    if overlay_active {
        libmpv_overlay::reveal();
        return;
    }

    if let Some((window, show_state)) = views_target {
        show_views_window(&window, show_state);
    }
}

wrap_task! {
    struct RevealDeadlineTask {
        state: std::sync::Weak<Mutex<BrowserStateInner>>,
        generation: u64,
    }

    impl Task {
        fn execute(&self) {
            if let Some(state) = self.state.upgrade() {
                handle_reveal_deadline(&state, self.generation);
            }
        }
    }
}

fn schedule_reveal_deadline(state: &BrowserState) {
    let generation = state.lock().ok().and_then(|mut state| {
        let exiting = state.close.is_exiting();
        state.reveal.arm_deadline(exiting)
    });
    let Some(generation) = generation else {
        return;
    };
    let mut task = RevealDeadlineTask::new(Arc::downgrade(state), generation);
    if post_delayed_ui_task(&mut task, recovery::REVEAL_DEADLINE_MS) == PostOutcome::Failed {
        tracing::warn!(target: "shell", "failed to schedule main window reveal deadline");
    }
}

fn handle_reveal_deadline(state: &BrowserState, generation: u64) {
    debug_assert_ne!(currently_on(ThreadId::UI), 0);

    let overlay_active = libmpv_overlay::is_active();
    let (action, views_target) = {
        let Ok(mut state) = state.lock() else {
            return;
        };
        let exiting = state.close.is_exiting();
        let target_available = overlay_active || state.main_window.is_some();
        let action = state
            .reveal
            .on_deadline(generation, target_available, exiting);
        if action == recovery::RevealAction::Skip {
            return;
        }
        let views_target = if action == recovery::RevealAction::RevealNow && !overlay_active {
            state
                .main_window
                .clone()
                .map(|window| (window, state.initial_show_state))
        } else {
            None
        };
        (action, views_target)
    };

    tracing::warn!(
        target: "shell",
        deadline_ms = recovery::REVEAL_DEADLINE_MS,
        "main window did not signal ready before the reveal deadline; revealing now"
    );
    if let Some(services) = locator::services() {
        services.sync.release_startup_hold();
    }
    if action != recovery::RevealAction::RevealNow {
        return;
    }

    #[cfg(target_os = "linux")]
    if overlay_active {
        libmpv_overlay::reveal();
        return;
    }

    if let Some((window, show_state)) = views_target {
        show_views_window(&window, show_state);
    }
}

fn new_browser_state(
    title: String,
    settings: AppSettings,
    initial_show_state: ShowState,
) -> BrowserState {
    let (playback_event_tx, playback_event_rx) = mpsc::channel();
    let playback = Arc::new(PlaybackCoordinator::new(playback_event_tx, |events| {
        build_backend(&settings, events)
    }));
    let state = Arc::new(Mutex::new(BrowserStateInner {
        title,
        settings,
        browsers: Vec::new(),
        main_window: None,
        initial_show_state,
        reveal: recovery::WindowRevealState::new(initial_show_state == ShowState::HIDDEN),
        renderer_recovery: recovery::RendererRecoveryState::default(),
        playback: playback.clone(),
        update_available: None,
        update_download_started: false,
        mpv_setup_started: false,
        close: WindowCloseState::default(),
        player_warmed: false,
    }));
    start_playback_event_bridge(&state, playback_event_rx);
    let weak = Arc::downgrade(&state);
    locator::on_services(move |services| {
        if let Some(state) = weak.upgrade() {
            connect_services(&state, services, playback);
        }
    });
    start_update_check_bridge(state.clone());
    state
}

fn connect_services(
    state: &BrowserState,
    services: &Arc<Services>,
    playback: Arc<PlaybackCoordinator>,
) {
    services.attach_playback(playback);
    start_preferences_event_bridge(state, services);
    start_shell_request_bridge(state, services);
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

#[cfg(all(test, target_os = "linux"))]
fn test_browser_state(playback: Arc<PlaybackCoordinator>) -> BrowserState {
    Arc::new(Mutex::new(BrowserStateInner {
        title: "Test".to_string(),
        settings: AppSettings::default(),
        browsers: Vec::new(),
        main_window: None,
        initial_show_state: ShowState::NORMAL,
        reveal: recovery::WindowRevealState::new(false),
        renderer_recovery: recovery::RendererRecoveryState::default(),
        playback,
        update_available: None,
        update_download_started: false,
        mpv_setup_started: false,
        close: WindowCloseState::default(),
        player_warmed: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state(close_behavior: CloseBehavior) -> BrowserState {
        let settings = AppSettings {
            close_behavior,
            ..AppSettings::default()
        };
        let (tx, _rx) = mpsc::channel();
        let playback = Arc::new(PlaybackCoordinator::new(tx, |events| {
            build_backend(&settings, events)
        }));
        Arc::new(Mutex::new(BrowserStateInner {
            title: "MediaFlick Desktop".to_string(),
            settings,
            browsers: Vec::new(),
            main_window: None,
            initial_show_state: ShowState::NORMAL,
            reveal: recovery::WindowRevealState::new(false),
            renderer_recovery: recovery::RendererRecoveryState::default(),
            playback,
            update_available: None,
            update_download_started: false,
            mpv_setup_started: false,
            close: WindowCloseState::default(),
            player_warmed: false,
        }))
    }

    #[test]
    fn guarded_post_skips_callback_once_shutdown_starts() {
        let shutting_down = AtomicBool::new(false);
        let mut called = 0usize;
        assert_eq!(
            guarded_post(&shutting_down, || {
                called += 1;
                true
            }),
            PostOutcome::Posted
        );
        assert_eq!(called, 1);

        assert_eq!(
            guarded_post(&shutting_down, || {
                called += 1;
                false
            }),
            PostOutcome::Failed
        );
        assert_eq!(called, 2);

        shutting_down.store(true, Ordering::SeqCst);
        assert_eq!(
            guarded_post(&shutting_down, || {
                called += 1;
                true
            }),
            PostOutcome::ShuttingDown
        );
        assert_eq!(called, 2);
    }

    #[test]
    fn non_close_before_unload_does_not_notify_window_close_blocked() {
        let mut close = WindowCloseState::default();
        assert_eq!(
            close.on_before_unload(false),
            BeforeUnloadDecision {
                proceed: false,
                notify_window_close_blocked: false,
            }
        );
        assert_eq!(
            close.on_before_unload(true),
            BeforeUnloadDecision {
                proceed: false,
                notify_window_close_blocked: false,
            }
        );
    }

    #[test]
    fn pending_window_close_notifies_once_and_does_not_leak_to_later_navigation() {
        let state = test_state(CloseBehavior::ExitApp);
        let closed = request_main_window_close_with(
            Some(&state),
            || false,
            || {
                panic!("Exit close behavior must not minimize");
            },
        );
        assert!(!closed);

        let first = state.lock().unwrap().close.on_before_unload(false);
        assert_eq!(
            first,
            BeforeUnloadDecision {
                proceed: false,
                notify_window_close_blocked: true,
            }
        );

        let later_navigation = state.lock().unwrap().close.on_before_unload(false);
        assert_eq!(
            later_navigation,
            BeforeUnloadDecision {
                proceed: false,
                notify_window_close_blocked: false,
            }
        );

        let second_attempt = request_main_window_close_with(
            Some(&state),
            || false,
            || {
                panic!("Exit close behavior must not minimize");
            },
        );
        assert!(!second_attempt);
        assert_eq!(
            state.lock().unwrap().close.on_before_unload(false),
            BeforeUnloadDecision {
                proceed: false,
                notify_window_close_blocked: true,
            }
        );
    }

    #[test]
    fn minimize_close_behavior_does_not_arm_window_close_blocked() {
        let state = test_state(CloseBehavior::MinimizeWindow);
        let mut minimized = false;
        let closed = request_main_window_close_with(
            Some(&state),
            || panic!("MinimizeWindow must not call try_close"),
            || minimized = true,
        );
        assert!(!closed);
        assert!(minimized);
        assert_eq!(
            state.lock().unwrap().close.on_before_unload(false),
            BeforeUnloadDecision {
                proceed: false,
                notify_window_close_blocked: false,
            }
        );
    }

    #[test]
    fn completed_close_attempt_clears_pending_flag() {
        let state = test_state(CloseBehavior::ExitApp);
        let closed = request_main_window_close_with(Some(&state), || true, || {});
        assert!(closed);
        assert_eq!(
            state.lock().unwrap().close.on_before_unload(false),
            BeforeUnloadDecision {
                proceed: false,
                notify_window_close_blocked: false,
            }
        );
    }

    #[test]
    fn reload_during_pending_close_does_not_notify_window_close_blocked() {
        let state = test_state(CloseBehavior::ExitApp);
        let closed = request_main_window_close_with(Some(&state), || false, || {});
        assert!(!closed);
        assert_eq!(
            state.lock().unwrap().close.on_before_unload(true),
            BeforeUnloadDecision {
                proceed: false,
                notify_window_close_blocked: false,
            }
        );
    }

    #[test]
    fn force_close_allows_unload_without_window_close_blocked() {
        let mut close = WindowCloseState::default();
        close.begin_close();
        close.request_force_close();
        assert!(close.is_exiting());
        assert_eq!(
            close.on_before_unload(false),
            BeforeUnloadDecision {
                proceed: true,
                notify_window_close_blocked: false,
            }
        );
        assert_eq!(
            close.on_before_unload(true),
            BeforeUnloadDecision {
                proceed: true,
                notify_window_close_blocked: false,
            }
        );
    }

    #[test]
    fn exiting_tracks_accepted_and_forced_closes_but_not_blocked_or_minimized_closes() {
        let mut close = WindowCloseState::default();
        assert!(!close.is_exiting());
        close.begin_close();
        close.finish_close_attempt(false);
        assert!(!close.is_exiting());
        close.begin_close();
        close.finish_close_attempt(true);
        assert!(close.is_exiting());

        let mut shutdown = WindowCloseState::default();
        shutdown.mark_shutting_down();
        assert!(shutdown.is_exiting());
    }
}
