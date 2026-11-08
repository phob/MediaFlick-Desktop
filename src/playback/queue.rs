//! The audio play queue: a fixed order with a current entry. Pure data; the
//! coordinator's actor owns the one live queue and does all I/O.

use std::collections::VecDeque;

use super::model::{
    AudioDisplay, AudioKind, AudioNowPlaying, AudioPhase, PlaylistItemId, QueueReport,
    QueueReportEntry,
};

/// The most entries one queue may hold. A longer list is rejected where it
/// is resolved rather than silently truncated.
pub const MAX_QUEUE_LENGTH: usize = 10_000;

/// An audio item ready to queue, built from Jellyfin metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTrack {
    pub kind: AudioKind,
    pub display: AudioDisplay,
}

impl AudioTrack {
    pub fn item_id(&self) -> &str {
        &self.display.item_id
    }
}

#[derive(Debug, Clone)]
pub(super) struct QueueEntry {
    pub(super) playlist_item_id: PlaylistItemId,
    pub(super) track: AudioTrack,
}

/// Past entries, the current one and those still to come. The current entry
/// always exists, so no index can point past the end.
#[derive(Debug, Clone)]
pub(super) struct AudioQueue {
    past: Vec<QueueEntry>,
    current: QueueEntry,
    upcoming: VecDeque<QueueEntry>,
}

impl AudioQueue {
    /// `None` for an empty list or a start outside it. Entries before
    /// `start` stay as past entries, so Previous can reach them.
    pub(super) fn new(queue_id: u64, tracks: Vec<AudioTrack>, start: usize) -> Option<Self> {
        if start >= tracks.len() || tracks.len() > MAX_QUEUE_LENGTH {
            return None;
        }
        let mut entries = tracks
            .into_iter()
            .enumerate()
            .map(|(ordinal, track)| QueueEntry {
                playlist_item_id: PlaylistItemId::new(queue_id, ordinal),
                track,
            });
        let past = entries.by_ref().take(start).collect();
        let current = entries.next()?;
        Some(Self {
            past,
            current,
            upcoming: entries.collect(),
        })
    }

    pub(super) fn current(&self) -> &QueueEntry {
        &self.current
    }

    pub(super) fn advance(&mut self) -> bool {
        let Some(next) = self.upcoming.pop_front() else {
            return false;
        };
        self.past.push(std::mem::replace(&mut self.current, next));
        true
    }

    pub(super) fn retreat(&mut self) -> bool {
        let Some(previous) = self.past.pop() else {
            return false;
        };
        self.upcoming
            .push_front(std::mem::replace(&mut self.current, previous));
        true
    }

    pub(super) fn has_previous(&self) -> bool {
        !self.past.is_empty()
    }

    pub(super) fn report(&self) -> QueueReport {
        QueueReport {
            entries: self
                .past
                .iter()
                .chain(std::iter::once(&self.current))
                .chain(&self.upcoming)
                .map(|entry| QueueReportEntry {
                    item_id: entry.track.item_id().to_string(),
                    playlist_item_id: entry.playlist_item_id.clone(),
                })
                .collect(),
            current: self.current.playlist_item_id.clone(),
        }
    }

    pub(super) fn view(&self, phase: AudioPhase) -> AudioNowPlaying {
        AudioNowPlaying {
            media_kind: self.current.track.kind,
            current: self.current.track.display.clone(),
            next: self
                .upcoming
                .front()
                .map(|entry| entry.track.display.clone()),
            playlist_item_id: self.current.playlist_item_id.clone(),
            index: self.past.len(),
            length: self.past.len() + 1 + self.upcoming.len(),
            can_previous: self.has_previous(),
            can_next: !self.upcoming.is_empty(),
            phase,
        }
    }
}
