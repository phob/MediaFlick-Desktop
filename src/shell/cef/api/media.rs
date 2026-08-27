use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["technical", "batch"] if request.is("POST") => technical_batch(services, request),
        ["item", id, "media"] if request.is("GET") => media_info(services, &percent_decode(id)),
        ["item", id, "playback-preference"] if request.is("PATCH") => {
            set_item_playback_preference(services, &percent_decode(id), request)
        }
        ["item", id, "trailer"] if request.is("GET") => trailer_info(services, &percent_decode(id)),
        ["item", id, "nextup"] if request.is("GET") => next_up(services, &percent_decode(id)),
        ["item", id, "external"] if request.is("POST") => {
            open_external(services, &percent_decode(id), request)
        }
        ["item", id, "played"] if request.is("POST") => {
            set_played(services, &percent_decode(id), request)
        }
        ["item", id, "favorite"] if request.is("POST") => {
            set_favorite(services, &percent_decode(id), request)
        }
        ["trailer", id, "stream"] if request.is("GET") => {
            trailer_stream(services, &percent_decode(id), request)
        }
        _ => return None,
    };
    Some(response)
}

const TECHNICAL_BATCH_SIZE: usize = 40;

/// Live technical stream descriptors for visible cards, batched by the UI's
/// badge scheduler. Container ids (Series, Season) answer with the streams of
/// a representative episode. Nothing is persisted; a failure is silent on
/// cards.
fn technical_batch(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let mut seen = HashSet::new();
    let ids = request
        .json()
        .get("ids")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .filter(|id| seen.insert(id.to_string()))
                .take(100)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return ApiResponse::ok(json!({ "items": [] }));
    }
    // Series and Season cards advertise the same badges as movies, but their
    // rows are containers Jellyfin reports without streams. Each is answered
    // by a representative cached episode; a container with no cached episode
    // is dropped rather than queried uselessly.
    let sources = match services.library.technical_stream_sources(&ids) {
        Ok(sources) => sources,
        Err(error) => return storage_failure(&error),
    };
    let mut cards_by_source: HashMap<String, Vec<String>> = HashMap::new();
    let mut source_ids = Vec::new();
    for (card_id, source_id) in sources {
        let cards = cards_by_source.entry(source_id.clone()).or_default();
        if cards.is_empty() {
            source_ids.push(source_id);
        }
        cards.push(card_id);
    }
    if source_ids.is_empty() {
        return ApiResponse::ok(json!({ "items": [] }));
    }
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let mut results = Vec::new();
    for chunk in source_ids.chunks(TECHNICAL_BATCH_SIZE) {
        // The UI aborts a batch once none of its cards remains mounted; the
        // fetch dies in the browser, but this handler is already blocked in
        // upstream calls. Checking between chunks keeps rapid scrolling from
        // running every remaining request for an answer nobody will read.
        if request.is_cancelled() {
            return ApiResponse::error(499, "the browser abandoned the request");
        }
        match items::fetch_media_stream_batch(&client, &user_id, chunk) {
            Ok(response) => {
                for dto in &response.items {
                    let streams = technical_media_streams_json(&dto.media_streams);
                    for card_id in cards_by_source.get(&dto.id).into_iter().flatten() {
                        results.push(json!({
                            "id": card_id,
                            "mediaStreams": streams.clone(),
                        }));
                    }
                }
            }
            Err(error) => {
                services.session.note_error(&error);
                return ApiResponse::from_api_error(&error);
            }
        }
    }
    ApiResponse::ok(json!({ "items": results }))
}

