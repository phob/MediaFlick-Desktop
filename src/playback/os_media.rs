use std::path::PathBuf;
use std::sync::Arc;

use super::PlayerSnapshot;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsPlaybackStatus {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsMediaMetadata {
    pub item_id: String,
    pub is_video: bool,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_us: i64,
    pub artwork_path: Option<PathBuf>,
    pub can_go_next: bool,
    pub can_go_previous: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OsMediaState {
    pub status: OsPlaybackStatus,
    pub position_us: i64,
    pub duration_us: i64,
    pub volume: f64,
    pub can_seek: bool,
    pub metadata: Option<OsMediaMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OsMediaAction {
    Play,
    Pause,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    PlayPause,
    Stop,
    Next,
    Previous,
    #[cfg(target_os = "linux")]
    SeekDeltaUs(i64),
    SetPositionMs(f64),
    #[cfg(target_os = "linux")]
    SetVolume(f64),
    #[cfg(target_os = "linux")]
    Raise,
}

pub fn os_media_state_from_snapshot(
    snapshot: &PlayerSnapshot,
    metadata: Option<&OsMediaMetadata>,
) -> OsMediaState {
    let status = if !snapshot.active {
        OsPlaybackStatus::Stopped
    } else if snapshot.paused {
        OsPlaybackStatus::Paused
    } else {
        OsPlaybackStatus::Playing
    };
    let position_us = if snapshot.position_ms.is_finite() && snapshot.position_ms > 0.0 {
        (snapshot.position_ms * 1000.0).round() as i64
    } else {
        0
    };
    let duration_us = metadata
        .map(|m| m.duration_us.max(0))
        .filter(|d| *d > 0)
        .or_else(|| {
            snapshot
                .duration_ms
                .filter(|d| d.is_finite() && *d > 0.0)
                .map(|d| (d * 1000.0).round() as i64)
        })
        .unwrap_or(0);
    let volume = if snapshot.mute == Some(true) {
        0.0
    } else {
        snapshot
            .volume
            .map_or(1.0, |v| (v as f64 / 100.0).clamp(0.0, 1.0))
    };
    OsMediaState {
        status,
        position_us: position_us.min(if duration_us > 0 {
            duration_us
        } else {
            position_us
        }),
        duration_us,
        volume,
        can_seek: snapshot.active && duration_us > 0,
        metadata: metadata.cloned(),
    }
}

#[cfg(any(target_os = "linux", test))]
pub fn clamp_seek_target_ms(current_ms: f64, delta_us: i64, duration_ms: Option<f64>) -> f64 {
    let base = if current_ms.is_finite() {
        current_ms.max(0.0)
    } else {
        0.0
    };
    let target = (base + (delta_us as f64) / 1000.0).max(0.0);
    match duration_ms {
        Some(dur) if dur.is_finite() && dur > 0.0 => target.min(dur),
        _ => target,
    }
}

pub struct OsMediaController {
    #[cfg(target_os = "linux")]
    inner: linux::LinuxOsMedia,
    #[cfg(target_os = "windows")]
    inner: windows::WindowsOsMedia,
    #[cfg(target_os = "macos")]
    inner: macos::MacosOsMedia,
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    _handler: Arc<dyn Fn(OsMediaAction) + Send + Sync>,
}

impl OsMediaController {
    pub fn start(handler: Arc<dyn Fn(OsMediaAction) + Send + Sync>) -> Self {
        Self {
            #[cfg(target_os = "linux")]
            inner: linux::LinuxOsMedia::start(handler),
            #[cfg(target_os = "windows")]
            inner: windows::WindowsOsMedia::start(handler),
            #[cfg(target_os = "macos")]
            inner: macos::MacosOsMedia::start(handler),
            #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
            _handler: handler,
        }
    }

    pub fn update(&self, state: &OsMediaState) {
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        self.inner.update(state);
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        let _ = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_media_state_maps_status_units_and_clamps_volume() {
        let meta = OsMediaMetadata {
            item_id: "movie-1".into(),
            is_video: true,
            title: "Spirited Away".into(),
            artist: Some("Miyazaki".into()),
            album: None,
            duration_us: 120_000_000,
            artwork_path: None,
            can_go_next: false,
            can_go_previous: false,
        };
        let playing = PlayerSnapshot {
            active: true,
            paused: false,
            position_ms: 42_500.0,
            duration_ms: Some(120_000.0),
            volume: Some(135),
            mute: Some(false),
            ..PlayerSnapshot::default()
        };
        let state = os_media_state_from_snapshot(&playing, Some(&meta));
        assert_eq!(state.status, OsPlaybackStatus::Playing);
        assert_eq!(state.position_us, 42_500_000);
        assert_eq!(state.duration_us, 120_000_000);
        assert!((state.volume - 1.0).abs() < 1e-9);
        assert!(state.can_seek);
        assert_eq!(
            state.metadata.as_ref().map(|m| m.title.as_str()),
            Some("Spirited Away")
        );

        let muted = PlayerSnapshot {
            paused: true,
            mute: Some(true),
            position_ms: -50.0,
            ..playing
        };
        let paused_state = os_media_state_from_snapshot(&muted, Some(&meta));
        assert_eq!(paused_state.status, OsPlaybackStatus::Paused);
        assert_eq!(paused_state.position_us, 0);
        assert!((paused_state.volume - 0.0).abs() < 1e-9);

        let stopped = os_media_state_from_snapshot(&PlayerSnapshot::default(), None);
        assert_eq!(stopped.status, OsPlaybackStatus::Stopped);
        assert!(!stopped.can_seek);
    }

    #[test]
    fn clamp_seek_target_bounds_negative_and_past_duration() {
        let _actions = [
            OsMediaAction::Play,
            OsMediaAction::Pause,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            OsMediaAction::PlayPause,
            OsMediaAction::Stop,
            OsMediaAction::Next,
            OsMediaAction::Previous,
            #[cfg(target_os = "linux")]
            OsMediaAction::SeekDeltaUs(0),
            OsMediaAction::SetPositionMs(0.0),
            #[cfg(target_os = "linux")]
            OsMediaAction::SetVolume(0.5),
            #[cfg(target_os = "linux")]
            OsMediaAction::Raise,
        ];
        assert!((clamp_seek_target_ms(5_000.0, -10_000_000, Some(60_000.0)) - 0.0).abs() < 1e-6);
        assert!(
            (clamp_seek_target_ms(55_000.0, 10_000_000, Some(60_000.0)) - 60_000.0).abs() < 1e-6
        );
        assert!(
            (clamp_seek_target_ms(12_000.0, 2_500_000, Some(60_000.0)) - 14_500.0).abs() < 1e-6
        );
    }
}
