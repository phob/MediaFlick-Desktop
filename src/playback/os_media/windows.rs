use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::Playback::MediaPlayer;
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::core::{HSTRING, Result as WinResult};

use super::{OsMediaAction, OsMediaMetadata, OsMediaState, OsPlaybackStatus};

type ActionHandler = Arc<dyn Fn(OsMediaAction) + Send + Sync>;

enum WorkerMsg {
    Update(OsMediaState),
    Stop,
}

pub(super) struct WindowsOsMedia {
    tx: Sender<WorkerMsg>,
    thread: Option<JoinHandle<()>>,
}

impl WindowsOsMedia {
    pub(super) fn start(handler: ActionHandler) -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("os-media-smtc".to_string())
            .spawn(move || run_smtc_worker(&rx, &handler))
            .ok();
        Self { tx, thread }
    }

    pub(super) fn update(&self, state: &OsMediaState) {
        let _ = self.tx.send(WorkerMsg::Update(state.clone()));
    }
}

impl Drop for WindowsOsMedia {
    fn drop(&mut self) {
        let _ = self.tx.send(WorkerMsg::Stop);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: paired with `CoInitializeEx` on this worker thread.
        unsafe {
            windows_sys::Win32::System::Com::CoUninitialize();
        }
    }
}

fn configure_controls(
    controls: &SystemMediaTransportControls,
    handler: &ActionHandler,
) -> WinResult<()> {
    controls.SetIsEnabled(true)?;
    controls.SetIsPlayEnabled(true)?;
    controls.SetIsPauseEnabled(true)?;
    controls.SetIsStopEnabled(true)?;
    controls.SetIsNextEnabled(false)?;
    controls.SetIsPreviousEnabled(false)?;

    let btn_handler = Arc::clone(handler);
    controls.ButtonPressed(&TypedEventHandler::<
        SystemMediaTransportControls,
        SystemMediaTransportControlsButtonPressedEventArgs,
    >::new(move |_sender, args| {
        if let Some(args) = args.as_ref()
            && let Ok(button) = args.Button()
        {
            let action = match button {
                SystemMediaTransportControlsButton::Play => Some(OsMediaAction::Play),
                SystemMediaTransportControlsButton::Pause => Some(OsMediaAction::Pause),
                SystemMediaTransportControlsButton::Stop => Some(OsMediaAction::Stop),
                SystemMediaTransportControlsButton::Next => Some(OsMediaAction::Next),
                SystemMediaTransportControlsButton::Previous => Some(OsMediaAction::Previous),
                _ => None,
            };
            if let Some(action) = action {
                btn_handler(action);
            }
        }
        Ok(())
    }))?;

    let seek_handler = Arc::clone(handler);
    controls.PlaybackPositionChangeRequested(&TypedEventHandler::<
        SystemMediaTransportControls,
        PlaybackPositionChangeRequestedEventArgs,
    >::new(move |_sender, args| {
        if let Some(args) = args.as_ref()
            && let Ok(ts) = args.RequestedPlaybackPosition()
            && ts.Duration >= 0
        {
            seek_handler(OsMediaAction::SetPositionMs(
                (ts.Duration as f64) / 10_000.0,
            ));
        }
        Ok(())
    }))?;
    Ok(())
}

fn apply_metadata_updater(
    controls: &SystemMediaTransportControls,
    meta: Option<&OsMediaMetadata>,
    last_meta: &mut Option<OsMediaMetadata>,
) {
    let changed = match (last_meta.as_ref(), meta) {
        (None, None) => false,
        (Some(prev), Some(curr)) => prev != curr,
        _ => true,
    };
    if !changed {
        return;
    }
    *last_meta = meta.cloned();
    let Ok(updater) = controls.DisplayUpdater() else {
        return;
    };
    let Some(meta) = meta else {
        let _ = updater.ClearAll();
        let _ = updater.Update();
        return;
    };
    if meta.is_video {
        let _ = updater.SetType(MediaPlaybackType::Video);
        if let Ok(props) = updater.VideoProperties() {
            let _ = props.SetTitle(&HSTRING::from(meta.title.as_str()));
            let subtitle = [meta.artist.as_deref(), meta.album.as_deref()]
                .into_iter()
                .flatten()
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            if !subtitle.is_empty() {
                let _ = props.SetSubtitle(&HSTRING::from(subtitle.as_str()));
            }
        }
    } else {
        let _ = updater.SetType(MediaPlaybackType::Music);
        if let Ok(props) = updater.MusicProperties() {
            let _ = props.SetTitle(&HSTRING::from(meta.title.as_str()));
            if let Some(artist) = meta.artist.as_deref() {
                let _ = props.SetArtist(&HSTRING::from(artist));
            }
            if let Some(album) = meta.album.as_deref() {
                let _ = props.SetAlbumTitle(&HSTRING::from(album));
            }
        }
    }
    let _ = updater.Update();
}

