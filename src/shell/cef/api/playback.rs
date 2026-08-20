use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["play"] if request.is("POST") => play_item(services, request),
        ["play", "next"] if request.is("POST") => play_next(services, request),
        ["player", "state"] if request.is("GET") => player_state(services),
        ["player", "command"] if request.is("POST") => player_command(services, request),
        ["sync"] if request.is("POST") => {
            services.sync.request();
            ApiResponse::ok(json!({ "requested": true }))
        }
        _ => return None,
    };
    Some(response)
}

// ------------------------------------------------------------------ playback

fn play_item(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let Some(item_id) = body["itemId"].as_str().filter(|id| !id.is_empty()) else {
        return ApiResponse::error(400, "itemId is required");
    };
    let options = PlayOptions {
        item_id: item_id.to_string(),
        resume: body["resume"].as_bool().unwrap_or(false),
        start_ticks: body["startTicks"].as_i64(),
        media_source_id: body["mediaSourceId"].as_str().map(str::to_string),
        media_source_index: body["mediaSourceIndex"]
            .as_u64()
            .and_then(|index| usize::try_from(index).ok()),
        audio_stream_index: body["audioStreamIndex"].as_i64(),
        subtitle_stream_index: body["subtitleStreamIndex"].as_i64(),
        quality: body["quality"].as_str().and_then(StreamingQuality::from_id),
    };
    start_playback(services, &options)
}

/// Used by the UI when mpv reports end-of-file or a mark-watched-and-next.
fn play_next(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let Some(item_id) = body["itemId"].as_str().filter(|id| !id.is_empty()) else {
        return ApiResponse::error(400, "itemId is required");
    };
    let next = match services.library.next_episode(item_id) {
        Ok(Some(next)) => next,
        Ok(None) => return ApiResponse::ok(json!({ "started": false })),
        Err(error) => return storage_failure(&error),
    };
    let Some(next_id) = next["id"].as_str() else {
        return ApiResponse::ok(json!({ "started": false }));
    };
    start_playback(
        services,
        &PlayOptions {
            item_id: next_id.to_string(),
            resume: true,
            ..Default::default()
        },
    )
}

fn start_playback(services: &Arc<Services>, options: &PlayOptions) -> ApiResponse {
    match play::start(services, options, "own UI") {
        Ok(prepared) => ApiResponse::ok(json!({
            "started": true,
            "itemId": options.item_id,
            "playMethod": prepared.play_method,
            "mediaSource": prepared.media_source_name,
            "startTicks": prepared.request.start_time_ticks.unwrap_or(0),
        })),
        Err(play::StartError::NoPlayer) => ApiResponse::error(
            409,
            "No media player is configured. Open Settings to set up mpv or MPC-HC.",
        ),
        Err(play::StartError::NotReady) => {
            ApiResponse::error(503, "the playback coordinator is not ready yet")
        }
        Err(play::StartError::Api(error)) => {
            // A 404 from `PlaybackInfo` means the item no longer exists on the
            // server, so the cached row is a phantom: drop it now rather than
            // offering a Play button that can never work.
            if matches!(error, ApiError::Status { status: 404 }) {
                forget_item(services, &options.item_id);
            }
            ApiResponse::from_api_error(&error)
        }
    }
}

fn player_state(services: &Arc<Services>) -> ApiResponse {
    let Some(playback) = services.playback() else {
        return ApiResponse::ok(json!({ "active": false }));
    };
    let snapshot = playback.snapshot();
    let capabilities = playback.capabilities();
    ApiResponse::ok(json!({
        "active": snapshot.active,
        "playbackId": snapshot.playback_id,
        "itemId": snapshot.item_id,
        "mediaSourceId": snapshot.media_source_id,
        "playSessionId": snapshot.play_session_id,
        "positionMs": snapshot.position_ms,
        "durationMs": snapshot.duration_ms,
        "paused": snapshot.paused,
        "volume": snapshot.volume,
        "mute": snapshot.mute,
        "stopReason": snapshot.stop_reason,
        "capabilities": {
            "chapterMarkers": capabilities.chapter_markers,
            "externalSubtitles": capabilities.external_subtitles,
            "injectedHotkeys": capabilities.injected_hotkeys,
            "absoluteVolume": capabilities.absolute_volume,
            "pushesPosition": capabilities.pushes_position,
        },
    }))
}

fn player_command(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    use crate::playback::PlayerCommand;

    let body = request.json();
    let command = match body["command"].as_str().unwrap_or_default() {
        "pause" => Some(PlayerCommand::SetPause(true)),
        "resume" => Some(PlayerCommand::SetPause(false)),
        "toggle-pause" => Some(PlayerCommand::SetPause(
            !body["paused"].as_bool().unwrap_or(false),
        )),
        "seek" => body["positionMs"]
            .as_f64()
            .filter(|value| value.is_finite())
            .map(PlayerCommand::SeekMilliseconds),
        "set-volume" => body["volume"]
            .as_f64()
            .filter(|value| value.is_finite())
            .map(PlayerCommand::SetVolume),
        "set-mute" => body["mute"].as_bool().map(PlayerCommand::SetMute),
        "set-playback-rate" => body["rate"]
            .as_f64()
            .filter(|rate| rate.is_finite() && *rate > 0.0)
            .map(PlayerCommand::SetPlaybackRate),
        "set-audio-track" => body["audioTrack"]
            .as_i64()
            .filter(|track| *track > 0)
            .map(PlayerCommand::SetAudioTrack),
        "set-subtitle-track" => match body["subtitleUrl"].as_str().map(str::trim) {
            Some(url) if !url.is_empty() => Some(PlayerCommand::AddSubtitle(url.to_string())),
            // A null track turns subtitles off.
            _ => Some(PlayerCommand::SetSubtitleTrack(
                body["subtitleTrack"].as_i64().filter(|track| *track > 0),
            )),
        },
        "stop" => Some(PlayerCommand::Stop),
        _ => None,
    };
    let Some(command) = command else {
        return ApiResponse::error(400, "unsupported player command");
    };
    let Some(playback) = services.playback() else {
        return ApiResponse::error(503, "the playback coordinator is not ready yet");
    };
    playback.control(command);
    ApiResponse::ok(json!({ "accepted": true }))
}
