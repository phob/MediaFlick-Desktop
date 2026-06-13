use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mpv::{HttpHeader, MpvLaunch};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlaybackContext {
    #[serde(alias = "url")]
    pub media_url: Option<String>,
    pub item_id: Option<String>,
    pub media_source_id: Option<String>,
    pub play_session_id: Option<String>,
    pub device_id: Option<String>,
    #[serde(alias = "startPositionTicks")]
    pub start_time_ticks: Option<i64>,
    pub start_milliseconds: Option<f64>,
    pub runtime_ticks: Option<i64>,
    pub title: Option<String>,
    pub audio_stream_index: Option<i64>,
    pub subtitle_stream_index: Option<i64>,
    pub play_method: Option<String>,
    pub playlist_item_id: Option<String>,
    pub queue: Option<Value>,
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlayerCommandPayload {
    pub command: String,
    pub pause: Option<bool>,
    pub position_ms: Option<f64>,
    pub volume: Option<f64>,
    pub mute: Option<bool>,
    pub rate: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlaybackStopAckPayload {
    pub active: Option<bool>,
    pub position_ms: Option<f64>,
    pub handled_players: usize,
    pub handled_synthetic: usize,
    pub active_players: usize,
}

impl PlaybackContext {
    pub fn merge_into_launch(&self, launch: &mut MpvLaunch) {
        let context_launch = MpvLaunch {
            media_url: self.media_url.clone().unwrap_or_default(),
            item_id: self.item_id.clone(),
            media_source_id: self.media_source_id.clone(),
            play_session_id: self.play_session_id.clone(),
            device_id: self.device_id.clone(),
            start_time_ticks: self.start_time_ticks,
            start_milliseconds: self.start_milliseconds,
            runtime_ticks: self.runtime_ticks,
            title: self.title.clone(),
            audio_stream_index: self.audio_stream_index,
            subtitle_stream_index: self.subtitle_stream_index,
            play_method: self.play_method.clone(),
            playlist_item_id: self.playlist_item_id.clone(),
            queue: self.queue.clone(),
            details: self.details.clone(),
            ..Default::default()
        };
        launch.merge_missing_from(&context_launch);
    }

    pub fn match_score(&self, launch: &MpvLaunch) -> u8 {
        let mut score = 0;
        if same_non_empty(self.media_url.as_deref(), Some(launch.media_url.as_str())) {
            score = score.max(5);
        }
        if same_non_empty(
            self.play_session_id.as_deref(),
            launch.play_session_id.as_deref(),
        ) {
            score = score.max(4);
        }
        if same_non_empty(
            self.media_source_id.as_deref(),
            launch.media_source_id.as_deref(),
        ) {
            score = score.max(3);
        }
        if same_non_empty(self.item_id.as_deref(), launch.item_id.as_deref()) {
            score = score.max(2);
        }
        score
    }
}

pub fn bridge_script() -> &'static str {
    include_str!("bridge.js")
}

pub fn parse_context_payload(query: &str) -> Result<PlaybackContext, serde_json::Error> {
    let payload = query_param(query, "payload").unwrap_or_default();
    serde_json::from_str(&payload)
}

pub fn parse_launch_payload(query: &str) -> Result<MpvLaunch, serde_json::Error> {
    let payload = query_param(query, "payload").unwrap_or_default();
    serde_json::from_str(&payload)
}

pub fn parse_player_command_payload(
    query: &str,
) -> Result<PlayerCommandPayload, serde_json::Error> {
    let payload = query_param(query, "payload").unwrap_or_default();
    serde_json::from_str(&payload)
}

pub fn parse_playback_stop_ack_payload(
    query: &str,
) -> Result<PlaybackStopAckPayload, serde_json::Error> {
    let payload = query_param(query, "payload").unwrap_or_default();
    serde_json::from_str(&payload)
}

pub fn launch_from_stream_url(url: &str, headers: Vec<HttpHeader>) -> Option<MpvLaunch> {
    if !is_direct_stream_url(url) {
        return None;
    }

    let mut launch = MpvLaunch::new(url.to_string());
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

pub fn redact_url_secrets(url: &str) -> String {
    crate::app::logger::redact_url_secrets(url)
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

fn same_non_empty(left: Option<&str>, right: Option<&str>) -> bool {
    let Some(left) = left.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let Some(right) = right.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    left.eq_ignore_ascii_case(right)
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
    use super::{item_id_from_stream_url, launch_from_stream_url, redact_url_secrets};

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

    #[test]
    fn redacts_jellyfin_apikey_query() {
        assert_eq!(
            redact_url_secrets("https://server/Videos/abc/stream.mkv?ApiKey=secret&Static=true"),
            "https://server/Videos/abc/stream.mkv?ApiKey=REDACTED&Static=true"
        );
    }
}
