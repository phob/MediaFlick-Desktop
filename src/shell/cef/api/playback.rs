use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<Handled> {
    let response = match segments {
        ["play"] if request.is("POST") => play_item(services, request),
        ["play", "next"] if request.is("POST") => play_next(services, request),
        ["play", "previous"] if request.is("POST") => play_previous(services, request),
        ["play", "neighbors"] if request.is("POST") => playback_neighbors(services, request),
        ["player", "state"] if request.is("GET") => player_state(services),
        ["player", "command"] if request.is("POST") => player_command(services, request),
        ["sync"] if request.is("POST") => {
            services.sync.request();
            Ok(ApiResponse::ok(SyncRequested { requested: true }))
        }
        _ => return None,
    };
    Some(response)
}

/// `/api/sync`: a library sync was queued.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct SyncRequested {
    requested: bool,
}

/// The answer to every `/api/play` route. `started: false`, with no other
/// field, comes back when there is no next or previous episode.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlayStarted {
    started: bool,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    item_id: Option<String>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    play_method: Option<String>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    media_source: Option<String>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    start_ticks: Option<i64>,
}

impl PlayStarted {
    const NOT_STARTED: Self = Self {
        started: false,
        item_id: None,
        play_method: None,
        media_source: None,
        start_ticks: None,
    };
}

/// `/api/play/neighbors`: the episodes either side of an item.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct PlaybackNeighbors {
    previous: Option<ItemSummary>,
    next: Option<ItemSummary>,
}

