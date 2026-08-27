const TMDB_SIZES: &[&str] = &[
    "w92", "w154", "w185", "w300", "w342", "w500", "w780", "w1280",
];

/// Validates the rendition and filename before the fixed-origin request is
/// delegated to the Companion. Desktop never receives or constructs a
/// provider URL.
pub fn tmdb_image_path(size: &str, file: &str) -> Option<String> {
    if !TMDB_SIZES.contains(&size) {
        return None;
    }
    let file = file.trim_start_matches('/');
    let (stem, extension) = file.rsplit_once('.')?;
    let plain = |value: &str, extra: &[u8]| {
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || extra.contains(&byte))
    };
    if !plain(stem, b"-_") || !matches!(extension, "jpg" | "jpeg" | "png" | "webp") {
        return None;
    }
    Some(format!("/{stem}.{extension}"))
}

#[cfg(test)]
mod tests {
    use super::tmdb_image_path;

    #[test]
    fn poster_addresses_use_only_named_sizes_and_plain_files() {
        assert_eq!(
            tmdb_image_path("w300", "/f89U3ADr1oiB1s9GkdPOEpXUk5H.jpg").as_deref(),
            Some("/f89U3ADr1oiB1s9GkdPOEpXUk5H.jpg")
        );
        assert_eq!(
            tmdb_image_path("w780", "abc-_1.png").as_deref(),
            Some("/abc-_1.png")
        );

        let too_long = format!("{}.jpg", "a".repeat(65));
        for (size, file) in [
            ("w300", "../../../etc/passwd"),
            ("w300", "abc.jpg?x=1"),
            ("w300", "abc.jpg#f"),
            ("w300", "ab c.jpg"),
            ("w300", "abc.exe"),
            ("w300", "abc"),
            ("w300", ""),
            ("w300", too_long.as_str()),
            ("original", "abc.jpg"),
            ("../w300", "abc.jpg"),
            ("", "abc.jpg"),
        ] {
            assert_eq!(tmdb_image_path(size, file), None, "{size}/{file}");
        }
    }
}
