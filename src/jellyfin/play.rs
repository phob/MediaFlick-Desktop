//! Turns "play this item" into a fully-formed [`PlaybackRequest`].
//!
//! This is the work jellyfin-web used to do before the bridge captured its
//! stream URL: negotiate `PlaybackInfo` with a device profile, pick a media
//! source, and build the direct-stream or transcoding URL ourselves.

use std::sync::Arc;

use crate::app::services::Services;
use crate::app::urls::{build_query, encode_path_segment, join_url};
use crate::library::model::ResolvedPlaybackPreference;
use crate::library::{Library, resolve_playback_preference};
use crate::playback::{PlaybackContext, PlaybackRequest};
use crate::preferences::StreamingQuality;

use super::api::items::{self, PlaybackInfoRequest};
use super::api::model::{MediaSourceInfo, MediaStream, PlaybackInfoResponse};
use super::api::{ApiError, JellyfinClient};
use super::session::Session;

/// What the UI asked for.
#[derive(Debug, Clone, Default)]
pub struct PlayOptions {
    pub item_id: String,
    /// Resume from the cached playback position instead of the start.
    pub resume: bool,
    pub start_ticks: Option<i64>,
    pub media_source_id: Option<String>,
    /// Used only when a Jellyfin source has no stable id. Persisted choices
    /// retain the source's position and identity so playback can still select
    /// the same source from the negotiated response.
    pub media_source_index: Option<usize>,
    pub audio_stream_index: Option<i64>,
    pub subtitle_stream_index: Option<i64>,
    pub quality: Option<StreamingQuality>,
}

/// The negotiated request plus what the UI needs to describe it.
#[derive(Debug, Clone)]
pub struct PreparedPlayback {
    pub request: PlaybackRequest,
    pub play_method: String,
    pub media_source_name: String,
}

/// Why [`start`] could not launch the player.
#[derive(Debug)]
pub enum StartError {
    /// No mpv/MPC-HC path is configured in Settings.
    NoPlayer,
    /// The playback coordinator has not been attached yet.
    NotReady,
    Api(ApiError),
}

/// Negotiates `options` and launches the configured external player.
///
/// This is the one launch path shared by the UI's play endpoints and
/// remote-control Play messages, so both start playback identically.
pub fn start(
    services: &Arc<Services>,
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

    let prepared = prepare(
        &services.session,
        &services.library,
        services
            .session
            .account_key()
            .and_then(|account| {
                services
                    .playback_preferences
                    .get(&account, &options.item_id)
            })
            .as_ref(),
        settings.streaming_quality,
        options,
    )
    .map_err(StartError::Api)?;

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
    playback.open(
        player_path,
        settings.default_fullscreen,
        prepared.request.clone(),
    );
    playback.update_context(context_from(&prepared.request));
    Ok(prepared)
}

/// Feeds the player adapters the same identity the launch carries so their
/// pending playback is complete before the file loads.
fn context_from(request: &PlaybackRequest) -> PlaybackContext {
    PlaybackContext {
        media_url: Some(request.media_url.clone()),
        item_id: request.item_id.clone(),
        media_source_id: request.media_source_id.clone(),
        play_session_id: request.play_session_id.clone(),
        device_id: request.device_id.clone(),
        start_time_ticks: request.start_time_ticks,
        runtime_ticks: request.runtime_ticks,
        title: request.title.clone(),
        audio_stream_index: request.audio_stream_index,
        subtitle_stream_index: request.subtitle_stream_index,
        play_method: request.play_method.clone(),
        ..Default::default()
    }
}

