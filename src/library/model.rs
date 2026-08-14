//! Row shapes for the metadata cache and their translation from Jellyfin DTOs.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::jellyfin::api::model::{BaseItemDto, MediaSourceInfo, MediaStream, UserItemDataDto};

/// A durable description of one selected track.
///
/// The absolute Jellyfin stream index is what this item needs today. The
/// descriptive fields are intentionally stored with it so a later series-wide
/// preference can match equivalent tracks by language and characteristics
/// without trying to recover intent from an opaque index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TrackPreference {
    pub index: i64,
    pub language: Option<String>,
    pub title: Option<String>,
    pub display_title: Option<String>,
    pub codec: Option<String>,
    pub channels: Option<i64>,
    pub is_default: bool,
    pub is_forced: bool,
    pub is_hearing_impaired: bool,
    pub is_external: bool,
}

impl TrackPreference {
    pub fn from_stream(stream: &MediaStream) -> Self {
        Self {
            index: stream.index,
            language: normalized_owned(stream.language.as_deref()),
            title: normalized_owned(stream.title.as_deref()),
            display_title: normalized_owned(stream.display_title.as_deref()),
            codec: normalized_owned(stream.codec.as_deref()),
            channels: stream.channels.filter(|channels| *channels > 0),
            is_default: stream.is_default,
            is_forced: stream.is_forced,
            is_hearing_impaired: stream.is_hearing_impaired,
            is_external: stream.is_external,
        }
    }

    /// An index can be reused for a different stream after a file replacement.
    /// Compare the descriptors that were actually available when this choice
    /// was saved before trusting it. Newly enriched metadata does not make an
    /// otherwise matching old snapshot stale.
    fn matches(&self, stream: &MediaStream) -> bool {
        let display_title_matches = if self.language.is_none()
            && self.title.is_none()
            && self.codec.is_none()
            && self.channels.is_none()
        {
            optional_text_matches(
                self.display_title.as_deref(),
                stream.display_title.as_deref(),
            )
        } else {
            // DisplayTitle is generated/localized by Jellyfin and can change
            // when a Default/External badge changes. Treat it as identity only
            // when no stronger descriptor was available in the snapshot.
            true
        };
        self.index == stream.index
            && optional_text_matches(self.language.as_deref(), stream.language.as_deref())
            && optional_text_matches(self.title.as_deref(), stream.title.as_deref())
            && display_title_matches
            && optional_text_matches(self.codec.as_deref(), stream.codec.as_deref())
            && self
                .channels
                .is_none_or(|channels| stream.channels == Some(channels))
            && self.is_forced == stream.is_forced
            && self.is_hearing_impaired == stream.is_hearing_impaired
            && self.is_external == stream.is_external
    }
}

/// The media source identity saved with the selected tracks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaSourcePreference {
    pub id: Option<String>,
    pub index: usize,
    pub name: String,
    pub container: Option<String>,
}

/// Per-item playback selection persisted in the library database.
///
/// A missing `subtitle_track` in an existing row means subtitles explicitly
/// off. A missing row means the server/default selection has never been
/// overridden, which is why this is separate from [`ResolvedPlaybackPreference`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ItemPlaybackPreference {
    pub media_source: MediaSourcePreference,
    pub audio_track: Option<TrackPreference>,
    pub subtitle_track: Option<TrackPreference>,
}

impl ItemPlaybackPreference {
    pub fn capture(
        source: &MediaSourceInfo,
        source_index: usize,
        audio_track: Option<&MediaStream>,
        subtitle_track: Option<&MediaStream>,
    ) -> Self {
        Self {
            media_source: MediaSourcePreference {
                id: normalized_owned(source.id.as_deref()),
                index: source_index,
                name: source.display_name().to_string(),
                container: normalized_owned(source.container.as_deref()),
            },
            audio_track: audio_track.map(TrackPreference::from_stream),
            subtitle_track: subtitle_track.map(TrackPreference::from_stream),
        }
    }
}

