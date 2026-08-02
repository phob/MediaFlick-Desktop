//! Item queries, user-data writes, and playback negotiation.
//!
//! Endpoints that Jellyfin moved out of `/Users/{userId}/…` in 10.9 are tried
//! at the modern path first and fall back to the legacy one, so the app works
//! against both current and older servers.

use serde_json::{Value, json};

use crate::app::urls::encode_path_segment;
use crate::preferences::StreamingQuality;

use super::client::{ApiError, JellyfinClient};
use super::device_profile::device_profile;
use super::model::{BaseItemDto, ItemsResponse, MediaSourceInfo, MediaUrl, PlaybackInfoResponse};

/// Item types the local cache mirrors. Music and live TV are out of scope.
pub const SYNCED_ITEM_TYPES: &str = "Movie,Series,Season,Episode";

/// Metadata pulled for every synced item. Provider IDs are the join keys for
/// the external-rating and list features planned on top of the cache.
/// `DateLastSaved` is deliberately absent: it is a valid `ItemFields` value but
/// servers return it empty, and it is not a valid `ItemSortBy` value, so the
/// cache keys freshness on `DateCreated` instead. See `library::sync`.
pub const SYNC_FIELDS: &str = "ProviderIds,Overview,Genres,Tags,Studios,People,\
DateCreated,OriginalTitle,SortName,PremiereDate,OfficialRating,\
CommunityRating,CriticRating,ChildCount,ParentId";

/// Page size for the bootstrap and incremental sweeps. Kept modest because
/// each row carries cast, studios, and provider ids.
pub const PAGE_SIZE: i64 = 200;

/// Page size for one parent's children. A series' season list and a season's
/// episode list both fit inside a single page in practice; the paging loop is
/// only there so a pathological parent still resolves completely.
pub const CHILDREN_PAGE_SIZE: i64 = 500;

fn user_query(user_id: &str) -> (&'static str, String) {
    ("userId", user_id.to_string())
}

fn items_page_query(
    user_id: &str,
    start_index: i64,
    sort_by: &str,
    sort_order: &str,
) -> Vec<(&'static str, String)> {
    vec![
        user_query(user_id),
        ("Recursive", "true".to_string()),
        ("IncludeItemTypes", SYNCED_ITEM_TYPES.to_string()),
        ("Fields", SYNC_FIELDS.to_string()),
        ("EnableUserData", "true".to_string()),
        ("EnableImages", "true".to_string()),
        ("StartIndex", start_index.to_string()),
        ("Limit", PAGE_SIZE.to_string()),
        ("SortBy", sort_by.to_string()),
        ("SortOrder", sort_order.to_string()),
    ]
}

/// One page of the library, ordered so paging stays stable during a sweep.
pub fn fetch_items_page(
    client: &JellyfinClient,
    user_id: &str,
    start_index: i64,
    sort_by: &str,
    sort_order: &str,
) -> Result<ItemsResponse, ApiError> {
    client.get_json(
        "/Items",
        &items_page_query(user_id, start_index, sort_by, sort_order),
    )
}

/// Identity-only sweep used to detect items deleted on the server, and the
/// cheap user-data mirror.
pub fn fetch_identity_page(
    client: &JellyfinClient,
    user_id: &str,
    start_index: i64,
    page_size: i64,
) -> Result<ItemsResponse, ApiError> {
    client.get_json(
        "/Items",
        &[
            user_query(user_id),
            ("Recursive", "true".to_string()),
            ("IncludeItemTypes", SYNCED_ITEM_TYPES.to_string()),
            ("Fields", String::new()),
            ("EnableUserData", "true".to_string()),
            ("EnableImages", "false".to_string()),
            ("StartIndex", start_index.to_string()),
            ("Limit", page_size.to_string()),
            ("SortBy", "DateCreated".to_string()),
            ("SortOrder", "Ascending".to_string()),
        ],
    )
}

