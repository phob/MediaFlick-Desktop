//! What the native app pushes into the UI's pages. Each [`Channel`] names
//! where a page listens and the wire type it receives there, and it is the
//! only way to hand a page a payload, so no `json!` value reaches a script.

use std::marker::PhantomData;

use serde::Serialize;
use ts_rs::TS;

use super::api::Wire;
use super::document::js_json;
use super::*;
use crate::playback::PlayerSnapshot;

/// Where a page receives one kind of payload, and the wire type it is.
pub(super) struct Channel<T> {
    listener: Listener,
    source: &'static str,
    payload: PhantomData<fn(&T)>,
}

#[derive(Clone, Copy)]
enum Listener {
    /// A function the UI installs on `window`; a page without it ignores the
    /// payload.
    Hook(&'static str),
    /// A `CustomEvent` dispatched on `window`.
    Event(&'static str),
    /// A self-contained script that renders the payload JSON itself.
    Script(fn(&str) -> String),
}

impl<T: Serialize + Wire> Channel<T> {
    const fn new(listener: Listener, source: &'static str) -> Self {
        Self {
            listener,
            source,
            payload: PhantomData,
        }
    }

    fn script(&self, payload: &T) -> String {
        let json = js_json(payload);
        match self.listener {
            Listener::Hook(hook) => format!("window.{hook}&&window.{hook}({json});"),
            Listener::Event(name) => {
                format!("window.dispatchEvent(new CustomEvent('{name}', {{ detail: {json} }}));")
            }
            Listener::Script(render) => render(&json),
        }
    }

    /// Sends the payload to every page, and returns how many received it.
    pub(super) fn broadcast(&self, state: &BrowserState, payload: &T) -> usize {
        let browsers = state
            .lock()
            .map(|state| state.browsers.clone())
            .unwrap_or_default();
        let script = self.script(payload);
        let mut frame_count = 0;
        for frame in browsers.iter().filter_map(Browser::main_frame) {
            self.run(&frame, &script);
            frame_count += 1;
        }
        frame_count
    }

    pub(super) fn send(&self, frame: &Frame, payload: &T) {
        self.run(frame, &self.script(payload));
    }

    fn run(&self, frame: &Frame, script: &str) {
        frame.execute_java_script(
            Some(&CefString::from(script)),
            Some(&CefString::from(self.source)),
            1,
        );
    }
}

pub(super) const SHELL_EVENTS: Channel<ShellEvent> = Channel::new(
    Listener::Event("mediaflick-desktop-shell"),
    "mediaflick-desktop://shell-event",
);
pub(super) const PLAYBACK_STATE_CHANGED: Channel<PlayerSnapshot> = Channel::new(
    Listener::Hook("__mediaFlickDesktopPlaybackStateChanged"),
    "mediaflick-desktop://playback-event",
);
pub(super) const PLAYBACK_STOPPED: Channel<PlayerSnapshot> = Channel::new(
    Listener::Hook("__mediaFlickDesktopPlaybackStopped"),
    "mediaflick-desktop://playback-event",
);
pub(super) const PLAYBACK_CACHE_REFRESHED: Channel<PlaybackCacheRefreshed> = Channel::new(
    Listener::Hook("__mediaFlickDesktopPlaybackCacheRefreshed"),
    "mediaflick-desktop://playback-cache-refreshed",
);
pub(super) const UPDATE_AVAILABLE: Channel<UpdateRelease> = Channel::new(
    Listener::Script(updater::update_available_script),
    UPDATE_TOAST_SOURCE,
);
pub(super) const UPDATE_PROGRESS: Channel<UpdateProgress> = Channel::new(
    Listener::Hook("__mediaFlickDesktopUpdateProgress"),
    UPDATE_TOAST_SOURCE,
);
pub(super) const ERROR_TOAST: Channel<ErrorToast> = Channel::new(
    Listener::Script(error_toast::error_toast_script),
    "mediaflick-desktop://error-toast",
);
pub(super) const ABOUT_DIALOG: Channel<AboutInfo> = Channel::new(
    Listener::Script(about::dialog_script),
    "mediaflick-desktop://app-about",
);

const UPDATE_TOAST_SOURCE: &str = "mediaflick-desktop://update-toast";

/// A message on the `mediaflick-desktop-shell` event. Listeners tell them
/// apart by `type`.
#[derive(Debug, Serialize, TS)]
#[serde(tag = "type", content = "payload", rename_all = "kebab-case")]
pub(super) enum ShellEvent {
    LibraryChanged(ItemChanges),
    LiveChanged(LiveChanges),
    CatalogChanged(CatalogChanges),
    CollectionsChanged {},
    LibrariesChanged {},
    LibraryMembershipsSettled {},
    JellyfinSessionExpired {},
    /// Closing the window waits for the Settings page to save or discard
    /// its drafts.
    WindowCloseBlocked {},
    MpvInstallProgress(MpvInstallProgress),
    FilePickerCompleted(FilePickerCompleted),
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct ItemChanges {
    pub(super) item_ids: Vec<String>,
    pub(super) context_ids: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct LiveChanges {
    pub(super) item_ids: Vec<String>,
    pub(super) context_ids: Vec<String>,
    /// Any page Jellyfin serves may have changed: the event stream was lost.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional = nullable)]
    pub(super) all: bool,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct CatalogChanges {
    pub(super) item_ids: Vec<String>,
    pub(super) context_ids: Vec<String>,
    pub(super) library_ids: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct MpvInstallProgress {
    pub(super) request_id: String,
    #[serde(flatten)]
    pub(super) state: MpvInstallState,
}

#[derive(Debug, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(super) enum MpvInstallState {
    Downloading { downloaded: u64, total: Option<u64> },
    Extracting,
    Completed { path: String },
    Failed { message: String },
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct FilePickerCompleted {
    pub(super) request_id: String,
    pub(super) path: Option<String>,
    pub(super) error: Option<String>,
}

/// Jellyfin's view of an item after its final playstate report, or a
/// library refresh scheduled in its place.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlaybackCacheRefreshed {
    pub(super) item_id: String,
    pub(super) status: PlaybackCacheRefreshOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub(super) enum PlaybackCacheRefreshOutcome {
    Refreshed,
    Deferred,
}

#[derive(Debug, Serialize, TS)]
#[serde(tag = "state", content = "payload", rename_all = "camelCase")]
pub(super) enum UpdateProgress {
    Downloading(DownloadProgress),
    Installing(DownloadProgress),
    Error { message: String },
}

#[derive(Debug, Serialize, TS)]
pub(super) struct DownloadProgress {
    pub(super) downloaded: u64,
    pub(super) total: Option<u64>,
}

#[derive(Debug, Serialize, TS)]
pub(super) struct ErrorToast {
    pub(super) title: String,
    pub(super) body: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct AboutInfo {
    app_name: &'static str,
    version: &'static str,
    git_version: &'static str,
    created_by: &'static str,
}

impl AboutInfo {
    pub(super) const CURRENT: Self = Self {
        app_name: about::APP_NAME,
        version: about::APP_VERSION,
        git_version: about::GIT_VERSION,
        created_by: about::CREATED_BY,
    };
}
