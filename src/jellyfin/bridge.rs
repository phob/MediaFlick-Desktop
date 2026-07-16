use std::sync::OnceLock;

use serde::Deserialize;

use crate::app::build_info;
pub use crate::playback::PlaybackContext;
use crate::playback::{HttpHeader, PlaybackRequest, PlayerCommand};
use crate::preferences::AppSettings;

static BRIDGE_TOKEN: OnceLock<String> = OnceLock::new();
const BRIDGE_TOKEN_ENV: &str = "MEDIAFLICK_BRIDGE_TOKEN";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlayerCommandPayload {
    pub command: String,
    pub pause: Option<bool>,
    pub position_ms: Option<f64>,
    pub volume: Option<f64>,
    pub mute: Option<bool>,
    pub rate: Option<f64>,
    pub audio_mpv_id: Option<i64>,
    pub subtitle_mpv_id: Option<i64>,
    pub subtitle_url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlaybackStopAckPayload {
    pub active: Option<bool>,
    pub playback_id: Option<i64>,
    pub item_id: Option<String>,
    pub media_source_id: Option<String>,
    pub play_session_id: Option<String>,
    pub position_ms: Option<f64>,
    pub stop_reason: Option<String>,
    pub handled_players: usize,
    pub handled_synthetic: usize,
    pub ignored_players: usize,
    pub ignored_synthetic: usize,
    pub active_players: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeAction<'a> {
    SelectMpv,
    SelectMpcHc,
    About,
    DownloadMpv,
    MpvHelp,
    ClientSettings,
    Exit,
    DownloadUpdate(&'a str),
    OpenUpdateRelease,
    SaveWelcome(&'a str),
    SaveClientSettings(&'a str),
    RememberPlaybackContext(&'a str),
    StartPlayback(&'a str),
    PlayerState(&'a str),
    PlaybackStopAck(&'a str),
    PlayerCommand(&'a str),
}

pub fn parse_bridge_action(request_url: &str) -> Option<BridgeAction<'_>> {
    let request = request_url.strip_prefix("mediaflick-desktop://")?;
    let request = request.split('#').next().unwrap_or_default();
    let (path, query) = request
        .split_once('?')
        .map_or((request, ""), |(path, query)| (path, query));
    let action = path.strip_suffix('/').unwrap_or(path);
    if action.contains('/') {
        return None;
    }

    Some(match action {
        "select-mpv" => BridgeAction::SelectMpv,
        "select-mpchc" => BridgeAction::SelectMpcHc,
        "app-about" => BridgeAction::About,
        "mpv-download" => BridgeAction::DownloadMpv,
        "mpv-help" => BridgeAction::MpvHelp,
        "client-settings" => BridgeAction::ClientSettings,
        "app-exit" => BridgeAction::Exit,
        "update-download" => BridgeAction::DownloadUpdate(query),
        "update-release" => BridgeAction::OpenUpdateRelease,
        "save" => BridgeAction::SaveWelcome(query),
        "client-settings-save" => BridgeAction::SaveClientSettings(query),
        "play-context" => BridgeAction::RememberPlaybackContext(query),
        "play" => BridgeAction::StartPlayback(query),
        "player-state" => BridgeAction::PlayerState(query),
        "playback-stop-ack" => BridgeAction::PlaybackStopAck(query),
        "player-command" => BridgeAction::PlayerCommand(query),
        _ => return None,
    })
}

pub fn bridge_script(settings: &AppSettings) -> String {
    include_str!("bridge.js")
        .replace(
            "__MEDIAFLICK_PLAYBACK_SETTINGS_JSON__",
            &playback_settings_json(settings),
        )
        .replace("{{app_version}}", build_info::APP_VERSION)
        .replace("{{git_version}}", build_info::GIT_VERSION)
        .replace("{{created_by}}", build_info::CREATED_BY)
        .replace("{{bridge_token}}", bridge_token())
}

pub fn bridge_settings_script(settings: &AppSettings) -> String {
    format!(
        "window.__mediaFlickDesktopConfigureBridge&&window.__mediaFlickDesktopConfigureBridge({});",
        playback_settings_json(settings)
    )
}

fn playback_settings_json(settings: &AppSettings) -> String {
    serde_json::json!({
        "quality": settings.streaming_quality.as_str(),
        "enableTranscoding": settings.streaming_quality.allows_transcoding(),
        "maxStreamingBitrate": settings.streaming_quality.max_streaming_bitrate(),
        "playerBackend": settings.effective_backend().as_str(),
    })
    .to_string()
}

pub fn ensure_session_token() {
    BRIDGE_TOKEN.get_or_init(|| {
        if let Ok(token) = std::env::var(BRIDGE_TOKEN_ENV)
            && !token.is_empty()
        {
            return token;
        }
        let token = generate_bridge_token();
        unsafe {
            std::env::set_var(BRIDGE_TOKEN_ENV, &token);
        }
        token
    });
}

pub fn bridge_token() -> &'static str {
    BRIDGE_TOKEN.get_or_init(|| {
        std::env::var(BRIDGE_TOKEN_ENV)
            .ok()
            .filter(|token| !token.is_empty())
            .unwrap_or_else(generate_bridge_token)
    })
}

fn generate_bridge_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let mut token = String::with_capacity(64);
    for index in 0..4u64 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(index);
        hasher.write_u128(nanos);
        hasher.write_usize(token.len());
        token.push_str(&format!("{:016x}", hasher.finish()));
    }
    token
}

pub fn parse_context_payload(query: &str) -> Result<PlaybackContext, serde_json::Error> {
    let payload = query_param(query, "payload").unwrap_or_default();
    serde_json::from_str(&payload)
}

pub fn parse_launch_payload(query: &str) -> Result<PlaybackRequest, serde_json::Error> {
    let payload = query_param(query, "payload").unwrap_or_default();
    serde_json::from_str(&payload)
}

pub fn parse_player_command(query: &str) -> Result<Option<PlayerCommand>, serde_json::Error> {
    let payload = query_param(query, "payload").unwrap_or_default();
    let payload: PlayerCommandPayload = serde_json::from_str(&payload)?;
    Ok(match payload.command.as_str() {
        "set-pause" => payload.pause.map(PlayerCommand::SetPause),
        "seek" => payload
            .position_ms
            .filter(|value| value.is_finite())
            .map(PlayerCommand::SeekMilliseconds),
        "set-volume" => payload
            .volume
            .filter(|value| value.is_finite())
            .map(PlayerCommand::SetVolume),
        "set-mute" => payload.mute.map(PlayerCommand::SetMute),
        "set-playback-rate" => payload
            .rate
            .filter(|value| value.is_finite())
            .map(PlayerCommand::SetPlaybackRate),
        "set-audio-stream" => payload
            .audio_mpv_id
            .filter(|id| *id > 0)
            .map(PlayerCommand::SetAudioTrack),
        "set-subtitle-stream" => payload
            .subtitle_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(|url| PlayerCommand::AddSubtitle(url.to_string()))
            .or(Some(PlayerCommand::SetSubtitleTrack(
                payload.subtitle_mpv_id,
            ))),
        "stop" => Some(PlayerCommand::Stop),
        _ => None,
    })
}

pub fn parse_playback_stop_ack_payload(
    query: &str,
) -> Result<PlaybackStopAckPayload, serde_json::Error> {
    let payload = query_param(query, "payload").unwrap_or_default();
    serde_json::from_str(&payload)
}

pub fn launch_from_stream_url(url: &str, headers: Vec<HttpHeader>) -> Option<PlaybackRequest> {
    if !is_direct_stream_url(url) {
        return None;
    }

    let mut launch = PlaybackRequest::new(url.to_string());
    launch.headers = headers;
    launch.item_id = item_id_from_stream_url(url);
    launch.media_source_id = query_param_ci_from_url(url, "MediaSourceId");
    launch.play_session_id = query_param_ci_from_url(url, "PlaySessionId");
    launch.device_id = query_param_ci_from_url(url, "DeviceId");
    launch.start_time_ticks = query_param_ci_from_url(url, "StartTimeTicks")
        .or_else(|| query_param_ci_from_url(url, "startTimeTicks"))
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|ticks| *ticks > 0);
    launch.start_milliseconds = query_param_ci_from_url(url, "StartPositionTicks")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|ticks| *ticks > 0)
        .map(|ticks| ticks as f64 / 10_000.0);
    Some(launch)
}

fn is_direct_stream_url(url: &str) -> bool {
    let path = url_path(url).to_ascii_lowercase();
    if !(path.contains("/videos/") || path.contains("/audio/")) {
        return false;
    }
    if path.contains("/hls") || path.contains("/dash") || path.contains("/transcoding") {
        return false;
    }
    path.contains("/stream") || path.contains("/original")
}

fn item_id_from_stream_url(url: &str) -> Option<String> {
    let path = url_path(url);
    item_id_after_segment(path, "Videos").or_else(|| item_id_after_segment(path, "Audio"))
}

fn item_id_after_segment(path: &str, segment: &str) -> Option<String> {
    let needle = format!("/{segment}/");
    let lower_path = path.to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();
    let start = lower_path.find(&lower_needle)? + needle.len();
    let id = path[start..].split('/').next().unwrap_or_default().trim();
    (!id.is_empty()).then(|| percent_decode(id))
}

fn url_path(url: &str) -> &str {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let path = after_scheme
        .find('/')
        .map(|index| &after_scheme[index..])
        .unwrap_or_default();
    path.split(['?', '#']).next().unwrap_or_default()
}

fn query_param_ci_from_url(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next().unwrap_or_default();
    query_param_ci(query, key)
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        (percent_decode(raw_key) == key).then(|| percent_decode(raw_value))
    })
}

fn query_param_ci(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        percent_decode(raw_key)
            .eq_ignore_ascii_case(key)
            .then(|| percent_decode(raw_value))
    })
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BridgeAction, bridge_script, bridge_settings_script, bridge_token, generate_bridge_token,
        item_id_from_stream_url, launch_from_stream_url, parse_bridge_action,
    };
    use crate::preferences::{AppSettings, StreamingQuality};

    #[test]
    fn parses_bridge_actions_exactly() {
        assert_eq!(
            parse_bridge_action("mediaflick-desktop://play?payload=value#ignored"),
            Some(BridgeAction::StartPlayback("payload=value"))
        );
        assert_eq!(
            parse_bridge_action("mediaflick-desktop://client-settings/"),
            Some(BridgeAction::ClientSettings)
        );
        assert_eq!(
            parse_bridge_action("mediaflick-desktop://app-exit-malicious"),
            None
        );
        assert_eq!(
            parse_bridge_action("mediaflick-desktop://play/extra?payload=value"),
            None
        );
    }

    #[test]
    fn bridge_script_contains_configured_streaming_quality() {
        let settings = AppSettings {
            streaming_quality: StreamingQuality::Mbps10,
            ..Default::default()
        };
        let script = bridge_script(&settings);
        assert!(script.contains("\"quality\":\"10_mbps\""));
        assert!(script.contains("\"maxStreamingBitrate\":10000000"));
        assert!(script.contains("if (!isPlaybackInfoUrl(value) && !isLiveStreamOpenUrl(value))"));
        assert!(script.contains("{ Format, Method: 'External' }"));
        assert!(!script.contains("__MEDIAFLICK_PLAYBACK_SETTINGS_JSON__"));

        let update = bridge_settings_script(&settings);
        assert!(update.contains("__mediaFlickDesktopConfigureBridge"));
        assert!(update.contains("\"quality\":\"10_mbps\""));
        assert!(update.contains("\"enableTranscoding\":true"));
    }

    #[test]
    fn original_quality_disables_configured_bitrate() {
        let script = bridge_settings_script(&AppSettings::default());
        assert!(script.contains("\"quality\":\"original\""));
        assert!(script.contains("\"enableTranscoding\":false"));
        assert!(script.contains("\"maxStreamingBitrate\":null"));
    }

    #[test]
    fn generated_tokens_are_unique_and_hex() {
        let first = generate_bridge_token();
        let second = generate_bridge_token();
        assert_ne!(first, second);
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn session_bridge_token_is_stable() {
        assert_eq!(bridge_token(), bridge_token());
    }

    #[test]
    fn extracts_stream_metadata() {
        let launch = launch_from_stream_url(
            "http://server/Videos/abc/stream.mkv?MediaSourceId=ms&PlaySessionId=ps&StartTimeTicks=10000000&api_key=secret",
            Vec::new(),
        )
        .expect("stream URL");
        assert_eq!(launch.item_id.as_deref(), Some("abc"));
        assert_eq!(launch.media_source_id.as_deref(), Some("ms"));
        assert_eq!(launch.play_session_id.as_deref(), Some("ps"));
        assert_eq!(launch.start_time_ticks, Some(10_000_000));
    }

    #[test]
    fn ignores_hls_transcodes() {
        assert!(
            launch_from_stream_url("http://server/Videos/abc/hls/master.m3u8", Vec::new())
                .is_none()
        );
    }

    #[test]
    fn handles_audio_streams() {
        assert_eq!(
            item_id_from_stream_url("https://server/Audio/song-id/stream.mp3?Static=true")
                .as_deref(),
            Some("song-id")
        );
    }
}
