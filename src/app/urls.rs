//! URL helpers shared by the Jellyfin REST client and the app-scheme server.

/// Percent-encode a value so it survives as a single path segment.
pub fn encode_path_segment(value: &str) -> String {
    encode_with(
        value,
        |byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'),
    )
}

/// Percent-encode a value so it survives as a query-string key or value.
pub fn encode_query_component(value: &str) -> String {
    encode_path_segment(value)
}

fn encode_with(value: &str, is_unreserved: impl Fn(u8) -> bool) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if is_unreserved(byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Build a `key=value&…` query string, skipping nothing and encoding everything.
pub fn build_query(pairs: &[(&str, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                encode_query_component(key),
                encode_query_component(value)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Decode a percent-encoded query component. `+` decodes to a space.
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            out.push((high << 4) | low);
            index += 3;
            continue;
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Look up a single query parameter by exact (decoded) name.
pub fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        (percent_decode(raw_key) == key).then(|| percent_decode(raw_value))
    })
}

/// Join a server base URL with an API path, collapsing the separating slash.
pub fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[cfg(test)]
mod tests {
    use super::{build_query, encode_path_segment, join_url, percent_decode, query_param};

    #[test]
    fn path_segments_escape_separators() {
        assert_eq!(encode_path_segment("../Users"), "..%2FUsers");
        assert_eq!(encode_path_segment("a b"), "a%20b");
        assert_eq!(encode_path_segment("abc-1_2.3~4"), "abc-1_2.3~4");
    }

    #[test]
    fn query_round_trips_through_decoding() {
        let query = build_query(&[
            ("searchTerm", "the matrix".to_string()),
            ("kind", "Movie".to_string()),
        ]);
        assert_eq!(query, "searchTerm=the%20matrix&kind=Movie");
        assert_eq!(
            query_param(&query, "searchTerm").as_deref(),
            Some("the matrix")
        );
        assert_eq!(query_param(&query, "missing"), None);
    }

    #[test]
    fn percent_decode_handles_escapes_and_plus() {
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn join_url_collapses_slashes() {
        assert_eq!(join_url("http://host/", "/Items"), "http://host/Items");
        assert_eq!(
            join_url("http://host/jellyfin", "Items"),
            "http://host/jellyfin/Items"
        );
    }
}