pub fn prepare(
    session: &Session,
    library: &Library,
    saved_preference: Option<&crate::library::ItemPlaybackPreference>,
    quality: StreamingQuality,
    options: &PlayOptions,
) -> Result<PreparedPlayback, ApiError> {
    let (client, user_id) = session.client_and_user()?;
    let quality = options.quality.unwrap_or(quality);
    let cached = library.item(&options.item_id).ok().flatten();

    let start_ticks = options
        .start_ticks
        .or_else(|| {
            options.resume.then(|| {
                cached
                    .as_ref()
                    .and_then(|item| item["positionTicks"].as_i64())
                    .unwrap_or(0)
            })
        })
        .unwrap_or(0)
        .max(0);

    let mut effective_options = options.clone();
    let has_explicit_track_options = options.media_source_id.is_some()
        || options.media_source_index.is_some()
        || options.audio_stream_index.is_some()
        || options.subtitle_stream_index.is_some();
    if !has_explicit_track_options && let Some(preference) = saved_preference {
        match items::fetch_media_sources(&client, &user_id, &options.item_id) {
            Ok(sources) => {
                if let Some(resolved) = resolve_playback_preference(Some(preference), &sources) {
                    apply_saved_preference(&mut effective_options, resolved);
                }
            }
            Err(error) => {
                session.note_error(&error);
                tracing::warn!(
                    target: "playback",
                    item_id = %options.item_id,
                    "could not validate saved track preference; using server defaults: {error}"
                );
            }
        }
    }

    let info_request = PlaybackInfoRequest {
        media_source_id: effective_options.media_source_id.clone(),
        start_time_ticks: Some(start_ticks),
        audio_stream_index: effective_options.audio_stream_index,
        subtitle_stream_index: effective_options.subtitle_stream_index,
    };
    let info = items::playback_info(&client, &user_id, &options.item_id, quality, &info_request)
        .inspect_err(|error| session.note_error(error))?;

    build(
        &client,
        &info,
        quality,
        &effective_options,
        start_ticks,
        cached.as_ref(),
    )
}

fn apply_saved_preference(options: &mut PlayOptions, resolved: ResolvedPlaybackPreference) {
    options.media_source_id = resolved.media_source_id;
    options.media_source_index = Some(resolved.media_source_index);
    options.audio_stream_index = resolved.audio_stream_index;
    // Existing preference + no resolved subtitle is an explicit/current
    // subtitles-off result. Carry the sentinel through both Jellyfin and
    // player mapping.
    options.subtitle_stream_index = Some(resolved.subtitle_stream_index.unwrap_or(-1));
}

fn build(
    client: &JellyfinClient,
    info: &PlaybackInfoResponse,
    quality: StreamingQuality,
    options: &PlayOptions,
    start_ticks: i64,
    cached: Option<&serde_json::Value>,
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
        .or_else(|| cached.and_then(|item| item["runtimeTicks"].as_i64()));
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

/// Prefers the source the caller asked for, then anything playable, then
/// anything transcodable.
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
    // A positional fallback is only safe for a source that had no stable id.
    // If a saved id disappeared between validation and negotiation, the same
    // position can now be a different file and must use the ordinary default.
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

/// Builds the URL the external player opens, together with the `PlayMethod`
/// value Jellyfin expects in playstate reports.
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
        // No `api_key`: the token travels in `request.headers` instead, so it
        // stays out of the player's command line, its recent-files list, and
        // its logs. mpv sends those headers; the MPC-HC controller, which
        // cannot, appends the token to the URL itself at launch time.
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
        return Ok((
            absolute_url(client.base_url(), transcoding_url),
            "Transcode".to_string(),
        ));
    }

    Err(ApiError::Decode(format!(
        "media source {} cannot be direct played and offers no transcode (quality: {})",
        source.display_name(),
        quality.as_str()
    )))
}

/// Jellyfin returns transcoding and subtitle URLs relative to the server root.
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

/// Players address embedded tracks by their one-based position among tracks of
/// the same type, not by Jellyfin's absolute stream index.
fn embedded_ordinal(source: &MediaSourceInfo, kind: &str, index: i64) -> Option<i64> {
    source
        .streams_of_type(kind)
        .filter(|stream| !stream.is_external)
        .position(|stream| stream.index == index)
        .map(|position| position as i64 + 1)
}

