use std::collections::VecDeque;

use super::model::{
    AudioDisplay, AudioKind, AudioNowPlaying, AudioPhase, PlaylistItemId, QueueReport,
    QueueReportEntry,
};

/// The most entries one queue may hold. A longer list is rejected where it
/// is resolved rather than silently truncated.
pub const MAX_QUEUE_LENGTH: usize = 10_000;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::model::AudioDisplay;

    const QUEUE_ID: u64 = 7;

    /// Four distinct items, so longer queues hold the same item twice.
    fn track(ordinal: usize) -> AudioTrack {
        let item = ordinal % 4;
        AudioTrack {
            kind: AudioKind::Music,
            display: AudioDisplay {
                item_id: format!("item-{item}"),
                title: format!("Track {item}"),
                artists: Vec::new(),
                album: None,
                album_id: None,
                artwork: None,
                disc_number: None,
                track_number: None,
                runtime_ticks: None,
            },
        }
    }

    /// The music changelog and the now-playing bar's contract: "n of m",
    /// Up next, Previous and Next enabled only where they lead somewhere, and
    /// Jellyfin sees the whole queue in order with the entry that plays.
    fn assert_queue_at(queue: &AudioQueue, tracks: &[AudioTrack], position: usize) {
        let view = queue.view(AudioPhase::Playing);
        let report = queue.report();
        assert_eq!(view.current, tracks[position].display);
        assert_eq!(
            view.next.as_ref(),
            tracks.get(position + 1).map(|track| &track.display)
        );
        assert_eq!((view.index, view.length), (position, tracks.len()));
        assert_eq!(view.can_previous, position > 0);
        assert_eq!(view.can_next, position + 1 < tracks.len());

        let reported: Vec<&str> = report
            .entries
            .iter()
            .map(|entry| entry.item_id.as_str())
            .collect();
        let queued: Vec<&str> = tracks
            .iter()
            .map(|track| track.display.item_id.as_str())
            .collect();
        assert_eq!(reported, queued);
        let mut ids: Vec<&str> = report
            .entries
            .iter()
            .map(|entry| entry.playlist_item_id.as_str())
            .collect();
        assert_eq!(report.current, report.entries[position].playlist_item_id);
        assert_eq!(view.playlist_item_id, report.current);
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), tracks.len(), "every queue entry has its own id");
    }

    /// Checked against a plain position in the track list, for every length
    /// up to 12, every start and generated Next/Previous sequences.
    #[test]
    fn next_and_previous_follow_a_plain_position_in_the_list() {
        let mut seed: u64 = 0x5eed;
        for length in 1..=12 {
            let tracks: Vec<AudioTrack> = (0..length).map(track).collect();
            for start in 0..length {
                for _ in 0..20 {
                    let mut queue = AudioQueue::new(QUEUE_ID, tracks.clone(), start)
                        .expect("start inside the list");
                    let mut position = start;
                    assert_queue_at(&queue, &tracks, position);
                    for _ in 0..3 * length {
                        seed = seed
                            .wrapping_mul(6_364_136_223_846_793_005)
                            .wrapping_add(1_442_695_040_888_963_407);
                        if seed >> 63 == 0 {
                            let can_move = position + 1 < length;
                            assert_eq!(queue.advance(), can_move, "Next at {position} of {length}");
                            position += usize::from(can_move);
                        } else {
                            let can_move = position > 0;
                            assert_eq!(
                                queue.retreat(),
                                can_move,
                                "Previous at {position} of {length}"
                            );
                            position -= usize::from(can_move);
                        }
                        assert_eq!(queue.has_previous(), position > 0);
                        assert_queue_at(&queue, &tracks, position);
                    }
                }
            }
        }
    }
}
