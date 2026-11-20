//! What a CEF app must do itself on macOS before CEF runs. The framework is
//! not linked into the executable, so every process loads it at run time
//! before its first CEF call; and Chromium's event loop needs `NSApp` to be an
//! `NSApplication` that speaks its `CrAppProtocol`. That `NSApp` also turns
//! the system's quit requests into MediaFlick's orderly exit.

use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObjectProtocol};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{NSApplication, NSEvent, NSMenu, NSMenuItem};
use objc2_foundation::ns_string;

use super::BrowserState;
use super::bridge::initiate_app_exit;

const FRAMEWORK: &str = "Chromium Embedded Framework.framework";
const HELPER: &str = "MediaFlick Desktop Helper";

thread_local! {
    /// The main window's state, set on CEF's UI thread, which on macOS is the
    /// main thread that receives `terminate:`. Set whether or not the services
    /// opened, so the startup error screen's window closes the same way.
    static EXIT_STATE: RefCell<Option<BrowserState>> = const { RefCell::new(None) };
}

/// Makes the system's quit requests close the app through `state`.
pub(super) fn quit_through(state: &BrowserState) {
    EXIT_STATE.with(|slot| *slot.borrow_mut() = Some(state.clone()));
}

/// Where the framework sits for the given executable. The app's own binary
/// (`Contents/MacOS/`) finds it in `Contents/Frameworks/`; a helper's binary
/// (`Contents/Frameworks/<helper>.app/Contents/MacOS/`) three levels up.
pub(super) fn framework_dir(exe: &Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    [
        macos_dir.join("../Frameworks").join(FRAMEWORK),
        macos_dir.join("../../..").join(FRAMEWORK),
    ]
    .into_iter()
    .find_map(|path| path.canonicalize().ok())
}

/// The helper Chromium starts its subprocesses from. Chromium derives the
/// GPU, Renderer, Plugin and Alerts variants from this path, so the bundle
/// ships all of them beside it (`distribution/macos/build-dmg.sh`).
pub(super) fn helper_executable(contents_dir: &Path) -> PathBuf {
    contents_dir
        .join("Frameworks")
        .join(format!("{HELPER}.app"))
        .join("Contents/MacOS")
        .join(HELPER)
}

/// Keeps the framework loaded; unloads it when dropped, after CEF shut down.
pub(super) struct LoadedFramework;

impl Drop for LoadedFramework {
    fn drop(&mut self) {
        if cef::unload_library() != 1 {
            eprintln!("could not unload the Chromium Embedded Framework");
        }
    }
}

pub(super) fn load_framework(exe: &Path) -> Result<LoadedFramework, String> {
    let framework = framework_dir(exe)
        .ok_or_else(|| format!("{FRAMEWORK} not found beside {}", exe.display()))?;
    let binary = framework.join("Chromium Embedded Framework");
    let path = CString::new(binary.as_os_str().as_bytes())
        .map_err(|_| format!("framework path contains a NUL byte: {}", binary.display()))?;
    // SAFETY: `path` is a NUL-terminated C string that outlives the call;
    // cef_load_library reads it as `const char*` and keeps no reference.
    if cef::load_library(Some(unsafe { &*path.as_ptr() })) != 1 {
        return Err(format!("could not load {}", binary.display()));
    }
    Ok(LoadedFramework)
}

#[derive(Default)]
struct ApplicationIvars {
    handling_send_event: Cell<bool>,
}

define_class!(
    /// `NSApp` for the browser process. Chromium asks it whether an event is
    /// being dispatched (`CrAppProtocol`) to decide when its nested message
    /// loops may run.
    #[unsafe(super(NSApplication))]
    #[ivars = ApplicationIvars]
    struct MediaFlickApplication;

    impl MediaFlickApplication {
        #[unsafe(method(sendEvent:))]
        fn send_event(&self, event: &NSEvent) {
            let was_handling = self.ivars().handling_send_event.replace(true);
            // SAFETY: NSApplication's sendEvent: takes one NSEvent and
            // returns nothing, matching this call.
            let _: () = unsafe { msg_send![super(self), sendEvent: event] };
            self.ivars().handling_send_event.set(was_handling);
        }

        /// Every quit path ends here: the menu's Quit (Cmd+Q), the Dock's
        /// Quit and the quit Apple event. NSApplication's own version calls
        /// exit() at once, which would skip the playstate flush and settings
        /// save that closing the browsers runs.
        #[unsafe(method(terminate:))]
        fn terminate(&self, _sender: Option<&AnyObject>) {
            match EXIT_STATE.with(|slot| slot.borrow().clone()) {
                Some(state) => initiate_app_exit(&state),
                None => {
                    // No window exists yet, so there is nothing to flush or save.
                    tracing::info!(target: "app", "quit before the main window was created");
                    cef::quit_message_loop();
                }
            }
        }
    }

    unsafe impl NSObjectProtocol for MediaFlickApplication {}

    unsafe impl CrAppProtocol for MediaFlickApplication {
        #[unsafe(method(isHandlingSendEvent))]
        fn is_handling_send_event(&self) -> Bool {
            Bool::new(self.ivars().handling_send_event.get())
        }
    }

    unsafe impl CrAppControlProtocol for MediaFlickApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        fn set_handling_send_event(&self, handling: Bool) {
            self.ivars().handling_send_event.set(handling.as_bool());
        }
    }

    unsafe impl CefAppProtocol for MediaFlickApplication {}
);

/// Makes `NSApp` a `MediaFlickApplication` with an app menu whose Quit item
/// is Cmd+Q. Must run on the main thread before CEF or anything else touches
/// `NSApp`, which would create a plain `NSApplication` instead.
pub(super) fn install_application() -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("CEF must start on the main thread")?;
    // SAFETY: +sharedApplication takes no arguments and returns the
    // retained shared instance, which this subclass of NSApplication is.
    let app: Retained<NSApplication> =
        unsafe { msg_send![MediaFlickApplication::class(), sharedApplication] };
    if !app.isKindOfClass(MediaFlickApplication::class()) {
        return Err("NSApp was created before MediaFlick could install its own".to_string());
    }

    // SAFETY: terminate: is a valid NSApplication action taking the sender.
    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Quit MediaFlick Desktop"),
            Some(sel!(terminate:)),
            ns_string!("q"),
        )
    };
    let app_menu = NSMenu::new(mtm);
    app_menu.addItem(&quit);
    let app_menu_item = NSMenuItem::new(mtm);
    app_menu_item.setSubmenu(Some(&app_menu));
    let menu_bar = NSMenu::new(mtm);
    menu_bar.addItem(&app_menu_item);
    app.setMainMenu(Some(&menu_bar));
    Ok(())
}
