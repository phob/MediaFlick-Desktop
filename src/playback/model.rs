use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};

use serde::{Deserialize, Serialize};

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

/// The first non-empty access token `url`'s query carries under one of
/// [`TOKEN_QUERY_KEYS`].
pub fn query_auth_token(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next().unwrap_or_default();
    TOKEN_QUERY_KEYS.iter().find_map(|key| {
        query.split('&').find_map(|pair| {
            let (raw_key, raw_value) = pair.split_once('=')?;
            percent_decode(raw_key)
                .eq_ignore_ascii_case(key)
                .then(|| percent_decode(raw_value))
                .filter(|value| !value.trim().is_empty())
        })
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

/// What a load presents. Only the coordinator's audio queue builds `Audio`;
/// every other load is video, which keeps today's window and fullscreen
/// policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaybackContent {
    #[default]
    Video,
    Audio(AudioKind),
}

impl PlaybackContent {
    pub fn is_audio(self) -> bool {
        matches!(self, Self::Audio(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
pub enum AudioKind {
    Music,
    Audiobook,
}

/// One queue occurrence, so the same track queued twice reports two entries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, ts_rs::TS)]
#[ts(type = "string")]
pub struct PlaylistItemId(String);

impl PlaylistItemId {
    pub(crate) fn new(queue: u64, ordinal: usize) -> Self {
        Self(format!("q{queue}-{ordinal}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Jellyfin's NowPlayingQueue: every entry in play order and the one playing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueReport {
    pub entries: Vec<QueueReportEntry>,
    pub current: PlaylistItemId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueReportEntry {
    pub item_id: String,
    pub playlist_item_id: PlaylistItemId,
}

/// A request to open media in the selected player. Backend-specific track
/// translation happens in each player adapter.
#[derive(Debug, Clone, Default)]
pub struct PlaybackRequest {
    /// Assigned by the coordinator for each dispatch and echoed as the
    /// snapshot's `playback_id`, so its events can be told from a replaced
    /// load's.
    pub load_id: i64,
    pub content: PlaybackContent,
    pub media_url: String,
    pub headers: Vec<HttpHeader>,
    pub item_id: Option<String>,
    pub media_source_id: Option<String>,
    pub play_session_id: Option<String>,
    pub device_id: Option<String>,
    pub start_time_ticks: Option<i64>,
    pub start_milliseconds: Option<f64>,
    pub runtime_ticks: Option<i64>,
    pub title: Option<String>,
    pub audio_stream_index: Option<i64>,
    pub subtitle_stream_index: Option<i64>,
    // One-based embedded track ordinals, translated by each adapter.
    pub audio_mpv_id: Option<i64>,
    pub subtitle_mpv_id: Option<i64>,
    pub subtitle_url: Option<String>,
    pub play_method: Option<String>,
    pub queue: Option<QueueReport>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum PlayerTrackKind {
    Audio,
    Subtitle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
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

#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
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

#[derive(Debug, Clone, Copy, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackDiagnostics {
    pub buffered_until_ms: Option<f64>,
    pub buffering: bool,
    pub dropped_frames: Option<i64>,
    pub frame_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkRef {
    pub item_id: String,
    pub tag: String,
}

/// What the player bar and the queue show for one audio item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDisplay {
    pub item_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub artwork: Option<ArtworkRef>,
    pub disc_number: Option<i64>,
    pub track_number: Option<i64>,
    pub runtime_ticks: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum AudioPhase {
    /// Negotiating the entry with Jellyfin.
    Preparing,
    /// Handed to the player, which has not reported the file loaded yet.
    Loading,
    Playing,
}

/// The audio queue as the UI sees it: the current and next entry, never the
/// whole queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioNowPlaying {
    pub media_kind: AudioKind,
    pub current: AudioDisplay,
    pub next: Option<AudioDisplay>,
    pub playlist_item_id: PlaylistItemId,
    pub index: usize,
    pub length: usize,
    pub can_previous: bool,
    pub can_next: bool,
    pub phase: AudioPhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NowPlaying {
    Video,
    Audio(Box<AudioNowPlaying>),
}

#[derive(Debug, Clone, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    /// Set by the coordinator from what it asked to play, never from the
    /// player's tracks: an MP3's cover art is a video track.
    pub now_playing: Option<NowPlaying>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
pub enum StopReason {
    Eof,
    WatchedNext,
    Stop,
    Quit,
    Error,
    Redirect,
    Shutdown,
    Unknown,
}

impl StopReason {
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

/// What a player adapter reports. Only the coordinator receives these.
#[derive(Debug, Clone)]
pub enum BackendEvent {
    StateChanged(PlayerSnapshot),
    /// The player loaded the file of this load and its playstate started.
    Started {
        playback_id: i64,
    },
    Stopped(PlayerSnapshot),
    /// The player refused this load.
    Failed {
        load_id: i64,
        message: String,
    },
    /// The player's own Next or Previous key, for the coordinator's queue.
    TrackRequested {
        next: bool,
    },
}

/// What the coordinator tells the shell.
#[derive(Debug, Clone)]
pub enum PlaybackEvent {
    StateChanged(PlayerSnapshot),
    /// One queue entry ended and the queue moves on: refresh the item's
    /// cached state, but playback continues.
    ItemFinished(PlayerSnapshot),
    Stopped(PlayerSnapshot),
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    SetPause(bool),
    /// Flips pause against the player's own state, so two quick presses never
    /// both pause on a stale view of it.
    TogglePause,
    SeekMilliseconds(f64),
    SetVolume(f64),
    SetMute(bool),
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
    NextTrack,
    PreviousTrack,
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
