use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["image", id, image_type] if request.is("GET") => image(
            services,
            &percent_decode(id),
            &percent_decode(image_type),
            request,
        ),
        _ => return None,
    };
    Some(response)
}

// -------------------------------------------------------------------- images

static IMAGE_WRITES: AtomicUsize = AtomicUsize::new(0);
/// Prune once the cache is clearly larger than a big library's poster set.
const IMAGE_CACHE_MAX_FILES: usize = 4_000;
const IMAGE_CACHE_PRUNE_EVERY: usize = 200;

fn image(
    services: &Arc<Services>,
    item_id: &str,
    image_type: &str,
    request: &ApiRequest,
) -> ApiResponse {
    let tag = request.param("tag").unwrap_or_default();
    let max_width = request
        .param("maxWidth")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
        .min(4_000);
    let key = cache_key(item_id, image_type, &tag, max_width);
    let cache_path = crate::app::paths::image_cache_dir().join(&key);

    if let Ok(bytes) = std::fs::read(&cache_path)
        && !bytes.is_empty()
    {
        return ApiResponse::bytes(mime_for_image(&bytes), bytes, IMMUTABLE_CACHE);
    }

    let (client, user_id) = match services.session.client_and_user() {
        Ok(pair) => pair,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let mut query = Vec::new();
    if !tag.is_empty() {
        query.push(("tag", tag));
    }
    if max_width > 0 {
        query.push(("maxWidth", max_width.to_string()));
    }
    query.push(("quality", "90".to_string()));

    match client.get_bytes(&items::image_path(item_id, image_type), &query) {
        Ok((bytes, content_type)) => {
            store_image(&cache_path, &bytes);
            ApiResponse::bytes(content_type, bytes, IMMUTABLE_CACHE)
        }
        Err(error) => {
            // A missing image is the first sign of a replaced file, because the
            // grid renders posters long before anything tries to play them.
            if matches!(error, ApiError::Status { status: 404 }) {
                forget_if_server_disowns(services, &client, &user_id, item_id);
            }
            services.session.note_error(&error);
            ApiResponse::from_api_error(&error)
        }
    }
}

/// Evicts a cached item, but only once the server confirms it is really gone.
///
/// An image 404 alone is not proof: an item can exist with no artwork under
/// that tag. `fetch_item` queries `/Items?ids=`, so a missing item comes back as
/// an empty result rather than an error, which cleanly separates "deleted" from
/// "the server is unwell" — the latter lands in `Err` and is left alone.
fn forget_if_server_disowns(
    services: &Arc<Services>,
    client: &JellyfinClient,
    user_id: &str,
    item_id: &str,
) {
    match items::fetch_item(client, user_id, item_id) {
        Ok(None) => forget_item(services, item_id),
        Ok(Some(_)) => {}
        Err(error) => {
            tracing::debug!(
                target: "app.api",
                "could not confirm whether {item_id} still exists: {error}"
            );
        }
    }
}

/// Only characters that are safe in a file name survive, so a hostile item id
/// cannot escape the cache directory.
pub(super) fn cache_key(item_id: &str, image_type: &str, tag: &str, max_width: u32) -> String {
    let sanitize = |value: &str| -> String {
        value
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .take(64)
            .collect()
    };
    format!(
        "{}-{}-{}-{max_width}.img",
        sanitize(item_id),
        sanitize(image_type),
        sanitize(tag)
    )
}

pub(super) fn store_image(path: &std::path::Path, bytes: &[u8]) {
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() || std::fs::write(path, bytes).is_err() {
        return;
    }
    if IMAGE_WRITES
        .fetch_add(1, Ordering::Relaxed)
        .is_multiple_of(IMAGE_CACHE_PRUNE_EVERY)
    {
        prune_image_cache(parent);
    }
}

/// Drops the oldest quarter of the cache once it grows past the cap.
fn prune_image_cache(directory: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files = entries
        .flatten()
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect::<Vec<_>>();
    if files.len() <= IMAGE_CACHE_MAX_FILES {
        return;
    }
    files.sort_by_key(|(modified, _)| *modified);
    let remove = files.len() - IMAGE_CACHE_MAX_FILES * 3 / 4;
    for (_, path) in files.into_iter().take(remove) {
        let _ = std::fs::remove_file(path);
    }
    tracing::debug!(target: "app.api", removed = remove, "pruned the poster cache");
}

pub(super) fn mime_for_image(bytes: &[u8]) -> String {
    let kind = match bytes {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
        [b'G', b'I', b'F', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', ..] => "image/webp",
        _ => "application/octet-stream",
    };
    kind.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_keys_cannot_escape_the_cache_directory() {
        let key = cache_key("../../etc/passwd", "Primary", "tag", 400);
        assert!(!key.contains('/'));
        assert!(!key.contains('.') || key.ends_with(".img"));
        assert_eq!(key, "etcpasswd-Primary-tag-400.img");
    }

    #[test]
    fn image_mime_types_are_sniffed_from_the_payload() {
        assert_eq!(mime_for_image(&[0x89, b'P', b'N', b'G', 0]), "image/png");
        assert_eq!(mime_for_image(&[0xFF, 0xD8, 0xFF, 0]), "image/jpeg");
        assert_eq!(mime_for_image(b"RIFF...."), "image/webp");
        assert_eq!(mime_for_image(b"nonsense"), "application/octet-stream");
    }
}
