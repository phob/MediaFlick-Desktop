use std::sync::{Arc, Weak};

mod audio;

pub use audio::{QueueAsk, audio_kind, audio_tracks, start_item_queue, start_queue};

use crate::app::services::Services;
use crate::app::urls::{build_query, encode_path_segment, join_url, remove_query_params};
use crate::library::model::ResolvedPlaybackPreference;
use crate::library::{ItemSummary, Library, resolve_playback_preference};
use crate::playback::{PlaybackOwner, PlaybackRequest, TOKEN_QUERY_KEYS, seconds_to_ticks};
use crate::preferences::StreamingQuality;

use super::api::items::{self, PlaybackInfoRequest};
use super::api::model::{MediaSourceInfo, MediaStream, PlaybackInfoResponse};
use super::api::{ApiError, JellyfinClient};
use super::session::SessionScope;

#[derive(Debug, Clone, Default)]
pub struct PlayOptions {
    pub item_id: String,
    pub resume: bool,
    pub start_ticks: Option<i64>,
    pub media_source_id: Option<String>,
    pub media_source_index: Option<usize>,
    pub audio_stream_index: Option<i64>,
    pub subtitle_stream_index: Option<i64>,
    pub quality: Option<StreamingQuality>,
    pub viewing: crate::preferences::ViewingSettings,
    pub audio_passthrough: bool,
}

#[derive(Debug, Clone)]
pub struct PreparedPlayback {
    pub request: PlaybackRequest,
    pub play_method: String,
    pub media_source_name: String,
}

