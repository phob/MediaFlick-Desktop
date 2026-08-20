use crate::playback::PlaybackRequest;

pub(super) struct TrackSelection {
    pub(super) audio_index: Option<i64>,
    pub(super) subtitle_index: Option<i64>,
}

pub(super) fn media_url(launch: &PlaybackRequest) -> String {
    let url = apply_subtitle_burn_in(launch.media_url.clone(), launch);
    if url_has_token(&url) {
        return url;
    }
    let Some(token) = token_from_headers(launch) else {
        return url;
    };
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}api_key={token}")
}

pub(super) fn track_selection(launch: &PlaybackRequest) -> TrackSelection {
    let audio_index = launch.audio_mpv_id.and_then(audio_index);
    let has_external_subtitle = launch
        .subtitle_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let subtitle_index = if has_external_subtitle {
        Some(-1)
    } else {
        launch.subtitle_mpv_id.map(|id| subtitle_index(Some(id)))
    };
    TrackSelection {
        audio_index,
        subtitle_index,
    }
}

pub(super) fn audio_index(mpv_id: i64) -> Option<i64> {
    (mpv_id > 0).then_some(mpv_id - 1)
}

pub(super) fn subtitle_index(mpv_id: Option<i64>) -> i64 {
    match mpv_id {
        Some(id) if id > 0 => id - 1,
        _ => -1,
    }
}

fn apply_subtitle_burn_in(url: String, launch: &PlaybackRequest) -> String {
    let Some(index) = launch.subtitle_stream_index.filter(|index| *index >= 0) else {
        return url;
    };
    let is_external = launch
        .subtitle_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if !is_external || query_has_key(&url, "subtitlestreamindex") {
        return url;
    }
    let url = remove_query_keys(&url, &["static"]);
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}SubtitleStreamIndex={index}&SubtitleMethod=Encode")
}

fn query_has_key(url: &str, key: &str) -> bool {
    let Some((_, rest)) = url.split_once('?') else {
        return false;
    };
    let query = rest.split('#').next().unwrap_or(rest);
    query.split('&').any(|pair| {
        pair.split('=')
            .next()
            .unwrap_or_default()
            .eq_ignore_ascii_case(key)
    })
}

