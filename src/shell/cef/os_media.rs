use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

#[cfg(target_os = "linux")]
use cef::*;

use super::{BrowserStateInner, api};
#[cfg(target_os = "linux")]
use super::{focus_main_window, post_ui_task};
use crate::app::paths::image_cache_dir;
use crate::library::ItemSummary;
#[cfg(target_os = "linux")]
use crate::playback::os_media::clamp_seek_target_ms;
use crate::playback::os_media::{
    OsMediaAction, OsMediaController, OsMediaMetadata, os_media_state_from_snapshot,
};
use crate::playback::{PlaybackEvent, PlayerCommand, PlayerSnapshot};
use crate::shell::locator;

#[cfg(target_os = "linux")]
wrap_task! {
    struct FocusWindowTask {
        state: Arc<Mutex<BrowserStateInner>>,
    }

    impl Task {
        fn execute(&self) {
            focus_main_window(&self.state);
        }
    }
}

pub(super) fn start_controller(state: &Weak<Mutex<BrowserStateInner>>) -> OsMediaController {
    let weak_state = Weak::clone(state);
    OsMediaController::start(Arc::new(move |action| {
        handle_action(&weak_state, action);
    }))
}

pub(super) fn sync(
    controller: &OsMediaController,
    cache: &mut Option<OsMediaMetadata>,
    event: &PlaybackEvent,
) {
    let snapshot = match event {
        PlaybackEvent::StateChanged(snapshot) | PlaybackEvent::Stopped(snapshot) => snapshot,
        PlaybackEvent::ItemFinished(_) | PlaybackEvent::Failed { .. } => return,
    };
    if !snapshot.active {
        *cache = None;
        controller.update(&os_media_state_from_snapshot(snapshot, None));
        return;
    }
    let metadata = resolve_metadata(cache, snapshot);
    controller.update(&os_media_state_from_snapshot(snapshot, metadata.as_ref()));
}

