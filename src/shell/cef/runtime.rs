use super::handlers::{ShellClient, cef_i32};
use super::*;

#[cfg(not(target_os = "windows"))]
wrap_app! {
    struct RelaunchApp {}

    impl App {}
}

#[cfg(not(target_os = "windows"))]
pub fn notify_already_running() {
    #[cfg(target_os = "macos")]
    let _framework = match current_exe_path()
        .ok_or_else(|| "could not resolve the executable path".to_string())
        .and_then(|exe| mac::load_framework(&exe))
    {
        Ok(framework) => framework,
        Err(error) => {
            tracing::warn!(target: "app", "could not load CEF for relaunch handoff: {error}");
            return;
        }
    };

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = args::Args::new();
    let mut app = RelaunchApp::new();
    if execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    ) >= 0
    {
        return;
    }

    #[cfg(target_os = "macos")]
    if let Err(error) = mac::install_application() {
        tracing::warn!(target: "app", "could not install macOS application for relaunch handoff: {error}");
        return;
    }

    let paths = RuntimePaths::new();
    if !paths.cache_dir.is_dir() {
        return;
    }

    let cache_path = paths.cache_dir.to_string_lossy();
    let settings = Settings {
        no_sandbox: 1,
        browser_subprocess_path: cef_string_from_path(paths.browser_subprocess_path.as_ref()),
        cache_path: CefString::from(cache_path.as_ref()),
        root_cache_path: CefString::from(cache_path.as_ref()),
        log_severity: LogSeverity::DISABLE,
        resources_dir_path: cef_string_from_path(paths.resources_dir_path.as_ref()),
        locales_dir_path: cef_string_from_path(paths.locales_dir_path.as_ref()),
        framework_dir_path: cef_string_from_path(paths.framework_dir_path.as_ref()),
        disable_signal_handlers: 1,
        ..Default::default()
    };

    CEF_SHUTTING_DOWN.store(false, Ordering::Release);
    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) == 1
    {
        mark_cef_shutting_down();
        shutdown();
    }
}