#[derive(Debug)]
pub enum StartError {
    NoPlayer,
    NotReady,
    AccountChanged,
    Rejected(&'static str),
    Api(ApiError),
}

pub fn start(
    services: &Arc<Services>,
    scope: &SessionScope,
    options: &PlayOptions,
    origin: &str,
) -> Result<PreparedPlayback, StartError> {
    let settings = services.preferences.snapshot();
    let Some(player_path) = crate::players::configured_player_path(&settings) else {
        return Err(StartError::NoPlayer);
    };
    let Some(playback) = services.playback() else {
        return Err(StartError::NotReady);
    };

    if !services.session.scope_is_current(scope) {
        return Err(StartError::AccountChanged);
    }

    let mut options = options.clone();
    options.viewing = services.accounts.viewing(scope.account());
    options.audio_passthrough = settings.audio_passthrough;
    let prepared = prepare(
        scope,
        &services.library,
        services
            .playback_preferences
            .get(scope.account(), &options.item_id)
            .as_ref(),
        settings.streaming_quality,
        &options,
    )
    .map_err(|error| match services.session.scoped_error(scope, error) {
        ApiError::AccountChanged => StartError::AccountChanged,
        error => StartError::Api(error),
    })?;

    services.session.commit_if_current(
        scope,
        || StartError::AccountChanged,
        |_live| {
            tracing::info!(
                target: "playback",
                item_id = %options.item_id,
                play_method = %prepared.play_method,
                media_source = %prepared.media_source_name,
                headers = %crate::app::logger::redacted_header_summary(&prepared.request.headers),
                launch = %crate::app::logger::launch_summary(&prepared.request),
                origin,
                "starting playback"
            );
            let mut launch = prepared.request.clone();
            launch.account = Some(scope.account().clone());
            launch.playstate_outbox = Some(Arc::clone(&services.playstate_outbox));
            playback.open(
                player_path,
                settings.default_fullscreen,
                launch,
                Arc::new(AccountOwner::new(services, scope)),
            );
            Ok(())
        },
    )?;
    Ok(prepared)
}

struct AccountOwner {
    services: Weak<Services>,
    scope: SessionScope,
}

impl AccountOwner {
    fn new(services: &Arc<Services>, scope: &SessionScope) -> Self {
        Self {
            services: Arc::downgrade(services),
            scope: scope.clone(),
        }
    }
}

impl PlaybackOwner for AccountOwner {
    fn is_current(&self) -> bool {
        self.services
            .upgrade()
            .is_some_and(|services| services.session.scope_is_current(&self.scope))
    }
}

pub fn prepare(
    scope: &SessionScope,
    library: &Library,
    saved_preference: Option<&crate::library::ItemPlaybackPreference>,
    quality: StreamingQuality,
    options: &PlayOptions,
) -> Result<PreparedPlayback, ApiError> {
    let (client, user_id) = (scope.client(), scope.user_id());
    let quality = options.quality.unwrap_or(quality);
    let cached = library
        .item(&options.item_id)
        .ok()
        .flatten()
        .map(|item| item.summary);

    let position = cached.as_ref().map_or(0, |item| item.position_ticks);
    let start_ticks = resume_start_ticks(options, position);

    let initial_request = PlaybackInfoRequest {
        media_source_id: options.media_source_id.clone(),
        start_time_ticks: Some(start_ticks),
        audio_stream_index: options.audio_stream_index,
        subtitle_stream_index: options.subtitle_stream_index,
    };
    let mut info = items::playback_info_with_profile(
        client,
        user_id,
        &options.item_id,
        quality,
        &initial_request,
        options.audio_passthrough,
    )?;

    let mut effective_options = options.clone();
    let has_explicit_track_options = options.media_source_id.is_some()
        || options.media_source_index.is_some()
        || options.audio_stream_index.is_some()
        || options.subtitle_stream_index.is_some();
    if !has_explicit_track_options
        && let Some(preference) = saved_preference
        && let Some(resolved) = resolve_playback_preference(Some(preference), &info.media_sources)
    {
        apply_saved_preference(&mut effective_options, resolved);
    }

    if !has_explicit_track_options
        && effective_options.audio_stream_index.is_none()
        && effective_options.subtitle_stream_index.is_none()
        && (!options.viewing.audio_languages.is_empty()
            || options.viewing.prefer_original_audio
            || options.viewing.subtitle_mode != crate::preferences::SubtitleMode::Server)
        && let Some(source) = select_source(
            &info,
            effective_options.media_source_id.as_deref(),
            effective_options.media_source_index,
        )
    {
        apply_language_defaults(&mut effective_options, source);
    }

    let track_selection_changed = effective_options.media_source_id
        != initial_request.media_source_id
        || effective_options.audio_stream_index != initial_request.audio_stream_index
        || effective_options.subtitle_stream_index != initial_request.subtitle_stream_index;
    let chosen_is_transcode = select_source(
        &info,
        effective_options.media_source_id.as_deref(),
        effective_options.media_source_index,
    )
    .is_some_and(|source| !(source.supports_direct_play || source.supports_direct_stream));
    if track_selection_changed && chosen_is_transcode {
        let updated_request = PlaybackInfoRequest {
            media_source_id: effective_options.media_source_id.clone(),
            start_time_ticks: Some(start_ticks),
            audio_stream_index: effective_options.audio_stream_index,
            subtitle_stream_index: effective_options.subtitle_stream_index,
        };
        info = items::playback_info_with_profile(
            client,
            user_id,
            &options.item_id,
            quality,
            &updated_request,
            options.audio_passthrough,
        )?;
    }

    build(
        client,
        &info,
        quality,
        &effective_options,
        start_ticks,
        cached.as_ref(),
    )
}

fn resume_start_ticks(options: &PlayOptions, position: i64) -> i64 {
    if let Some(explicit) = options.start_ticks {
        return explicit.max(0);
    }
    if !options.resume {
        return 0;
    }
    position
        .saturating_sub(
            seconds_to_ticks(f64::from(options.viewing.resume_rewind_seconds)).unwrap_or(0),
        )
        .max(0)
}

fn language_matches(language: Option<&str>, preferred: &str) -> bool {
    fn canonical(value: &str) -> &str {
        match value {
            "eng" => "en",
            "deu" | "ger" => "de",
            "fra" | "fre" => "fr",
            "spa" => "es",
            "ita" => "it",
            "jpn" => "ja",
            "kor" => "ko",
            "zho" | "chi" => "zh",
            "por" => "pt",
            "rus" => "ru",
            _ => value,
        }
    }
    language.is_some_and(|value| {
        canonical(&value.to_ascii_lowercase()) == canonical(&preferred.to_ascii_lowercase())
    })
}

fn apply_language_defaults(options: &mut PlayOptions, source: &MediaSourceInfo) {
    use crate::preferences::SubtitleMode;
    let settings = &options.viewing;
    let audio = source
        .media_streams
        .iter()
        .filter(|stream| stream.stream_type.as_deref() == Some("Audio"));
    let original = settings
        .prefer_original_audio
        .then(|| {
            audio.clone().find(|stream| {
                stream
                    .title
                    .as_deref()
                    .is_some_and(|title| title.to_ascii_lowercase().contains("original"))
            })
        })
        .flatten();
    let selected_audio = original.or_else(|| {
        settings.audio_languages.iter().find_map(|language| {
            audio
                .clone()
                .find(|stream| language_matches(stream.language.as_deref(), language))
        })
    });
    options.audio_stream_index = selected_audio.map(|stream| stream.index);
    let effective_audio = selected_audio.or_else(|| choose_stream(source, "Audio", None));
    options.media_source_id.clone_from(&source.id);
    options.media_source_index = Some(0);
    let enabled = match settings.subtitle_mode {
        SubtitleMode::Server => return,
        SubtitleMode::Off => false,
        SubtitleMode::Forced | SubtitleMode::Always => true,
        SubtitleMode::ForeignAudio => !settings.audio_languages.iter().any(|language| {
            language_matches(
                effective_audio.and_then(|stream| stream.language.as_deref()),
                language,
            )
        }),
    };
    let subtitles = source.media_streams.iter().filter(|stream| {
        stream.stream_type.as_deref() == Some("Subtitle")
            && (settings.subtitle_mode != SubtitleMode::Forced || stream.is_forced)
    });
    let selected = settings
        .subtitle_languages
        .iter()
        .find_map(|language| {
            subtitles
                .clone()
                .find(|stream| language_matches(stream.language.as_deref(), language))
        })
        .or_else(|| subtitles.clone().find(|stream| stream.is_default))
        .or_else(|| subtitles.clone().next());
    options.subtitle_stream_index = Some(if enabled {
        selected.map_or(-1, |stream| stream.index)
    } else {
        -1
    });
}

fn apply_saved_preference(options: &mut PlayOptions, resolved: ResolvedPlaybackPreference) {
    options.media_source_id = resolved.media_source_id;
    options.media_source_index = Some(resolved.media_source_index);
    options.audio_stream_index = resolved.audio_stream_index;
    options.subtitle_stream_index = Some(resolved.subtitle_stream_index.unwrap_or(-1));
}

fn build(
    client: &JellyfinClient,
    info: &PlaybackInfoResponse,
    quality: StreamingQuality,
    options: &PlayOptions,
    start_ticks: i64,
    cached: Option<&ItemSummary>,
) -> Result<PreparedPlayback, ApiError> {
    let source = select_source(
        info,
        options.media_source_id.as_deref(),
        options.media_source_index,
    )
    .ok_or_else(|| {
        ApiError::Decode(format!(
            "Jellyfin returned no playable media source for item {}",
            options.item_id
        ))
    })?;

    let play_session_id = info.play_session_id.clone();
    let (media_url, play_method) = stream_url(
        client,
        &options.item_id,
        source,
        quality,
        play_session_id.as_deref(),
    )?;

    let audio = choose_stream(source, "Audio", options.audio_stream_index);
    let subtitle = choose_stream(source, "Subtitle", options.subtitle_stream_index);

    let mut request = PlaybackRequest::new(media_url);
    request.headers = client.auth_headers();
    request.item_id = Some(options.item_id.clone());
    request.media_source_id.clone_from(&source.id);
    request.play_session_id = play_session_id;
    request.device_id = Some(client.device_id().to_string());
    request.start_time_ticks = (start_ticks > 0).then_some(start_ticks);
    request.runtime_ticks = source
        .run_time_ticks
        .filter(|ticks| *ticks > 0)
        .or_else(|| cached.and_then(|item| item.runtime_ticks));
    request.title = cached.map(display_title);
    request.play_method = Some(play_method.clone());
    request.audio_stream_index = audio.map(|stream| stream.index);
    let subtitles_off = options.subtitle_stream_index.is_some_and(|index| index < 0);
    request.subtitle_stream_index = if subtitles_off {
        Some(-1)
    } else {
        subtitle.map(|stream| stream.index)
    };
    request.audio_mpv_id = audio.and_then(|stream| embedded_ordinal(source, "Audio", stream.index));
    request.subtitle_mpv_id = if subtitles_off {
        Some(-1)
    } else {
        subtitle.and_then(|stream| embedded_ordinal(source, "Subtitle", stream.index))
    };
    request.subtitle_url = subtitle
        .filter(|stream| stream.is_external)
        .and_then(|stream| stream.delivery_url.as_deref())
        .map(|url| absolute_url(client.base_url(), url));

    Ok(PreparedPlayback {
        request,
        play_method,
        media_source_name: source.display_name().to_string(),
    })
}

fn select_source<'a>(
    info: &'a PlaybackInfoResponse,
    preferred_id: Option<&str>,
    preferred_index: Option<usize>,
) -> Option<&'a MediaSourceInfo> {
    if let Some(preferred_id) = preferred_id
        && let Some(source) = info
            .media_sources
            .iter()
            .find(|source| source.id.as_deref() == Some(preferred_id))
    {
        return Some(source);
    }
    if preferred_id.is_none()
        && let Some(preferred_index) = preferred_index
        && let Some(source) = info.media_sources.get(preferred_index)
    {
        return Some(source);
    }
    info.media_sources
        .iter()
        .find(|source| source.supports_direct_play || source.supports_direct_stream)
        .or_else(|| {
            info.media_sources
                .iter()
                .find(|source| source.transcoding_url.is_some())
        })
        .or_else(|| info.media_sources.first())
}