fn children_query(user_id: &str, parent_id: &str, start_index: i64) -> Vec<(&'static str, String)> {
    vec![
        user_query(user_id),
        ("parentId", parent_id.to_string()),
        // Deliberately not recursive: a series must answer with its seasons,
        // not with every episode underneath it.
        ("IncludeItemTypes", SYNCED_ITEM_TYPES.to_string()),
        ("Fields", SYNC_FIELDS.to_string()),
        ("EnableUserData", "true".to_string()),
        ("EnableImages", "true".to_string()),
        ("StartIndex", start_index.to_string()),
        ("Limit", CHILDREN_PAGE_SIZE.to_string()),
        (
            "SortBy",
            "ParentIndexNumber,IndexNumber,SortName".to_string(),
        ),
        ("SortOrder", "Ascending".to_string()),
    ]
}

/// The direct children of one series or season, as the server sees them now.
///
/// Used to reconcile a detail page against the server, which is the only way
/// the season view can avoid showing episodes that were deleted since the last
/// sweep.
pub fn fetch_children(
    client: &JellyfinClient,
    user_id: &str,
    parent_id: &str,
    start_index: i64,
) -> Result<ItemsResponse, ApiError> {
    client.get_json("/Items", &children_query(user_id, parent_id, start_index))
}

/// Fetch a single item through `/Items?ids=`, which every supported server
/// version exposes.
pub fn fetch_item(
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) -> Result<Option<BaseItemDto>, ApiError> {
    let response: ItemsResponse = client.get_json(
        "/Items",
        &[
            user_query(user_id),
            ("ids", item_id.to_string()),
            ("Fields", SYNC_FIELDS.to_string()),
            ("EnableUserData", "true".to_string()),
            ("EnableImages", "true".to_string()),
        ],
    )?;
    Ok(response.items.into_iter().next())
}

/// Container, codec, and track detail for one item.
///
/// `MediaSources` is deliberately absent from `SYNC_FIELDS`: it multiplies the
/// size of every synced row and only the detail page ever reads it, so it is
/// fetched on demand and never cached.
pub fn fetch_media_sources(
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) -> Result<Vec<MediaSourceInfo>, ApiError> {
    let response: ItemsResponse = client.get_json(
        "/Items",
        &[
            user_query(user_id),
            ("ids", item_id.to_string()),
            ("Fields", "MediaSources,MediaStreams".to_string()),
            ("EnableUserData", "false".to_string()),
            ("EnableImages", "false".to_string()),
        ],
    )?;
    Ok(response
        .items
        .into_iter()
        .next()
        .map(|item| item.media_sources)
        .unwrap_or_default())
}

/// Local trailer items attached to one film.
///
/// Remote trailer URLs are deliberately not returned: the app scheme's
/// content policy keeps browsing self-contained, while local trailers can be
/// fetched through the authenticated byte-range proxy.
pub fn fetch_local_trailers(
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) -> Result<Vec<BaseItemDto>, ApiError> {
    let path = format!("/Items/{}/LocalTrailers", encode_path_segment(item_id));
    match client.get_json(&path, &[user_query(user_id)]) {
        Err(ApiError::Status { status }) if status == 404 || status == 405 => {
            let legacy = format!(
                "/Users/{}/Items/{}/LocalTrailers",
                encode_path_segment(user_id),
                encode_path_segment(item_id)
            );
            client.get_json(&legacy, &[])
        }
        result => result,
    }
}

/// Remote trailers recorded on one item. This is a separate, on-demand field:
/// it has no place in every row of the local metadata mirror.
pub fn fetch_remote_trailers(
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) -> Result<Vec<MediaUrl>, ApiError> {
    let response: ItemsResponse = client.get_json(
        "/Items",
        &[
            user_query(user_id),
            ("ids", item_id.to_string()),
            ("Fields", "RemoteTrailers".to_string()),
            ("EnableUserData", "false".to_string()),
            ("EnableImages", "false".to_string()),
        ],
    )?;
    Ok(response
        .items
        .into_iter()
        .next()
        .map(|item| item.remote_trailers)
        .unwrap_or_default())
}

