use super::bridge::*;
use super::document::*;
use super::events::*;
use super::*;

wrap_client! {
    pub(super) struct JellyfinClient {
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

pub(super) fn cef_i32<T>(value: T) -> i32
where
    T: TryInto<i32>,
    T::Error: std::fmt::Debug,
{
    value
        .try_into()
        .unwrap_or_else(|error| panic!("CEF enum value does not fit in i32: {error:?}"))
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
                    services.socket.stop();
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

            // Main-frame navigation failures are replaced with the native
            // recovery document below. It has no React startup cover to report
            // readiness, so reveal it once CEF confirms that document loaded.
            let frame_url = CefString::from(&frame.url()).to_string();
            if frame_url.starts_with("data:text/html") {
                reveal_main_window(&self.state);
            }
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
                    request_url,
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
