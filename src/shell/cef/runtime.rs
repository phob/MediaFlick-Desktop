use super::handlers::{JellyfinClient, cef_i32};
use super::*;

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

            // Trackpad history swipes navigate back and forward like a browser;
            // the shell owns one surface and has no history to leave.
            command_line.append_switch_with_value(
                Some(&CefString::from("disable-features")),
                Some(&CefString::from("OverscrollHistoryNavigation")),
            );

            #[cfg(target_os = "windows")]
            {
                // CEF's separate Windows GPU process can loop through
                // STATUS_BREAKPOINT exits with GL disabled. Keeping the GPU
                // service in-process avoids that crash loop for both browser modes.
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

            let show_state = if self.config.hidden {
                ShowState::HIDDEN
            } else if self.config.settings.webui_window.maximized {
                ShowState::MAXIMIZED
            } else {
                ShowState::NORMAL
            };
            let settings = services::services()
                .map(|services| services.preferences.snapshot())
                .unwrap_or_else(|| self.config.settings.clone());
            let handler_state = new_browser_state(
                self.config.title.clone(),
                settings.clone(),
                show_state,
            );
            let url = CefString::from(app_scheme::APP_URL);
            let runtime_style = RuntimeStyle::ALLOY;

            let playback = handler_state
                .lock()
                .ok()
                .map(|state| state.playback.clone());
            if let Some(surface) = playback.and_then(|playback| {
                prototype_osr::PrototypeOsrSurface::select(&settings, playback)
            }) {
                prepare_player_for_window(&handler_state);
                let native_window = configured_player_native_window(
                    &handler_state,
                    std::time::Duration::from_secs(20),
                );
                if let Some(native_window) = native_window {
                    match surface.bind(native_window) {
                        Ok(()) => {
                            let mut client = JellyfinClient::new(
                                handler_state.clone(),
                                Some(surface.clone()),
                            );
                            if surface.create_browser(&mut client).is_some() {
                                *self.client.borrow_mut() = Some(client);
                                surface.show();
                                return;
                            }
                            tracing::error!(target: "cef.osr", "failed to create the windowless React browser");
                            surface.destroy();
                        }
                        Err(error) => {
                            tracing::error!(target: "cef.osr", "failed to bind the libmpv overlay: {error}");
                        }
                    }
                } else {
                    tracing::error!(target: "cef.osr", "libmpv did not expose a native window; using the regular CEF shell");
                }
            }

            *self.client.borrow_mut() = Some(JellyfinClient::new(handler_state.clone(), None));
            let mut client = self.default_client();
            let settings = BrowserSettings::default();
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

            let mut window_delegate = JellyfinWindowDelegate::new(
                RefCell::new(Some(browser_view)),
                runtime_style,
                show_state,
                true,
                WindowIdentity {
                    title: self.config.title.clone(),
                    settings: self.config.settings.webui_window,
                },
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
            if should_minimize_instead_of_close(self.state.as_ref()) {
                update_webui_window_from_window(self.state.as_ref(), window.as_deref());
                save_webui_window_settings(self.state.as_ref());
                if let Some(window) = window {
                    window.minimize();
                }
                return 0;
            }

            let browser_view = self.browser_view.borrow();
            let browser = browser_view
                .as_ref()
                .and_then(BrowserView::browser);
            let Some(browser) = browser else {
                return 1;
            };
            let Some(browser_host) = browser.host() else {
                return 1;
            };
            browser_host.try_close_browser()
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
