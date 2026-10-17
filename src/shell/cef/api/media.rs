use crate::library::model::ResolvedPlaybackPreference;

use super::*;

routes! {
    |services, request|
    POST ["technical", "batch"] (body: IdsBody) -> TechnicalBatch =
        technical_batch(services, body, request),
    GET ["item", id, "media"] -> MediaInfoResponse = media_info(services, id),
    PATCH ["item", id, "playback-preference"] (body: PlaybackPreferenceBody) ->
        PlaybackPreferenceSaved = set_item_playback_preference(services, id, body),
    GET ["item", id, "trailer"] -> TrailerResponse = trailer_info(services, id),
    GET ["item", id, "nextup"] -> NextUpResponse = next_up(services, id),
    POST ["item", id, "external"] (body: ExternalBody) -> ExternalLinkOpened =
        open_external(services, id, body),
    POST ["item", id, "played"] (body: PlayedBody) -> PlayedState = set_played(services, id, body),
    POST ["item", id, "favorite"] (body: FavoriteBody) -> FavoriteState =
        set_favorite(services, id, body),
    GET ["trailer", id, "stream"] -> Binary =
        trailer_stream(services, &percent_decode(id), request),
}

const TECHNICAL_BATCH_SIZE: usize = 40;

#[derive(Deserialize, ts_rs::TS)]
#[ts(rename = "TechnicalBatchBody")]
pub(super) struct IdsBody {
    ids: Vec<String>,
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "PlaybackTrackPreferenceWrite")]
pub(super) struct PlaybackPreferenceBody {
    media_source_index: usize,
    media_source_id: Option<String>,
    audio_stream_index: Option<i64>,
    subtitle_stream_index: Option<i64>,
}