fn stream_url(
    client: &JellyfinClient,
    item_id: &str,
    source: &MediaSourceInfo,
    quality: StreamingQuality,
    play_session_id: Option<&str>,
) -> Result<(String, String), ApiError> {
    let direct = source.supports_direct_play || source.supports_direct_stream;
    if direct {
        let container = source
            .container
            .as_deref()
            .map(str::trim)
            .filter(|container| !container.is_empty())
            .and_then(|container| container.split(',').next())
            .unwrap_or("mkv");
        let mut query = vec![("static", "true".to_string())];
        if let Some(media_source_id) = &source.id {
            query.push(("mediaSourceId", media_source_id.clone()));
        }
        if let Some(play_session_id) = play_session_id {
            query.push(("playSessionId", play_session_id.to_string()));
        }
        if let Some(tag) = &source.e_tag {
            query.push(("tag", tag.clone()));
        }
        let url = format!(
            "{}?{}",
            join_url(
                client.base_url(),
                &format!(
                    "/Videos/{}/stream.{}",
                    encode_path_segment(item_id),
                    encode_path_segment(container)
                )
            ),
            build_query(&query)
        );
        let method = if source.supports_direct_play {
            "DirectPlay"
        } else {
            "DirectStream"
        };
        return Ok((url, method.to_string()));
    }

    if let Some(transcoding_url) = &source.transcoding_url {
        let url = remove_query_params(
            &absolute_url(client.base_url(), transcoding_url),
            TOKEN_QUERY_KEYS,
        );
        return Ok((url, "Transcode".to_string()));
    }

    Err(ApiError::Decode(format!(
        "media source {} cannot be direct played and offers no transcode (quality: {})",
        source.display_name(),
        quality.as_str()
    )))
}

