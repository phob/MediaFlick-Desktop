//! External media-player adapters.
//!
//! Player implementations translate backend-specific protocols into the
//! backend-neutral contracts owned by the playback domain.

#[cfg(windows)]
pub mod mpchc;
pub mod mpv;

use std::sync::mpsc::Sender;

use crate::playback::{PlaybackEvent, PlayerBackend};
use crate::players::mpv::MpvController;
use crate::preferences::{AppSettings, PlayerBackend as PlayerBackendKind};
#[cfg(windows)]
use mpchc::MpcHcController;

/// Build the configured player adapter at the application boundary.
pub fn build_backend(
    settings: &AppSettings,
    event_tx: Sender<PlaybackEvent>,
) -> Box<dyn PlayerBackend> {
    match settings.effective_backend() {
        PlayerBackendKind::Mpv => Box::new(MpvController::new(
            Some(event_tx),
            settings.segment_skip_config(),
        )),
        PlayerBackendKind::Mpchc => {
            #[cfg(windows)]
            {
                Box::new(MpcHcController::new(
                    Some(event_tx),
                    settings.segment_skip_config(),
                ))
            }
            #[cfg(not(windows))]
            {
                Box::new(MpvController::new(
                    Some(event_tx),
                    settings.segment_skip_config(),
                ))
            }
        }
    }
}