#[derive(Deserialize, ts_rs::TS)]
#[ts(rename = "ExternalLinkBody")]
pub(super) struct ExternalBody {
    provider: String,
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct PlayedBody {
    played: bool,
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct FavoriteBody {
    favorite: bool,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct TechnicalBatch {
    items: Vec<ItemTechnical>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct ItemTechnical {
    id: String,
    media_streams: Vec<MediaStreamView>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct MediaInfoResponse {
    sources: Vec<MediaSourceView>,
    playback_preference: Option<ResolvedPlaybackPreference>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlaybackPreferenceSaved {
    playback_preference: Option<ResolvedPlaybackPreference>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct MediaSourceView {
    id: Option<String>,
    name: String,
    container: Option<String>,
    file_name: Option<String>,
    size: Option<i64>,
    bitrate: Option<i64>,
    default_audio_stream_index: Option<i64>,
    default_subtitle_stream_index: Option<i64>,
    video: Vec<MediaStreamView>,
    audio: Vec<MediaStreamView>,
    subtitles: Vec<MediaStreamView>,
}

#[derive(Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct MediaStreamView {
    index: i64,
    #[serde(rename = "type")]
    stream_type: Option<String>,
    codec: Option<String>,
    profile: Option<String>,
    language: Option<String>,
    title: Option<String>,
    display_title: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    channels: Option<i64>,
    audio_spatial_format: Option<String>,
    video_range: Option<String>,
    video_range_type: Option<String>,
    bit_depth: Option<i64>,
    is_default: bool,
    is_forced: bool,
    is_hearing_impaired: bool,
    is_external: bool,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct TrailerResponse {
    trailer: Option<TrailerSummary>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct TrailerSummary {
    id: Option<String>,
    name: String,
    embed_url: Option<String>,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct NextUpResponse {
    item: Option<ItemSummary>,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct ExternalLinkOpened {
    pub(super) opened: bool,
    pub(super) url: String,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct PlayedState {
    played: bool,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct FavoriteState {
    favorite: bool,
}

/// Live technical stream descriptors for visible cards, batched by the UI's
/// badge scheduler. Container ids (Series, Season) answer with the streams of
/// a representative episode. Nothing is persisted; a failure is silent on
/// cards.
fn technical_batch(
    services: &Arc<Services>,
    body: Body<'_, IdsBody>,
    request: &ApiRequest,
) -> Result<TechnicalBatch, ApiResponse> {
    let body = body.decode()?;
    let mut seen = HashSet::new();
    let ids = body
        .ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .filter(|id| seen.insert(id.to_string()))
        .take(100)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(TechnicalBatch { items: Vec::new() });
    }
    // Series and Season cards advertise the same badges as movies, but their
    // rows are containers Jellyfin reports without streams. Each is answered
    // by a representative cached episode; a container with no cached episode
    // is dropped rather than queried uselessly.
    let sources = match services.library.technical_stream_sources(&ids) {
        Ok(sources) => sources,
        Err(error) => return Err(storage_failure(&error)),
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
        return Ok(TechnicalBatch { items: Vec::new() });
    }
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    let mut results = Vec::new();
    for chunk in source_ids.chunks(TECHNICAL_BATCH_SIZE) {
        // The UI aborts a batch once none of its cards remains mounted; the
        // fetch dies in the browser, but this handler is already blocked in
        // upstream calls. Checking between chunks keeps rapid scrolling from
        // running every remaining request for an answer nobody will read.
        if request.is_cancelled() {
            return Err(ApiResponse::error(499, "the browser abandoned the request"));
        }
        match items::fetch_media_stream_batch(client, user_id, chunk) {
            Ok(response) => {
                for dto in &response.items {
                    let streams = technical_streams(&dto.media_streams);
                    for card_id in cards_by_source.get(&dto.id).into_iter().flatten() {
                        results.push(ItemTechnical {
                            id: card_id.clone(),
                            media_streams: streams.clone(),
                        });
                    }
                }
            }
            Err(error) => {
                services.session.note_scoped_error(&scope, &error);
                return Err(ApiResponse::from_api_error(&error));
            }
        }
    }
    Ok(TechnicalBatch { items: results })
}

/// Only fields useful to a concise technical readout are retained. Subtitle
/// tracks, paths, delivery URLs, and playback-negotiation flags never reach
/// the card-badge channel; the media endpoint serves those from full sources.
/// Nothing here is persisted — this shapes live DTO streams for the UI.
fn technical_streams(streams: &[MediaStream]) -> Vec<MediaStreamView> {
    streams
        .iter()
        .filter(|stream| {
            stream.stream_type.as_deref().is_some_and(|kind| {
                kind.eq_ignore_ascii_case("video") || kind.eq_ignore_ascii_case("audio")
            })
        })
        .map(MediaStreamView::of)
        .collect()
}

fn cached_kind(services: &Services, item_id: &str) -> Result<Option<CatalogKind>, ApiResponse> {
    services
        .library
        .catalog_kind(item_id)
        .map_err(|error| storage_failure(&error))
}

/// Container, codec, and track detail for the detail page.
///
/// Folders have no streams of their own, so they are answered from here without
/// a round trip rather than letting the server return an empty source list.
fn media_info(services: &Arc<Services>, item_id: &str) -> Result<MediaInfoResponse, ApiResponse> {
    if matches!(
        cached_kind(services, item_id)?,
        Some(CatalogKind::Series | CatalogKind::Season)
    ) {
        return Ok(MediaInfoResponse {
            sources: Vec::new(),
            playback_preference: None,
        });
    }
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    match items::fetch_media_sources(client, user_id, item_id) {
        Ok(sources) => {
            let preference = services
                .session
                .account_key()
                .and_then(|account| services.playback_preferences.get(&account, item_id));
            Ok(MediaInfoResponse {
                sources: sources.iter().map(MediaSourceView::of).collect(),
                playback_preference: resolve_playback_preference(preference.as_ref(), &sources),
            })
        }
        Err(error) => Err(scoped_failure(services, &scope, &error)),
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
    body: Body<'_, PlaybackPreferenceBody>,
) -> Result<PlaybackPreferenceSaved, ApiResponse> {
    if matches!(
        cached_kind(services, item_id)?,
        None | Some(CatalogKind::Series | CatalogKind::Season)
    ) {
        return Err(ApiResponse::error(
            404,
            "this item has no selectable media tracks",
        ));
    }
    let body = body.decode()?;
    let source_index = body.media_source_index;
    let requested_source_id = body
        .media_source_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let scope = session_scope(services)?;
    let sources = match items::fetch_media_sources(scope.client(), scope.user_id(), item_id) {
        Ok(sources) => sources,
        Err(error) => {
            services.session.note_scoped_error(&scope, &error);
            return Err(ApiResponse::from_api_error(&error));
        }
    };
    let Some(source) = sources
        .get(source_index)
        .filter(|source| match requested_source_id {
            Some(id) => source.id.as_deref() == Some(id),
            None => source.id.as_deref().is_none_or(str::is_empty),
        })
    else {
        return Err(ApiResponse::error(
            409,
            "the available media sources changed; try again",
        ));
    };

    let requested_audio_index = body.audio_stream_index;
    let audio = match requested_audio_index {
        Some(index) if index >= 0 => source
            .streams_of_type("Audio")
            .find(|stream| stream.index == index),
        Some(_) => {
            return Err(ApiResponse::error(
                400,
                "audioStreamIndex must not be negative",
            ));
        }
        None if source.streams_of_type("Audio").next().is_none() => None,
        None => {
            return Err(ApiResponse::error(
                400,
                "audioStreamIndex is required for this source",
            ));
        }
    };
    if requested_audio_index.is_some() && audio.is_none() {
        return Err(ApiResponse::error(
            409,
            "the selected audio track is no longer available",
        ));
    }

    let requested_subtitle_index = body.subtitle_stream_index;
    let subtitle = match requested_subtitle_index {
        Some(index) if index >= 0 => source
            .streams_of_type("Subtitle")
            .find(|stream| stream.index == index),
        Some(_) => {
            return Err(ApiResponse::error(
                400,
                "subtitleStreamIndex must not be negative",
            ));
        }
        None => None,
    };
    if requested_subtitle_index.is_some() && subtitle.is_none() {
        return Err(ApiResponse::error(
            409,
            "the selected subtitle track is no longer available",
        ));
    }

    let preference = ItemPlaybackPreference::capture(source, source_index, audio, subtitle);
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            if let Err(error) =
                services
                    .playback_preferences
                    .save(scope.account(), item_id, &preference)
            {
                tracing::warn!(target: "app.api", "could not save playback preference: {error}");
                return Err(ApiResponse::error(
                    500,
                    format!("could not save playback preference: {error}"),
                ));
            }
            Ok(PlaybackPreferenceSaved {
                playback_preference: resolve_playback_preference(Some(&preference), &sources),
            })
        })
}

fn trailer_info(services: &Arc<Services>, item_id: &str) -> Result<TrailerResponse, ApiResponse> {
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    match items::fetch_local_trailers(client, user_id, item_id) {
        Ok(trailers) => match trailers
            .into_iter()
            .find(|trailer| !trailer.id.trim().is_empty())
        {
            Some(trailer) => Ok(TrailerResponse {
                trailer: Some(TrailerSummary {
                    name: trailer.display_name().to_string(),
                    id: Some(trailer.id),
                    embed_url: None,
                }),
            }),
            None => remote_trailer_info(services, &scope, item_id),
        },
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

fn remote_trailer_info(
    services: &Arc<Services>,
    scope: &SessionScope,
    item_id: &str,
) -> Result<TrailerResponse, ApiResponse> {
    match items::fetch_remote_trailers(scope.client(), scope.user_id(), item_id) {
        Ok(trailers) => {
            let trailer = trailers.into_iter().find_map(|trailer| {
                youtube_embed_url(&trailer.url).map(|embed_url| TrailerSummary {
                    id: None,
                    name: trailer.name.unwrap_or_else(|| "Trailer".to_string()),
                    embed_url: Some(embed_url),
                })
            });
            Ok(TrailerResponse { trailer })
        }
        Err(error) => Err(scoped_failure(services, scope, &error)),
    }
}

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
fn trailer_stream(services: &Arc<Services>, trailer_id: &str, request: &ApiRequest) -> Handled {
    let scope = session_scope(services)?;
    let path = format!("/Videos/{}/stream", encode_path_segment(trailer_id));
    let range = bounded_byte_range(request.range.as_deref());
    match scope
        .client()
        .get_bytes_range(&path, &[("static", "true".to_string())], Some(&range))
    {
        Ok(response) => Ok(ApiResponse::ranged_bytes(
            response.status,
            response.content_type,
            response.body,
            response.content_range,
            response.accept_ranges,
        )),
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

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

impl MediaSourceView {
    fn of(source: &MediaSourceInfo) -> Self {
        let streams = |kind: &str| {
            source
                .streams_of_type(kind)
                .map(MediaStreamView::of)
                .collect::<Vec<_>>()
        };
        Self {
            id: source.id.clone(),
            name: source.display_name().to_string(),
            container: source.container.clone(),
            // Only the file name: the server's directory layout is not the UI's
            // business, but "which file is this" is exactly what this page is for.
            // Jellyfin answers non-admin users with an opaque id in `Path`, which
            // `file_name_of` filters out — the source name carries the release
            // there, and the UI falls back to it.
            file_name: source
                .path
                .as_deref()
                .and_then(file_name_of)
                .map(str::to_string),
            size: source.size,
            bitrate: source.bitrate,
            default_audio_stream_index: source.default_audio_stream_index,
            default_subtitle_stream_index: source.default_subtitle_stream_index,
            video: streams("video"),
            audio: streams("audio"),
            subtitles: streams("subtitle"),
        }
    }
}

impl MediaStreamView {
    fn of(stream: &MediaStream) -> Self {
        Self {
            index: stream.index,
            stream_type: stream.stream_type.clone(),
            codec: stream.codec.clone(),
            profile: stream.profile.clone(),
            language: stream.language.clone(),
            title: stream.title.clone(),
            display_title: stream.display_title.clone(),
            width: stream.width,
            height: stream.height,
            channels: stream.channels,
            audio_spatial_format: stream.audio_spatial_format.clone(),
            video_range: stream.video_range.clone(),
            video_range_type: stream.video_range_type.clone(),
            bit_depth: stream.bit_depth,
            is_default: stream.is_default,
            is_forced: stream.is_forced,
            is_hearing_impaired: stream.is_hearing_impaired,
            is_external: stream.is_external,
        }
    }
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
fn next_up(services: &Arc<Services>, item_id: &str) -> Result<NextUpResponse, ApiResponse> {
    if cached_kind(services, item_id)? != Some(CatalogKind::Series) {
        return Ok(NextUpResponse { item: None });
    }
    let from_server = match services.session.scope() {
        Ok(scope) => items::fetch_next_up(scope.client(), scope.user_id(), Some(item_id), 1)
            .map(|response| response.items.first().map(ItemSummary::from_dto))
            .unwrap_or_else(|error| {
                services.session.note_scoped_error(&scope, &error);
                tracing::debug!(target: "app.api", "Next Up unavailable for {item_id}: {error}");
                None
            }),
        Err(_) => None,
    };
    let item = match from_server {
        Some(item) => Some(item),
        None => services.library.first_episode(item_id).unwrap_or_default(),
    };
    Ok(NextUpResponse { item })
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
fn open_external(
    services: &Arc<Services>,
    item_id: &str,
    body: Body<'_, ExternalBody>,
) -> Result<ExternalLinkOpened, ApiResponse> {
    let body = body.decode()?;
    let Some(provider) = ExternalProvider::parse(&body.provider) else {
        return Err(ApiResponse::error(
            404,
            "unknown external information provider",
        ));
    };
    let item = match services.library.item(item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return Err(ApiResponse::error(404, "no cached item with that id")),
        Err(error) => return Err(storage_failure(&error)),
    };
    let kind = item.summary.kind.as_str();
    let id = item.provider_ids.get(provider.id_field()).unwrap_or("");
    let Some(url) = provider.url(id, kind) else {
        return Err(ApiResponse::error(
            404,
            "this item has no id for that database",
        ));
    };
    super::super::bridge::open_external_link(&url);
    Ok(ExternalLinkOpened { opened: true, url })
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

fn set_played(
    services: &Arc<Services>,
    item_id: &str,
    body: Body<'_, PlayedBody>,
) -> Result<PlayedState, ApiResponse> {
    let played = body.decode()?.played;
    let scope = user_data_write(services, item_id, |client, user_id| {
        items::set_played(client, user_id, item_id, played)
    })?;
    let _ = services.session.commit_if_current(
        &scope,
        || (),
        || Ok(services.library.set_local_played(item_id, played)),
    );
    Ok(PlayedState { played })
}

fn set_favorite(
    services: &Arc<Services>,
    item_id: &str,
    body: Body<'_, FavoriteBody>,
) -> Result<FavoriteState, ApiResponse> {
    let favorite = body.decode()?.favorite;
    let scope = user_data_write(services, item_id, |client, user_id| {
        items::set_favorite(client, user_id, item_id, favorite)
    })?;
    let _ = services.session.commit_if_current(
        &scope,
        || (),
        || Ok(services.library.set_local_favorite(item_id, favorite)),
    );
    Ok(FavoriteState { favorite })
}

fn user_data_write(
    services: &Arc<Services>,
    item_id: &str,
    write: impl FnOnce(&JellyfinClient, &str) -> Result<(), ApiError>,
) -> Result<SessionScope, ApiResponse> {
    let scope = session_scope(services)?;
    write(scope.client(), scope.user_id()).map_err(|error| {
        tracing::warn!(target: "app.api", item_id, "user-data write failed: {error}");
        scoped_failure(services, &scope, &error)
    })?;
    Ok(scope)
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
        let embed = youtube_embed_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ&feature=share")
            .expect("plain watch URL");
        assert!(
            embed.starts_with("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?"),
            "{embed}"
        );
        assert!(youtube_embed_url("https://youtu.be/dQw4w9WgXcQ").is_some());
        assert!(youtube_embed_url("http://www.youtube.com/watch?v=dQw4w9WgXcQ").is_none());
        assert!(youtube_embed_url("https://example.com/embed/dQw4w9WgXcQ").is_none());
        assert!(youtube_embed_url("https://youtu.be/not-valid").is_none());
    }

    #[test]
    fn external_links_are_built_per_provider_and_kind() {
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
    fn media_source_paths_that_are_not_file_names_are_dropped() {
        assert_eq!(file_name_of("6967c5ef-2daf-4951-9d03-38db9a7f5351"), None);
        assert_eq!(file_name_of("/mnt/media/Movies/The Matrix (1999)"), None);
        assert_eq!(file_name_of(""), None);
    }
}
