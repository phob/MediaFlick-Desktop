//! Item queries, user-data writes, and playback negotiation.
//!
//! Endpoints that Jellyfin moved out of `/Users/{userId}/…` in 10.9 are tried
//! at the modern path first and fall back to the legacy one, so the app works
//! against both current and older servers.

use serde_json::{Value, json};

use crate::app::urls::encode_path_segment;
use crate::library::CatalogKind;
use crate::preferences::StreamingQuality;

use super::client::{ApiError, JellyfinClient};
use super::device_profile::device_profile;
use super::model::{
    BaseItemDto, IdentityPage, ItemsResponse, MediaSourceInfo, MediaUrl, PlaybackInfoResponse,
    UserViewDto, UserViewsResponse,
};

/// Lightweight fields required to browse, sort, filter, join, and draw cards.
/// Name, id, type, year, runtime, hierarchy, and image tags are part of
/// Jellyfin's base item shape; the fields below opt into only the additional
/// catalog signals the local index needs. Rich prose, cast, studios, tags,
/// and media streams are never cached.
///
/// `DateLastSaved` is deliberately absent: it is a valid `ItemFields` value but
/// servers return it empty, and it is not a valid `ItemSortBy` value, so the
/// cache keys freshness on `DateCreated` instead. See `library::sync`.
pub const CATALOG_FIELDS: &str = "ProviderIds,Genres,DateCreated,OriginalTitle,SortName,\
PremiereDate,OfficialRating,CommunityRating,ChildCount,ParentId";

const CHILD_FIELDS: &str = "ProviderIds,Overview,Genres,DateCreated,OriginalTitle,SortName,\
PremiereDate,OfficialRating,CommunityRating,ChildCount,ParentId";

const NEXT_UP_FIELDS: &str = "PremiereDate,OfficialRating,CommunityRating,ChildCount";

const UPCOMING_FIELDS: &str = "ProviderIds,PremiereDate";

pub const PAGE_SIZE: i64 = 200;

pub const CHILDREN_PAGE_SIZE: i64 = 500;

pub const PERSON_PAGE_SIZE: i64 = 500;

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
        (
            "IncludeItemTypes",
            CatalogKind::wire_list(&CatalogKind::ALL),
        ),
        ("Fields", CATALOG_FIELDS.to_string()),
        ("EnableUserData", "true".to_string()),
        ("EnableImages", "true".to_string()),
        ("StartIndex", start_index.to_string()),
        ("Limit", PAGE_SIZE.to_string()),
        ("SortBy", sort_by.to_string()),
        ("SortOrder", sort_order.to_string()),
    ]
}

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

