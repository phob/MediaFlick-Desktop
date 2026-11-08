use crate::books::{
    self, BookFormat, BookResume, ProgressUnits, ReaderAppearance, ReadingLocation, SavedPosition,
};
use crate::jellyfin::api::client::ByteResponse;
use crate::jellyfin::api::model::BaseItemDto;
use crate::library::browse::is_book;
use crate::library::model::{BookDetail, GenericItemDetail};

use super::*;

/// The most one answer of a book's bytes carries. The reader reads a book by
/// ranges, so a 300 MB comic never sits whole in this process.
const BOOK_CHUNK_SIZE: u64 = 8 * 1024 * 1024;
/// How much of each end of a file format sniffing reads.
const SNIFF_WINDOW: u64 = 64 * 1024;

routes! {
    |services, request|
    GET ["item", id, "book"] -> BookManifest = book_manifest(services, id, request),
    GET ["item", id, "book", "file"] -> Binary = book_bytes(services, id, request),
    PUT ["item", id, "book", "position"] (body: BookPositionWrite) -> BookPositionSaved =
        save_position(services, id, body),
    PUT ["reader", "appearance"] (body: ReaderAppearance) -> ReaderAppearance =
        save_appearance(services, body),
}

/// What the reader needs to open a book: the format its bytes are, where to
/// open it, and how to show it.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct BookManifest {
    title: String,
    format: BookFormat,
    resume: BookResume,
    appearance: ReaderAppearance,
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BookPositionWrite {
    location: ReadingLocation,
    /// The book's last content is on screen: Jellyfin marks it played.
    finished: bool,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct BookPositionSaved {
    /// `false` when only this app kept the position because Jellyfin's write
    /// failed; the next save writes it again.
    synced: bool,
}

/// `?from=start` opens the book at its beginning (Read again).
fn book_manifest(
    services: &Arc<Services>,
    item_id: &str,
    request: &ApiRequest,
) -> Result<BookManifest, ApiResponse> {
    let scope = session_scope(services)?;
    let item = fetch_current(services, &scope, |client, user_id| {
        items::fetch_detailed_item(client, user_id, item_id)
    })?
    .filter(is_book)
    .ok_or_else(|| ApiResponse::error(404, "the server has no book with that id"))?;
    let head = read_range(
        services,
        &scope,
        item_id,
        &format!("bytes=0-{}", SNIFF_WINDOW - 1),
    )?;
    let size = file_size(&head)?;
    let tail = if size > SNIFF_WINDOW {
        read_range(services, &scope, item_id, &format!("bytes=-{SNIFF_WINDOW}"))?.body
    } else {
        Vec::new()
    };
    let format = BookFormat::sniff(&head.body, &tail, item.path.as_deref().unwrap_or_default())
        .ok_or_else(|| ApiResponse::error(415, "the reader cannot open this book's file"))?;
    let server_ticks = item
        .user_data
        .as_ref()
        .map_or(0, |data| data.playback_position_ticks);
    let from_start = request.param("from").as_deref() == Some("start");
    services
        .session
        .commit_if_current(&scope, stale_account_response, |live| {
            let saved = services.reader.position(live.account(), item_id);
            Ok(BookManifest {
                title: item.display_name().to_string(),
                format,
                resume: if from_start {
                    BookResume::Start
                } else {
                    books::resume(saved.as_ref(), server_ticks, format.units())
                },
                appearance: services.reader.appearance(live.account()),
            })
        })
}

/// A book's detail answer: how far this account has read it, from the same
/// merge the reader opens with, in the units the file's name gives.
pub(super) fn book_detail(
    services: &Arc<Services>,
    scope: &SessionScope,
    item: &BaseItemDto,
) -> Result<BookDetail, ApiResponse> {
    let (server_ticks, played) = item.user_data.as_ref().map_or((0, false), |data| {
        (data.playback_position_ticks, data.played)
    });
    let units = item
        .path
        .as_deref()
        .and_then(BookFormat::from_name)
        .map_or(ProgressUnits::Fraction, BookFormat::units);
    services
        .session
        .commit_if_current(scope, stale_account_response, |live| {
            let saved = services.reader.position(live.account(), &item.id);
            Ok(BookDetail {
                item: GenericItemDetail::from_dto(item),
                reading: books::reading_state(
                    saved.as_ref(),
                    server_ticks,
                    played,
                    units,
                    item.run_time_ticks,
                ),
            })
        })
}

fn read_range(
    services: &Arc<Services>,
    scope: &SessionScope,
    item_id: &str,
    range: &str,
) -> Result<ByteResponse, ApiResponse> {
    // ureq fails a read that reaches its limit, so one byte more lets a full
    // chunk through and still stops any longer answer.
    let response = fetch_current(services, scope, |client, _| {
        items::fetch_book_bytes(client, item_id, range, BOOK_CHUNK_SIZE + 1)
    })?;
    if response.status != 206 || response.content_range.is_none() {
        return Err(ApiResponse::error(
            502,
            format!(
                "Jellyfin answered a book's byte range with HTTP {} and no Content-Range",
                response.status
            ),
        ));
    }
    Ok(response)
}

fn file_size(response: &ByteResponse) -> Result<u64, ApiResponse> {
    response
        .content_range
        .as_deref()
        .and_then(|range| range.rsplit_once('/'))
        .and_then(|(_, total)| total.trim().parse().ok())
        .ok_or_else(|| ApiResponse::error(502, "Jellyfin did not say how long the book's file is"))
}

/// Bytes `start..=end` of a book's file. A request without both, or a
/// longer one, gets at most `BOOK_CHUNK_SIZE` bytes from where it starts, as
/// a 206 whose `Content-Range` names the file's size.
///
/// The range comes in the query, not a `Range` header: CEF answers a
/// request's `Range` itself by skipping that far into the handler's answer,
/// which here already is the range, so every read past the first chunk
/// would fail as unsatisfiable.
fn book_bytes(
    services: &Arc<Services>,
    item_id: &str,
    request: &ApiRequest,
) -> Result<BinaryAnswer, ApiResponse> {
    let scope = session_scope(services)?;
    let requested = request
        .param("start")
        .zip(request.param("end"))
        .map(|(start, end)| format!("bytes={start}-{end}"));
    let range = bounded_byte_range(requested.as_deref(), BOOK_CHUNK_SIZE);
    let response = read_range(services, &scope, item_id, &range)?;
    Ok(BinaryAnswer::ranged_bytes(
        response.status,
        response.content_type,
        response.body,
        response.content_range,
        response.accept_ranges,
    ))
}

/// Keeps the exact place locally first, then writes Jellyfin's position in
/// jellyfin-web's units; once Jellyfin has it, the local record notes it as
/// synced. A failed Jellyfin write keeps the place and answers
/// `synced: false`; the reader's next save writes it again.
fn save_position(
    services: &Arc<Services>,
    item_id: &str,
    body: Body<'_, BookPositionWrite>,
) -> Result<BookPositionSaved, ApiResponse> {
    let body = body.decode()?;
    if !body.location.is_valid() {
        return Err(ApiResponse::error(400, "the page is not inside the book"));
    }
    let scope = session_scope(services)?;
    let ticks = body.location.jellyfin_ticks();
    let keep = |synced_ticks: Option<i64>| {
        services
            .session
            .commit_if_current(&scope, stale_account_response, |live| {
                let synced_ticks = synced_ticks
                    .or_else(|| {
                        services
                            .reader
                            .position(live.account(), item_id)
                            .map(|previous| previous.synced_ticks)
                    })
                    .unwrap_or(0);
                let position = SavedPosition {
                    location: body.location.clone(),
                    synced_ticks,
                };
                services
                    .reader
                    .save_position(live, item_id, position)
                    .map_err(|error| {
                        ApiResponse::error(
                            500,
                            format!("could not keep the place in the book: {error}"),
                        )
                    })
            })
    };
    keep(None)?;
    let played = body.finished.then_some(true);
    let written = scope.fetch(|client, user_id| {
        items::write_book_progress(client, user_id, item_id, ticks, played)
    });
    match written {
        Ok(written) => {
            confirmed(services, written)?;
            keep(Some(ticks))?;
            Ok(BookPositionSaved { synced: true })
        }
        Err(error) => {
            let error = services.session.scoped_error(&scope, error);
            if matches!(error, ApiError::AccountChanged | ApiError::Unauthorized) {
                return Err(ApiResponse::from_api_error(&error));
            }
            tracing::warn!(target: "app.api", item_id, "kept a book's place only locally: {error}");
            Ok(BookPositionSaved { synced: false })
        }
    }
}

fn save_appearance(
    services: &Arc<Services>,
    body: Body<'_, ReaderAppearance>,
) -> Result<ReaderAppearance, ApiResponse> {
    let appearance = body.decode()?;
    let scope = session_scope(services)?;
    services
        .session
        .commit_if_current(&scope, stale_account_response, |live| {
            services
                .reader
                .save_appearance(live, appearance)
                .map(|()| appearance)
                .map_err(|error| {
                    ApiResponse::error(
                        500,
                        format!("could not keep the reader's appearance: {error}"),
                    )
                })
        })
}
