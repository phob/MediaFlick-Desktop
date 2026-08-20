use super::*;

pub(super) fn js_json(value: &serde_json::Value) -> String {
    escape_js_line_separators(&value.to_string())
}

fn escape_js_line_separators(json: &str) -> String {
    if json.contains('\u{2028}') || json.contains('\u{2029}') {
        json.replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029")
    } else {
        json.to_string()
    }
}

pub(super) fn load_error_html(
    title: &str,
    failed_url: &str,
    error_text: &str,
    error_code: i32,
) -> String {
    let retry_url = if failed_url.starts_with("mediaflick-desktop://")
        || failed_url.starts_with("https://")
        || failed_url.starts_with("http://")
    {
        failed_url
    } else {
        "mediaflick-desktop://app/"
    };
    include_str!("../ui/load_error.html")
        .replace("{{title}}", &html_escape(title))
        .replace("{{retry_url}}", &html_escape(retry_url))
        .replace("{{failed_url}}", &html_escape(failed_url))
        .replace("{{error_text}}", &html_escape(error_text))
        .replace("{{error_code}}", &error_code.to_string())
}

pub(super) fn data_uri(data: &[u8], mime_type: &str) -> String {
    let data = CefString::from(&base64_encode(Some(data)));
    let uri = CefString::from(&uriencode(Some(&data), 0)).to_string();
    format!("data:{mime_type};base64,{uri}")
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_js_line_separators_neutralizes_unicode_terminators() {
        let input = "value\u{2028}with\u{2029}terminators";
        let escaped = escape_js_line_separators(input);
        assert_eq!(escaped, "value\\u2028with\\u2029terminators");
        assert!(!escaped.contains('\u{2028}'));
        assert!(!escaped.contains('\u{2029}'));
    }

    #[test]
    fn escape_js_line_separators_leaves_plain_text_untouched() {
        assert_eq!(escape_js_line_separators("plain text"), "plain text");
    }

    #[test]
    fn html_escape_encodes_all_markup_characters() {
        assert_eq!(
            html_escape("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn load_error_page_is_mediaflick_branded_and_retries_safe_urls() {
        let html = load_error_html(
            "MediaFlick Desktop",
            "mediaflick-desktop://app/settings",
            "failed",
            -2,
        );
        assert!(html.contains("MediaFlick couldn’t load this page"));
        assert!(html.contains("href=\"mediaflick-desktop://app/settings\""));
        assert!(!html.contains("Could not load Jellyfin"));
    }

    #[test]
    fn load_error_page_does_not_retry_unsafe_schemes() {
        let html = load_error_html("MediaFlick Desktop", "javascript:alert(1)", "failed", -2);
        assert!(html.contains("href=\"mediaflick-desktop://app/\""));
        assert!(!html.contains("href=\"javascript:alert(1)\""));
    }
}