fn apply_state(
    controls: &SystemMediaTransportControls,
    state: &OsMediaState,
    last_meta: &mut Option<OsMediaMetadata>,
) {
    if state.status == OsPlaybackStatus::Stopped {
        let _ = controls.SetPlaybackStatus(MediaPlaybackStatus::Stopped);
        let _ = controls.SetIsPlayEnabled(false);
        let _ = controls.SetIsPauseEnabled(false);
        let _ = controls.SetIsStopEnabled(false);
        let _ = controls.SetIsNextEnabled(false);
        let _ = controls.SetIsPreviousEnabled(false);
        let _ = controls.SetIsEnabled(false);
        apply_metadata_updater(controls, None, last_meta);
        return;
    }

    let status = match state.status {
        OsPlaybackStatus::Stopped => MediaPlaybackStatus::Stopped,
        OsPlaybackStatus::Playing => MediaPlaybackStatus::Playing,
        OsPlaybackStatus::Paused => MediaPlaybackStatus::Paused,
    };
    let _ = controls.SetIsEnabled(true);
    let _ = controls.SetIsPlayEnabled(true);
    let _ = controls.SetIsPauseEnabled(true);
    let _ = controls.SetIsStopEnabled(true);
    let _ = controls.SetPlaybackStatus(status);
    let _ = controls.SetIsNextEnabled(state.metadata.as_ref().is_some_and(|m| m.can_go_next));
    let _ =
        controls.SetIsPreviousEnabled(state.metadata.as_ref().is_some_and(|m| m.can_go_previous));

    apply_metadata_updater(controls, state.metadata.as_ref(), last_meta);

    if let Ok(timeline) = SystemMediaTransportControlsTimelineProperties::new() {
        let _ = timeline.SetStartTime(TimeSpan { Duration: 0 });
        let _ = timeline.SetMinSeekTime(TimeSpan { Duration: 0 });
        let dur_100ns = state.duration_us.max(0).saturating_mul(10);
        let pos_100ns = state.position_us.max(0).saturating_mul(10);
        let _ = timeline.SetEndTime(TimeSpan {
            Duration: dur_100ns,
        });
        let _ = timeline.SetMaxSeekTime(TimeSpan {
            Duration: dur_100ns,
        });
        let _ = timeline.SetPosition(TimeSpan {
            Duration: pos_100ns,
        });
        let _ = controls.UpdateTimelineProperties(&timeline);
    }
}

fn run_smtc_worker(rx: &mpsc::Receiver<WorkerMsg>, handler: &ActionHandler) {
    // SAFETY: initializes COM multithreaded apartment on this dedicated SMTC worker thread.
    let hr = unsafe {
        windows_sys::Win32::System::Com::CoInitializeEx(
            std::ptr::null(),
            windows_sys::Win32::System::Com::COINIT_MULTITHREADED as u32,
        )
    };
    let _guard = (hr >= 0).then_some(ComGuard);

    let Ok(player) = MediaPlayer::new() else {
        return;
    };
    if let Ok(cmd) = player.CommandManager() {
        let _ = cmd.SetIsEnabled(false);
    }
    let Ok(controls) = player.SystemMediaTransportControls() else {
        return;
    };
    if configure_controls(&controls, handler).is_err() {
        return;
    }

    let mut last_meta = None;
    while let Ok(msg) = rx.recv() {
        match msg {
            WorkerMsg::Update(state) => apply_state(&controls, &state, &mut last_meta),
            WorkerMsg::Stop => break,
        }
    }
    let _ = controls.SetPlaybackStatus(MediaPlaybackStatus::Closed);
    let _ = controls.SetIsEnabled(false);
    let _ = player.Close();
}
