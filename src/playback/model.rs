//! Backend-neutral playback values shared by the application and player adapters.

use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::urls::percent_decode;

/// Jellyfin positions and durations are ticks of 100 ns.
pub const TICKS_PER_SECOND: f64 = 10_000_000.0;
pub const TICKS_PER_MILLISECOND: f64 = 10_000.0;

static PLAYBACK_COUNTER: AtomicI64 = AtomicI64::new(1);

pub(crate) fn allocate_playback_id() -> i64 {
    PLAYBACK_COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub fn seconds_to_ticks(seconds: f64) -> Option<i64> {
    seconds
        .is_finite()
        .then(|| (seconds.max(0.0) * TICKS_PER_SECOND).round() as i64)
}

pub fn ticks_to_seconds(ticks: i64) -> f64 {
    ticks as f64 / TICKS_PER_SECOND
}

pub fn ticks_to_milliseconds(ticks: i64) -> f64 {
    ticks as f64 / TICKS_PER_MILLISECOND
}

/// The trimmed text, or `None` when it is missing or blank.
pub fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Query parameter names Jellyfin accepts an access token under, compared
/// ASCII case-insensitively. Media URLs handed to a player must not carry
/// them; the token travels in [`PlaybackRequest::headers`] instead.
pub const TOKEN_QUERY_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "access_token",
    "accesstoken",
    "x-emby-token",
    "x-mediabrowser-token",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

/// A request to open media in the selected player.
///
/// The serialized field names remain compatible with the existing Jellyfin Web
/// bridge. Backend-specific track translation happens in each player adapter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlaybackRequest {
    #[serde(alias = "url")]
    pub media_url: String,
    pub headers: Vec<HttpHeader>,
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
    // These names are retained on the wire for compatibility. They represent
    // one-based embedded track ordinals and are translated by each adapter.
    pub audio_mpv_id: Option<i64>,
    pub subtitle_mpv_id: Option<i64>,
    pub subtitle_url: Option<String>,
    pub play_method: Option<String>,
    pub playlist_item_id: Option<String>,
    pub queue: Option<Value>,
    pub details: Option<Value>,
}

impl PlaybackRequest {
    pub fn new(media_url: impl Into<String>) -> Self {
        Self {
            media_url: media_url.into(),
            ..Default::default()
        }
    }

    pub fn start_seconds(&self) -> Option<f64> {
        if let Some(milliseconds) = self.start_milliseconds.filter(|value| *value > 0.0) {
            return Some(milliseconds / 1000.0);
        }
        self.start_time_ticks
            .filter(|ticks| *ticks > 0)
            .map(ticks_to_seconds)
    }

    pub fn dedupe_key(&self) -> String {
        if let Some(play_session_id) = non_empty(self.play_session_id.as_deref()) {
            return format!("play-session:{play_session_id}");
        }
        if let (Some(item_id), Some(media_source_id)) = (
            non_empty(self.item_id.as_deref()),
            non_empty(self.media_source_id.as_deref()),
        ) {
            return format!("item:{item_id}:source:{media_source_id}");
        }
        redact_url_query_value(&self.media_url, TOKEN_QUERY_KEYS)
    }
}

/// State used for playstate reporting. Time is represented in Jellyfin ticks
/// until the reporting protocol can migrate independently from its wire format.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReportingState {
    pub position_ticks: i64,
    pub pause: bool,
    pub duration_ticks: Option<i64>,
    pub volume: Option<i64>,
    pub mute: Option<bool>,
    pub eof_reached: bool,
}

