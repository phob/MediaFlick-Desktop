use std::sync::{Arc, Weak};

use crate::app::services::Services;
use crate::playback::{
    ArtworkRef, AudioDisplay, AudioKind, AudioPlan, AudioSource, AudioStart, AudioTrack,
    MAX_QUEUE_LENGTH, PreparedAudio,
};

use super::super::api::items::{self, PlaybackInfoRequest};
use super::super::api::model::BaseItemDto;
use super::super::api::{ApiError, JellyfinClient};
use super::super::session::SessionScope;
use super::{PlayOptions, StartError, build, resume_start_ticks};

pub struct QueueAsk {
    /// An album, an audiobook or one track.
    pub parent_id: String,
    /// Where in the parent's tracks to start; the first track without one.
    pub start_item_id: Option<String>,
    pub shuffle: bool,
    pub start: AudioStart,
}

pub fn audio_kind(item_type: Option<&str>) -> Option<AudioKind> {
    match item_type? {
        "Audio" => Some(AudioKind::Music),
        "AudioBook" => Some(AudioKind::Audiobook),
        _ => None,
    }
}

fn audio_track(item: &BaseItemDto) -> Option<AudioTrack> {
    let kind = audio_kind(item.item_type.as_deref())?;
    let artwork = item
        .primary_image_tag()
        .map(|tag| ArtworkRef {
            item_id: item.id.clone(),
            tag: tag.to_string(),
        })
        .or_else(|| {
            Some(ArtworkRef {
                item_id: item.album_id.clone()?,
                tag: item.album_primary_image_tag.clone()?,
            })
        });
    let artists = if item.artists.is_empty() {
        item.album_artist.iter().cloned().collect()
    } else {
        item.artists.clone()
    };
    Some(AudioTrack {
        kind,
        display: AudioDisplay {
            item_id: item.id.clone(),
            title: item.display_name().to_string(),
            artists,
            album: item.album.clone(),
            album_id: item.album_id.clone(),
            artwork,
            disc_number: item.parent_index_number,
            track_number: item.index_number,
            runtime_ticks: item.run_time_ticks.filter(|ticks| *ticks > 0),
        },
    })
}

/// The tracks `item` plays, in the order its page lists them: an audio item
/// alone, or a folder's audio by disc, track, sort name and id. A missing
/// disc counts as disc 1; a missing track number sorts after the numbered
/// tracks of its disc. At most one more than [`MAX_QUEUE_LENGTH`], so a
/// caller can tell a list that is too long.
pub fn audio_tracks(
    client: &JellyfinClient,
    user_id: &str,
    item: &BaseItemDto,
) -> Result<Vec<AudioTrack>, ApiError> {
    if let Some(track) = audio_track(item) {
        return Ok(vec![track]);
    }
    if !item.is_folder {
        return Ok(Vec::new());
    }
    let mut children =
        items::fetch_audio_tracks(client, user_id, &item.id, MAX_QUEUE_LENGTH + 1)?.items;
    children.sort_by_cached_key(|child| {
        (
            child.parent_index_number.unwrap_or(1),
            child.index_number.is_none(),
            child.index_number,
            child
                .sort_name
                .as_deref()
                .unwrap_or_else(|| child.display_name())
                .to_lowercase(),
            child.id.clone(),
        )
    });
    Ok(children.iter().filter_map(audio_track).collect())
}

