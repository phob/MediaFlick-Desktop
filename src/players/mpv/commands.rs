use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Map, Value, json};

use crate::app::urls::percent_decode;
use crate::playback::{
    HttpHeader, PlaybackRequest, PlayerCommand, ToneMapping, VideoAspect, VideoFit, non_empty,
};

static REQUEST_COUNTER: AtomicI64 = AtomicI64::new(100);

pub fn loadfile_command(launch: &PlaybackRequest) -> Value {
    let mut options = Map::new();
    // Intentionally do not set mpv's `start` or `pause` options here. Every
    // runtime resumes with a delayed absolute seek after file-loaded; see
    // `kick_start_playback`.
    if let Some(title) = non_empty(launch.title.as_deref()) {
        options.insert(
            "force-media-title".to_string(),
            json!(sanitize_option(title)),
        );
    }
    if let Some(audio_id) = launch.audio_mpv_id.filter(|id| *id > 0) {
        options.insert("aid".to_string(), json!(audio_id.to_string()));
    }
    if non_empty(launch.subtitle_url.as_deref()).is_some() {
        // Avoid briefly showing an automatically selected embedded subtitle;
        // the selected external Jellyfin subtitle is added with `sub-add select`
        // after mpv reports file-loaded.
        options.insert("sid".to_string(), json!("no"));
    } else if let Some(subtitle_id) = launch.subtitle_mpv_id {
        let value = if subtitle_id > 0 {
            subtitle_id.to_string()
        } else {
            "no".to_string()
        };
        options.insert("sid".to_string(), json!(value));
    }
    let headers = mpv_headers(launch);
    if !headers.is_empty() {
        options.insert(
            "http-header-fields".to_string(),
            json!(mpv_string_list(
                headers
                    .iter()
                    .map(|header| format!("{}: {}", header.name, header.value))
            )),
        );
    }

    json!({
        "command": ["loadfile", media_url_without_fragment(&launch.media_url), "replace", -1, Value::Object(options)],
        "request_id": next_request_id(),
    })
}

pub fn control_command(command: &PlayerCommand) -> Option<Value> {
    let command = match command {
        PlayerCommand::SetPause(pause) => {
            json!(["set_property", "pause", pause])
        }
        PlayerCommand::SeekMilliseconds(position_ms) => {
            if !position_ms.is_finite() {
                return None;
            }
            let seconds = (position_ms / 1000.0).max(0.0);
            json!(["seek", seconds, "absolute+exact"])
        }
        PlayerCommand::SetVolume(volume) => {
            if !volume.is_finite() {
                return None;
            }
            json!(["set_property", "volume", volume.clamp(0.0, 100.0)])
        }
        PlayerCommand::SetMute(mute) => {
            json!(["set_property", "mute", mute])
        }
        PlayerCommand::SetPlaybackRate(rate) => {
            if !rate.is_finite() {
                return None;
            }
            json!(["set_property", "speed", rate.clamp(0.1, 10.0)])
        }
        PlayerCommand::SetAudioDelay(delay) => {
            if !delay.is_finite() {
                return None;
            }
            json!(["set_property", "audio-delay", delay.clamp(-30.0, 30.0)])
        }
        PlayerCommand::SetSubtitleDelay(delay) => {
            if !delay.is_finite() {
                return None;
            }
            json!(["set_property", "sub-delay", delay.clamp(-30.0, 30.0)])
        }
        PlayerCommand::SetSubtitleScale(scale) => {
            if !scale.is_finite() {
                return None;
            }
            json!(["set_property", "sub-scale", scale.clamp(0.5, 2.0)])
        }
        PlayerCommand::SetVideoFit(fit) => {
            let panscan = match fit {
                VideoFit::Fit => 0.0,
                VideoFit::Fill => 1.0,
            };
            json!(["set_property", "panscan", panscan])
        }
        PlayerCommand::SetVideoAspect(aspect) => {
            let value = match aspect {
                VideoAspect::Source => "-1",
                VideoAspect::Ratio4x3 => "4:3",
                VideoAspect::Ratio16x9 => "16:9",
                VideoAspect::Ratio21x9 => "21:9",
            };
            json!(["set_property", "video-aspect-override", value])
        }
        PlayerCommand::SetDeinterlace(enabled) => {
            json!(["set_property", "deinterlace", enabled])
        }
        PlayerCommand::SetToneMapping(mode) => {
            let value = match mode {
                ToneMapping::Auto => "auto",
                ToneMapping::Clip => "clip",
                ToneMapping::Mobius => "mobius",
                ToneMapping::Reinhard => "reinhard",
                ToneMapping::Hable => "hable",
                ToneMapping::Bt2390 => "bt.2390",
            };
            json!(["set_property", "tone-mapping", value])
        }
        PlayerCommand::SetAudioTrack(id) => {
            if *id <= 0 {
                return None;
            }
            json!(["set_property", "aid", id])
        }
        PlayerCommand::SetSubtitleTrack(id) => match id.filter(|id| *id > 0) {
            Some(id) => json!(["set_property", "sid", id]),
            None => json!(["set_property", "sid", "no"]),
        },
        PlayerCommand::AddSubtitle(url) => {
            let url = non_empty(Some(url.as_str()))?;
            return Some(json!({
                "command": ["sub-add", url, "select"],
                "request_id": next_request_id(),
            }));
        }
        PlayerCommand::ToggleSubtitleVisibility => json!(["cycle", "sub-visibility"]),
        PlayerCommand::ToggleFullscreen => json!(["cycle", "fullscreen"]),
        PlayerCommand::MarkWatchedAndPlayNext => return None,
        PlayerCommand::Stop => json!(["stop"]),
    };

    Some(json!({
        "command": command,
        "request_id": next_request_id(),
    }))
}

