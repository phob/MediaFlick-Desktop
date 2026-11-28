use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2::{AnyThread, define_class, msg_send, sel};
use objc2_foundation::{NSObject, NSString};

use super::{OsMediaAction, OsMediaState, OsPlaybackStatus};

#[link(name = "MediaPlayer", kind = "framework")]
unsafe extern "C" {}

type ActionHandler = Arc<dyn Fn(OsMediaAction) + Send + Sync>;

static GLOBAL_HANDLER: OnceLock<Mutex<Option<ActionHandler>>> = OnceLock::new();

fn dispatch(action: OsMediaAction) {
    let handler = GLOBAL_HANDLER
        .get()
        .and_then(|lock| lock.lock().unwrap_or_else(PoisonError::into_inner).clone());
    if let Some(handler) = handler {
        handler(action);
    }
}

struct TargetIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = TargetIvars]
    struct RemoteCommandTarget;

    impl RemoteCommandTarget {
        #[unsafe(method(onPlay:))]
        fn on_play(&self, _event: *const AnyObject) -> isize {
            dispatch(OsMediaAction::Play);
            0
        }

        #[unsafe(method(onPause:))]
        fn on_pause(&self, _event: *const AnyObject) -> isize {
            dispatch(OsMediaAction::Pause);
            0
        }

        #[unsafe(method(onTogglePlayPause:))]
        fn on_toggle(&self, _event: *const AnyObject) -> isize {
            dispatch(OsMediaAction::PlayPause);
            0
        }

        #[unsafe(method(onStop:))]
        fn on_stop(&self, _event: *const AnyObject) -> isize {
            dispatch(OsMediaAction::Stop);
            0
        }

        #[unsafe(method(onNext:))]
        fn on_next(&self, _event: *const AnyObject) -> isize {
            dispatch(OsMediaAction::Next);
            0
        }

        #[unsafe(method(onPrev:))]
        fn on_prev(&self, _event: *const AnyObject) -> isize {
            dispatch(OsMediaAction::Previous);
            0
        }

        #[unsafe(method(onChangePosition:))]
        fn on_change_position(&self, event: *const AnyObject) -> isize {
            if !event.is_null() {
                // SAFETY: `MPRemoteCommandCenter` delivers an `MPChangePlaybackPositionCommandEvent` with selector `positionTime`.
                let secs: f64 = unsafe { msg_send![event, positionTime] };
                if secs.is_finite() && secs >= 0.0 {
                    dispatch(OsMediaAction::SetPositionMs((secs * 1000.0).max(0.0)));
                }
            }
            0
        }
    }
);

pub(super) struct MacosOsMedia {
    target: Option<Retained<RemoteCommandTarget>>,
}

// SAFETY: `RemoteCommandTarget` is an immutable NSObject stateless dispatcher backed by synchronized `GLOBAL_HANDLER`.
unsafe impl Send for MacosOsMedia {}

impl MacosOsMedia {
    pub(super) fn start(handler: ActionHandler) -> Self {
        let slot = GLOBAL_HANDLER.get_or_init(|| Mutex::new(None));
        *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(handler);

        let target = autoreleasepool(|_| {
            let target = {
                let alloc = RemoteCommandTarget::alloc().set_ivars(TargetIvars);
                // SAFETY: initializing subclass of `NSObject`.
                unsafe { msg_send![super(alloc), init] }
            };
            let target: Retained<RemoteCommandTarget> = target;
            // SAFETY: invoking documented MediaPlayer framework `MPRemoteCommandCenter` selectors on valid class/target.
            unsafe {
                if let Some(center_cls) = objc2::runtime::AnyClass::get(c"MPRemoteCommandCenter") {
                    let center: *mut AnyObject = msg_send![center_cls, sharedCommandCenter];
                    if !center.is_null() {
                        let target_obj: &AnyObject = &target;
                        for (cmd_sel, action_sel) in [
                            (sel!(playCommand), sel!(onPlay:)),
                            (sel!(pauseCommand), sel!(onPause:)),
                            (sel!(togglePlayPauseCommand), sel!(onTogglePlayPause:)),
                            (sel!(stopCommand), sel!(onStop:)),
                            (sel!(nextTrackCommand), sel!(onNext:)),
                            (sel!(previousTrackCommand), sel!(onPrev:)),
                            (sel!(changePlaybackPositionCommand), sel!(onChangePosition:)),
                        ] {
                            let cmd: *mut AnyObject = msg_send![center, performSelector: cmd_sel];
                            if !cmd.is_null() {
                                let _: () = msg_send![cmd, setEnabled: true];
                                let _: () =
                                    msg_send![cmd, addTarget: target_obj, action: action_sel];
                            }
                        }
                    }
                }
            }
            target
        });
        Self {
            target: Some(target),
        }
    }