/// `/api/player/command`: the player took the command.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct PlayerCommandAccepted {
    accepted: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayBody {
    item_id: String,
    #[serde(default)]
    resume: bool,
    start_ticks: Option<i64>,
    media_source_id: Option<String>,
    media_source_index: Option<usize>,
    audio_stream_index: Option<i64>,
    subtitle_stream_index: Option<i64>,
    quality: Option<StreamingQuality>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemBody {
    item_id: String,
}

fn item_id_of(request: &ApiRequest) -> Result<String, ApiResponse> {
    let body = request.untyped_body::<ItemBody>()?;
    if body.item_id.is_empty() {
        return Err(ApiResponse::error(400, "itemId is required"));
    }
    Ok(body.item_id)
}

fn play_item(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let body = request.untyped_body::<PlayBody>()?;
    if body.item_id.is_empty() {
        return Err(ApiResponse::error(400, "itemId is required"));
    }
    let options = PlayOptions {
        item_id: body.item_id,
        resume: body.resume,
        start_ticks: body.start_ticks,
        media_source_id: body.media_source_id,
        media_source_index: body.media_source_index,
        audio_stream_index: body.audio_stream_index,
        subtitle_stream_index: body.subtitle_stream_index,
        quality: body.quality,
        ..Default::default()
    };
    start_playback(services, &options)
}

fn play_next(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let item_id = item_id_of(request)?;
    let next = match services.library.next_episode(&item_id) {
        Ok(Some(next)) => next,
        Ok(None) => return Ok(ApiResponse::ok(PlayStarted::NOT_STARTED)),
        Err(error) => return Err(storage_failure(&error)),
    };
    start_playback(
        services,
        &PlayOptions {
            item_id: next.id,
            resume: true,
            ..Default::default()
        },
    )
}

fn play_previous(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let item_id = item_id_of(request)?;
    let previous = match services.library.previous_episode(&item_id) {
        Ok(Some(previous)) => previous,
        Ok(None) => return Ok(ApiResponse::ok(PlayStarted::NOT_STARTED)),
        Err(error) => return Err(storage_failure(&error)),
    };
    start_playback(
        services,
        &PlayOptions {
            item_id: previous.id,
            resume: true,
            ..Default::default()
        },
    )
}

fn playback_neighbors(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let item_id = item_id_of(request)?;
    let previous = match services.library.previous_episode(&item_id) {
        Ok(previous) => previous,
        Err(error) => return Err(storage_failure(&error)),
    };
    let next = match services.library.next_episode(&item_id) {
        Ok(next) => next,
        Err(error) => return Err(storage_failure(&error)),
    };
    Ok(ApiResponse::ok(PlaybackNeighbors { previous, next }))
}

fn start_playback(services: &Arc<Services>, options: &PlayOptions) -> Handled {
    let scope = session_scope(services)?;
    match play::start(services, &scope, options, "own UI") {
        Ok(prepared) => Ok(ApiResponse::ok(PlayStarted {
            started: true,
            item_id: Some(options.item_id.clone()),
            play_method: Some(prepared.play_method),
            media_source: Some(prepared.media_source_name),
            start_ticks: Some(prepared.request.start_time_ticks.unwrap_or(0)),
        })),
        Err(play::StartError::NoPlayer) => Err(ApiResponse::error(
            409,
            "No media player is configured. Open Settings to set up the built-in player or mpv.",
        )),
        Err(play::StartError::NotReady) => Err(ApiResponse::error(
            503,
            "the playback coordinator is not ready yet",
        )),
        Err(play::StartError::AccountChanged) => Err(stale_account_response()),
        Err(play::StartError::Api(error)) => {
            if matches!(error, ApiError::Status { status: 404 }) {
                forget_item(services, &scope, &options.item_id);
            }
            Err(ApiResponse::from_api_error(&error))
        }
    }
}

fn player_state(services: &Arc<Services>) -> Handled {
    let snapshot = services
        .playback()
        .map(|playback| playback.snapshot())
        .unwrap_or_default();
    Ok(ApiResponse::ok(snapshot))
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "command",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub(super) enum PlayerCommandBody {
    Pause,
    Resume,
    Seek {
        position_ms: f64,
    },
    SetVolume {
        volume: f64,
    },
    SetMute {
        mute: bool,
    },
    SetAudioDelay {
        delay_seconds: f64,
    },
    SetSubtitleDelay {
        delay_seconds: f64,
    },
    SetSubtitleScale {
        scale: f64,
    },
    SetVideoFit {
        fit: VideoFitBody,
    },
    SetVideoAspect {
        aspect: VideoAspectBody,
    },
    SetDeinterlace {
        enabled: bool,
    },
    SetToneMapping {
        mode: ToneMappingBody,
    },
    SetAudioTrack {
        audio_track: i64,
    },
    /// A null track turns subtitles off.
    SetSubtitleTrack {
        subtitle_track: Option<i64>,
    },
    ToggleSubtitles,
    ToggleFullscreen,
    Stop,
    MarkWatchedNext,
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub(super) enum VideoFitBody {
    Fit,
    Fill,
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) enum VideoAspectBody {
    #[serde(rename = "source")]
    Source,
    #[serde(rename = "4:3")]
    Ratio4x3,
    #[serde(rename = "16:9")]
    Ratio16x9,
    #[serde(rename = "21:9")]
    Ratio21x9,
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub(super) enum ToneMappingBody {
    Auto,
    Clip,
    Mobius,
    Reinhard,
    Hable,
    #[serde(rename = "bt.2390")]
    Bt2390,
}

impl PlayerCommandBody {
    /// Values the player cannot apply (a zero subtitle scale, a non-positive track) are
    /// rejected here rather than forwarded.
    fn into_command(self) -> Option<crate::playback::PlayerCommand> {
        use crate::playback::{PlayerCommand, ToneMapping, VideoAspect, VideoFit};

        Some(match self {
            Self::Pause => PlayerCommand::SetPause(true),
            Self::Resume => PlayerCommand::SetPause(false),
            Self::Seek { position_ms } => PlayerCommand::SeekMilliseconds(position_ms),
            Self::SetVolume { volume } => PlayerCommand::SetVolume(volume),
            Self::SetMute { mute } => PlayerCommand::SetMute(mute),
            Self::SetAudioDelay { delay_seconds } => PlayerCommand::SetAudioDelay(delay_seconds),
            Self::SetSubtitleDelay { delay_seconds } => {
                PlayerCommand::SetSubtitleDelay(delay_seconds)
            }
            Self::SetSubtitleScale { scale } if scale > 0.0 => {
                PlayerCommand::SetSubtitleScale(scale)
            }
            Self::SetSubtitleScale { .. } => return None,
            Self::SetVideoFit { fit } => PlayerCommand::SetVideoFit(match fit {
                VideoFitBody::Fit => VideoFit::Fit,
                VideoFitBody::Fill => VideoFit::Fill,
            }),
            Self::SetVideoAspect { aspect } => PlayerCommand::SetVideoAspect(match aspect {
                VideoAspectBody::Source => VideoAspect::Source,
                VideoAspectBody::Ratio4x3 => VideoAspect::Ratio4x3,
                VideoAspectBody::Ratio16x9 => VideoAspect::Ratio16x9,
                VideoAspectBody::Ratio21x9 => VideoAspect::Ratio21x9,
            }),
            Self::SetDeinterlace { enabled } => PlayerCommand::SetDeinterlace(enabled),
            Self::SetToneMapping { mode } => PlayerCommand::SetToneMapping(match mode {
                ToneMappingBody::Auto => ToneMapping::Auto,
                ToneMappingBody::Clip => ToneMapping::Clip,
                ToneMappingBody::Mobius => ToneMapping::Mobius,
                ToneMappingBody::Reinhard => ToneMapping::Reinhard,
                ToneMappingBody::Hable => ToneMapping::Hable,
                ToneMappingBody::Bt2390 => ToneMapping::Bt2390,
            }),
            Self::SetAudioTrack { audio_track } if audio_track > 0 => {
                PlayerCommand::SetAudioTrack(audio_track)
            }
            Self::SetAudioTrack { .. } => return None,
            Self::SetSubtitleTrack { subtitle_track } => {
                PlayerCommand::SetSubtitleTrack(subtitle_track.filter(|track| *track > 0))
            }
            Self::ToggleSubtitles => PlayerCommand::ToggleSubtitleVisibility,
            Self::ToggleFullscreen => PlayerCommand::ToggleFullscreen,
            Self::Stop => PlayerCommand::Stop,
            Self::MarkWatchedNext => PlayerCommand::MarkWatchedAndPlayNext,
        })
    }
}

fn player_command(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let body = request.body::<PlayerCommandBody>()?;
    let Some(command) = body.into_command() else {
        return Err(ApiResponse::error(400, "unsupported player command"));
    };
    let Some(playback) = services.playback() else {
        return Err(ApiResponse::error(
            503,
            "the playback coordinator is not ready yet",
        ));
    };
    playback.control(command);
    Ok(ApiResponse::ok(PlayerCommandAccepted { accepted: true }))
}