/// On-screen text. It uses mpv's async IPC mode: the writer acknowledges it
/// once written instead of holding the controller thread for mpv's reply, and a
/// later command's reply wait drains the completion. Only cosmetic commands
/// whose result is merely logged may do this; seeks, `loadfile` and `sub-add`
/// stay reply-validated because playback depends on their outcome.
pub(super) fn osd_text_command(text: &str, duration_ms: i64) -> Value {
    json!({
        "command": ["show-text", text, duration_ms, 1],
        "request_id": next_request_id(),
        "async": true,
    })
}

pub(super) fn next_request_id() -> i64 {
    REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn media_url_without_fragment(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

fn mpv_headers(launch: &PlaybackRequest) -> Vec<HttpHeader> {
    let mut headers = Vec::<HttpHeader>::new();
    for header in &launch.headers {
        let name = sanitize_header_name(&header.name);
        let value = sanitize_option(header.value.trim());
        if name.is_empty() || value.is_empty() || !is_forwarded_header(&name) {
            continue;
        }
        if !headers
            .iter()
            .any(|existing| existing.name.eq_ignore_ascii_case(&name))
        {
            headers.push(HttpHeader { name, value });
        }
    }
    if !headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case("X-Emby-Token"))
        && let Some(token) = query_auth_token(&launch.media_url)
            .map(|value| sanitize_option(&value))
            .filter(|value| !value.is_empty())
    {
        headers.push(HttpHeader {
            name: "X-Emby-Token".to_string(),
            value: token,
        });
    }
    headers
}

fn is_forwarded_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "x-emby-authorization"
            | "x-emby-token"
            | "x-mediabrowser-token"
            | "cookie"
            | "user-agent"
            | "referer"
            | "origin"
    )
}

fn mpv_string_list(values: impl IntoIterator<Item = String>) -> String {
    values
        .into_iter()
        .map(|value| value.replace('\\', "\\\\").replace(',', "\\,"))
        .collect::<Vec<_>>()
        .join(",")
}

fn sanitize_header_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>()
}

fn sanitize_option(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '\0' | '\r' | '\n'))
        .collect::<String>()
}

fn query_auth_token(url: &str) -> Option<String> {
    [
        "api_key",
        "apikey",
        "access_token",
        "accesstoken",
        "x-emby-token",
        "x-mediabrowser-token",
    ]
    .into_iter()
    .find_map(|key| query_param_ci(url, key))
}

fn query_param_ci(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next().unwrap_or_default();
    query.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        percent_decode(raw_key)
            .eq_ignore_ascii_case(key)
            .then(|| percent_decode(raw_value))
    })
}

#[cfg(test)]
mod tests {
    use super::{control_command, loadfile_command, osd_text_command};
    use crate::playback::{HttpHeader, PlaybackRequest, PlayerCommand};
    use serde_json::json;

    #[test]
    fn loadfile_command_contains_url_replace_options_and_request_id() {
        let mut launch = PlaybackRequest::new("https://example.test/video.mkv");
        launch.start_time_ticks = Some(20_000_000);
        launch.title = Some("A Movie".to_string());

        let command = loadfile_command(&launch);
        let args = command["command"].as_array().expect("command array");
        assert_eq!(args[0], "loadfile");
        assert_eq!(args[1], "https://example.test/video.mkv");
        assert_eq!(args[2], "replace");
        assert_eq!(args[3], -1);
        assert!(command["request_id"].as_i64().is_some());
        assert!(args[4].get("start").is_none());
        assert!(args[4].get("pause").is_none());
        assert_eq!(args[4]["force-media-title"], "A Movie");
    }