/// A stored preference resolved against the media sources currently reported
/// by Jellyfin. This is the small index mapping the UI and play negotiation use.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPlaybackPreference {
    pub media_source_id: Option<String>,
    pub media_source_index: usize,
    pub audio_stream_index: Option<i64>,
    /// `None` is subtitles off. Playback translates it to Jellyfin/mpv's off
    /// sentinel only when an existing stored row made that choice explicit.
    pub subtitle_stream_index: Option<i64>,
}

/// Resolves an optional stored selection to safe, current source/track indices.
/// Missing or changed source/track identities fall back to the same server
/// defaults used by playback instead of applying a now-unrelated stream index.
pub fn resolve_playback_preference(
    preference: Option<&ItemPlaybackPreference>,
    sources: &[MediaSourceInfo],
) -> Option<ResolvedPlaybackPreference> {
    let default_source_index = sources
        .iter()
        .position(|source| source.supports_direct_play || source.supports_direct_stream)
        .or_else(|| {
            sources
                .iter()
                .position(|source| source.transcoding_url.is_some())
        })
        .unwrap_or(0);

    let (source_index, saved_source_matches) = preference
        .and_then(|preference| {
            matching_source_index(&preference.media_source, sources).map(|index| (index, true))
        })
        .unwrap_or((default_source_index, false));
    let source = sources.get(source_index)?;

    let saved_audio = saved_source_matches
        .then(|| preference.and_then(|preference| preference.audio_track.as_ref()))
        .flatten()
        .and_then(|saved| matching_stream(source, "Audio", saved));
    let audio = saved_audio.or_else(|| default_audio(source));

    let subtitle = if saved_source_matches {
        match preference.and_then(|preference| preference.subtitle_track.as_ref()) {
            // The row explicitly records subtitles off.
            None => None,
            Some(saved) => {
                matching_stream(source, "Subtitle", saved).or_else(|| default_subtitle(source))
            }
        }
    } else {
        default_subtitle(source)
    };

    Some(ResolvedPlaybackPreference {
        media_source_id: normalized_owned(source.id.as_deref()),
        media_source_index: source_index,
        audio_stream_index: audio.map(|stream| stream.index),
        subtitle_stream_index: subtitle.map(|stream| stream.index),
    })
}

fn matching_source_index(
    saved: &MediaSourcePreference,
    sources: &[MediaSourceInfo],
) -> Option<usize> {
    if let Some(id) = saved.id.as_deref() {
        return sources
            .iter()
            .position(|source| source.id.as_deref() == Some(id));
    }
    let source = sources.get(saved.index)?;
    (source.display_name() == saved.name
        && optional_text_matches(saved.container.as_deref(), source.container.as_deref()))
    .then_some(saved.index)
}

fn matching_stream<'a>(
    source: &'a MediaSourceInfo,
    kind: &str,
    saved: &TrackPreference,
) -> Option<&'a MediaStream> {
    source
        .streams_of_type(kind)
        .find(|stream| saved.matches(stream))
}

fn default_audio(source: &MediaSourceInfo) -> Option<&MediaStream> {
    source
        .default_audio_stream_index
        .filter(|index| *index >= 0)
        .and_then(|index| {
            source
                .streams_of_type("Audio")
                .find(|stream| stream.index == index)
        })
        .or_else(|| {
            source
                .streams_of_type("Audio")
                .find(|stream| stream.is_default)
        })
        .or_else(|| source.streams_of_type("Audio").next())
}

fn default_subtitle(source: &MediaSourceInfo) -> Option<&MediaStream> {
    source
        .default_subtitle_stream_index
        .filter(|index| *index >= 0)
        .and_then(|index| {
            source
                .streams_of_type("Subtitle")
                .find(|stream| stream.index == index)
        })
}