/// Container, codec, and track detail for the detail page.
///
/// Folders have no streams of their own, so they are answered from here without
/// a round trip rather than letting the server return an empty source list.
fn media_info(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    if matches!(
        services.library.kind(item_id).as_deref(),
        Some("Series" | "Season")
    ) {
        return ApiResponse::ok(json!({
            "sources": [],
            "playbackPreference": Value::Null,
        }));
    }
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_media_sources(&client, &user_id, item_id) {
        Ok(sources) => {
            let preference = services
                .session
                .account_key()
                .and_then(|account| services.playback_preferences.get(&account, item_id));
            ApiResponse::ok(json!({
                "sources": sources.iter().map(media_source_json).collect::<Vec<_>>(),
                "playbackPreference": resolve_playback_preference(preference.as_ref(), &sources),
            }))
        }
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

/// Saves one complete source/audio/subtitle selection for an exact item.
///
/// The browser only submits indices. Metadata snapshots are rebuilt from the
/// current Jellyfin response here, so persisted language/accessibility intent
/// cannot drift from the track the user actually selected.
fn set_item_playback_preference(
    services: &Arc<Services>,
    item_id: &str,
    request: &ApiRequest,
) -> ApiResponse {
    if matches!(
        services.library.kind(item_id).as_deref(),
        None | Some("Series" | "Season")
    ) {
        return ApiResponse::error(404, "this item has no selectable media tracks");
    }
    let body = request.json();
    let Some(source_index) = body["mediaSourceIndex"]
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
    else {
        return ApiResponse::error(400, "mediaSourceIndex is required");
    };
    let requested_source_id = body["mediaSourceId"]
        .as_str()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let sources = match items::fetch_media_sources(&client, &user_id, item_id) {
        Ok(sources) => sources,
        Err(error) => {
            services.session.note_error(&error);
            return ApiResponse::from_api_error(&error);
        }
    };
    let Some(source) = sources
        .get(source_index)
        .filter(|source| match requested_source_id {
            Some(id) => source.id.as_deref() == Some(id),
            None => source.id.as_deref().is_none_or(str::is_empty),
        })
    else {
        return ApiResponse::error(409, "the available media sources changed; try again");
    };

    let requested_audio_index = body["audioStreamIndex"].as_i64();
    let audio = match requested_audio_index {
        Some(index) if index >= 0 => source
            .streams_of_type("Audio")
            .find(|stream| stream.index == index),
        Some(_) => return ApiResponse::error(400, "audioStreamIndex must not be negative"),
        None if source.streams_of_type("Audio").next().is_none() => None,
        None => return ApiResponse::error(400, "audioStreamIndex is required for this source"),
    };
    if requested_audio_index.is_some() && audio.is_none() {
        return ApiResponse::error(409, "the selected audio track is no longer available");
    }

    let requested_subtitle_index = body["subtitleStreamIndex"].as_i64();
    let subtitle = match requested_subtitle_index {
        Some(index) if index >= 0 => source
            .streams_of_type("Subtitle")
            .find(|stream| stream.index == index),
        Some(_) => return ApiResponse::error(400, "subtitleStreamIndex must not be negative"),
        None => None,
    };
    if requested_subtitle_index.is_some() && subtitle.is_none() {
        return ApiResponse::error(409, "the selected subtitle track is no longer available");
    }

    let preference = ItemPlaybackPreference::capture(source, source_index, audio, subtitle);
    let Some(account) = services.session.account_key() else {
        return ApiResponse::error(401, "sign in to save playback preferences");
    };
    if let Err(error) = services
        .playback_preferences
        .save(&account, item_id, &preference)
    {
        tracing::warn!(target: "app.api", "could not save playback preference: {error}");
        return ApiResponse::error(500, format!("could not save playback preference: {error}"));
    }
    ApiResponse::ok(json!({
        "playbackPreference": resolve_playback_preference(Some(&preference), &sources),
    }))
}

/// The first local trailer attached to an item, if the server has one.
fn trailer_info(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    match items::fetch_local_trailers(&client, &user_id, item_id) {
        Ok(trailers) => match trailers
            .into_iter()
            .find(|trailer| !trailer.id.trim().is_empty())
        {
            Some(trailer) => ApiResponse::ok(json!({
                "trailer": {
                    "id": trailer.id,
                    "name": trailer.display_name(),
                    "embedUrl": Value::Null,
                }
            })),
            None => remote_trailer_info(services, &client, &user_id, item_id),
        },
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

fn remote_trailer_info(
    services: &Arc<Services>,
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) -> ApiResponse {
    match items::fetch_remote_trailers(client, user_id, item_id) {
        Ok(trailers) => {
            let trailer = trailers.into_iter().find_map(|trailer| {
                youtube_embed_url(&trailer.url).map(|embed_url| {
                    json!({
                        "id": Value::Null,
                        "name": trailer.name.unwrap_or_else(|| "Trailer".to_string()),
                        "embedUrl": embed_url,
                    })
                })
            });
            ApiResponse::ok(json!({ "trailer": trailer }))
        }
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

/// Converts only a plain YouTube watch/share/embed URL into the privacy-enhanced
/// player. Jellyfin metadata is external input, so it never becomes an iframe
/// address verbatim.
fn youtube_embed_url(value: &str) -> Option<String> {
    let value = value.trim();
    let prefixes = [
        "https://www.youtube.com/watch?v=",
        "https://youtube.com/watch?v=",
        "https://m.youtube.com/watch?v=",
        "https://youtu.be/",
        "https://www.youtube.com/embed/",
        "https://www.youtube-nocookie.com/embed/",
    ];
    let remainder = prefixes
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))?;
    let id = remainder.split(['&', '?', '#', '/']).next()?;
    if id.len() != 11
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    Some(format!(
        "https://www.youtube-nocookie.com/embed/{id}?autoplay=1&mute=1&controls=0&disablekb=1&enablejsapi=1&fs=0&iv_load_policy=3&modestbranding=1&playsinline=1&rel=0&showinfo=0&start=5"
    ))
}

/// Authenticated, byte-range-aware access to one local trailer.
///
/// The UI receives only an opaque item id. The Jellyfin token stays in the
/// native client, just as it does for artwork and full playback.
fn trailer_stream(services: &Arc<Services>, trailer_id: &str, request: &ApiRequest) -> ApiResponse {
    let client = match services.session.client() {
        Ok(client) => client,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let path = format!("/Videos/{}/stream", encode_path_segment(trailer_id));
    let range = bounded_byte_range(request.range.as_deref());
    match client.get_bytes_range(&path, &[("static", "true".to_string())], Some(&range)) {
        Ok(response) => ApiResponse::ranged_bytes(
            response.status,
            response.content_type,
            response.body,
            response.content_range,
            response.accept_ranges,
        ),
        Err(error) => {
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

/// Accept one simple `bytes=` range and cap it to a small proxy chunk.
fn bounded_byte_range(value: Option<&str>) -> String {
    let fallback = format!("bytes=0-{}", TRAILER_CHUNK_SIZE - 1);
    let Some(spec) = value
        .and_then(|value| value.trim().strip_prefix("bytes="))
        .filter(|value| !value.contains(','))
    else {
        return fallback;
    };
    let Some((start, end)) = spec.split_once('-') else {
        return fallback;
    };
    if let Ok(start) = start.parse::<u64>() {
        let max_end = start.saturating_add(TRAILER_CHUNK_SIZE - 1);
        let end = if end.is_empty() {
            max_end
        } else {
            match end.parse::<u64>() {
                Ok(end) if end >= start => end.min(max_end),
                _ => return fallback,
            }
        };
        return format!("bytes={start}-{end}");
    }
    if start.is_empty()
        && let Ok(suffix) = end.parse::<u64>()
        && suffix > 0
    {
        return format!("bytes=-{}", suffix.min(TRAILER_CHUNK_SIZE));
    }
    fallback
}

fn media_source_json(source: &MediaSourceInfo) -> Value {
    let streams = |kind: &str| {
        source
            .streams_of_type(kind)
            .map(media_stream_json)
            .collect::<Vec<_>>()
    };
    json!({
        "id": source.id,
        "name": source.display_name(),
        "container": source.container,
        // Only the file name: the server's directory layout is not the UI's
        // business, but "which file is this" is exactly what this page is for.
        // Jellyfin answers non-admin users with an opaque id in `Path`, which
        // `file_name_of` filters out — the source name carries the release
        // there, and the UI falls back to it.
        "fileName": source.path.as_deref().and_then(file_name_of),
        "size": source.size,
        "bitrate": source.bitrate,
        "defaultAudioStreamIndex": source.default_audio_stream_index,
        "defaultSubtitleStreamIndex": source.default_subtitle_stream_index,
        "video": streams("video"),
        "audio": streams("audio"),
        "subtitles": streams("subtitle"),
    })
}

fn media_stream_json(stream: &MediaStream) -> Value {
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
        "audioSpatialFormat": stream.audio_spatial_format,
        "videoRange": stream.video_range,
        "videoRangeType": stream.video_range_type,
        "bitDepth": stream.bit_depth,
        "isDefault": stream.is_default,
        "isForced": stream.is_forced,
        "isHearingImpaired": stream.is_hearing_impaired,
        "isExternal": stream.is_external,
    })
}

/// The basename of a real file path.
///
/// `Path` is admin-only in Jellyfin; ordinary users get an opaque id there
/// instead, so a name with no extension is not a file name and is dropped
/// rather than printed as if it were one.
fn file_name_of(path: &str) -> Option<&str> {
    let name = path.rsplit(['/', '\\']).next()?;
    let extension = name.rsplit_once('.')?.1;
    (!extension.is_empty() && extension.len() <= 5 && extension.chars().all(char::is_alphanumeric))
        .then_some(name)
}

/// The episode a series' Play button should start.
///
/// Next Up is server-side logic, exactly as on the home screen, so a series
/// page agrees with the Next Up row. It runs out on a fully watched show and is
/// unavailable offline, and in both cases the first episode is a better answer
/// than a page with nothing to play.
fn next_up(services: &Arc<Services>, item_id: &str) -> ApiResponse {
    if services.library.kind(item_id).as_deref() != Some("Series") {
        return ApiResponse::ok(json!({ "item": Value::Null }));
    }
    let from_server = match services.session.client_and_user() {
        Ok((client, user_id)) => items::fetch_next_up(&client, &user_id, Some(item_id), 1)
            .map(|response| response.items.first().map(summary_from_dto))
            .unwrap_or_else(|error| {
                services.session.note_error(&error);
                tracing::debug!(target: "app.api", "Next Up unavailable for {item_id}: {error}");
                None
            }),
        Err(_) => None,
    };
    let item = match from_server {
        Some(item) => Some(item),
        None => services.library.first_episode(item_id).unwrap_or_default(),
    };
    ApiResponse::ok(json!({ "item": item }))
}

#[derive(Clone, Copy)]
enum ExternalProvider {
    Imdb,
    Tmdb,
    Tvdb,
    Letterboxd,
    Trakt,
}

impl ExternalProvider {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "imdb" => Self::Imdb,
            "tmdb" => Self::Tmdb,
            "tvdb" => Self::Tvdb,
            "letterboxd" => Self::Letterboxd,
            "trakt" => Self::Trakt,
            _ => return None,
        })
    }

    const fn id_field(self) -> &'static str {
        match self {
            Self::Imdb | Self::Trakt => "imdb",
            Self::Tmdb | Self::Letterboxd => "tmdb",
            Self::Tvdb => "tvdb",
        }
    }

    fn url(self, id: &str, kind: &str) -> Option<String> {
        if !valid_external_id(self.id_field(), id) {
            return None;
        }
        Some(match (self, kind) {
            // IMDb keeps movies, series, and episodes under one `/title/` route.
            (Self::Imdb, "Movie" | "Series" | "Episode") => {
                format!("https://www.imdb.com/title/{id}/")
            }
            (Self::Tmdb, "Movie") => format!("https://www.themoviedb.org/movie/{id}"),
            (Self::Tmdb, "Series") => format!("https://www.themoviedb.org/tv/{id}"),
            (Self::Tvdb, "Movie") => format!("https://thetvdb.com/dereferrer/movie/{id}"),
            (Self::Tvdb, "Series") => format!("https://thetvdb.com/dereferrer/series/{id}"),
            (Self::Tvdb, "Episode") => {
                format!("https://thetvdb.com/dereferrer/episode/{id}")
            }
            (Self::Letterboxd, "Movie") => format!("https://letterboxd.com/tmdb/{id}"),
            (Self::Trakt, "Movie") => format!("https://trakt.tv/movies/{id}"),
            (Self::Trakt, "Series") => format!("https://trakt.tv/shows/{id}"),
            _ => return None,
        })
    }
}

/// Opens an item's exact provider page in the default browser.
///
/// The UI names a provider, never a URL: the id and the item kind both come
/// from the cached row here, so nothing the page can say turns into a launched
/// address.
fn open_external(services: &Arc<Services>, item_id: &str, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let Some(provider) = body["provider"].as_str().and_then(ExternalProvider::parse) else {
        return ApiResponse::error(404, "unknown external information provider");
    };
    let item = match services.library.item(item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return ApiResponse::error(404, "no cached item with that id"),
        Err(error) => return storage_failure(&error),
    };
    let kind = item["kind"].as_str().unwrap_or_default();
    let id = item["providerIds"][provider.id_field()]
        .as_str()
        .unwrap_or("");
    let Some(url) = provider.url(id, kind) else {
        return ApiResponse::error(404, "this item has no id for that database");
    };
    super::super::bridge::open_external_link(&url);
    ApiResponse::ok(json!({ "opened": true, "url": url }))
}

fn valid_external_id(source: &str, id: &str) -> bool {
    if id.is_empty() || id.len() > 32 {
        return false;
    }
    match source {
        "imdb" => id.strip_prefix("tt").is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        }),
        "tmdb" | "tvdb" => {
            id.bytes().all(|byte| byte.is_ascii_digit()) && id.bytes().any(|byte| byte != b'0')
        }
        _ => false,
    }
}