/// Server-side "what should I watch next" logic; deliberately not replicated
/// against the local cache.
pub fn fetch_next_up(
    client: &JellyfinClient,
    user_id: &str,
    series_id: Option<&str>,
    limit: i64,
) -> Result<ItemsResponse, ApiError> {
    let mut query = vec![
        user_query(user_id),
        ("Limit", limit.to_string()),
        ("Fields", SYNC_FIELDS.to_string()),
        ("EnableUserData", "true".to_string()),
    ];
    if let Some(series_id) = series_id {
        query.push(("seriesId", series_id.to_string()));
    }
    client.get_json("/Shows/NextUp", &query)
}

/// Jellyfin's metadata-only calendar fallback. Unlike the companion calendar
/// this has no Sonarr truth (monitored/file state) and covers episodes only,
/// but it keeps the releases surface useful when the plugin is absent.
pub fn fetch_upcoming(
    client: &JellyfinClient,
    user_id: &str,
    limit: i64,
) -> Result<ItemsResponse, ApiError> {
    client.get_json(
        "/Shows/Upcoming",
        &[
            user_query(user_id),
            ("Limit", limit.clamp(1, 500).to_string()),
            ("Fields", SYNC_FIELDS.to_string()),
            ("EnableUserData", "true".to_string()),
            ("EnableImages", "true".to_string()),
        ],
    )
}

pub fn set_played(
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
    played: bool,
) -> Result<(), ApiError> {
    let modern = format!("/UserPlayedItems/{}", encode_path_segment(item_id));
    let legacy = format!(
        "/Users/{}/PlayedItems/{}",
        encode_path_segment(user_id),
        encode_path_segment(item_id)
    );
    user_data_write(client, user_id, &modern, &legacy, played)
}

pub fn set_favorite(
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
    favorite: bool,
) -> Result<(), ApiError> {
    let modern = format!("/UserFavoriteItems/{}", encode_path_segment(item_id));
    let legacy = format!(
        "/Users/{}/FavoriteItems/{}",
        encode_path_segment(user_id),
        encode_path_segment(item_id)
    );
    user_data_write(client, user_id, &modern, &legacy, favorite)
}

fn user_data_write(
    client: &JellyfinClient,
    user_id: &str,
    modern_path: &str,
    legacy_path: &str,
    enable: bool,
) -> Result<(), ApiError> {
    let query = [user_query(user_id)];
    let result = if enable {
        client.post_empty(modern_path, &query, &json!({}))
    } else {
        client.delete(modern_path, &query)
    };
    match result {
        Err(ApiError::Status { status }) if status == 404 || status == 405 => {
            if enable {
                client.post_empty(legacy_path, &[], &json!({}))
            } else {
                client.delete(legacy_path, &[])
            }
        }
        other => other,
    }
}

/// What the caller wants out of `PlaybackInfo`.
#[derive(Debug, Clone, Default)]
pub struct PlaybackInfoRequest {
    pub media_source_id: Option<String>,
    pub start_time_ticks: Option<i64>,
    pub audio_stream_index: Option<i64>,
    pub subtitle_stream_index: Option<i64>,
}