fn handle_action(state: &Weak<Mutex<BrowserStateInner>>, action: OsMediaAction) {
    #[cfg(target_os = "linux")]
    if let OsMediaAction::Raise = action {
        if let Some(state) = state.upgrade() {
            let mut task = FocusWindowTask::new(state);
            let _ = post_ui_task(&mut task);
        }
        return;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = state;
    let Some(services) = locator::services() else {
        return;
    };
    let Some(playback) = services.playback() else {
        return;
    };
    match action {
        OsMediaAction::Play => playback.control(PlayerCommand::SetPause(false)),
        OsMediaAction::Pause => playback.control(PlayerCommand::SetPause(true)),
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        OsMediaAction::PlayPause => playback.control(PlayerCommand::TogglePause),
        OsMediaAction::Stop => playback.control(PlayerCommand::Stop),
        OsMediaAction::Next => skip_track(&services, &playback, true),
        OsMediaAction::Previous => skip_track(&services, &playback, false),
        #[cfg(target_os = "linux")]
        OsMediaAction::SeekDeltaUs(delta_us) => {
            let snapshot = playback.snapshot();
            let target = clamp_seek_target_ms(snapshot.position_ms, delta_us, snapshot.duration_ms);
            playback.control(PlayerCommand::SeekMilliseconds(target));
        }
        OsMediaAction::SetPositionMs(position_ms) => {
            playback.control(PlayerCommand::SeekMilliseconds(position_ms));
        }
        #[cfg(target_os = "linux")]
        OsMediaAction::SetVolume(volume) => {
            playback.control(PlayerCommand::SetVolume(
                (volume * 100.0).clamp(0.0, 100.0).round(),
            ));
        }
        #[cfg(target_os = "linux")]
        OsMediaAction::Raise => {}
    }
}

fn skip_track(
    services: &Arc<crate::app::services::Services>,
    playback: &crate::playback::PlaybackCoordinator,
    next: bool,
) {
    let snapshot = playback.snapshot();
    if matches!(
        snapshot.now_playing,
        Some(crate::playback::NowPlaying::Audio(_))
    ) {
        playback.control(if next {
            PlayerCommand::NextTrack
        } else {
            PlayerCommand::PreviousTrack
        });
        return;
    }
    if let Ok(scope) = services.session.scope() {
        crate::jellyfin::remote::skip(services, &scope, next);
    }
}

fn resolve_metadata(
    cache: &mut Option<OsMediaMetadata>,
    snapshot: &PlayerSnapshot,
) -> Option<OsMediaMetadata> {
    if let Some(crate::playback::NowPlaying::Audio(audio)) = &snapshot.now_playing {
        let item_id = &audio.current.item_id;
        if let Some(cached) = cache.as_mut().filter(|meta| &meta.item_id == item_id) {
            cached.can_go_next = audio.can_next;
            cached.can_go_previous = audio.can_previous;
            return Some(cached.clone());
        }
        let artwork_path = audio
            .current
            .artwork
            .as_ref()
            .and_then(|art| cached_artwork_path(&art.item_id, &art.tag));
        let built = OsMediaMetadata {
            item_id: item_id.clone(),
            is_video: false,
            title: audio.current.title.clone(),
            artist: (!audio.current.artists.is_empty()).then(|| audio.current.artists.join(", ")),
            album: audio.current.album.clone(),
            duration_us: audio
                .current
                .runtime_ticks
                .map_or(0, |ticks| (ticks / 10).max(0)),
            artwork_path,
            can_go_next: audio.can_next,
            can_go_previous: audio.can_previous,
        };
        *cache = Some(built.clone());
        return Some(built);
    }

    let item_id = snapshot.item_id.as_ref()?;
    if let Some(cached) = cache.as_ref().filter(|meta| &meta.item_id == item_id) {
        return Some(cached.clone());
    }
    let services = locator::services()?;
    let summary = services
        .library
        .item(item_id)
        .ok()
        .flatten()
        .map(|d| d.summary);
    let built = match summary.as_ref() {
        Some(summary) => video_metadata(item_id, summary, &services),
        None => OsMediaMetadata {
            item_id: item_id.clone(),
            is_video: true,
            title: item_id.clone(),
            artist: None,
            album: None,
            duration_us: 0,
            artwork_path: None,
            can_go_next: false,
            can_go_previous: false,
        },
    };
    *cache = Some(built.clone());
    Some(built)
}

fn video_metadata(
    item_id: &str,
    summary: &ItemSummary,
    services: &crate::app::services::Services,
) -> OsMediaMetadata {
    let artwork_path = summary
        .primary_image_tag
        .as_deref()
        .and_then(|tag| cached_artwork_path(item_id, tag));
    let is_episode = summary.kind == "Episode";
    let (can_go_next, can_go_previous) = if is_episode {
        (
            services
                .library
                .next_episode(item_id)
                .ok()
                .flatten()
                .is_some(),
            services
                .library
                .previous_episode(item_id)
                .ok()
                .flatten()
                .is_some(),
        )
    } else {
        (false, false)
    };
    let album = match (summary.parent_index_number, summary.index_number) {
        (Some(season), Some(episode)) => Some(format!("S{season:02}E{episode:02}")),
        (Some(season), None) => Some(format!("Season {season}")),
        _ => None,
    };
    OsMediaMetadata {
        item_id: item_id.to_string(),
        is_video: true,
        title: summary.name.clone(),
        artist: summary
            .series_name
            .clone()
            .or_else(|| summary.year.map(|year| year.to_string())),
        album,
        duration_us: summary.runtime_ticks.map_or(0, |ticks| (ticks / 10).max(0)),
        artwork_path,
        can_go_next,
        can_go_previous,
    }
}

fn cached_artwork_path(item_id: &str, tag: &str) -> Option<PathBuf> {
    let dir = image_cache_dir();
    [96, 400, 600, 800, 0]
        .into_iter()
        .map(|width| dir.join(api::images::cache_key(item_id, "Primary", tag, width)))
        .find(|path| path.is_file())
}