fn display_title(item: &serde_json::Value) -> String {
    let name = item["name"].as_str().unwrap_or("Unknown");
    let (Some(series), Some(season), Some(episode)) = (
        item["seriesName"].as_str(),
        item["parentIndexNumber"].as_i64(),
        item["indexNumber"].as_i64(),
    ) else {
        return name.to_string();
    };
    format!("{series} · S{season:02}E{episode:02} · {name}")
}

#[cfg(test)]
mod tests {
    use super::{
        PlayOptions, absolute_url, apply_saved_preference, build, choose_stream, display_title,
        embedded_ordinal, select_source,
    };
    use crate::jellyfin::api::JellyfinClient;
    use crate::jellyfin::api::model::{MediaSourceInfo, PlaybackInfoResponse};
    use crate::library::model::ResolvedPlaybackPreference;
    use crate::preferences::StreamingQuality;
    use serde_json::json;

    fn client() -> JellyfinClient {
        JellyfinClient::new("http://server:8096", "device-1", Some("secret"))
    }

    fn info(json: &str) -> PlaybackInfoResponse {
        serde_json::from_str(json).expect("playback info")
    }

    fn media_source(json: &str) -> MediaSourceInfo {
        serde_json::from_str(json).expect("media source")
    }

    fn options() -> PlayOptions {
        PlayOptions {
            item_id: "item-1".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn direct_stream_sources_get_a_static_stream_url() {
        let response = info(
            r#"{"PlaySessionId":"session","MediaSources":[{"Id":"src","Container":"mkv",
                "SupportsDirectStream":true,"ETag":"etag","RunTimeTicks":120000000}]}"#,
        );
        let prepared = build(
            &client(),
            &response,
            StreamingQuality::Original,
            &options(),
            0,
            None,
        )
        .expect("prepared");
        assert_eq!(prepared.play_method, "DirectStream");
        assert!(
            prepared
                .request
                .media_url
                .starts_with("http://server:8096/Videos/item-1/stream.mkv?")
        );
        assert!(prepared.request.media_url.contains("static=true"));
        assert!(!prepared.request.media_url.contains("api_key"));
        assert!(prepared.request.media_url.contains("mediaSourceId=src"));
        assert!(prepared.request.media_url.contains("playSessionId=session"));
        assert!(prepared.request.media_url.contains("tag=etag"));
        assert_eq!(prepared.request.play_session_id.as_deref(), Some("session"));
        assert_eq!(prepared.request.runtime_ticks, Some(120_000_000));
        assert_eq!(prepared.request.device_id.as_deref(), Some("device-1"));
        assert!(
            prepared
                .request
                .headers
                .iter()
                .any(|header| header.name == "X-Emby-Token")
        );
    }

    #[test]
    fn direct_play_is_reported_separately_from_direct_stream() {
        let response =
            info(r#"{"MediaSources":[{"Id":"src","Container":"mp4","SupportsDirectPlay":true}]}"#);
        let prepared = build(
            &client(),
            &response,
            StreamingQuality::Original,
            &options(),
            0,
            None,
        )
        .expect("prepared");
        assert_eq!(prepared.play_method, "DirectPlay");
        assert!(prepared.request.media_url.contains("/stream.mp4?"));
    }

    #[test]
    fn transcoding_urls_are_resolved_against_the_server_root() {
        let response = info(
            r#"{"MediaSources":[{"Id":"src","SupportsTranscoding":true,
                "TranscodingUrl":"/videos/item-1/master.m3u8?api_key=secret"}]}"#,
        );
        let prepared = build(
            &client(),
            &response,
            StreamingQuality::Mbps10,
            &options(),
            0,
            None,
        )
        .expect("prepared");
        assert_eq!(prepared.play_method, "Transcode");
        assert_eq!(
            prepared.request.media_url,
            "http://server:8096/videos/item-1/master.m3u8?api_key=secret"
        );
    }

    #[test]
    fn a_source_without_any_delivery_option_is_an_error() {
        let response = info(r#"{"MediaSources":[{"Id":"src"}]}"#);
        assert!(
            build(
                &client(),
                &response,
                StreamingQuality::Original,
                &options(),
                0,
                None
            )
            .is_err()
        );
        assert!(
            build(
                &client(),
                &info(r#"{"MediaSources":[]}"#),
                StreamingQuality::Original,
                &options(),
                0,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn the_requested_media_source_wins_over_the_first_playable_one() {
        let response = info(
            r#"{"MediaSources":[{"Id":"a","SupportsDirectStream":true},
                                {"Id":"b","SupportsDirectStream":true}]}"#,
        );
        assert_eq!(
            select_source(&response, Some("b"), None).and_then(|source| source.id.clone()),
            Some("b".to_string())
        );
        assert_eq!(
            select_source(&response, Some("missing"), None).and_then(|source| source.id.clone()),
            Some("a".to_string())
        );
    }

    #[test]
    fn transcode_only_sources_are_used_when_nothing_direct_plays() {
        let response =
            info(r#"{"MediaSources":[{"Id":"a"},{"Id":"b","TranscodingUrl":"/x.m3u8"}]}"#);
        assert_eq!(
            select_source(&response, None, None).and_then(|source| source.id.clone()),
            Some("b".to_string())
        );
    }

    #[test]
    fn saved_preferences_map_to_play_options_including_subtitles_off() {
        let mut selected = options();
        apply_saved_preference(
            &mut selected,
            ResolvedPlaybackPreference {
                media_source_id: Some("source-b".to_string()),
                media_source_index: 1,
                audio_stream_index: Some(7),
                subtitle_stream_index: None,
            },
        );
        assert_eq!(selected.media_source_id.as_deref(), Some("source-b"));
        assert_eq!(selected.media_source_index, Some(1));
        assert_eq!(selected.audio_stream_index, Some(7));
        assert_eq!(selected.subtitle_stream_index, Some(-1));
    }

    #[test]
    fn source_index_selects_a_source_without_a_stable_id() {
        let response = info(
            r#"{"MediaSources":[{"Name":"A","SupportsDirectStream":true},
                                {"Name":"B","SupportsDirectStream":true}]}"#,
        );
        assert_eq!(
            select_source(&response, None, Some(1)).map(MediaSourceInfo::display_name),
            Some("B")
        );
        assert_eq!(
            select_source(&response, Some("gone"), Some(1)).map(MediaSourceInfo::display_name),
            Some("A")
        );
    }

    #[test]
    fn track_selection_prefers_the_server_default_then_the_first_audio_track() {
        let source = media_source(
            r#"{"Id":"src","DefaultAudioStreamIndex":3,"MediaStreams":[
                {"Index":1,"Type":"Video"},
                {"Index":2,"Type":"Audio","Language":"eng"},
                {"Index":3,"Type":"Audio","Language":"deu"}]}"#,
        );
        assert_eq!(
            choose_stream(&source, "Audio", None).map(|stream| stream.index),
            Some(3)
        );
        assert_eq!(
            choose_stream(&source, "Audio", Some(2)).map(|stream| stream.index),
            Some(2)
        );
        assert!(choose_stream(&source, "Audio", Some(99)).is_none());
        assert!(choose_stream(&source, "Audio", Some(-1)).is_none());

        let without_default = media_source(
            r#"{"Id":"src","MediaStreams":[{"Index":5,"Type":"Audio"},{"Index":6,"Type":"Audio"}]}"#,
        );
        assert_eq!(
            choose_stream(&without_default, "Audio", None).map(|stream| stream.index),
            Some(5)
        );
    }

    #[test]
    fn subtitles_are_off_unless_the_server_or_the_user_picks_one() {
        let source = media_source(
            r#"{"Id":"src","MediaStreams":[{"Index":3,"Type":"Subtitle","Language":"eng"}]}"#,
        );
        assert!(choose_stream(&source, "Subtitle", None).is_none());
        assert_eq!(
            choose_stream(&source, "Subtitle", Some(3)).map(|stream| stream.index),
            Some(3)
        );
    }

    #[test]
    fn explicit_subtitles_off_is_mapped_to_player_off_sentinels() {
        let response = info(
            r#"{"MediaSources":[{"Id":"src","Container":"mkv","SupportsDirectStream":true,
                "DefaultSubtitleStreamIndex":3,
                "MediaStreams":[{"Index":3,"Type":"Subtitle","Language":"eng"}]}]}"#,
        );
        let prepared = build(
            &client(),
            &response,
            StreamingQuality::Original,
            &PlayOptions {
                subtitle_stream_index: Some(-1),
                ..options()
            },
            0,
            None,
        )
        .expect("prepared");
        assert_eq!(prepared.request.subtitle_stream_index, Some(-1));
        assert_eq!(prepared.request.subtitle_mpv_id, Some(-1));
        assert_eq!(prepared.request.subtitle_url, None);
    }

    #[test]
    fn embedded_ordinals_count_only_internal_tracks_of_the_same_type() {
        let source = media_source(
            r#"{"Id":"src","MediaStreams":[
                {"Index":0,"Type":"Video"},
                {"Index":1,"Type":"Audio"},
                {"Index":2,"Type":"Audio"},
                {"Index":3,"Type":"Subtitle","IsExternal":true},
                {"Index":4,"Type":"Subtitle"}]}"#,
        );
        assert_eq!(embedded_ordinal(&source, "Audio", 1), Some(1));
        assert_eq!(embedded_ordinal(&source, "Audio", 2), Some(2));
        assert_eq!(embedded_ordinal(&source, "Subtitle", 4), Some(1));
        assert_eq!(embedded_ordinal(&source, "Subtitle", 3), None);
    }

    #[test]
    fn external_subtitles_are_handed_to_the_player_as_an_absolute_url() {
        let response = info(
            r#"{"MediaSources":[{"Id":"src","Container":"mkv","SupportsDirectStream":true,
                "MediaStreams":[{"Index":3,"Type":"Subtitle","IsExternal":true,
                                 "DeliveryUrl":"/Videos/item-1/subs.srt"}]}]}"#,
        );
        let prepared = build(
            &client(),
            &response,
            StreamingQuality::Original,
            &PlayOptions {
                subtitle_stream_index: Some(3),
                ..options()
            },
            0,
            None,
        )
        .expect("prepared");
        assert_eq!(
            prepared.request.subtitle_url.as_deref(),
            Some("http://server:8096/Videos/item-1/subs.srt")
        );
        assert_eq!(prepared.request.subtitle_stream_index, Some(3));
        assert_eq!(prepared.request.subtitle_mpv_id, None);
    }

    #[test]
    fn resume_positions_travel_with_the_request() {
        let response = info(
            r#"{"MediaSources":[{"Id":"src","Container":"mkv","SupportsDirectStream":true}]}"#,
        );
        let prepared = build(
            &client(),
            &response,
            StreamingQuality::Original,
            &options(),
            600_000_000,
            None,
        )
        .expect("prepared");
        assert_eq!(prepared.request.start_time_ticks, Some(600_000_000));
        assert_eq!(prepared.request.start_seconds(), Some(60.0));
    }

    #[test]
    fn episode_titles_read_as_series_season_episode() {
        let episode = json!({
            "name": "Half Loop",
            "seriesName": "Severance",
            "parentIndexNumber": 1,
            "indexNumber": 2,
        });
        assert_eq!(display_title(&episode), "Severance · S01E02 · Half Loop");
        assert_eq!(display_title(&json!({ "name": "Arrival" })), "Arrival");
    }

    #[test]
    fn absolute_urls_are_left_alone() {
        assert_eq!(
            absolute_url("http://server:8096", "https://cdn.test/x.m3u8"),
            "https://cdn.test/x.m3u8"
        );
        assert_eq!(
            absolute_url("http://server:8096", "/x.m3u8"),
            "http://server:8096/x.m3u8"
        );
    }
}
