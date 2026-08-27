//! The subset of Jellyfin's OpenAPI shapes this app actually consumes.
//!
//! Everything is `#[serde(default)]` so a server that drops or adds fields
//! between releases degrades to missing metadata instead of a failed sync.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct PublicSystemInfo {
    pub server_name: String,
    pub version: String,
    pub id: String,
    pub startup_wizard_completed: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct AuthenticationResult {
    pub access_token: String,
    pub server_id: String,
    pub user: Option<UserDto>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct UserDto {
    pub id: String,
    pub name: String,
    pub policy: Option<UserPolicy>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct UserPolicy {
    pub max_parental_rating: Option<i32>,
    pub blocked_tags: Vec<String>,
    pub block_unrated_items: Vec<String>,
    #[serde(default = "enabled_by_default")]
    pub enable_all_folders: bool,
    pub enabled_folders: Vec<String>,
}

impl Default for UserPolicy {
    fn default() -> Self {
        Self {
            max_parental_rating: None,
            blocked_tags: Vec::new(),
            block_unrated_items: Vec::new(),
            enable_all_folders: true,
            enabled_folders: Vec::new(),
        }
    }
}

impl UserPolicy {
    pub fn restricts_catalog(&self) -> bool {
        self.max_parental_rating.is_some()
            || !self.blocked_tags.is_empty()
            || !self.block_unrated_items.is_empty()
            || !self.enable_all_folders
            || !self.enabled_folders.is_empty()
    }
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct QuickConnectResult {
    pub secret: String,
    pub code: String,
    pub authenticated: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct ItemsResponse {
    pub items: Vec<BaseItemDto>,
    pub total_record_count: i64,
    pub start_index: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct BaseItemDto {
    pub id: String,
    pub name: Option<String>,
    pub original_title: Option<String>,
    pub sort_name: Option<String>,
    #[serde(rename = "Type")]
    pub item_type: Option<String>,
    pub production_year: Option<i64>,
    pub premiere_date: Option<String>,
    pub run_time_ticks: Option<i64>,
    pub overview: Option<String>,
    pub community_rating: Option<f64>,
    pub critic_rating: Option<f64>,
    pub official_rating: Option<String>,
    pub parent_id: Option<String>,
    pub series_id: Option<String>,
    pub series_name: Option<String>,
    pub season_id: Option<String>,
    pub index_number: Option<i64>,
    pub parent_index_number: Option<i64>,
    pub child_count: Option<i64>,
    pub provider_ids: BTreeMap<String, String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub studios: Vec<NameIdPair>,
    pub people: Vec<BaseItemPerson>,
    pub image_tags: BTreeMap<String, String>,
    pub backdrop_image_tags: Vec<String>,
    pub parent_backdrop_image_tags: Vec<String>,
    pub series_primary_image_tag: Option<String>,
    pub date_created: Option<String>,
    pub date_last_saved: Option<String>,
    pub user_data: Option<UserItemDataDto>,
    /// Lightweight stream descriptors returned by the `MediaStreams` item
    /// field. Unlike `media_sources`, these can be cached with library rows
    /// without retaining paths, source negotiation, or subtitle delivery data.
    pub media_streams: Vec<MediaStream>,
    pub media_sources: Vec<MediaSourceInfo>,
    pub remote_trailers: Vec<MediaUrl>,
    pub is_folder: Option<bool>,
}

impl BaseItemDto {
    pub fn provider_id(&self, name: &str) -> Option<&str> {
        self.provider_ids.iter().find_map(|(key, value)| {
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim())
                .filter(|value| !value.is_empty())
        })
    }

    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("Untitled")
    }

    /// Finds a tag in Jellyfin's per-image-type map without relying on the
    /// server's key casing.
    pub fn image_tag(&self, image_type: &str) -> Option<&str> {
        self.image_tags.iter().find_map(|(key, value)| {
            key.eq_ignore_ascii_case(image_type)
                .then_some(value.as_str())
        })
    }

    /// The image tag to use for the poster, falling back to the parent series
    /// so episodes still render art.
    pub fn primary_image_tag(&self) -> Option<&str> {
        self.image_tag("Primary")
            .or(self.series_primary_image_tag.as_deref())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct MediaUrl {
    pub name: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct NameIdPair {
    pub name: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct BaseItemPerson {
    pub name: Option<String>,
    pub id: Option<String>,
    pub role: Option<String>,
    #[serde(rename = "Type")]
    pub person_type: Option<String>,
    pub primary_image_tag: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct UserItemDataDto {
    pub played: bool,
    pub play_count: i64,
    pub playback_position_ticks: i64,
    pub is_favorite: bool,
    pub played_percentage: Option<f64>,
    pub last_played_date: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct PlaybackInfoResponse {
    pub media_sources: Vec<MediaSourceInfo>,
    pub play_session_id: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct MediaSourceInfo {
    pub id: Option<String>,
    pub name: Option<String>,
    pub container: Option<String>,
    pub path: Option<String>,
    pub protocol: Option<String>,
    pub size: Option<i64>,
    pub bitrate: Option<i64>,
    pub run_time_ticks: Option<i64>,
    pub supports_direct_play: bool,
    pub supports_direct_stream: bool,
    pub supports_transcoding: bool,
    pub is_remote: bool,
    pub requires_opening: bool,
    pub requires_closing: bool,
    pub open_token: Option<String>,
    pub live_stream_id: Option<String>,
    pub transcoding_url: Option<String>,
    pub transcoding_sub_protocol: Option<String>,
    pub transcoding_container: Option<String>,
    pub direct_stream_url: Option<String>,
    pub e_tag: Option<String>,
    pub media_streams: Vec<MediaStream>,
    pub default_audio_stream_index: Option<i64>,
    pub default_subtitle_stream_index: Option<i64>,
}

impl MediaSourceInfo {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("Default")
    }

    pub fn streams_of_type(&self, kind: &str) -> impl Iterator<Item = &MediaStream> {
        self.media_streams.iter().filter(move |stream| {
            stream
                .stream_type
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(kind))
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct MediaStream {
    pub index: i64,
    #[serde(rename = "Type")]
    pub stream_type: Option<String>,
    pub codec: Option<String>,
    pub profile: Option<String>,
    pub language: Option<String>,
    pub display_title: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
    /// Jellyfin marks SDH/closed-caption-style subtitle tracks separately from
    /// ordinary dialogue subtitles. Keep the flag even when the server does
    /// not localize a display title so per-item choices retain accessibility
    /// intent.
    pub is_hearing_impaired: bool,
    pub is_external: bool,
    pub delivery_url: Option<String>,
    pub delivery_method: Option<String>,
    pub height: Option<i64>,
    pub width: Option<i64>,
    pub channels: Option<i64>,
    /// Jellyfin's normalized spatial-audio value (for example
    /// `DolbyAtmos`). Older servers may only mention it in Profile/DisplayTitle.
    pub audio_spatial_format: Option<String>,
    /// "SDR" or "HDR". `video_range_type` narrows the latter to HDR10, HLG, or
    /// one of the Dolby Vision profiles, which is what decides whether a file
    /// tone-maps on a given display.
    pub video_range: Option<String>,
    pub video_range_type: Option<String>,
    pub bit_depth: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::{BaseItemDto, ItemsResponse, PlaybackInfoResponse};

    #[test]
    fn items_response_tolerates_missing_fields() {
        let response: ItemsResponse = serde_json::from_str(r#"{"Items":[{"Id":"a"}]}"#).unwrap();
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.total_record_count, 0);
        assert_eq!(response.items[0].display_name(), "Untitled");
    }

    #[test]
    fn provider_ids_are_matched_case_insensitively() {
        let item: BaseItemDto = serde_json::from_str(
            r#"{"Id":"a","ProviderIds":{"Tmdb":"603","imdb":"tt0133093","Tvdb":"  "}}"#,
        )
        .unwrap();
        assert_eq!(item.provider_id("tmdb"), Some("603"));
        assert_eq!(item.provider_id("IMDB"), Some("tt0133093"));
        assert_eq!(item.provider_id("tvdb"), None);
    }

    #[test]
    fn primary_image_tag_falls_back_to_the_series_poster() {
        let episode: BaseItemDto =
            serde_json::from_str(r#"{"Id":"a","SeriesPrimaryImageTag":"series-tag"}"#).unwrap();
        assert_eq!(episode.primary_image_tag(), Some("series-tag"));

        let movie: BaseItemDto = serde_json::from_str(
            r#"{"Id":"a","ImageTags":{"Primary":"own-tag","thumb":"wide-tag"}}"#,
        )
        .unwrap();
        assert_eq!(movie.primary_image_tag(), Some("own-tag"));
        assert_eq!(movie.image_tag("Thumb"), Some("wide-tag"));
    }

    #[test]
    fn item_media_streams_keep_hdr_and_spatial_audio_descriptors() {
        let item: BaseItemDto = serde_json::from_str(
            r#"{"Id":"movie","MediaStreams":[
                {"Index":0,"Type":"Video","Codec":"hevc","Width":3840,"Height":2160,
                 "VideoRange":"HDR","VideoRangeType":"DOVIWithHDR10","BitDepth":10},
                {"Index":1,"Type":"Audio","Codec":"truehd","Profile":"Dolby TrueHD",
                 "Channels":8,"AudioSpatialFormat":"DolbyAtmos"}]}"#,
        )
        .unwrap();

        assert_eq!(item.media_streams.len(), 2);
        assert_eq!(
            item.media_streams[0].video_range_type.as_deref(),
            Some("DOVIWithHDR10")
        );
        assert_eq!(
            item.media_streams[1].audio_spatial_format.as_deref(),
            Some("DolbyAtmos")
        );
        assert_eq!(
            item.media_streams[1].profile.as_deref(),
            Some("Dolby TrueHD")
        );
    }

    #[test]
    fn playback_info_reads_sources_and_the_play_session() {
        let response: PlaybackInfoResponse = serde_json::from_str(
            r#"{"MediaSources":[{"Id":"src","SupportsDirectStream":true,
                "MediaStreams":[{"Index":1,"Type":"Audio","Language":"eng"}]}],
                "PlaySessionId":"session"}"#,
        )
        .unwrap();
        assert_eq!(response.play_session_id.as_deref(), Some("session"));
        let source = &response.media_sources[0];
        assert!(source.supports_direct_stream);
        assert_eq!(source.streams_of_type("audio").count(), 1);
    }
}
