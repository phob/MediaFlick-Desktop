use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::playback::PlaybackRequest;
use crate::players::mpv::runtime::MpvRuntimeKind;
use crate::preferences::{LibmpvProfile, SegmentSkipConfig};

use super::{ControllerState, PendingPlayback, PlaybackIdentity, RuntimeSelection};

pub(super) fn controller_with_pending_load(start_time_ticks: Option<i64>) -> ControllerState {
    let (tx, rx) = mpsc::channel();
    let mut launch = PlaybackRequest::new("https://example.test/video.mkv?ApiKey=secret");
    launch.start_time_ticks = start_time_ticks;

    let mut state = ControllerState::new(
        tx,
        rx,
        Arc::new(Mutex::new(Default::default())),
        None,
        Arc::new(AtomicBool::new(false)),
        SegmentSkipConfig::default(),
        RuntimeSelection {
            kind: MpvRuntimeKind::External,
            libmpv_profile: LibmpvProfile::Standard,
        },
    );
    state.pending = Some(PendingPlayback {
        key: "test-load".to_string(),
        identity: PlaybackIdentity::from_launch(1, &launch),
        launch,
        reporter: None,
        requested_at: Instant::now(),
    });
    state
}

pub(super) fn snapshot_active(state: &ControllerState) -> bool {
    state
        .snapshot
        .lock()
        .map(|snapshot| snapshot.active)
        .unwrap_or(false)
}