wrap_app! {
    pub struct ShellApp {
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

            command_line.append_switch_with_value(
                Some(&CefString::from("disable-features")),
                Some(&CefString::from("OverscrollHistoryNavigation")),
            );

            #[cfg(target_os = "linux")]
            if libmpv_overlay::is_configured(&self.config.settings) {
                let wayland = crate::players::mpv::use_native_wayland();
                command_line.append_switch_with_value(
                    Some(&CefString::from("ozone-platform")),
                    Some(&CefString::from(if wayland { "wayland" } else { "x11" })),
                );
                if wayland {
                    for (name, value) in [("use-gl", "angle"), ("use-angle", "gl-egl")] {
                        command_line.append_switch_with_value(
                            Some(&CefString::from(name)),
                            Some(&CefString::from(value)),
                        );
                    }
                } else {
                    command_line.append_switch(Some(&CefString::from("disable-gpu-compositing")));
                }
                command_line.append_switch_with_value(
                    Some(&CefString::from("blink-settings")),
                    Some(&CefString::from(
                        "primaryPointerType=4,availablePointerTypes=4,primaryHoverType=2,availableHoverTypes=2",
                    )),
                );
            }

            #[cfg(target_os = "windows")]
            {
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
            if let Some(options) = cef_i32(scheme_options) {
                registrar.add_custom_scheme(Some(&scheme), options);
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(JellyfinBrowserProcessHandler::new(
                RefCell::new(None),
                RefCell::new(None),
                self.config.clone(),
            ))
        }
    }
}

wrap_browser_process_handler! {
    struct JellyfinBrowserProcessHandler {
        client: RefCell<Option<Client>>,
        state: RefCell<Option<BrowserState>>,
        config: AppConfig,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);

            app_scheme::register();
            locator::init_with_settings(self.config.settings.clone());

            let show_state = if self.config.hidden {
                ShowState::HIDDEN
            } else if self.config.settings.webui_window.maximized {
                ShowState::MAXIMIZED
            } else {
                ShowState::NORMAL
            };
            let settings = locator::services()
                .map(|services| services.preferences.snapshot())
                .unwrap_or_else(|| self.config.settings.clone());
            let handler_state = new_browser_state(
                self.config.title.clone(),
                settings.clone(),
                show_state,
            );
            *self.state.borrow_mut() = Some(handler_state.clone());
            #[cfg(target_os = "macos")]
            mac::quit_through(&handler_state);
            let url = CefString::from(app_scheme::APP_URL);
            let runtime_style = RuntimeStyle::ALLOY;

            let playback = handler_state
                .lock()
                .ok()
                .map(|state| state.playback.clone());
            if let Some(surface) = playback.and_then(|playback| {
                libmpv_overlay::LibmpvOverlaySurface::select(
                    &settings,
                    playback,
                    handler_state.clone(),
                )
            }) {
                #[cfg(target_os = "linux")]
                let bind_result = if crate::players::mpv::use_native_wayland() {
                    let result = configured_player_wayland_ui(
                        &handler_state,
                        std::time::Duration::from_secs(20),
                    )
                    .ok_or_else(|| "libmpv did not expose a Wayland UI handle".to_string())
                    .and_then(|ui| surface.bind_wayland(ui));
                    prepare_player_for_window(&handler_state);
                    result
                } else {
                    prepare_player_for_window(&handler_state);
                    configured_player_native_window(
                        &handler_state,
                        std::time::Duration::from_secs(20),
                    )
                    .ok_or_else(|| "libmpv did not expose a native window".to_string())
                    .and_then(|window| surface.bind(window))
                };
                #[cfg(not(target_os = "linux"))]
                let bind_result = {
                    prepare_player_for_window(&handler_state);
                    configured_player_native_window(
                        &handler_state,
                        std::time::Duration::from_secs(20),
                    )
                    .ok_or_else(|| "libmpv did not expose a native window".to_string())
                    .and_then(|window| surface.bind(window))
                };
                match bind_result {
                    Ok(()) => {
                        let mut client =
                            ShellClient::new(handler_state.clone(), Some(surface.clone()));
                        if surface.create_browser(&mut client).is_some() {
                            *self.client.borrow_mut() = Some(client);
                            surface.show();
                            schedule_reveal_deadline(&handler_state);
                            return;
                        }
                        tracing::error!(target: "cef.osr", "failed to create the windowless React browser");
                        surface.destroy();
                    }
                    Err(error) => {
                        tracing::error!(target: "cef.osr", "failed to bind the libmpv overlay: {error}");
                    }
                }
            }

            *self.client.borrow_mut() = Some(ShellClient::new(handler_state.clone(), None));
            let mut client = self.default_client();
            let browser_settings = BrowserSettings::default();
            let mut browser_delegate = JellyfinBrowserViewDelegate::new(runtime_style);
            let browser_view = browser_view_create(
                client.as_mut(),
                Some(&url),
                Some(&browser_settings),
                None,
                None,
                Some(&mut browser_delegate),
            );

            let Some(browser_view) = browser_view else {
                eprintln!("Failed to create CEF BrowserView");
                mark_cef_shutting_down();
                quit_message_loop();
                return;
            };

            let mut window_delegate = JellyfinWindowDelegate::new(
                RefCell::new(Some(browser_view)),
                runtime_style,
                show_state,
                true,
                WindowIdentity {
                    title: self.config.title.clone(),
                    settings: settings.webui_window,
                },
                Some(handler_state.clone()),
            );
            window_create_top_level(Some(&mut window_delegate));
            schedule_reveal_deadline(&handler_state);
        }

        fn on_already_running_app_relaunch(
            &self,
            _command_line: Option<&mut CommandLine>,
            _current_directory: Option<&CefString>,
        ) -> std::os::raw::c_int {
            tracing::info!(
                target: "app",
                "another launch attempted to reuse this running instance; focusing existing window"
            );
            if let Some(state) = self.state.borrow().as_ref() {
                focus_main_window(state);
            }
            1
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
                false,
                WindowIdentity {
                    title: "MediaFlick Desktop".to_string(),
                    settings: WebUiWindowSettings::default(),
                },
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

#[derive(Clone)]
struct WindowIdentity {
    title: String,
    settings: WebUiWindowSettings,
}

wrap_window_delegate! {
    struct JellyfinWindowDelegate {
        browser_view: RefCell<Option<BrowserView>>,
        runtime_style: RuntimeStyle,
        initial_show_state: ShowState,
        defer_show: bool,
        identity: WindowIdentity,
        state: Option<BrowserState>,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            let (width, height) = self.identity.settings.size();
            Size {
                width,
                height,
            }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn linux_window_properties(
            &self,
            _window: Option<&mut Window>,
            properties: Option<&mut LinuxWindowProperties>,
        ) -> i32 {
            let Some(properties) = properties else {
                return 0;
            };
            properties.wayland_app_id = linux_desktop_id();
            properties.wm_class_class = linux_desktop_id();
            properties.wm_class_name = linux_desktop_id();
            1
        }

        fn on_window_created(&self, window: Option<&mut Window>) {
            let Some(window) = window else {
                return;
            };
            window.set_title(Some(&CefString::from(self.identity.title.as_str())));
            set_window_icon(window);
            if let Some((x, y)) = self.identity.settings.position() {
                let (width, height) = self.identity.settings.size();
                window.set_bounds(Some(&Rect {
                    x,
                    y,
                    width,
                    height,
                }));
            }
            if let Some(state) = &self.state {
                register_main_window(state, window);
            }

            let browser_view = self.browser_view.borrow();
            let Some(browser_view) = browser_view.as_ref() else {
                return;
            };

            let mut view = View::from(browser_view);
            window.add_child_view(Some(&mut view));
            if let Some(state) = &self.state {
                prepare_player_for_window(state);
            }

            if !self.defer_show {
                if self.initial_show_state == ShowState::MAXIMIZED {
                    window.maximize();
                }
                if self.initial_show_state != ShowState::HIDDEN {
                    window.show();
                }
            }
        }

        fn on_window_closing(&self, window: Option<&mut Window>) {
            update_webui_window_from_window(self.state.as_ref(), window.as_deref());
            save_webui_window_settings(self.state.as_ref());
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            if let Some(state) = &self.state {
                clear_main_window(state);
            }
            *self.browser_view.borrow_mut() = None;
        }

        fn on_window_bounds_changed(
            &self,
            window: Option<&mut Window>,
            new_bounds: Option<&Rect>,
        ) {
            update_webui_window_settings(self.state.as_ref(), window.as_deref(), new_bounds);
        }

        fn with_standard_window_buttons(&self, _window: Option<&mut Window>) -> i32 {
            1
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
            let browser = self
                .browser_view
                .borrow()
                .as_ref()
                .and_then(BrowserView::browser);
            i32::from(request_main_window_close(self.state.as_ref(), browser, || {
                update_webui_window_from_window(self.state.as_ref(), window.as_deref());
                save_webui_window_settings(self.state.as_ref());
                if let Some(window) = window {
                    window.minimize();
                }
            }))
        }

        fn initial_show_state(&self, _window: Option<&mut Window>) -> ShowState {
            if self.defer_show {
                ShowState::HIDDEN
            } else {
                self.initial_show_state
            }
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            self.runtime_style
        }
    }
}