fn set_played(services: &Arc<Services>, item_id: &str, request: &ApiRequest) -> ApiResponse {
    let played = request.json()["played"].as_bool().unwrap_or(true);
    if let Err(response) = user_data_write(services, item_id, |client, user_id| {
        items::set_played(client, user_id, item_id, played)
    }) {
        return response;
    }
    let _ = services.library.set_local_played(item_id, played);
    ApiResponse::ok(json!({ "played": played }))
}

fn set_favorite(services: &Arc<Services>, item_id: &str, request: &ApiRequest) -> ApiResponse {
    let favorite = request.json()["favorite"].as_bool().unwrap_or(true);
    if let Err(response) = user_data_write(services, item_id, |client, user_id| {
        items::set_favorite(client, user_id, item_id, favorite)
    }) {
        return response;
    }
    let _ = services.library.set_local_favorite(item_id, favorite);
    ApiResponse::ok(json!({ "favorite": favorite }))
}

/// The server is the source of truth for watch state, so it is written first
/// and the local mirror only follows a success.
fn user_data_write(
    services: &Arc<Services>,
    item_id: &str,
    write: impl FnOnce(&JellyfinClient, &str) -> Result<(), ApiError>,
) -> Result<(), ApiResponse> {
    let (client, user_id) = services
        .session
        .client_and_user()
        .map_err(|error| ApiResponse::from_api_error(&error))?;
    write(&client, &user_id).map_err(|error| {
        services.session.note_error(&error);
        tracing::warn!(target: "app.api", item_id, "user-data write failed: {error}");
        ApiResponse::from_api_error(&error)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn external_url(provider: &str, id: &str, kind: &str) -> Option<String> {
        ExternalProvider::parse(provider)?.url(id, kind)
    }

    #[test]
    fn trailer_ranges_are_bounded_and_malformed_ranges_fall_back() {
        assert_eq!(bounded_byte_range(None), "bytes=0-4194303");
        assert_eq!(bounded_byte_range(Some("bytes=100-")), "bytes=100-4194403");
        assert_eq!(
            bounded_byte_range(Some("bytes=100-9999999")),
            "bytes=100-4194403"
        );
        assert_eq!(bounded_byte_range(Some("bytes=100-200")), "bytes=100-200");
        assert_eq!(bounded_byte_range(Some("bytes=-9999999")), "bytes=-4194304");
        assert_eq!(bounded_byte_range(Some("not-a-range")), "bytes=0-4194303");
    }

    #[test]
    fn only_plain_youtube_trailers_become_privacy_enhanced_embeds() {
        assert_eq!(
            youtube_embed_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ&feature=share"),
            Some(
                "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?autoplay=1&mute=1&controls=0&disablekb=1&enablejsapi=1&fs=0&iv_load_policy=3&modestbranding=1&playsinline=1&rel=0&showinfo=0&start=5"
                    .to_string()
            )
        );
        assert!(youtube_embed_url("https://youtu.be/dQw4w9WgXcQ").is_some());
        assert!(youtube_embed_url("http://www.youtube.com/watch?v=dQw4w9WgXcQ").is_none());
        assert!(youtube_embed_url("https://example.com/embed/dQw4w9WgXcQ").is_none());
        assert!(youtube_embed_url("https://youtu.be/not-valid").is_none());
    }

    #[test]
    fn external_links_are_built_per_provider_and_kind() {
        assert_eq!(
            ExternalProvider::parse("letterboxd").map(ExternalProvider::id_field),
            Some("tmdb")
        );
        assert_eq!(
            ExternalProvider::parse("trakt").map(ExternalProvider::id_field),
            Some("imdb")
        );
        assert_eq!(
            external_url("imdb", "tt0133093", "Movie").as_deref(),
            Some("https://www.imdb.com/title/tt0133093/")
        );
        assert_eq!(
            external_url("tmdb", "603", "Movie").as_deref(),
            Some("https://www.themoviedb.org/movie/603")
        );
        assert_eq!(
            external_url("tmdb", "95396", "Series").as_deref(),
            Some("https://www.themoviedb.org/tv/95396")
        );
        assert_eq!(
            external_url("tvdb", "371980", "Episode").as_deref(),
            Some("https://thetvdb.com/dereferrer/episode/371980")
        );
        assert_eq!(
            external_url("letterboxd", "603", "Movie").as_deref(),
            Some("https://letterboxd.com/tmdb/603")
        );
        assert_eq!(
            external_url("trakt", "tt0133093", "Movie").as_deref(),
            Some("https://trakt.tv/movies/tt0133093")
        );
        assert_eq!(
            external_url("trakt", "tt11280740", "Series").as_deref(),
            Some("https://trakt.tv/shows/tt11280740")
        );
    }

    /// An episode's TMDb id is an episode id, so there is no `/tv/` page for it
    /// and offering one would link an unrelated show.
    #[test]
    fn external_links_are_absent_where_the_id_does_not_address_a_page() {
        assert_eq!(external_url("tmdb", "12345", "Episode"), None);
        assert_eq!(external_url("tmdb", "12345", "Season"), None);
        assert_eq!(external_url("tvdb", "12345", "Season"), None);
        assert_eq!(external_url("letterboxd", "95396", "Series"), None);
        assert_eq!(external_url("trakt", "tt0133093", "Episode"), None);
        assert_eq!(external_url("imdb", "", "Movie"), None);
        assert_eq!(external_url("trakt", "12345", "Movie"), None);
    }

    /// Ids reach `open_external_link` as a path segment, so anything that is
    /// not a plain token must not produce a URL at all.
    #[test]
    fn external_links_reject_ids_that_are_not_plain_tokens() {
        let too_long = "a".repeat(33);
        for id in ["tt1/../evil", "603?x=1", "603#f", "603 604", &too_long] {
            assert_eq!(external_url("imdb", id, "Movie"), None, "id {id}");
        }
        assert_eq!(external_url("imdb", "603", "Movie"), None);
        assert_eq!(external_url("tmdb", "tt0133093", "Movie"), None);
        assert_eq!(external_url("tmdb", "000", "Movie"), None);
    }

    #[test]
    fn media_source_paths_are_reduced_to_a_file_name() {
        assert_eq!(
            file_name_of("/mnt/media/Movies/The Matrix (1999)/matrix.mkv"),
            Some("matrix.mkv")
        );
        assert_eq!(
            file_name_of(r"D:\Media\Movies\matrix.mkv"),
            Some("matrix.mkv")
        );
    }

    /// Non-admin users get an opaque id in `Path`, which must not be printed as
    /// if it were the name of a file.
    #[test]
    fn media_source_paths_that_are_not_file_names_are_dropped() {
        assert_eq!(file_name_of("6967c5ef-2daf-4951-9d03-38db9a7f5351"), None);
        assert_eq!(file_name_of("/mnt/media/Movies/The Matrix (1999)"), None);
        assert_eq!(file_name_of(""), None);
    }

    #[test]
    fn media_sources_are_flattened_into_video_audio_and_subtitle_lists() {
        let source: MediaSourceInfo = serde_json::from_str(
            r#"{"Id":"src","Name":"matrix","Container":"mkv","Size":123,"Bitrate":456,
                "Path":"/mnt/matrix.mkv",
                "MediaStreams":[
                    {"Index":0,"Type":"Video","Codec":"hevc","Width":3840,"Height":2160},
                    {"Index":1,"Type":"Audio","Codec":"dts","Channels":6,"IsDefault":true},
                    {"Index":2,"Type":"Subtitle","Codec":"subrip","Language":"eng",
                     "IsHearingImpaired":true,"IsExternal":true}]}"#,
        )
        .expect("source");
        let value = media_source_json(&source);
        assert_eq!(value["container"], "mkv");
        assert_eq!(value["fileName"], "matrix.mkv");
        assert_eq!(value["video"][0]["height"], 2160);
        assert_eq!(value["audio"][0]["channels"], 6);
        assert_eq!(value["subtitles"][0]["isExternal"], true);
        assert_eq!(value["subtitles"][0]["isHearingImpaired"], true);
        assert_eq!(value["video"].as_array().map(Vec::len), Some(1));
    }
}