    pub(super) fn update(&self, state: &OsMediaState) {
        autoreleasepool(|_| {
            // SAFETY: calling documented `MPNowPlayingInfoCenter` selectors cleanly from worker thread.
            unsafe {
                let Some(info_cls) = objc2::runtime::AnyClass::get(c"MPNowPlayingInfoCenter")
                else {
                    return;
                };
                let center: *mut AnyObject = msg_send![info_cls, defaultCenter];
                if center.is_null() {
                    return;
                }
                let playback_state: usize = match state.status {
                    OsPlaybackStatus::Stopped => 3,
                    OsPlaybackStatus::Playing => 1,
                    OsPlaybackStatus::Paused => 2,
                };
                let _: () = msg_send![center, setPlaybackState: playback_state];
                if state.status == OsPlaybackStatus::Stopped {
                    let _: () = msg_send![center, setNowPlayingInfo: std::ptr::null::<AnyObject>()];
                    return;
                }
                if let Some(meta) = &state.metadata
                    && let Some(dict_cls) = objc2::runtime::AnyClass::get(c"NSMutableDictionary")
                    && let Some(num_cls) = objc2::runtime::AnyClass::get(c"NSNumber")
                {
                    let dict: *mut AnyObject = msg_send![dict_cls, dictionary];
                    let k_title = NSString::from_str("title");
                    let v_title = NSString::from_str(&meta.title);
                    let _: () = msg_send![dict, setObject: &*v_title, forKey: &*k_title];
                    if let Some(artist) = &meta.artist {
                        let k_artist = NSString::from_str("artist");
                        let v_artist = NSString::from_str(artist);
                        let _: () = msg_send![dict, setObject: &*v_artist, forKey: &*k_artist];
                    }
                    if let Some(album) = &meta.album {
                        let k_album = NSString::from_str("albumTitle");
                        let v_album = NSString::from_str(album);
                        let _: () = msg_send![dict, setObject: &*v_album, forKey: &*k_album];
                    }
                    let k_dur = NSString::from_str("playbackDuration");
                    let v_dur: *mut AnyObject = msg_send![num_cls, numberWithDouble: (state.duration_us as f64) / 1_000_000.0];
                    let _: () = msg_send![dict, setObject: v_dur, forKey: &*k_dur];
                    let k_pos = NSString::from_str("MPNowPlayingInfoPropertyElapsedPlaybackTime");
                    let v_pos: *mut AnyObject = msg_send![num_cls, numberWithDouble: (state.position_us as f64) / 1_000_000.0];
                    let _: () = msg_send![dict, setObject: v_pos, forKey: &*k_pos];
                    let rate = if state.status == OsPlaybackStatus::Playing {
                        1.0_f64
                    } else {
                        0.0_f64
                    };
                    let k_rate = NSString::from_str("MPNowPlayingInfoPropertyPlaybackRate");
                    let v_rate: *mut AnyObject = msg_send![num_cls, numberWithDouble: rate];
                    let _: () = msg_send![dict, setObject: v_rate, forKey: &*k_rate];
                    let _: () = msg_send![center, setNowPlayingInfo: dict];
                }
            }
        });
    }
}

impl Drop for MacosOsMedia {
    fn drop(&mut self) {
        if let Some(slot) = GLOBAL_HANDLER.get() {
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = None;
        }
        if let Some(target) = self.target.take() {
            autoreleasepool(|_| {
                // SAFETY: clears `nowPlayingInfo` and detaches registered `RemoteCommandTarget`.
                unsafe {
                    if let Some(center_cls) =
                        objc2::runtime::AnyClass::get(c"MPRemoteCommandCenter")
                    {
                        let center: *mut AnyObject = msg_send![center_cls, sharedCommandCenter];
                        if !center.is_null() {
                            let target_obj: &AnyObject = &target;
                            for cmd_sel in [
                                sel!(playCommand),
                                sel!(pauseCommand),
                                sel!(togglePlayPauseCommand),
                                sel!(stopCommand),
                                sel!(nextTrackCommand),
                                sel!(previousTrackCommand),
                                sel!(changePlaybackPositionCommand),
                            ] {
                                let cmd: *mut AnyObject =
                                    msg_send![center, performSelector: cmd_sel];
                                if !cmd.is_null() {
                                    let _: () = msg_send![cmd, removeTarget: target_obj];
                                }
                            }
                        }
                    }
                    if let Some(info_cls) = objc2::runtime::AnyClass::get(c"MPNowPlayingInfoCenter")
                    {
                        let center: *mut AnyObject = msg_send![info_cls, defaultCenter];
                        if !center.is_null() {
                            let _: () =
                                msg_send![center, setNowPlayingInfo: std::ptr::null::<AnyObject>()];
                        }
                    }
                }
            });
        }
    }
}
