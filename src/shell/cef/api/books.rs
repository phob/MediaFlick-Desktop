use super::*;

/// The most one answer of a book's bytes carries. The reader reads a book by
/// ranges, so a 300 MB comic never sits whole in this process.
const BOOK_CHUNK_SIZE: u64 = 8 * 1024 * 1024;

routes! {
    |services, request|
    GET ["item", id, "book", "file"] -> Binary = book_bytes(services, id, request),
}

/// Bytes `start..=end` of a book's file, read from Jellyfin's
/// `/Items/{id}/File` (which needs only the session, unlike `/Download`). A
/// request without both, or a longer one, gets at most `BOOK_CHUNK_SIZE`
/// bytes from where it starts, as a 206 whose `Content-Range` names the
/// file's size. Jellyfin's answer is read only up to that size, so a server
/// that ignores the range cannot make this buffer the whole file.
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
    let path = format!("/Items/{}/File", encode_path_segment(item_id));
    let requested = request
        .param("start")
        .zip(request.param("end"))
        .map(|(start, end)| format!("bytes={start}-{end}"));
    let range = bounded_byte_range(requested.as_deref(), BOOK_CHUNK_SIZE);
    // ureq fails a read that reaches its limit, so one byte more lets a full
    // chunk through and still stops any longer answer.
    let response = fetch_current(services, &scope, |client, _| {
        client.get_bytes_range(&path, &[], Some(&range), BOOK_CHUNK_SIZE + 1)
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
    Ok(BinaryAnswer::ranged_bytes(
        response.status,
        response.content_type,
        response.body,
        response.content_range,
        response.accept_ranges,
    ))
}
