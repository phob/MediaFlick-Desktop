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

/// A child response persists only the catalog fields, but passes each episode's
/// synopsis directly to the open season page.
const CHILD_FIELDS: &str = "ProviderIds,Overview,Genres,DateCreated,OriginalTitle,SortName,\
PremiereDate,OfficialRating,CommunityRating,ChildCount,ParentId";

/// Next Up is shaped directly into a browsing card. Provider ids, prose, cast,
/// and streams are not read by that card response.
const NEXT_UP_FIELDS: &str = "PremiereDate,OfficialRating,CommunityRating,ChildCount";

/// The Jellyfin calendar fallback reads only stable provider ids and air date.
const UPCOMING_FIELDS: &str = "ProviderIds,PremiereDate";

/// Page size for lightweight catalog and incremental sweeps. Requests remain
/// serial; this bounds both each SQLite commit and one coalesced UI invalidation.
pub const PAGE_SIZE: i64 = 200;

/// Page size for one parent's children. A series' season list and a season's
/// episode list both fit inside a single page in practice; the paging loop is
/// only there so a pathological parent still resolves completely.
pub const CHILDREN_PAGE_SIZE: i64 = 500;

/// Cast filmographies are live server queries rather than local-cache joins.
/// A large page keeps Discover's ownership verification bounded while the UI
/// can still request its ordinary 60-card windows.
pub const PERSON_PAGE_SIZE: i64 = 500;

/// Cast surfaces promise titles — movies and series. Jellyfin also credits
/// people on seasons and episodes, and those rows would consume the visible
/// result window without naming anything a card can represent.
pub const PERSON_ITEM_TYPES: &str = "Movie,Series";

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
        ("Fields", CATALOG_FIELDS.to_string()),
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

/// Fetch exact catalog items through `/Items?ids=`, which every supported
/// server version exposes.
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

/// Exactly what the `/about` response serializes — prose, cast, tags, studios,
/// and critic score. User data, genres, and media streams are deliberately
/// absent from every detail request.
pub const ABOUT_FIELDS: &str = "Overview,Tags,Studios,People,CriticRating";

/// The billboard needs prose and nothing else from the live rich record.
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

/// One item's live synopsis without cast, tags, studios, ratings, or image
/// metadata. This is the rotating billboard's purpose-built request.
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
        // Provider ids carry the TMDB collection identity the native
        // collections feature matches on; ChildCount feeds the card counter.
        ("Fields", "ProviderIds,ChildCount".to_string()),
        ("EnableUserData", "false".to_string()),
        ("EnableImages", "true".to_string()),
        ("SortBy", "SortName".to_string()),
        ("SortOrder", "Ascending".to_string()),
    ]
}

/// Every BoxSet visible to the signed-in user, ordered like a library view.
/// Collection mode reads these rows without creating or modifying BoxSets.
pub fn fetch_box_sets(client: &JellyfinClient, user_id: &str) -> Result<ItemsResponse, ApiError> {
    client.get_json("/Items", &box_sets_query(user_id))
}

fn box_set_children_query(user_id: &str, parent_id: &str) -> Vec<(&'static str, String)> {
    vec![
        user_query(user_id),
        ("parentId", parent_id.to_string()),
        ("IncludeItemTypes", "Movie,Series".to_string()),
        ("Fields", CATALOG_FIELDS.to_string()),
        ("EnableUserData", "true".to_string()),
        ("EnableImages", "true".to_string()),
        ("Limit", CHILDREN_PAGE_SIZE.to_string()),
        ("SortBy", "SortName".to_string()),
        ("SortOrder", "Ascending".to_string()),
    ]
}

/// One BoxSet's movie and series children, shaped like any other card row.
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

/// Technical stream descriptors for a batch of exact ids, feeding the card
/// quality badges. Everything else is deliberately excluded: badges need no
/// images, no user data, and no prose.
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
        ("PersonTypes", "Actor".to_string()),
        ("IncludeItemTypes", PERSON_ITEM_TYPES.to_string()),
        // Cards need the lightweight catalog shape, not a broad People or
        // MediaStreams fetch per page.
        ("Fields", CATALOG_FIELDS.to_string()),
        ("EnableUserData", "true".to_string()),
        ("EnableImages", "true".to_string()),
        ("StartIndex", start_index.max(0).to_string()),
        ("Limit", limit.clamp(1, PERSON_PAGE_SIZE).to_string()),
        ("SortBy", "SortName".to_string()),
        ("SortOrder", "Ascending".to_string()),
    ]
}