fn normalized_owned(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn optional_text_matches(saved: Option<&str>, current: Option<&str>) -> bool {
    let Some(saved) = saved.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    current
        .map(str::trim)
        .is_some_and(|current| saved.eq_ignore_ascii_case(current))
}

/// One cached library item — the thin catalog index row. Provider IDs get
/// dedicated columns because they are the join keys for external metadata
/// features. Rich metadata (synopsis, cast, streams, tags, studios, critic
/// ratings) is never cached; surfaces fetch it live from Jellyfin.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemRecord {
    pub jellyfin_id: String,
    pub kind: String,
    pub name: String,
    pub original_title: Option<String>,
    pub sort_name: Option<String>,
    pub year: Option<i64>,
    pub premiere_date: Option<String>,
    pub runtime_ticks: Option<i64>,
    pub community_rating: Option<f64>,
    pub official_rating: Option<String>,
    pub parent_id: Option<String>,
    pub series_id: Option<String>,
    pub series_name: Option<String>,
    pub season_id: Option<String>,
    pub index_number: Option<i64>,
    pub parent_index_number: Option<i64>,
    pub child_count: Option<i64>,
    pub tmdb_id: Option<String>,
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<String>,
    pub genres: String,
    pub image_tags: String,
    pub primary_image_tag: Option<String>,
    pub backdrop_image_tag: Option<String>,
    pub search_genres: String,
    pub date_created: Option<String>,
    pub date_last_saved: Option<String>,
}

/// Whether a DTO's kind belongs in the catalog index at all. Guarding here
/// keeps live single-item fetches (which can resolve people, folders, or other
/// non-library kinds) from writing rows browsing would then surface.
pub(crate) fn is_synced_kind(kind: &str) -> bool {
    matches!(kind, "Movie" | "Series" | "Season" | "Episode")
}

/// Only fields useful to a concise technical readout are retained. Subtitle
/// tracks, paths, delivery URLs, and playback-negotiation flags never reach
/// the card-badge channel; the media endpoint serves those from full sources.
/// Nothing here is persisted — this shapes live DTO streams for the UI.
pub(crate) fn technical_media_streams_json(streams: &[MediaStream]) -> serde_json::Value {
    serde_json::Value::Array(
        streams
            .iter()
            .filter(|stream| {
                stream.stream_type.as_deref().is_some_and(|kind| {
                    kind.eq_ignore_ascii_case("video") || kind.eq_ignore_ascii_case("audio")
                })
            })
            .map(|stream| {
                json!({
                    "index": stream.index,
                    "type": stream.stream_type,
                    "codec": stream.codec,
                    "profile": stream.profile,
                    "language": stream.language,
                    "title": stream.title,
                    "displayTitle": stream.display_title,
                    "width": stream.width,
                    "height": stream.height,
                    "channels": stream.channels,
                    "videoRange": stream.video_range,
                    "videoRangeType": stream.video_range_type,
                    "audioSpatialFormat": stream.audio_spatial_format,
                    "bitDepth": stream.bit_depth,
                    "isDefault": stream.is_default,
                    "isForced": stream.is_forced,
                    "isHearingImpaired": stream.is_hearing_impaired,
                    "isExternal": stream.is_external,
                })
            })
            .collect(),
    )
}