pub fn fetch_identity_page(
    client: &JellyfinClient,
    user_id: &str,
    start_index: i64,
    page_size: i64,
) -> Result<IdentityPage, ApiError> {
    client.get_json(
        "/Items",
        &[
            user_query(user_id),
            ("Recursive", "true".to_string()),
            (
                "IncludeItemTypes",
                CatalogKind::wire_list(&CatalogKind::ALL),
            ),
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

pub fn fetch_user_views(
    client: &JellyfinClient,
    user_id: &str,
) -> Result<Vec<UserViewDto>, ApiError> {
    let response: UserViewsResponse = match client.get_json("/UserViews", &[user_query(user_id)]) {
        Err(ApiError::Status { status }) if status == 404 || status == 405 => {
            let legacy = format!("/Users/{}/Views", encode_path_segment(user_id));
            client.get_json(&legacy, &[])?
        }
        result => result?,
    };
    Ok(response.items)
}

/// An item's own `ParentId` is its folder, never its library, so this walk is
/// the only way to learn membership.
pub fn fetch_library_member_ids(
    client: &JellyfinClient,
    user_id: &str,
    library_id: &str,
    kind: CatalogKind,
    start_index: i64,
    page_size: i64,
) -> Result<ItemsResponse, ApiError> {
    client.get_json(
        "/Items",
        &[
            user_query(user_id),
            ("parentId", library_id.to_string()),
            ("Recursive", "true".to_string()),
            ("IncludeItemTypes", kind.as_str().to_string()),
            ("Fields", String::new()),
            ("EnableUserData", "false".to_string()),
            ("EnableImages", "false".to_string()),
            ("StartIndex", start_index.to_string()),
            ("Limit", page_size.to_string()),
            // SortName breaks DateCreated ties so pages neither skip nor
            // repeat titles. Jellyfin ignores an `Id` sort.
            ("SortBy", "DateCreated,SortName".to_string()),
            ("SortOrder", "Ascending".to_string()),
        ],
    )
}

fn children_query(user_id: &str, parent_id: &str, start_index: i64) -> Vec<(&'static str, String)> {
    vec![
        user_query(user_id),
        ("parentId", parent_id.to_string()),
        (
            "IncludeItemTypes",
            CatalogKind::wire_list(&CatalogKind::ALL),
        ),
        ("Fields", CHILD_FIELDS.to_string()),
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

fn exact_items_query(user_id: &str, item_ids: &[String]) -> Vec<(&'static str, String)> {
    vec![
        user_query(user_id),
        ("ids", item_ids.join(",")),
        ("Fields", CATALOG_FIELDS.to_string()),
        ("EnableUserData", "true".to_string()),
        ("EnableImages", "true".to_string()),
    ]
}

pub fn fetch_items(
    client: &JellyfinClient,
    user_id: &str,
    item_ids: &[String],
) -> Result<ItemsResponse, ApiError> {
    client.get_json("/Items", &exact_items_query(user_id, item_ids))
}

pub fn fetch_item(
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) -> Result<Option<BaseItemDto>, ApiError> {
    let response = fetch_items(client, user_id, &[item_id.to_string()])?;
    Ok(response.items.into_iter().next())
}

/// The catalog fields, so a movie or series can be cached on the way, plus what
/// a generic details page shows.
const DETAIL_FIELDS: &str = "ProviderIds,Genres,DateCreated,OriginalTitle,SortName,\
PremiereDate,OfficialRating,CommunityRating,ChildCount,ParentId,Overview,PrimaryImageAspectRatio";

pub fn fetch_item_detail(
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) -> Result<Option<BaseItemDto>, ApiError> {
    let response: ItemsResponse = client.get_json(
        "/Items",
        &[
            user_query(user_id),
            ("ids", item_id.to_string()),
            ("Fields", DETAIL_FIELDS.to_string()),
            ("EnableUserData", "true".to_string()),
            ("EnableImages", "true".to_string()),
        ],
    )?;
    Ok(response.items.into_iter().next())
}

const LIVE_CHILD_FIELDS: &str =
    "PrimaryImageAspectRatio,ChildCount,PremiereDate,OfficialRating,CommunityRating";

pub const LIVE_CHILDREN_MAX_PAGE: i64 = 200;

fn live_children_query(
    user_id: &str,
    parent_id: &str,
    start_index: i64,
    limit: i64,
) -> Vec<(&'static str, String)> {
    vec![
        user_query(user_id),
        ("parentId", parent_id.to_string()),
        ("Fields", LIVE_CHILD_FIELDS.to_string()),
        ("EnableUserData", "true".to_string()),
        ("EnableImages", "true".to_string()),
        ("StartIndex", start_index.max(0).to_string()),
        ("Limit", limit.clamp(1, LIVE_CHILDREN_MAX_PAGE).to_string()),
        (
            "SortBy",
            "ParentIndexNumber,IndexNumber,SortName".to_string(),
        ),
        ("SortOrder", "Ascending".to_string()),
    ]
}

/// One page of any container's direct children, of every type: an album's
/// tracks, a box set's titles, a folder's entries.
pub fn fetch_live_children(
    client: &JellyfinClient,
    user_id: &str,
    parent_id: &str,
    start_index: i64,
    limit: i64,
) -> Result<ItemsResponse, ApiError> {
    client.get_json(
        "/Items",
        &live_children_query(user_id, parent_id, start_index, limit),
    )
}

pub const ABOUT_FIELDS: &str = "Overview,Tags,Studios,People,CriticRating";

pub const SYNOPSIS_FIELDS: &str = "Overview";

fn synopsis_query(user_id: &str, item_id: &str) -> Vec<(&'static str, String)> {
    vec![
        user_query(user_id),
        ("ids", item_id.to_string()),
        ("Fields", SYNOPSIS_FIELDS.to_string()),
        ("EnableUserData", "false".to_string()),
        ("EnableImages", "false".to_string()),
    ]
}

pub fn fetch_item_synopsis(
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) -> Result<Option<BaseItemDto>, ApiError> {
    let response: ItemsResponse = client.get_json("/Items", &synopsis_query(user_id, item_id))?;
    Ok(response.items.into_iter().next())
}

fn box_sets_query(user_id: &str) -> Vec<(&'static str, String)> {
    vec![
        user_query(user_id),
        ("Recursive", "true".to_string()),
        ("IncludeItemTypes", "BoxSet".to_string()),
        ("Fields", "ProviderIds,ChildCount".to_string()),
        ("EnableUserData", "false".to_string()),
        ("EnableImages", "true".to_string()),
        ("SortBy", "SortName".to_string()),
        ("SortOrder", "Ascending".to_string()),
    ]
}

pub fn fetch_box_sets(client: &JellyfinClient, user_id: &str) -> Result<ItemsResponse, ApiError> {
    client.get_json("/Items", &box_sets_query(user_id))
}

fn box_set_children_query(user_id: &str, parent_id: &str) -> Vec<(&'static str, String)> {
    vec![
        user_query(user_id),
        ("parentId", parent_id.to_string()),
        (
            "IncludeItemTypes",
            CatalogKind::wire_list(&CatalogKind::TOP_LEVEL),
        ),
        ("Fields", CATALOG_FIELDS.to_string()),
        ("EnableUserData", "true".to_string()),
        ("EnableImages", "true".to_string()),
        ("Limit", CHILDREN_PAGE_SIZE.to_string()),
        ("SortBy", "SortName".to_string()),
        ("SortOrder", "Ascending".to_string()),
    ]
}

pub fn fetch_box_set_children(
    client: &JellyfinClient,
    user_id: &str,
    parent_id: &str,
) -> Result<ItemsResponse, ApiError> {
    client.get_json("/Items", &box_set_children_query(user_id, parent_id))
}

/// One item's live about-panel metadata. Images stay enabled because cast
/// entries carry their headshot tags through the `People` field.
pub fn fetch_item_about(
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) -> Result<Option<BaseItemDto>, ApiError> {
    let response: ItemsResponse = client.get_json(
        "/Items",
        &[
            user_query(user_id),
            ("ids", item_id.to_string()),
            ("Fields", ABOUT_FIELDS.to_string()),
            ("EnableUserData", "false".to_string()),
            ("EnableImages", "true".to_string()),
        ],
    )?;
    Ok(response.items.into_iter().next())
}

pub fn fetch_media_stream_batch(
    client: &JellyfinClient,
    user_id: &str,
    item_ids: &[String],
) -> Result<ItemsResponse, ApiError> {
    client.get_json(
        "/Items",
        &[
            user_query(user_id),
            ("ids", item_ids.join(",")),
            ("Fields", "MediaStreams".to_string()),
            ("EnableUserData", "false".to_string()),
            ("EnableImages", "false".to_string()),
        ],
    )
}

fn person_items_query(
    user_id: &str,
    person_id: &str,
    start_index: i64,
    limit: i64,
) -> Vec<(&'static str, String)> {
    vec![
        user_query(user_id),
        ("Recursive", "true".to_string()),
        ("PersonIds", person_id.to_string()),
        // Cast surfaces promise titles. Jellyfin also credits people on
        // seasons and episodes, and those rows would consume the visible
        // result window without naming anything a card can represent.
        (
            "IncludeItemTypes",
            CatalogKind::wire_list(&CatalogKind::TOP_LEVEL),
        ),
        ("Fields", CATALOG_FIELDS.to_string()),
        ("EnableUserData", "true".to_string()),
        ("EnableImages", "true".to_string()),
        ("StartIndex", start_index.max(0).to_string()),
        ("Limit", limit.clamp(1, PERSON_PAGE_SIZE).to_string()),
        ("SortBy", "SortName".to_string()),
        ("SortOrder", "Ascending".to_string()),
    ]
}

/// Every visible library item Jellyfin credits one exact person on, as cast or
/// crew.
///
/// Cast is not cached locally at all: names can be ambiguous, while Jellyfin's
/// `PersonIds` relation is exact and complete immediately.
pub fn fetch_person_items(
    client: &JellyfinClient,
    user_id: &str,
    person_id: &str,
    start_index: i64,
    limit: i64,
) -> Result<ItemsResponse, ApiError> {
    client.get_json(
        "/Items",
        &person_items_query(user_id, person_id, start_index, limit),
    )
}

pub fn fetch_people(
    client: &JellyfinClient,
    user_id: &str,
    search_term: &str,
) -> Result<ItemsResponse, ApiError> {
    client.get_json(
        "/Persons",
        &[
            user_query(user_id),
            ("searchTerm", search_term.trim().to_string()),
            ("Fields", "ProviderIds".to_string()),
            ("EnableImages", "true".to_string()),
            ("Limit", "50".to_string()),
        ],
    )
}

/// Container, codec, and track detail for one item.
///
/// Paths, source negotiation, and subtitle delivery data never enter the thin
/// index. The detail page reads full sources here, while cards use the separate
/// live `MediaStreams` batch above.
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

fn next_up_query(
    user_id: &str,
    series_id: Option<&str>,
    limit: i64,
) -> Vec<(&'static str, String)> {
    let mut query = vec![
        user_query(user_id),
        ("Limit", limit.to_string()),
        ("Fields", NEXT_UP_FIELDS.to_string()),
        ("EnableUserData", "true".to_string()),
        ("EnableImages", "true".to_string()),
    ];
    if let Some(series_id) = series_id {
        query.push(("seriesId", series_id.to_string()));
    }
    query
}

pub fn fetch_next_up(
    client: &JellyfinClient,
    user_id: &str,
    series_id: Option<&str>,
    limit: i64,
) -> Result<ItemsResponse, ApiError> {
    client.get_json("/Shows/NextUp", &next_up_query(user_id, series_id, limit))
}

fn upcoming_query(user_id: &str, limit: i64) -> Vec<(&'static str, String)> {
    vec![
        user_query(user_id),
        ("Limit", limit.clamp(1, 500).to_string()),
        ("Fields", UPCOMING_FIELDS.to_string()),
        ("EnableUserData", "false".to_string()),
        ("EnableImages", "false".to_string()),
    ]
}

pub fn fetch_upcoming(
    client: &JellyfinClient,
    user_id: &str,
    limit: i64,
) -> Result<ItemsResponse, ApiError> {
    client.get_json("/Shows/Upcoming", &upcoming_query(user_id, limit))
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

pub fn image_path(item_id: &str, image_type: &str) -> String {
    format!(
        "/Items/{}/Images/{}",
        encode_path_segment(item_id),
        encode_path_segment(image_type)
    )
}

#[cfg(test)]
mod tests {
    use super::{PERSON_PAGE_SIZE, children_query, person_items_query};

    #[test]
    fn a_children_request_asks_for_direct_children_only() {
        let query = children_query("uid", "season1", 0)
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query["parentId"], "season1");
        assert!(!query.contains_key("Recursive"));
        assert!(query["Fields"].contains("Overview"));
    }

    #[test]
    fn cast_queries_are_exact_complete_and_bounded() {
        let query = person_items_query("uid", "person-42", 60, 60)
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query["PersonIds"], "person-42");
        assert_eq!(query["Recursive"], "true");

        let bounded = person_items_query("uid", "person-42", -1, i64::MAX)
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(bounded["StartIndex"], "0");
        assert_eq!(bounded["Limit"], PERSON_PAGE_SIZE.to_string());
    }
}