/// Resolves `ask` and hands the queue to the coordinator, replacing whatever
/// plays. Returns the first entry's item id; the entry is negotiated after
/// this returns, and a failure then reaches the user as a playback error.
pub fn start_queue(
    services: &Arc<Services>,
    scope: &SessionScope,
    ask: &QueueAsk,
) -> Result<String, StartError> {
    let settings = services.preferences.snapshot();
    if crate::players::configured_player_path(&settings).is_none() {
        return Err(StartError::NoPlayer);
    }
    let playback = services.playback().ok_or(StartError::NotReady)?;
    let item = services
        .session
        .fetch_current(scope, |client, user_id| {
            items::fetch_item(client, user_id, &ask.parent_id)
        })
        .map_err(opened)?
        .ok_or(StartError::Api(ApiError::Status { status: 404 }))?;
    let mut tracks = services
        .session
        .fetch_current(scope, |client, user_id| {
            audio_tracks(client, user_id, &item)
        })
        .map_err(opened)?;
    if tracks.is_empty() {
        return Err(StartError::Rejected("this item has nothing to listen to"));
    }
    if tracks.len() > MAX_QUEUE_LENGTH {
        return Err(StartError::Rejected(
            "this item has more tracks than one queue can hold",
        ));
    }
    let start_index = match &ask.start_item_id {
        Some(id) => tracks
            .iter()
            .position(|track| track.item_id() == id)
            .ok_or(StartError::Rejected(
                "startItemId is not one of this item's tracks",
            ))?,
        None => 0,
    };
    let start_index = if ask.shuffle {
        let chosen = ask
            .start_item_id
            .is_some()
            .then(|| tracks.remove(start_index));
        shuffle(&mut tracks);
        if let Some(chosen) = chosen {
            tracks.insert(0, chosen);
        }
        0
    } else {
        start_index
    };
    let plan = AudioPlan::new(tracks, start_index, ask.start)
        .ok_or(StartError::Rejected("this item has nothing to listen to"))?;
    let first = plan.first_item_id().to_string();
    if !services.session.scope_is_current(scope) {
        return Err(StartError::AccountChanged);
    }
    tracing::info!(target: "playback", parent = %ask.parent_id, first, shuffle = ask.shuffle, "queued audio");
    playback.play_audio(
        plan,
        Arc::new(JellyfinAudioSource {
            services: Arc::downgrade(services),
            scope: scope.clone(),
        }),
    );
    Ok(first)
}

/// Remote PlayNow: `item_ids` in the sender's order, repeats kept, an album
/// or other folder expanded in place into its tracks, starting at the
/// entry `item_ids[start_index]` begins. `None` when that item is no audio,
/// so the caller plays it as video. A list mixing audio with other media
/// is rejected rather than played in part.
pub fn start_item_queue(
    services: &Arc<Services>,
    scope: &SessionScope,
    item_ids: &[String],
    start_index: usize,
    start: AudioStart,
) -> Result<Option<String>, StartError> {
    if item_ids.len() > MAX_QUEUE_LENGTH {
        return Err(StartError::Rejected(
            "the remote asked for more items than one queue can hold",
        ));
    }
    let Some(start_id) = item_ids.get(start_index) else {
        return Err(StartError::Rejected(
            "StartIndex is past the end of ItemIds",
        ));
    };
    let mut unique: Vec<String> = Vec::new();
    for id in item_ids {
        if !unique.contains(id) {
            unique.push(id.clone());
        }
    }
    let mut fetched = Vec::with_capacity(unique.len());
    for batch in unique.chunks(100) {
        fetched.extend(
            services
                .session
                .fetch_current(scope, |client, user_id| {
                    items::fetch_items(client, user_id, batch)
                })
                .map_err(opened)?
                .items,
        );
    }
    let item = |id: &str| fetched.iter().find(|item| item.id == id);
    let listenable = |item: &BaseItemDto| {
        audio_kind(item.item_type.as_deref()).is_some()
            || matches!(item.item_type.as_deref(), Some("MusicAlbum"))
    };
    if !item(start_id).is_some_and(listenable) {
        return Ok(None);
    }
    let mut tracks = Vec::new();
    let mut start_entry = 0;
    for (index, id) in item_ids.iter().enumerate() {
        let entry = item(id).ok_or(StartError::Api(ApiError::Status { status: 404 }))?;
        if !listenable(entry) {
            return Err(StartError::Rejected(
                "the remote mixed audio with other media in one play",
            ));
        }
        if index == start_index {
            start_entry = tracks.len();
        }
        tracks.extend(
            services
                .session
                .fetch_current(scope, |client, user_id| {
                    audio_tracks(client, user_id, entry)
                })
                .map_err(opened)?,
        );
        if tracks.len() > MAX_QUEUE_LENGTH {
            return Err(StartError::Rejected(
                "the remote asked for more tracks than one queue can hold",
            ));
        }
    }
    let plan = AudioPlan::new(tracks, start_entry, start).ok_or(StartError::Rejected(
        "the remote play has nothing to listen to",
    ))?;
    let first = plan.first_item_id().to_string();
    let playback = services.playback().ok_or(StartError::NotReady)?;
    if crate::players::configured_player_path(&services.preferences.snapshot()).is_none() {
        return Err(StartError::NoPlayer);
    }
    if !services.session.scope_is_current(scope) {
        return Err(StartError::AccountChanged);
    }
    tracing::info!(target: "playback", items = item_ids.len(), start_index, first, "queued audio from a remote client");
    playback.play_audio(
        plan,
        Arc::new(JellyfinAudioSource {
            services: Arc::downgrade(services),
            scope: scope.clone(),
        }),
    );
    Ok(Some(first))
}