    #[test]
    fn loadfile_command_applies_selected_tracks() {
        let mut launch = PlaybackRequest::new("https://example.test/video.mkv");
        launch.audio_stream_index = Some(3);
        launch.subtitle_stream_index = Some(5);
        launch.audio_mpv_id = Some(2);
        launch.subtitle_mpv_id = Some(1);

        let command = loadfile_command(&launch);
        let options = &command["command"][4];
        assert_eq!(options["aid"], "2");
        assert_eq!(options["sid"], "1");
    }

    #[test]
    fn loadfile_command_disables_embedded_subtitles_for_external_subtitle() {
        let mut launch = PlaybackRequest::new("https://example.test/video.mkv");
        launch.subtitle_stream_index = Some(7);
        launch.subtitle_url = Some("https://example.test/subtitle.srt".to_string());

        let command = loadfile_command(&launch);
        let options = &command["command"][4];
        assert_eq!(options["sid"], "no");
    }

    #[test]
    fn loadfile_filters_and_escapes_headers_for_mpv_string_list() {
        let mut launch = PlaybackRequest::new("https://example.test/video.mkv");
        launch.headers = vec![
            HttpHeader {
                name: "Authorization".to_string(),
                value: "MediaBrowser Client=\"Jellyfin Web\", Token=\"abc,def\"".to_string(),
            },
            HttpHeader {
                name: "Host".to_string(),
                value: "evil.test".to_string(),
            },
        ];

        let command = loadfile_command(&launch);
        let headers = command["command"][4]["http-header-fields"]
            .as_str()
            .expect("header list");
        assert!(headers.contains(
            "Authorization: MediaBrowser Client=\"Jellyfin Web\"\\, Token=\"abc\\,def\""
        ));
        assert!(!headers.contains("Host:"));
    }

    #[test]
    fn loadfile_adds_token_header_from_url_when_missing() {
        let launch = PlaybackRequest::new("https://example.test/video.mkv?ApiKey=secret");
        let command = loadfile_command(&launch);
        let headers = command["command"][4]["http-header-fields"]
            .as_str()
            .expect("header list");
        assert_eq!(headers, "X-Emby-Token: secret");
    }

    #[test]
    fn loadfile_strips_media_fragment_from_url() {
        let mut launch = PlaybackRequest::new("https://example.test/video.mkv?ApiKey=secret#t=30");
        launch.start_time_ticks = Some(300_000_000);

        let command = loadfile_command(&launch);
        assert_eq!(
            command["command"][1],
            "https://example.test/video.mkv?ApiKey=secret"
        );
    }

    /// mpv's `seek` takes seconds, and a negative absolute target counts from
    /// the end of the file, so a UI position must arrive as a non-negative
    /// absolute+exact seek in seconds.
    #[test]
    fn seeks_are_absolute_exact_seconds_that_never_count_from_the_end() {
        let seek = control_command(&PlayerCommand::SeekMilliseconds(12_345.0)).expect("seek");
        assert_eq!(seek["command"], json!(["seek", 12.345, "absolute+exact"]));
        let before_start =
            control_command(&PlayerCommand::SeekMilliseconds(-1_000.0)).expect("seek");
        assert_eq!(
            before_start["command"],
            json!(["seek", 0.0, "absolute+exact"])
        );
    }

    /// Playback depends on seeks and `sub-add`, so mpv's reply must be awaited;
    /// on-screen text is cosmetic and must not hold the controller thread.
    #[test]
    fn only_on_screen_text_skips_the_mpv_reply() {
        let seek = control_command(&PlayerCommand::SeekMilliseconds(1_000.0)).expect("seek");
        assert!(seek.get("async").is_none());
        let subtitle = control_command(&PlayerCommand::AddSubtitle(
            "https://example.test/sub.srt".to_string(),
        ))
        .expect("sub-add");
        assert_eq!(
            subtitle["command"],
            json!(["sub-add", "https://example.test/sub.srt", "select"])
        );
        assert!(subtitle.get("async").is_none());
        assert_eq!(osd_text_command("Skipped Intro", 1500)["async"], true);
    }

    #[test]
    fn non_finite_values_never_reach_mpv() {
        for command in [
            PlayerCommand::SeekMilliseconds(f64::NAN),
            PlayerCommand::SetVolume(f64::INFINITY),
            PlayerCommand::SetPlaybackRate(f64::NAN),
            PlayerCommand::SetAudioDelay(f64::NAN),
            PlayerCommand::SetSubtitleDelay(f64::NEG_INFINITY),
            PlayerCommand::SetSubtitleScale(f64::INFINITY),
        ] {
            assert!(control_command(&command).is_none(), "{command:?}");
        }
    }
}
