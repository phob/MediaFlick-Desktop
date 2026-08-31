//! Backend-neutral playback values shared by the application and player adapters.

use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::urls::percent_decode;

pub const TICKS_PER_SECOND: f64 = 10_000_000.0;

static PLAYBACK_COUNTER: AtomicI64 = AtomicI64::new(1);

pub(crate) fn allocate_playback_id() -> i64 {
    PLAYBACK_COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub fn seconds_to_ticks(seconds: f64) -> Option<i64> {
    seconds
        .is_finite()
        .then(|| (seconds.max(0.0) * TICKS_PER_SECOND).round() as i64)
}

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
            .map(|ticks| ticks as f64 / 10_000_000.0)
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
        redact_url_query_value(
            &self.media_url,
            &[
                "api_key",
                "apikey",
                "access_token",
                "accesstoken",
                "x-emby-token",
                "x-mediabrowser-token",
            ],
        )
    }

    pub fn merge_missing_from(&mut self, other: &Self) {
        if self.media_url.trim().is_empty() {
            self.media_url.clone_from(&other.media_url);
        }
        if self.headers.is_empty() {
            self.headers.clone_from(&other.headers);
        }
        merge_option(&mut self.item_id, &other.item_id);
        merge_option(&mut self.media_source_id, &other.media_source_id);
        merge_option(&mut self.play_session_id, &other.play_session_id);
        merge_option(&mut self.device_id, &other.device_id);
        merge_option(&mut self.start_time_ticks, &other.start_time_ticks);
        merge_option(&mut self.start_milliseconds, &other.start_milliseconds);
        merge_option(&mut self.runtime_ticks, &other.runtime_ticks);
        merge_option(&mut self.title, &other.title);
        merge_option(&mut self.audio_stream_index, &other.audio_stream_index);
        merge_option(
            &mut self.subtitle_stream_index,
            &other.subtitle_stream_index,
        );
        merge_option(&mut self.audio_mpv_id, &other.audio_mpv_id);
        merge_option(&mut self.subtitle_mpv_id, &other.subtitle_mpv_id);
        merge_option(&mut self.subtitle_url, &other.subtitle_url);
        merge_option(&mut self.play_method, &other.play_method);
        merge_option(&mut self.playlist_item_id, &other.playlist_item_id);
        merge_option(&mut self.queue, &other.queue);
        merge_option(&mut self.details, &other.details);
    }
}

/// Metadata received separately from a playback request by the web bridge.
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
    pub audio_mpv_id: Option<i64>,
    pub subtitle_mpv_id: Option<i64>,
    pub subtitle_url: Option<String>,
    pub play_method: Option<String>,
    pub playlist_item_id: Option<String>,
    pub queue: Option<Value>,
    pub details: Option<Value>,
}

impl PlaybackContext {
    pub fn merge_into_request(&self, request: &mut PlaybackRequest) {
        let context_request = PlaybackRequest {
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
            audio_mpv_id: self.audio_mpv_id,
            subtitle_mpv_id: self.subtitle_mpv_id,
            subtitle_url: self.subtitle_url.clone(),
            play_method: self.play_method.clone(),
            playlist_item_id: self.playlist_item_id.clone(),
            queue: self.queue.clone(),
            details: self.details.clone(),
            ..Default::default()
        };
        request.merge_missing_from(&context_request);
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

#[derive(Debug, Clone, Default)]
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
    pub stop_reason: Option<&'static str>,
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
        .map(|value| format!("{value} ({:.3}s)", value as f64 / 10_000_000.0))
        .unwrap_or_else(|| "unknown".to_string())
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn merge_option<T: Clone>(target: &mut Option<T>, source: &Option<T>) {
    if target.is_none() {
        target.clone_from(source);
    }
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