impl ItemRecord {
    pub fn from_dto(dto: &BaseItemDto) -> Self {
        Self {
            jellyfin_id: dto.id.clone(),
            kind: dto
                .item_type
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
            name: dto.display_name().to_string(),
            original_title: non_empty(dto.original_title.as_deref()),
            sort_name: non_empty(dto.sort_name.as_deref())
                .or_else(|| Some(dto.display_name().to_lowercase())),
            // Jellyfin normally supplies ProductionYear for both films and
            // series (for a series it is the first-air year). Older servers
            // and some metadata providers only supply PremiereDate, so keep a
            // small fallback here. The v5 database migration applies the same
            // fallback to rows that were cached before this code existed.
            year: dto.production_year.or_else(|| {
                dto.premiere_date
                    .as_deref()
                    .and_then(release_year_from_date)
            }),
            premiere_date: non_empty(dto.premiere_date.as_deref()),
            runtime_ticks: dto.run_time_ticks.filter(|ticks| *ticks > 0),
            community_rating: dto.community_rating,
            official_rating: non_empty(dto.official_rating.as_deref()),
            parent_id: non_empty(dto.parent_id.as_deref()),
            series_id: non_empty(dto.series_id.as_deref()),
            series_name: non_empty(dto.series_name.as_deref()),
            season_id: non_empty(dto.season_id.as_deref()),
            index_number: dto.index_number,
            parent_index_number: dto.parent_index_number,
            child_count: dto.child_count,
            tmdb_id: dto.provider_id("Tmdb").map(str::to_string),
            imdb_id: dto.provider_id("Imdb").map(str::to_string),
            tvdb_id: dto.provider_id("Tvdb").map(str::to_string),
            genres: json_text(&dto.genres),
            image_tags: json_text(&dto.image_tags),
            primary_image_tag: dto.primary_image_tag().map(str::to_string),
            backdrop_image_tag: dto
                .backdrop_image_tags
                .first()
                .or_else(|| dto.parent_backdrop_image_tags.first())
                .cloned(),
            search_genres: dto.genres.join(", "),
            date_created: non_empty(dto.date_created.as_deref()),
            date_last_saved: non_empty(dto.date_last_saved.as_deref()),
        }
    }
}

/// Watch state mirrored from the server and from our own playback reporting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserDataRecord {
    pub jellyfin_id: String,
    pub played: bool,
    pub play_count: i64,
    pub playback_position_ticks: i64,
    pub is_favorite: bool,
    pub played_percentage: Option<f64>,
    pub last_played_date: Option<String>,
}

impl UserDataRecord {
    pub fn from_dto(jellyfin_id: &str, dto: &UserItemDataDto) -> Self {
        Self {
            jellyfin_id: jellyfin_id.to_string(),
            played: dto.played,
            play_count: dto.play_count,
            playback_position_ticks: dto.playback_position_ticks.max(0),
            is_favorite: dto.is_favorite,
            played_percentage: dto.played_percentage,
            last_played_date: non_empty(dto.last_played_date.as_deref()),
        }
    }
}

/// Counts shown on the status card and by `--library-stats`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub movies: i64,
    pub series: i64,
    pub seasons: i64,
    pub episodes: i64,
    pub total: i64,
}

fn json_text<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".to_string())
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn release_year_from_date(value: &str) -> Option<i64> {
    let year = value.get(..4)?.parse::<i64>().ok()?;
    (1900..=9999).contains(&year).then_some(year)
}

#[cfg(test)]
mod tests {
    use super::{
        ItemPlaybackPreference, ItemRecord, UserDataRecord, is_synced_kind,
        resolve_playback_preference,
    };
    use crate::jellyfin::api::model::{BaseItemDto, MediaSourceInfo, UserItemDataDto};

    fn dto(json: &str) -> BaseItemDto {
        serde_json::from_str(json).expect("dto")
    }

    fn media_source(json: &str) -> MediaSourceInfo {
        serde_json::from_str(json).expect("media source")
    }