fn opened(error: ApiError) -> StartError {
    match error {
        ApiError::AccountChanged => StartError::AccountChanged,
        error => StartError::Api(error),
    }
}

/// Fisher-Yates over a SplitMix64 stream seeded from the OS. A shuffle needs
/// no secrecy, so a failed seed read falls back to the clock.
fn shuffle<T>(items: &mut [T]) {
    let mut seed = [0u8; 8];
    let mut state = match getrandom::fill(&mut seed) {
        Ok(()) => u64::from_le_bytes(seed),
        Err(_) => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9e37_79b9_7f4a_7c15, |elapsed| elapsed.as_nanos() as u64),
    };
    for index in (1..items.len()).rev() {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut mixed = state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^= mixed >> 31;
        let other = (mixed % (index as u64 + 1)) as usize;
        items.swap(index, other);
    }
}

struct JellyfinAudioSource {
    services: Weak<Services>,
    scope: SessionScope,
}

impl JellyfinAudioSource {
    fn prepare_entry(
        &self,
        services: &Services,
        track: &AudioTrack,
        start: AudioStart,
    ) -> Result<PreparedAudio, StartError> {
        let settings = services.preferences.snapshot();
        let player_path =
            crate::players::configured_player_path(&settings).ok_or(StartError::NoPlayer)?;
        let start_ticks = match (track.kind, start) {
            (AudioKind::Music, _) | (AudioKind::Audiobook, AudioStart::Beginning) => 0,
            (AudioKind::Audiobook, AudioStart::At(ticks)) => ticks.max(0),
            (AudioKind::Audiobook, AudioStart::Default) => {
                let position = services
                    .session
                    .fetch_current(&self.scope, |client, user_id| {
                        items::fetch_item(client, user_id, track.item_id())
                    })
                    .map_err(opened)?
                    .and_then(|item| item.user_data)
                    .map_or(0, |data| data.playback_position_ticks);
                let options = PlayOptions {
                    resume: true,
                    viewing: services.accounts.viewing(self.scope.account()),
                    ..PlayOptions::default()
                };
                resume_start_ticks(&options, position)
            }
        };
        let quality = settings.streaming_quality;
        let info = services
            .session
            .fetch_current(&self.scope, |client, user_id| {
                items::playback_info(
                    client,
                    user_id,
                    track.item_id(),
                    quality,
                    &PlaybackInfoRequest {
                        start_time_ticks: Some(start_ticks),
                        ..PlaybackInfoRequest::default()
                    },
                )
            })
            .map_err(opened)?;
        let options = PlayOptions {
            item_id: track.item_id().to_string(),
            ..PlayOptions::default()
        };
        let mut request = build(
            self.scope.client(),
            &info,
            quality,
            &options,
            start_ticks,
            None,
        )
        .map_err(StartError::Api)?
        .request;
        let display = &track.display;
        request.title = Some(if display.artists.is_empty() {
            display.title.clone()
        } else {
            format!("{} · {}", display.title, display.artists.join(", "))
        });
        request.runtime_ticks = request.runtime_ticks.or(display.runtime_ticks);
        tracing::info!(
            target: "playback",
            item_id = track.item_id(),
            start_ticks,
            launch = %crate::app::logger::launch_summary(&request),
            "prepared an audio queue entry"
        );
        Ok(PreparedAudio {
            player_path,
            request,
        })
    }
}

impl AudioSource for JellyfinAudioSource {
    fn prepare(&self, track: &AudioTrack, start: AudioStart) -> Result<PreparedAudio, String> {
        let services = self
            .services
            .upgrade()
            .ok_or_else(|| "MediaFlick is shutting down".to_string())?;
        self.prepare_entry(&services, track, start)
            .map_err(|error| match error {
                StartError::NoPlayer => "no media player is configured".to_string(),
                StartError::NotReady => "the player is not ready".to_string(),
                StartError::AccountChanged => "the Jellyfin account changed".to_string(),
                StartError::Rejected(reason) => reason.to_string(),
                StartError::Api(error) => error.to_string(),
            })
    }

    fn dispatch_if_current(&self, dispatch: &mut dyn FnMut()) -> bool {
        let Some(services) = self.services.upgrade() else {
            return false;
        };
        services
            .session
            .commit_if_current(
                &self.scope,
                || (),
                |_| {
                    dispatch();
                    Ok(())
                },
            )
            .is_ok()
    }

    fn is_current(&self) -> bool {
        self.services
            .upgrade()
            .is_some_and(|services| services.session.scope_is_current(&self.scope))
    }
}