fn absolute_url(base_url: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        join_url(base_url, url)
    }
}

fn choose_stream<'a>(
    source: &'a MediaSourceInfo,
    kind: &str,
    requested_index: Option<i64>,
) -> Option<&'a MediaStream> {
    if let Some(index) = requested_index {
        if index < 0 {
            return None;
        }
        return source
            .streams_of_type(kind)
            .find(|stream| stream.index == index);
    }
    let default_index = match kind {
        "Audio" => source.default_audio_stream_index,
        _ => source.default_subtitle_stream_index,
    };
    if let Some(index) = default_index
        && index >= 0
        && let Some(stream) = source
            .streams_of_type(kind)
            .find(|stream| stream.index == index)
    {
        return Some(stream);
    }
    if kind == "Audio" {
        return source
            .streams_of_type(kind)
            .find(|stream| stream.is_default)
            .or_else(|| source.streams_of_type(kind).next());
    }
    None
}

fn embedded_ordinal(source: &MediaSourceInfo, kind: &str, index: i64) -> Option<i64> {
    source
        .streams_of_type(kind)
        .filter(|stream| !stream.is_external)
        .position(|stream| stream.index == index)
        .map(|position| position as i64 + 1)
}

fn display_title(item: &ItemSummary) -> String {
    let name = if item.name.is_empty() {
        "Unknown"
    } else {
        item.name.as_str()
    };
    let (Some(series), Some(season), Some(episode)) = (
        item.series_name.as_deref(),
        item.parent_index_number,
        item.index_number,
    ) else {
        return name.to_string();
    };
    format!("{series} · S{season:02}E{episode:02} · {name}")
}

#[cfg(test)]
mod tests;
