//! Executes remote-control messages other Jellyfin clients send this session.
//!
//! Once the socket has announced media-control capabilities, this device
//! appears in the "Play On" menus of Jellyfin Web and the mobile apps. `Play`
//! starts an item in the configured external player through the same launch
//! path as the UI's own Play button, `Playstate` drives pause/stop/seek/next,
//! and `GeneralCommand` covers volume and mute. Every command here is
//! implemented by both the mpv and MPC-HC adapters; anything else is logged
//! and ignored rather than half-applied.

use serde_json::Value;

use crate::app::services::{self, Services};
use crate::playback::{PlayerCommand, TICKS_PER_SECOND};

use super::api::items;
use super::play::{self, PlayOptions};

/// What a `Play` message asked for, in this client's terms.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PlayAsk {
    pub item_ids: Vec<String>,
    pub start_index: usize,
    pub start_ticks: Option<i64>,
    pub media_source_id: Option<String>,
    pub audio_stream_index: Option<i64>,
    pub subtitle_stream_index: Option<i64>,
    pub play_command: String,
}

pub(crate) fn parse_play(data: &Value) -> PlayAsk {
    PlayAsk {
        item_ids: data["ItemIds"]
            .as_array()
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| id.as_str())
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        start_index: data["StartIndex"]
            .as_u64()
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or(0),
        start_ticks: data["StartPositionTicks"]
            .as_i64()
            .filter(|ticks| *ticks > 0),
        media_source_id: data["MediaSourceId"]
            .as_str()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        audio_stream_index: data["AudioStreamIndex"].as_i64(),
        subtitle_stream_index: data["SubtitleStreamIndex"].as_i64(),
        play_command: data["PlayCommand"]
            .as_str()
            .unwrap_or("PlayNow")
            .to_string(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlaystateAction {
    Stop,
    Pause,
    Unpause,
    PlayPause,
    Seek { position_ms: f64 },
    NextTrack,
    Unsupported(String),
}

pub(crate) fn parse_playstate(data: &Value) -> PlaystateAction {
    match data["Command"].as_str().unwrap_or_default() {
        "Stop" => PlaystateAction::Stop,
        "Pause" => PlaystateAction::Pause,
        "Unpause" => PlaystateAction::Unpause,
        "PlayPause" => PlaystateAction::PlayPause,
        "Seek" => PlaystateAction::Seek {
            position_ms: data["SeekPositionTicks"].as_i64().unwrap_or(0).max(0) as f64
                / (TICKS_PER_SECOND / 1_000.0),
        },
        "NextTrack" => PlaystateAction::NextTrack,
        other => PlaystateAction::Unsupported(other.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GeneralAction {
    SetVolume(f64),
    SetMute(bool),
    ToggleMute,
    Unsupported(String),
}

pub(crate) fn parse_general(data: &Value) -> GeneralAction {
    let name = data["Name"].as_str().unwrap_or_default();
    match name {
        "SetVolume" => {
            // Jellyfin serializes command arguments as strings.
            let volume = &data["Arguments"]["Volume"];
            volume
                .as_str()
                .and_then(|value| value.trim().parse::<f64>().ok())
                .or_else(|| volume.as_f64())
                .filter(|value| value.is_finite())
                .map(|value| GeneralAction::SetVolume(value.clamp(0.0, 100.0)))
                .unwrap_or_else(|| GeneralAction::Unsupported(name.to_string()))
        }
        "Mute" => GeneralAction::SetMute(true),
        "Unmute" => GeneralAction::SetMute(false),
        "ToggleMute" => GeneralAction::ToggleMute,
        other => GeneralAction::Unsupported(other.to_string()),
    }
}

pub fn handle_play(data: &Value) {
    let Some(services) = services::services() else {
        return;
    };
    let ask = parse_play(data);
    if !ask.play_command.eq_ignore_ascii_case("PlayNow") {
        // PlayNext/PlayLast are queueing commands and this client has no
        // playback queue to append to.
        tracing::debug!(
            target: "jellyfin.remote",
            command = %ask.play_command,
            "ignoring an unsupported remote play command"
        );
        return;
    }
    let Some(item_id) = ask
        .item_ids
        .get(ask.start_index)
        .or_else(|| ask.item_ids.first())
    else {
        tracing::debug!(target: "jellyfin.remote", "remote play named no items");
        return;
    };
    let Some((target_id, resume)) = resolve_playable(&services, item_id) else {
        tracing::debug!(
            target: "jellyfin.remote",
            item_id,
            "remote play target has nothing playable"
        );
        return;
    };

    let options = PlayOptions {
        item_id: target_id,
        resume,
        start_ticks: ask.start_ticks,
        media_source_id: ask.media_source_id,
        audio_stream_index: ask.audio_stream_index,
        subtitle_stream_index: ask.subtitle_stream_index,
        ..Default::default()
    };
    match play::start(&services, &options, "remote") {
        Ok(_) => {}
        Err(play::StartError::NoPlayer) => {
            tracing::warn!(
                target: "jellyfin.remote",
                "a remote client asked to play here, but no media player is configured"
            );
        }
        Err(play::StartError::NotReady) => {
            tracing::warn!(
                target: "jellyfin.remote",
                "a remote client asked to play here before the playback coordinator was ready"
            );
        }
        Err(play::StartError::Api(error)) => {
            services.session.note_error(&error);
            tracing::warn!(target: "jellyfin.remote", "remote play failed: {error}");
        }
    }
}

/// Maps a remote target onto something the player can open. Movies and
/// episodes play as themselves; a series plays its Next Up episode (first
/// episode once fully watched), matching the series page's Play button; a
/// season plays its first episode.
fn resolve_playable(services: &Services, item_id: &str) -> Option<(String, bool)> {
    match services.library.kind(item_id).as_deref() {
        Some("Series") => {
            let next_up = services
                .session
                .client_and_user()
                .ok()
                .and_then(|(client, user_id)| {
                    items::fetch_next_up(&client, &user_id, Some(item_id), 1)
                        .ok()
                        .and_then(|response| response.items.into_iter().next())
                        .map(|item| item.id)
                });
            let episode = match next_up {
                Some(id) => Some(id),
                None => services
                    .library
                    .first_episode(item_id)
                    .ok()
                    .flatten()
                    .and_then(|episode| episode["id"].as_str().map(str::to_string)),
            };
            episode.map(|id| (id, true))
        }
        Some("Season") => services
            .library
            .children(item_id)
            .ok()
            .and_then(|children| {
                children.into_iter().find_map(|child| {
                    (child["kind"] == "Episode")
                        .then(|| child["id"].as_str().map(str::to_string))
                        .flatten()
                })
            })
            .map(|id| (id, true)),
        // Movies, episodes, and deep-linked ids the cache has not seen play
        // as themselves; `PlaybackInfo` is the authority on playability.
        _ => Some((item_id.to_string(), false)),
    }
}

pub fn handle_playstate(data: &Value) {
    let Some(services) = services::services() else {
        return;
    };
    let Some(playback) = services.playback() else {
        return;
    };
    match parse_playstate(data) {
        PlaystateAction::Stop => playback.control(PlayerCommand::Stop),
        PlaystateAction::Pause => playback.control(PlayerCommand::SetPause(true)),
        PlaystateAction::Unpause => playback.control(PlayerCommand::SetPause(false)),
        PlaystateAction::PlayPause => {
            playback.control(PlayerCommand::SetPause(!playback.snapshot().paused));
        }
        PlaystateAction::Seek { position_ms } => {
            playback.control(PlayerCommand::SeekMilliseconds(position_ms));
        }
        PlaystateAction::NextTrack => {
            let Some(current) = playback.snapshot().item_id else {
                return;
            };
            let next = services
                .library
                .next_episode(&current)
                .ok()
                .flatten()
                .and_then(|episode| episode["id"].as_str().map(str::to_string));
            let Some(next_id) = next else {
                tracing::debug!(
                    target: "jellyfin.remote",
                    item_id = %current,
                    "remote next-track has no following episode"
                );
                return;
            };
            let options = PlayOptions {
                item_id: next_id,
                resume: true,
                ..Default::default()
            };
            if let Err(error) = play::start(&services, &options, "remote next") {
                tracing::warn!(target: "jellyfin.remote", "remote next-track failed: {error:?}");
            }
        }
        PlaystateAction::Unsupported(command) => {
            tracing::debug!(
                target: "jellyfin.remote",
                command,
                "ignoring an unsupported remote playstate command"
            );
        }
    }
}

pub fn handle_general_command(data: &Value) {
    let Some(services) = services::services() else {
        return;
    };
    let Some(playback) = services.playback() else {
        return;
    };
    match parse_general(data) {
        GeneralAction::SetVolume(volume) => playback.control(PlayerCommand::SetVolume(volume)),
        GeneralAction::SetMute(mute) => playback.control(PlayerCommand::SetMute(mute)),
        GeneralAction::ToggleMute => {
            let muted = playback.snapshot().mute.unwrap_or(false);
            playback.control(PlayerCommand::SetMute(!muted));
        }
        GeneralAction::Unsupported(name) => {
            tracing::debug!(
                target: "jellyfin.remote",
                command = %name,
                "ignoring an unsupported remote general command"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GeneralAction, PlaystateAction, parse_general, parse_play, parse_playstate};
    use serde_json::json;

    #[test]
    fn play_requests_carry_target_position_and_track_choices() {
        let ask = parse_play(&json!({
            "ItemIds": ["a", " ", "b"],
            "StartIndex": 1,
            "StartPositionTicks": 600_000_000_i64,
            "MediaSourceId": "src",
            "AudioStreamIndex": 2,
            "SubtitleStreamIndex": -1,
            "PlayCommand": "PlayNow",
        }));
        assert_eq!(ask.item_ids, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(ask.start_index, 1);
        assert_eq!(ask.start_ticks, Some(600_000_000));
        assert_eq!(ask.media_source_id.as_deref(), Some("src"));
        assert_eq!(ask.audio_stream_index, Some(2));
        assert_eq!(ask.subtitle_stream_index, Some(-1));
        assert_eq!(ask.play_command, "PlayNow");

        // An absent command means "play now", and a zero position means
        // "from the start" rather than a resume point.
        let bare = parse_play(&json!({ "ItemIds": ["a"], "StartPositionTicks": 0 }));
        assert_eq!(bare.play_command, "PlayNow");
        assert_eq!(bare.start_ticks, None);
        assert_eq!(bare.start_index, 0);
    }

    #[test]
    fn playstate_commands_map_to_player_actions() {
        assert_eq!(
            parse_playstate(&json!({ "Command": "Stop" })),
            PlaystateAction::Stop
        );
        assert_eq!(
            parse_playstate(&json!({ "Command": "Pause" })),
            PlaystateAction::Pause
        );
        assert_eq!(
            parse_playstate(&json!({ "Command": "Unpause" })),
            PlaystateAction::Unpause
        );
        assert_eq!(
            parse_playstate(&json!({ "Command": "PlayPause" })),
            PlaystateAction::PlayPause
        );
        assert_eq!(
            parse_playstate(&json!({ "Command": "NextTrack" })),
            PlaystateAction::NextTrack
        );
        assert_eq!(
            parse_playstate(&json!({ "Command": "Seek", "SeekPositionTicks": 600_000_000_i64 })),
            PlaystateAction::Seek {
                position_ms: 60_000.0
            }
        );
        assert_eq!(
            parse_playstate(&json!({ "Command": "Rewind" })),
            PlaystateAction::Unsupported("Rewind".to_string())
        );
    }

    #[test]
    fn general_commands_parse_volume_strings_and_mute_variants() {
        assert_eq!(
            parse_general(&json!({ "Name": "SetVolume", "Arguments": { "Volume": "55" } })),
            GeneralAction::SetVolume(55.0)
        );
        assert_eq!(
            parse_general(&json!({ "Name": "SetVolume", "Arguments": { "Volume": 30 } })),
            GeneralAction::SetVolume(30.0)
        );
        assert_eq!(
            parse_general(&json!({ "Name": "SetVolume", "Arguments": { "Volume": "999" } })),
            GeneralAction::SetVolume(100.0)
        );
        assert_eq!(
            parse_general(&json!({ "Name": "Mute" })),
            GeneralAction::SetMute(true)
        );
        assert_eq!(
            parse_general(&json!({ "Name": "Unmute" })),
            GeneralAction::SetMute(false)
        );
        assert_eq!(
            parse_general(&json!({ "Name": "ToggleMute" })),
            GeneralAction::ToggleMute
        );
        assert_eq!(
            parse_general(&json!({ "Name": "DisplayMessage" })),
            GeneralAction::Unsupported("DisplayMessage".to_string())
        );
        // A volume command without a usable value must not set anything.
        assert_eq!(
            parse_general(&json!({ "Name": "SetVolume", "Arguments": {} })),
            GeneralAction::Unsupported("SetVolume".to_string())
        );
    }
}
