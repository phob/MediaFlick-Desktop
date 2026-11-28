use std::io::Write;

use super::*;

routes! {
    |services, request|
    GET ["image", id, image_type] -> Binary =
        image(services, &percent_decode(id), &percent_decode(image_type), request),
    GET ["trickplay", id, width, tile] -> Binary =
        trickplay_tile(services, &percent_decode(id), width, tile, request),
}

static IMAGE_WRITES: AtomicUsize = AtomicUsize::new(0);
static IMAGE_TEMPORARIES: AtomicUsize = AtomicUsize::new(0);
const IMAGE_CACHE_MAX_FILES: usize = 4_000;
const IMAGE_CACHE_PRUNE_EVERY: usize = 200;

fn cached_image_response(cache_path: &std::path::Path) -> Option<BinaryAnswer> {
    let bytes = std::fs::read(cache_path).ok()?;
    (!bytes.is_empty()).then(|| BinaryAnswer::bytes(mime_for_image(&bytes), bytes, IMMUTABLE_CACHE))
}

fn image(
    services: &Arc<Services>,
    item_id: &str,
    image_type: &str,
    request: &ApiRequest,
) -> Result<BinaryAnswer, ApiResponse> {
    let tag = request.param("tag").unwrap_or_default();
    let max_width = request
        .param("maxWidth")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
        .min(4_000);
    let key = cache_key(item_id, image_type, &tag, max_width);
    let cache_path = crate::app::paths::image_cache_dir().join(&key);

    if let Some(cached) = cached_image_response(&cache_path) {
        return Ok(cached);
    }

    let scope = session_scope(services)?;
    let mut query = Vec::new();
    if !tag.is_empty() {
        query.push(("tag", tag));
    }
    if max_width > 0 {
        query.push(("maxWidth", max_width.to_string()));
    }
    query.push(("quality", "90".to_string()));

    match scope.fetch(|client, _| client.get_bytes(&items::image_path(item_id, image_type), &query))
    {
        Ok(fetched) => {
            let (bytes, content_type) = confirmed(services, fetched)?;
            store_image(&cache_path, &bytes);
            Ok(BinaryAnswer::bytes(content_type, bytes, IMMUTABLE_CACHE))
        }
        Err(error) => {
            if matches!(error, ApiError::Status { status: 404 }) {
                forget_if_server_disowns(services, &scope, item_id);
            }
            Err(scoped_failure(services, &scope, &error))
        }
    }
}

fn parse_trickplay_params(
    width: &str,
    tile: &str,
    media_source_id: Option<&str>,
) -> Result<(u32, u32, Option<String>), ApiResponse> {
    let width_val = width
        .parse::<u32>()
        .ok()
        .filter(|value| (16..=1920).contains(value))
        .ok_or_else(|| ApiResponse::error(400, "invalid trickplay width"))?;
    let tile_val = tile
        .parse::<u32>()
        .ok()
        .filter(|value| *value <= 100_000)
        .ok_or_else(|| ApiResponse::error(400, "invalid trickplay tile index"))?;
    let validated_source = match media_source_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => {
            if value.len() > 128
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                return Err(ApiResponse::error(400, "invalid mediaSourceId"));
            }
            Some(value.to_string())
        }
        None => None,
    };
    Ok((width_val, tile_val, validated_source))
}

fn trickplay_tile(
    services: &Arc<Services>,
    item_id: &str,
    width: &str,
    tile: &str,
    request: &ApiRequest,
) -> Result<BinaryAnswer, ApiResponse> {
    let (width_val, tile_val, media_source_id) =
        parse_trickplay_params(width, tile, request.param("mediaSourceId").as_deref())?;
    let key = cache_key(
        item_id,
        &format!("Trickplay-{width_val}-{tile_val}"),
        media_source_id.as_deref().unwrap_or(""),
        width_val,
    );
    let cache_path = crate::app::paths::image_cache_dir().join(&key);

    if let Some(cached) = cached_image_response(&cache_path) {
        return Ok(cached);
    }

    let scope = session_scope(services)?;
    let mut query = Vec::new();
    if let Some(source_id) = media_source_id {
        query.push(("mediaSourceId", source_id));
    }
    let path = items::trickplay_tile_path(item_id, width_val, tile_val);
    match scope.fetch(|client, _| client.get_bytes(&path, &query)) {
        Ok(fetched) => {
            let (bytes, content_type) = confirmed(services, fetched)?;
            store_image(&cache_path, &bytes);
            Ok(BinaryAnswer::bytes(content_type, bytes, IMMUTABLE_CACHE))
        }
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

fn forget_if_server_disowns(services: &Arc<Services>, scope: &SessionScope, item_id: &str) {
    let item = scope
        .fetch(|client, user_id| items::fetch_item(client, user_id, item_id))
        .map(|fetched| services.session.confirm(fetched));
    match item {
        Ok(Ok(None)) => forget_item(services, scope, item_id),
        Ok(Ok(Some(_)) | Err(_)) => {}
        Err(error) => {
            tracing::debug!(
                target: "app.api",
                "could not confirm whether {item_id} still exists: {error}"
            );
        }
    }
}

pub(crate) fn cache_key(item_id: &str, image_type: &str, tag: &str, max_width: u32) -> String {
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
    if std::fs::create_dir_all(parent).is_err() || replace_cached(parent, path, bytes).is_err() {
        return;
    }
    if IMAGE_WRITES
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1)
        .is_multiple_of(IMAGE_CACHE_PRUNE_EVERY)
    {
        prune_image_cache(parent);
    }
}

fn replace_cached(
    parent: &std::path::Path,
    path: &std::path::Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let counter = IMAGE_TEMPORARIES.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.tmp-{}-{counter}", std::process::id()));
    let result = std::fs::File::create_new(&temporary)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| std::fs::rename(&temporary, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

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

pub(super) fn image_mime_type(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [b'G', b'I', b'F', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', ..] => Some("image/webp"),
        _ => None,
    }
}

pub(super) fn mime_for_image(bytes: &[u8]) -> String {
    image_mime_type(bytes)
        .unwrap_or("application/octet-stream")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_keys_cannot_escape_the_cache_directory() {
        let key = cache_key("../../etc/passwd", r"..\..\Primary", "tag", 400);
        assert!(!key.contains(['/', '\\', ':']), "{key}");
        assert!(!key.contains(".."), "{key}");
    }

    #[test]
    fn trickplay_params_reject_invalid_bounds_and_source_ids() {
        assert_eq!(
            parse_trickplay_params("320", "0", Some("src-1")).map_err(|r| r.status),
            Ok((320, 0, Some("src-1".to_string())))
        );
        assert_eq!(
            parse_trickplay_params("16", "100000", None).map_err(|r| r.status),
            Ok((16, 100_000, None))
        );
        assert_eq!(
            parse_trickplay_params("15", "0", None).map_err(|r| r.status),
            Err(400)
        );
        assert_eq!(
            parse_trickplay_params("1921", "0", None).map_err(|r| r.status),
            Err(400)
        );
        assert_eq!(
            parse_trickplay_params("abc", "0", None).map_err(|r| r.status),
            Err(400)
        );
        assert_eq!(
            parse_trickplay_params("320", "100001", None).map_err(|r| r.status),
            Err(400)
        );
        assert_eq!(
            parse_trickplay_params("320", "-1", None).map_err(|r| r.status),
            Err(400)
        );
        assert_eq!(
            parse_trickplay_params("320", "0", Some("../secret")).map_err(|r| r.status),
            Err(400)
        );
    }
}