/// Every visible library item Jellyfin associates with one exact actor id.
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

/// Candidates used only to bridge a TMDB person link to Jellyfin's stable id.
/// Callers accept an exact provider id or one unambiguous exact name; this
/// endpoint never turns a fuzzy name match directly into a filmography.
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

/// Jellyfin's metadata-only calendar fallback. Unlike the companion calendar
/// this has no Sonarr truth (monitored/file state) and covers episodes only,
/// but it keeps the releases surface useful when the plugin is absent.
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
        CATALOG_FIELDS, CHILD_FIELDS, CHILDREN_PAGE_SIZE, NEXT_UP_FIELDS, PAGE_SIZE,
        PERSON_ITEM_TYPES, PERSON_PAGE_SIZE, SYNCED_ITEM_TYPES, SYNOPSIS_FIELDS, UPCOMING_FIELDS,
        box_set_children_query, box_sets_query, children_query, exact_items_query, image_path,
        items_page_query, next_up_query, person_items_query, synopsis_query, upcoming_query,
    };

    #[test]
    fn synced_types_exclude_music_and_live_tv() {
        assert_eq!(SYNCED_ITEM_TYPES, "Movie,Series,Season,Episode");
        assert!(!SYNCED_ITEM_TYPES.contains("Audio"));
    }

    #[test]
    fn purpose_specific_fields_exclude_discarded_rich_metadata() {
        for required in ["ProviderIds", "Genres", "DateCreated", "SortName"] {
            assert!(CATALOG_FIELDS.contains(required));
            assert!(CHILD_FIELDS.contains(required));
        }
        assert!(CHILD_FIELDS.contains("Overview"));
        for fields in [
            CATALOG_FIELDS,
            CHILD_FIELDS,
            NEXT_UP_FIELDS,
            UPCOMING_FIELDS,
        ] {
            for discarded in ["People", "Studios", "Tags", "MediaStreams", "MediaSources"] {
                assert!(!fields.contains(discarded));
            }
            assert!(!fields.contains(' '));
        }
        assert!(!CATALOG_FIELDS.contains("Overview"));
        assert!(!NEXT_UP_FIELDS.contains("ProviderIds"));
        assert_eq!(UPCOMING_FIELDS, "ProviderIds,PremiereDate");
    }

    /// The about panel serializes exactly these; anything more (media streams
    /// above all) would be fetched on every detail view only to be dropped.
    #[test]
    fn about_fields_carry_only_what_the_about_response_serializes() {
        for required in ["Overview", "People", "Tags", "Studios", "CriticRating"] {
            assert!(super::ABOUT_FIELDS.contains(required));
        }
        assert!(!super::ABOUT_FIELDS.contains("MediaStreams"));
        assert!(!super::ABOUT_FIELDS.contains("Genres"));
        assert!(!super::ABOUT_FIELDS.contains(' '));
    }

    #[test]
    fn billboard_synopsis_requests_only_overview_without_user_or_image_data() {
        let query = synopsis_query("uid", "movie-1")
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query["userId"], "uid");
        assert_eq!(query["ids"], "movie-1");
        assert_eq!(query["Fields"], SYNOPSIS_FIELDS);
        assert_eq!(query["Fields"], "Overview");
        assert_eq!(query["EnableUserData"], "false");
        assert_eq!(query["EnableImages"], "false");
        assert!(!query.contains_key("IncludeItemTypes"));
    }

    /// `DateLastSaved` is not a valid `ItemSortBy` value and servers return the
    /// field empty, so keying sync freshness on it silently disables the sweep.
    #[test]
    fn item_fields_do_not_request_date_last_saved() {
        for fields in [
            CATALOG_FIELDS,
            CHILD_FIELDS,
            NEXT_UP_FIELDS,
            UPCOMING_FIELDS,
            super::ABOUT_FIELDS,
            SYNOPSIS_FIELDS,
        ] {
            assert!(!fields.contains("DateLastSaved"));
        }
    }

    #[test]
    fn a_sync_page_request_carries_the_paging_and_field_selection() {
        let query = items_page_query("uid", 400, "DateCreated", "Descending")
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query["userId"], "uid");
        assert_eq!(query["Recursive"], "true");
        assert_eq!(query["IncludeItemTypes"], SYNCED_ITEM_TYPES);
        assert_eq!(query["Fields"], CATALOG_FIELDS);
        assert_eq!(query["EnableUserData"], "true");
        assert_eq!(query["StartIndex"], "400");
        assert_eq!(query["Limit"], PAGE_SIZE.to_string());
        assert_eq!(query["SortBy"], "DateCreated");
        assert_eq!(query["SortOrder"], "Descending");
    }

    #[test]
    fn a_box_sets_listing_is_lightweight_and_name_ordered() {
        let query = box_sets_query("uid")
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query["userId"], "uid");
        assert_eq!(query["Recursive"], "true");
        assert_eq!(query["IncludeItemTypes"], "BoxSet");
        assert_eq!(query["Fields"], "ProviderIds,ChildCount");
        assert_eq!(query["EnableUserData"], "false");
        assert_eq!(query["SortBy"], "SortName");
    }

    #[test]
    fn a_box_set_children_request_scopes_media_under_the_parent() {
        let query = box_set_children_query("uid", "boxset-1")
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query["parentId"], "boxset-1");
        assert_eq!(query["IncludeItemTypes"], "Movie,Series");
        assert_eq!(query["Fields"], CATALOG_FIELDS);
        assert_eq!(query["EnableUserData"], "true");
        assert_eq!(query["SortBy"], "SortName");
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
        assert_eq!(query["Fields"], CHILD_FIELDS);
        assert!(query["Fields"].contains("Overview"));
        assert!(!query["Fields"].contains("People"));
        assert!(!query["Fields"].contains("MediaStreams"));
        assert_eq!(query["EnableUserData"], "true");
        assert_eq!(query["Limit"], CHILDREN_PAGE_SIZE.to_string());
        assert_eq!(query["SortBy"], "ParentIndexNumber,IndexNumber,SortName");
    }

    #[test]
    fn cast_queries_are_exact_complete_and_lightweight() {
        let query = person_items_query("uid", "person-42", 60, 60)
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query["userId"], "uid");
        assert_eq!(query["PersonIds"], "person-42");
        assert_eq!(query["PersonTypes"], "Actor");
        // Titles only: a season or episode credit would burn a card slot
        // without naming anything the cast surfaces promise.
        assert_eq!(query["IncludeItemTypes"], PERSON_ITEM_TYPES);
        assert_eq!(query["IncludeItemTypes"], "Movie,Series");
        assert_eq!(query["Recursive"], "true");
        assert_eq!(query["StartIndex"], "60");
        assert_eq!(query["Limit"], "60");
        assert_eq!(query["Fields"], CATALOG_FIELDS);
        assert!(!query["Fields"].contains("People"));
        assert!(!query["Fields"].contains("MediaStreams"));
        assert_eq!(query["EnableUserData"], "true");

        let bounded = person_items_query("uid", "person-42", -1, i64::MAX)
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(bounded["StartIndex"], "0");
        assert_eq!(bounded["Limit"], PERSON_PAGE_SIZE.to_string());
    }

    #[test]
    fn image_paths_escape_the_item_id() {
        assert_eq!(image_path("abc", "Primary"), "/Items/abc/Images/Primary");
        assert_eq!(
            image_path("../secret", "Primary"),
            "/Items/..%2Fsecret/Images/Primary"
        );
    }

    #[test]
    fn multi_id_queries_are_one_bounded_catalog_read() {
        let ids = (0..20)
            .map(|index| format!("item-{index}"))
            .collect::<Vec<_>>();
        let query = exact_items_query("uid", &ids)
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query["ids"].split(',').count(), 20);
        assert_eq!(query["Fields"], CATALOG_FIELDS);
        assert!(!query["Fields"].contains("Overview"));
        assert!(!query["Fields"].contains("MediaStreams"));
        assert_eq!(query["EnableImages"], "true");
    }

    #[test]
    fn next_up_and_upcoming_request_only_the_fields_their_consumers_read() {
        let next_up = next_up_query("uid", Some("series-1"), 24)
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(next_up["seriesId"], "series-1");
        assert_eq!(next_up["Fields"], NEXT_UP_FIELDS);
        assert_eq!(next_up["EnableUserData"], "true");
        assert_eq!(next_up["EnableImages"], "true");

        let upcoming = upcoming_query("uid", i64::MAX)
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(upcoming["Limit"], "500");
        assert_eq!(upcoming["Fields"], UPCOMING_FIELDS);
        assert_eq!(upcoming["EnableUserData"], "false");
        assert_eq!(upcoming["EnableImages"], "false");
    }
}