fn remove_query_keys(url: &str, keys: &[&str]) -> String {
    let Some((before, rest)) = url.split_once('?') else {
        return url.to_string();
    };
    let (query, fragment) = rest
        .split_once('#')
        .map(|(query, fragment)| (query, Some(fragment)))
        .unwrap_or((rest, None));
    let kept = query
        .split('&')
        .filter(|pair| {
            let key = pair.split('=').next().unwrap_or_default();
            !keys.iter().any(|removed| key.eq_ignore_ascii_case(removed))
        })
        .collect::<Vec<_>>();
    let mut out = String::from(before);
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

fn url_has_token(url: &str) -> bool {
    let Some(query) = url.split_once('?').map(|(_, rest)| rest) else {
        return false;
    };
    query.split(['&', '#']).any(|pair| {
        let key = pair
            .split('=')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        matches!(
            key.as_str(),
            "api_key"
                | "apikey"
                | "access_token"
                | "accesstoken"
                | "x-emby-token"
                | "x-mediabrowser-token"
        )
    })
}

fn token_from_headers(launch: &PlaybackRequest) -> Option<String> {
    for header in &launch.headers {
        if header.name.eq_ignore_ascii_case("X-Emby-Token")
            || header.name.eq_ignore_ascii_case("X-MediaBrowser-Token")
        {
            let value = header.value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::HttpHeader;

    fn external_subtitle_launch() -> PlaybackRequest {
        PlaybackRequest {
            media_url: "https://host/Videos/abc/stream.mkv?static=true&MediaSourceId=src"
                .to_string(),
            subtitle_stream_index: Some(3),
            subtitle_url: Some("https://host/Videos/abc/sub.srt".to_string()),
            ..PlaybackRequest::default()
        }
    }

    #[test]
    fn burn_in_drops_static_and_requests_encoded_subtitle() {
        let url = apply_subtitle_burn_in(
            external_subtitle_launch().media_url,
            &external_subtitle_launch(),
        );
        assert!(!query_has_key(&url, "static"));
        assert!(url.contains("SubtitleStreamIndex=3"));
        assert!(url.contains("SubtitleMethod=Encode"));
        assert!(url.contains("MediaSourceId=src"));
    }

    #[test]
    fn burn_in_skipped_for_embedded_subtitle() {
        let launch = PlaybackRequest {
            subtitle_url: None,
            ..external_subtitle_launch()
        };
        let original = launch.media_url.clone();
        assert_eq!(apply_subtitle_burn_in(original.clone(), &launch), original);
    }

    #[test]
    fn burn_in_skipped_without_subtitle_index() {
        let launch = PlaybackRequest {
            subtitle_stream_index: None,
            ..external_subtitle_launch()
        };
        let original = launch.media_url.clone();
        assert_eq!(apply_subtitle_burn_in(original.clone(), &launch), original);
    }

    #[test]
    fn burn_in_is_idempotent() {
        let launch = external_subtitle_launch();
        let once = apply_subtitle_burn_in(launch.media_url.clone(), &launch);
        let twice = apply_subtitle_burn_in(once.clone(), &launch);
        assert_eq!(once, twice);
    }

    #[test]
    fn media_url_appends_token_after_burn_in() {
        let launch = PlaybackRequest {
            headers: vec![HttpHeader {
                name: "X-Emby-Token".to_string(),
                value: "secret".to_string(),
            }],
            ..external_subtitle_launch()
        };
        let url = media_url(&launch);
        assert!(url.contains("SubtitleMethod=Encode"));
        assert!(url.contains("api_key=secret"));
    }

    #[test]
    fn remove_query_keys_preserves_fragment_and_other_pairs() {
        assert_eq!(
            remove_query_keys("https://host/x?a=1&static=true&b=2#frag", &["static"]),
            "https://host/x?a=1&b=2#frag"
        );
        assert_eq!(
            remove_query_keys("https://host/x?static=true", &["static"]),
            "https://host/x"
        );
    }

    #[test]
    fn audio_index_is_zero_based_and_drops_non_tracks() {
        assert_eq!(audio_index(1), Some(0));
        assert_eq!(audio_index(3), Some(2));
        assert_eq!(audio_index(0), None);
        assert_eq!(audio_index(-1), None);
    }

    #[test]
    fn subtitle_index_is_zero_based_with_off_sentinel() {
        assert_eq!(subtitle_index(Some(1)), 0);
        assert_eq!(subtitle_index(Some(5)), 4);
        assert_eq!(subtitle_index(Some(-1)), -1);
        assert_eq!(subtitle_index(Some(0)), -1);
        assert_eq!(subtitle_index(None), -1);
    }

    #[test]
    fn track_selection_converts_embedded_tracks() {
        let launch = PlaybackRequest {
            audio_mpv_id: Some(2),
            subtitle_mpv_id: Some(5),
            ..PlaybackRequest::default()
        };
        let selection = track_selection(&launch);
        assert_eq!(selection.audio_index, Some(1));
        assert_eq!(selection.subtitle_index, Some(4));
    }

    #[test]
    fn track_selection_disables_embedded_subtitle_for_external() {
        let launch = PlaybackRequest {
            subtitle_mpv_id: Some(3),
            subtitle_url: Some("https://host/sub.srt".to_string()),
            ..PlaybackRequest::default()
        };
        assert_eq!(track_selection(&launch).subtitle_index, Some(-1));
    }

    #[test]
    fn track_selection_leaves_unset_tracks_alone() {
        let selection = track_selection(&PlaybackRequest::default());
        assert_eq!(selection.audio_index, None);
        assert_eq!(selection.subtitle_index, None);
    }
}