pub fn playback_info(
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
    quality: StreamingQuality,
    request: &PlaybackInfoRequest,
) -> Result<PlaybackInfoResponse, ApiError> {
    let mut body = json!({
        "UserId": user_id,
        "DeviceProfile": device_profile(quality),
        "EnableDirectPlay": true,
        "EnableDirectStream": true,
        "EnableTranscoding": quality.allows_transcoding(),
        "AllowVideoStreamCopy": true,
        "AllowAudioStreamCopy": true,
        "AutoOpenLiveStream": true,
        "StartTimeTicks": request.start_time_ticks.unwrap_or(0),
    });
    if let Some(bitrate) = quality.max_streaming_bitrate() {
        body["MaxStreamingBitrate"] = json!(bitrate);
    }
    for (key, value) in [
        (
            "MediaSourceId",
            request.media_source_id.clone().map(Value::from),
        ),
        (
            "AudioStreamIndex",
            request.audio_stream_index.map(Value::from),
        ),
        (
            "SubtitleStreamIndex",
            request.subtitle_stream_index.map(Value::from),
        ),
    ] {
        if let Some(value) = value {
            body[key] = value;
        }
    }

    let path = format!("/Items/{}/PlaybackInfo", encode_path_segment(item_id));
    client.post_json(&path, &[("userId", user_id.to_string())], &body)
}

/// Path of the poster/backdrop endpoint for an item.
pub fn image_path(item_id: &str, image_type: &str) -> String {
    format!(
        "/Items/{}/Images/{}",
        encode_path_segment(item_id),
        encode_path_segment(image_type)
    )
}

#[cfg(test)]
mod tests {
    use super::{
        CHILDREN_PAGE_SIZE, PAGE_SIZE, SYNC_FIELDS, SYNCED_ITEM_TYPES, children_query, image_path,
        items_page_query,
    };

    #[test]
    fn synced_types_exclude_music_and_live_tv() {
        assert_eq!(SYNCED_ITEM_TYPES, "Movie,Series,Season,Episode");
        assert!(!SYNCED_ITEM_TYPES.contains("Audio"));
    }

    #[test]
    fn sync_fields_request_the_provider_join_keys() {
        assert!(SYNC_FIELDS.contains("ProviderIds"));
        assert!(SYNC_FIELDS.contains("DateCreated"));
        assert!(!SYNC_FIELDS.contains(' '));
    }

    /// `DateLastSaved` is not a valid `ItemSortBy` value and servers return the
    /// field empty, so keying sync freshness on it silently disables the sweep.
    #[test]
    fn sync_fields_do_not_request_date_last_saved() {
        assert!(!SYNC_FIELDS.contains("DateLastSaved"));
    }

    #[test]
    fn a_sync_page_request_carries_the_paging_and_field_selection() {
        let query = items_page_query("uid", 400, "DateCreated", "Descending")
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query["userId"], "uid");
        assert_eq!(query["Recursive"], "true");
        assert_eq!(query["IncludeItemTypes"], SYNCED_ITEM_TYPES);
        assert_eq!(query["Fields"], SYNC_FIELDS);
        assert_eq!(query["EnableUserData"], "true");
        assert_eq!(query["StartIndex"], "400");
        assert_eq!(query["Limit"], PAGE_SIZE.to_string());
        assert_eq!(query["SortBy"], "DateCreated");
        assert_eq!(query["SortOrder"], "Descending");
    }

    /// `Recursive` must stay absent: with it, a series would answer with every
    /// episode underneath it and the season reconcile would delete its seasons.
    #[test]
    fn a_children_request_asks_for_direct_children_only() {
        let query = children_query("uid", "season1", 0)
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query["parentId"], "season1");
        assert!(!query.contains_key("Recursive"));
        assert_eq!(query["IncludeItemTypes"], SYNCED_ITEM_TYPES);
        assert_eq!(query["Fields"], SYNC_FIELDS);
        assert_eq!(query["EnableUserData"], "true");
        assert_eq!(query["Limit"], CHILDREN_PAGE_SIZE.to_string());
        assert_eq!(query["SortBy"], "ParentIndexNumber,IndexNumber,SortName");
    }

    #[test]
    fn image_paths_escape_the_item_id() {
        assert_eq!(image_path("abc", "Primary"), "/Items/abc/Images/Primary");
        assert_eq!(
            image_path("../secret", "Primary"),
            "/Items/..%2Fsecret/Images/Primary"
        );
    }
}