impl fmt::Display for ReportingState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "position={} duration={} paused={} volume={} muted={} eof={}",
            ticks_summary(Some(self.position_ticks.max(0))),
            ticks_summary(self.duration_ticks),
            self.pause,
            self.volume
                .map(|volume| volume.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            self.mute
                .map(|mute| mute.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            self.eof_reached
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerTrackKind {
    Audio,
    Subtitle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerTrack {
    pub id: i64,
    pub kind: PlayerTrackKind,
    pub language: Option<String>,
    pub title: Option<String>,
    pub codec: Option<String>,
    pub selected: bool,
    pub external: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerChapter {
    pub title: String,
    pub start_ms: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFit {
    Fit,
    Fill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoAspect {
    Source,
    Ratio4x3,
    Ratio16x9,
    Ratio21x9,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneMapping {
    Auto,
    Clip,
    Mobius,
    Reinhard,
    Hable,
    Bt2390,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackDiagnostics {
    pub buffered_until_ms: Option<f64>,
    pub buffering: bool,
    pub dropped_frames: Option<i64>,
    pub frame_rate: Option<f64>,
}

/// The player state the UI reads, from `GET /api/player/state` and from the
/// pushed playback events alike.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub active: bool,
    pub playback_id: Option<i64>,
    pub item_id: Option<String>,
    pub media_source_id: Option<String>,
    pub play_session_id: Option<String>,
    pub play_method: Option<String>,
    pub position_ms: f64,
    pub duration_ms: Option<f64>,
    pub paused: bool,
    pub volume: Option<i64>,
    pub mute: Option<bool>,
    pub tracks: Vec<PlayerTrack>,
    pub chapters: Vec<PlayerChapter>,
    pub skip_segments: Vec<crate::playback::segments::SkipSegment>,
    pub diagnostics: PlaybackDiagnostics,
    pub stop_reason: Option<StopReason>,
}

/// Why playback ended, sent to the UI as `stopReason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StopReason {
    /// The file played to its end.
    Eof,
    /// The viewer marked the item watched and asked for the next one.
    WatchedNext,
    Stop,
    Quit,
    Error,
    Redirect,
    Shutdown,
    /// A reason this app does not know.
    Unknown,
}

impl StopReason {
    /// Reads an mpv `end-file` reason, ignoring case and surrounding space.
    /// A missing or blank reason is `None`.
    pub fn parse(reason: &str) -> Option<Self> {
        let reason = reason.trim();
        if reason.is_empty() {
            return None;
        }
        Some(match reason.to_ascii_lowercase().as_str() {
            "eof" => Self::Eof,
            "watched-next" => Self::WatchedNext,
            "stop" => Self::Stop,
            "quit" => Self::Quit,
            "error" => Self::Error,
            "redirect" => Self::Redirect,
            "shutdown" => Self::Shutdown,
            _ => Self::Unknown,
        })
    }

    /// The item was finished, so it counts as watched.
    pub fn is_completion(self) -> bool {
        matches!(self, Self::Eof | Self::WatchedNext)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eof => "eof",
            Self::WatchedNext => "watched-next",
            Self::Stop => "stop",
            Self::Quit => "quit",
            Self::Error => "error",
            Self::Redirect => "redirect",
            Self::Shutdown => "shutdown",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub enum PlaybackEvent {
    StateChanged(PlayerSnapshot),
    Stopped(PlayerSnapshot),
    Failed { message: String },
}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    SetPause(bool),
    SeekMilliseconds(f64),
    SetVolume(f64),
    SetMute(bool),
    SetPlaybackRate(f64),
    SetAudioDelay(f64),
    SetSubtitleDelay(f64),
    SetSubtitleScale(f64),
    SetVideoFit(VideoFit),
    SetVideoAspect(VideoAspect),
    SetDeinterlace(bool),
    SetToneMapping(ToneMapping),
    SetAudioTrack(i64),
    SetSubtitleTrack(Option<i64>),
    AddSubtitle(String),
    ToggleSubtitleVisibility,
    ToggleFullscreen,
    MarkWatchedAndPlayNext,
    Stop,
}

fn ticks_summary(ticks: Option<i64>) -> String {
    ticks
        .map(|value| format!("{value} ({:.3}s)", ticks_to_seconds(value)))
        .unwrap_or_else(|| "unknown".to_string())
}

fn redact_url_query_value(url: &str, keys: &[&str]) -> String {
    let Some((before_query, rest)) = url.split_once('?') else {
        return url.to_string();
    };
    let (query, fragment) = rest
        .split_once('#')
        .map(|(query, fragment)| (query, Some(fragment)))
        .unwrap_or((rest, None));
    let redacted = query
        .split('&')
        .map(|pair| {
            let Some((raw_key, _)) = pair.split_once('=') else {
                return pair.to_string();
            };
            let decoded_key = percent_decode(raw_key);
            if keys.iter().any(|key| decoded_key.eq_ignore_ascii_case(key)) {
                format!("{raw_key}=REDACTED")
            } else {
                pair.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    match fragment {
        Some(fragment) => format!("{before_query}?{redacted}#{fragment}"),
        None => format!("{before_query}?{redacted}"),
    }
}

#[cfg(test)]
mod tests {
    use super::StopReason;

    #[test]
    fn stop_reasons_the_ui_keys_on_keep_their_wire_names() {
        for (reason, wire) in [
            (StopReason::Eof, "eof"),
            (StopReason::WatchedNext, "watched-next"),
        ] {
            assert_eq!(serde_json::to_value(reason).expect("serialize"), wire);
        }
    }
}