    #[test]
    fn provider_ids_land_in_dedicated_columns() {
        let record = ItemRecord::from_dto(&dto(
            r#"{"Id":"a","Name":"The Matrix","Type":"Movie","ProductionYear":1999,
                "ProviderIds":{"Tmdb":"603","Imdb":"tt0133093"}}"#,
        ));
        assert_eq!(record.tmdb_id.as_deref(), Some("603"));
        assert_eq!(record.imdb_id.as_deref(), Some("tt0133093"));
        assert_eq!(record.tvdb_id, None);
        assert_eq!(record.year, Some(1999));
        assert_eq!(record.kind, "Movie");
    }

    #[test]
    fn release_year_prefers_production_year_and_falls_back_to_premiere_date() {
        let movie = ItemRecord::from_dto(&dto(
            r#"{"Id":"m","Name":"Movie","Type":"Movie","ProductionYear":1999,
                "PremiereDate":"2000-01-01T00:00:00Z"}"#,
        ));
        let series = ItemRecord::from_dto(&dto(r#"{"Id":"s","Name":"Series","Type":"Series",
                "PremiereDate":"2017-02-15T00:00:00Z"}"#));
        let invalid = ItemRecord::from_dto(&dto(
            r#"{"Id":"x","Name":"Unknown","Type":"Series","PremiereDate":"unknown"}"#,
        ));

        assert_eq!(movie.year, Some(1999));
        assert_eq!(series.year, Some(2017));
        assert_eq!(invalid.year, None);
    }

    #[test]
    fn missing_sort_names_fall_back_to_a_lowercased_title() {
        let record = ItemRecord::from_dto(&dto(r#"{"Id":"a","Name":"The Matrix"}"#));
        assert_eq!(record.sort_name.as_deref(), Some("the matrix"));
        assert_eq!(record.kind, "Unknown");
    }

    #[test]
    fn genres_are_flattened_for_search_and_kept_as_json() {
        let record = ItemRecord::from_dto(&dto(
            r#"{"Id":"a","Name":"Speed","Genres":["Action","Thriller"]}"#,
        ));
        assert_eq!(record.search_genres, "Action, Thriller");
        assert_eq!(record.genres, r#"["Action","Thriller"]"#);
    }

    #[test]
    fn only_library_kinds_are_synced() {
        for kind in ["Movie", "Series", "Season", "Episode"] {
            assert!(is_synced_kind(kind));
        }
        for kind in ["Audio", "Person", "Folder", "Unknown", ""] {
            assert!(!is_synced_kind(kind));
        }
    }

    #[test]
    fn zero_runtimes_and_blank_strings_become_null() {
        let record = ItemRecord::from_dto(&dto(
            r#"{"Id":"a","Name":"A","RunTimeTicks":0,"OfficialRating":""}"#,
        ));
        assert_eq!(record.runtime_ticks, None);
        assert_eq!(record.official_rating, None);
    }

    #[test]
    fn user_data_clamps_negative_positions() {
        let dto: UserItemDataDto =
            serde_json::from_str(r#"{"PlaybackPositionTicks":-5,"Played":true,"PlayCount":2}"#)
                .expect("user data");
        let record = UserDataRecord::from_dto("a", &dto);
        assert_eq!(record.playback_position_ticks, 0);
        assert!(record.played);
        assert_eq!(record.play_count, 2);
    }

    #[test]
    fn playback_track_snapshots_retain_matching_and_accessibility_metadata() {
        let source = media_source(
            r#"{"Id":"src","Name":"Feature","Container":"mkv",
                "MediaStreams":[
                    {"Index":1,"Type":"Audio","Language":"eng","Codec":"aac","Channels":2},
                    {"Index":2,"Type":"Audio","Language":"jpn","Title":"Original",
                     "Codec":"dts","Channels":6,"IsDefault":true},
                    {"Index":3,"Type":"Subtitle","Language":"eng","Title":"English SDH",
                     "Codec":"subrip","IsHearingImpaired":true,"IsExternal":true}] }"#,
        );
        let preference = ItemPlaybackPreference::capture(
            &source,
            0,
            source
                .streams_of_type("Audio")
                .find(|stream| stream.index == 2),
            source
                .streams_of_type("Subtitle")
                .find(|stream| stream.index == 3),
        );

        let audio = preference.audio_track.as_ref().expect("audio snapshot");
        assert_eq!(audio.language.as_deref(), Some("jpn"));
        assert_eq!(audio.title.as_deref(), Some("Original"));
        assert_eq!(audio.codec.as_deref(), Some("dts"));
        assert_eq!(audio.channels, Some(6));
        let subtitle = preference
            .subtitle_track
            .as_ref()
            .expect("subtitle snapshot");
        assert!(subtitle.is_hearing_impaired);
        assert!(subtitle.is_external);

        let resolved =
            resolve_playback_preference(Some(&preference), &[source]).expect("resolved preference");
        assert_eq!(resolved.audio_stream_index, Some(2));
        assert_eq!(resolved.subtitle_stream_index, Some(3));
    }

    #[test]
    fn stale_track_snapshots_fall_back_to_current_source_defaults() {
        let old = media_source(
            r#"{"Id":"src","DefaultAudioStreamIndex":1,"DefaultSubtitleStreamIndex":3,
                "MediaStreams":[
                    {"Index":1,"Type":"Audio","Language":"eng"},
                    {"Index":2,"Type":"Audio","Language":"jpn"},
                    {"Index":3,"Type":"Subtitle","Language":"eng"},
                    {"Index":4,"Type":"Subtitle","Language":"jpn"}] }"#,
        );
        let preference = ItemPlaybackPreference::capture(
            &old,
            0,
            old.streams_of_type("Audio")
                .find(|stream| stream.index == 2),
            old.streams_of_type("Subtitle")
                .find(|stream| stream.index == 4),
        );
        // The same absolute indices now describe different tracks. Applying
        // them would silently play the wrong language, so both use defaults.
        let changed = media_source(
            r#"{"Id":"src","DefaultAudioStreamIndex":1,"DefaultSubtitleStreamIndex":3,
                "MediaStreams":[
                    {"Index":1,"Type":"Audio","Language":"eng"},
                    {"Index":2,"Type":"Audio","Language":"fra"},
                    {"Index":3,"Type":"Subtitle","Language":"eng"},
                    {"Index":4,"Type":"Subtitle","Language":"fra"}] }"#,
        );

        let resolved = resolve_playback_preference(Some(&preference), &[changed])
            .expect("fallback preference");
        assert_eq!(resolved.audio_stream_index, Some(1));
        assert_eq!(resolved.subtitle_stream_index, Some(3));
    }

    #[test]
    fn explicit_subtitles_off_survives_resolution() {
        let source = media_source(
            r#"{"Id":"src","DefaultAudioStreamIndex":1,"DefaultSubtitleStreamIndex":2,
                "MediaStreams":[
                    {"Index":1,"Type":"Audio","Language":"eng"},
                    {"Index":2,"Type":"Subtitle","Language":"eng","IsForced":true}] }"#,
        );
        let preference = ItemPlaybackPreference::capture(
            &source,
            0,
            source.streams_of_type("Audio").next(),
            None,
        );

        let resolved =
            resolve_playback_preference(Some(&preference), &[source]).expect("resolved preference");
        assert_eq!(resolved.subtitle_stream_index, None);
    }

    #[test]
    fn a_missing_saved_source_falls_back_without_reusing_its_track_indices() {
        let old = media_source(
            r#"{"Id":"gone","MediaStreams":[
                    {"Index":8,"Type":"Audio","Language":"jpn"},
                    {"Index":9,"Type":"Subtitle","Language":"jpn"}] }"#,
        );
        let preference = ItemPlaybackPreference::capture(
            &old,
            0,
            old.streams_of_type("Audio").next(),
            old.streams_of_type("Subtitle").next(),
        );
        let current = media_source(
            r#"{"Id":"current","SupportsDirectStream":true,
                "DefaultAudioStreamIndex":1,"MediaStreams":[
                    {"Index":1,"Type":"Audio","Language":"eng"},
                    {"Index":9,"Type":"Subtitle","Language":"fra"}] }"#,
        );

        let resolved = resolve_playback_preference(Some(&preference), &[current])
            .expect("fallback preference");
        assert_eq!(resolved.media_source_id.as_deref(), Some("current"));
        assert_eq!(resolved.audio_stream_index, Some(1));
        assert_eq!(resolved.subtitle_stream_index, None);
    }
}
