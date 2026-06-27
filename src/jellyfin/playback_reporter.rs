use std::collections::HashSet;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use crate::app::logger;
use crate::jellyfin::bridge::PlaybackContext;
use crate::mpv::{HttpHeader, MpvLaunch};

const HTTP_TIMEOUT: Duration = Duration::from_secs(5);
pub const TICKS_PER_SECOND: f64 = 10_000_000.0;

static IPC_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct PlaybackSession {
    base_url: String,
    item_id: String,
    media_source_id: Option<String>,
    play_session_id: Option<String>,
    play_method: String,
    playlist_item_id: Option<String>,
    audio_stream_index: Option<i64>,
    subtitle_stream_index: Option<i64>,
    start_position_ticks: i64,
    runtime_ticks: Option<i64>,
    playback_start_time_ticks: i64,
    auth_headers: Vec<HttpHeader>,
    queue: Option<Value>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MpvPlaybackState {
    pub position_ticks: i64,
    pub pause: bool,
    pub duration_ticks: Option<i64>,
    pub volume: Option<i64>,
    pub mute: Option<bool>,
    pub eof_reached: bool,
}

impl fmt::Display for MpvPlaybackState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "position={} duration={} paused={} volume={} muted={} eof={}",
            ticks_summary(Some(self.position_ticks.max(0))),
            ticks_summary(self.duration_ticks),
            self.pause,
            self.volume
                .map(|volume| volume.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            self.mute
                .map(|mute| mute.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            self.eof_reached
        )
    }
}

pub fn make_mpv_ipc_path() -> String {
    let counter = IPC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    #[cfg(target_os = "windows")]
    {
        format!(
            r"\\.\pipe\mediaflick-desktop-{}-{timestamp}-{counter}",
            std::process::id()
        )
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::temp_dir()
            .join(format!(
                "mediaflick-desktop-{}-{timestamp}-{counter}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }
}

pub struct PlaybackReporter {
    pub session: PlaybackSession,
    agent: ureq::Agent,
}

impl PlaybackReporter {
    pub fn new(session: PlaybackSession) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .user_agent(format!("mediaflick-desktop/{}", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            session,
            agent: config.into(),
        }
    }

    pub fn from_launch(launch: &MpvLaunch) -> Option<Self> {
        PlaybackSession::from_launch(launch).map(Self::new)
    }

    pub fn merge_context(&mut self, context: &PlaybackContext) {
        self.session.merge_context(context);
    }

    pub fn report_start(&self, state: &MpvPlaybackState) {
        self.post_playstate(
            "Sessions/Playing",
            playback_progress_body(&self.session, state),
            state,
        );
    }

    pub fn report_progress(&self, state: &MpvPlaybackState) {
        self.post_playstate(
            "Sessions/Playing/Progress",
            playback_progress_body(&self.session, state),
            state,
        );
    }

    pub fn report_stopped(&self, state: &MpvPlaybackState, failed: bool) {
        self.post_playstate(
            "Sessions/Playing/Stopped",
            playback_stop_body(&self.session, state, failed),
            state,
        );
    }

    fn post_playstate(&self, endpoint: &str, body: Value, state: &MpvPlaybackState) {
        let url = join_api_url(&self.session.base_url, endpoint);
        tracing::trace!(
            target: "jellyfin.playstate",
            endpoint,
            item_id = %self.session.item_id,
            play_session_id = %display_opt(self.session.play_session_id.as_deref()),
            state = %state,
            body = %logger::redacted_json(&body),
            "sending Jellyfin playback state"
        );
        let mut request = self
            .agent
            .post(url.as_str())
            .header("Accept", "application/json");

        for header in &self.session.auth_headers {
            request = request.header(header.name.as_str(), header.value.as_str());
        }

        match request.send_json(&body) {
            Ok(response) => tracing::trace!(
                target: "jellyfin.playstate",
                endpoint,
                item_id = %self.session.item_id,
                status = response.status().as_u16(),
                "sent Jellyfin playback state"
            ),
            Err(error) => tracing::warn!(
                target: "jellyfin.playstate",
                endpoint,
                item_id = %self.session.item_id,
                state = %state,
                "failed to report Jellyfin playback state: {error}"
            ),
        }
    }
}

impl PlaybackSession {
    pub fn from_launch(launch: &MpvLaunch) -> Option<Self> {
        let item_id = non_empty(launch.item_id.as_deref())?.to_string();
        let base_url = server_base_url(&launch.media_url)?;
        let auth_headers = playback_auth_headers(launch);
        if auth_headers.is_empty() {
            tracing::warn!(
                target: "jellyfin.playstate",
                item_id,
                "cannot report Jellyfin playback state: missing auth token"
            );
            return None;
        }

        Some(Self {
            base_url,
            item_id,
            media_source_id: non_empty(launch.media_source_id.as_deref()).map(str::to_string),
            play_session_id: non_empty(launch.play_session_id.as_deref()).map(str::to_string),
            play_method: non_empty(launch.play_method.as_deref())
                .unwrap_or("DirectPlay")
                .to_string(),
            playlist_item_id: non_empty(launch.playlist_item_id.as_deref()).map(str::to_string),
            audio_stream_index: launch.audio_stream_index,
            subtitle_stream_index: launch.subtitle_stream_index,
            start_position_ticks: launch
                .start_time_ticks
                .or_else(|| {
                    launch
                        .start_milliseconds
                        .filter(|value| *value > 0.0)
                        .map(|value| (value * 10_000.0).round() as i64)
                })
                .unwrap_or(0)
                .max(0),
            runtime_ticks: launch.runtime_ticks.filter(|ticks| *ticks > 0),
            playback_start_time_ticks: unix_now_ticks(),
            auth_headers,
            queue: launch
                .queue
                .as_ref()
                .filter(|value| value.is_array())
                .cloned(),
        })
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn item_id(&self) -> &str {
        &self.item_id
    }

    pub(crate) fn media_source_id(&self) -> Option<&str> {
        self.media_source_id.as_deref()
    }

    pub(crate) fn auth_headers(&self) -> &[HttpHeader] {
        &self.auth_headers
    }

    fn merge_context(&mut self, context: &PlaybackContext) {
        fill_string(
            &mut self.media_source_id,
            context.media_source_id.as_deref(),
        );
        fill_string(
            &mut self.play_session_id,
            context.play_session_id.as_deref(),
        );
        fill_string(
            &mut self.playlist_item_id,
            context.playlist_item_id.as_deref(),
        );
        if let Some(play_method) = non_empty(context.play_method.as_deref()) {
            self.play_method = play_method.to_string();
        }
        if self.audio_stream_index.is_none() {
            self.audio_stream_index = context.audio_stream_index;
        }
        if self.subtitle_stream_index.is_none() {
            self.subtitle_stream_index = context.subtitle_stream_index;
        }
        if self.runtime_ticks.is_none() {
            self.runtime_ticks = context.runtime_ticks.filter(|ticks| *ticks > 0);
        }
        if self.queue.is_none() {
            self.queue = context
                .queue
                .as_ref()
                .filter(|value| value.is_array())
                .cloned();
        }
    }
}

pub fn playback_progress_body(session: &PlaybackSession, state: &MpvPlaybackState) -> Value {
    let mut body = Map::new();
    insert_common_body_fields(&mut body, session, state);
    body.insert(
        "PlaybackStartTimeTicks".to_string(),
        json!(session.playback_start_time_ticks),
    );
    Value::Object(body)
}

pub fn playback_stop_body(
    session: &PlaybackSession,
    state: &MpvPlaybackState,
    failed: bool,
) -> Value {
    let mut body = Map::new();
    insert_stop_body_fields(&mut body, session, state);
    body.insert("Failed".to_string(), json!(failed));
    Value::Object(body)
}

fn insert_common_body_fields(
    body: &mut Map<String, Value>,
    session: &PlaybackSession,
    state: &MpvPlaybackState,
) {
    body.insert("ItemId".to_string(), json!(session.item_id));
    insert_string_opt(body, "MediaSourceId", session.media_source_id.as_deref());
    insert_string_opt(body, "PlaySessionId", session.play_session_id.as_deref());
    insert_string_opt(body, "PlaylistItemId", session.playlist_item_id.as_deref());
    insert_i64_opt(body, "AudioStreamIndex", session.audio_stream_index);
    insert_i64_opt(body, "SubtitleStreamIndex", session.subtitle_stream_index);
    if let Some(queue) = &session.queue {
        body.insert("NowPlayingQueue".to_string(), queue.clone());
    }
    body.insert(
        "PositionTicks".to_string(),
        json!(state.position_ticks.max(0)),
    );
    body.insert(
        "PlaybackStartPositionTicks".to_string(),
        json!(session.start_position_ticks),
    );
    body.insert("CanSeek".to_string(), json!(can_seek(session, state)));
    body.insert("IsPaused".to_string(), json!(state.pause));
    body.insert("IsMuted".to_string(), json!(state.mute.unwrap_or(false)));
    body.insert("PlayMethod".to_string(), json!(session.play_method));
    body.insert(
        "VolumeLevel".to_string(),
        json!(state.volume.unwrap_or(100)),
    );
    body.insert("RepeatMode".to_string(), json!("RepeatNone"));
    body.insert("BufferedRanges".to_string(), json!([]));
}

fn insert_stop_body_fields(
    body: &mut Map<String, Value>,
    session: &PlaybackSession,
    state: &MpvPlaybackState,
) {
    body.insert("ItemId".to_string(), json!(session.item_id));
    insert_string_opt(body, "MediaSourceId", session.media_source_id.as_deref());
    insert_string_opt(body, "PlaySessionId", session.play_session_id.as_deref());
    insert_string_opt(body, "PlaylistItemId", session.playlist_item_id.as_deref());
    if let Some(queue) = &session.queue {
        body.insert("NowPlayingQueue".to_string(), queue.clone());
    }
    body.insert(
        "PositionTicks".to_string(),
        json!(state.position_ticks.max(0)),
    );
}

fn insert_string_opt(body: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = non_empty(value) {
        body.insert(key.to_string(), json!(value));
    }
}

fn insert_i64_opt(body: &mut Map<String, Value>, key: &str, value: Option<i64>) {
    if let Some(value) = value {
        body.insert(key.to_string(), json!(value));
    }
}

fn can_seek(session: &PlaybackSession, state: &MpvPlaybackState) -> bool {
    state.duration_ticks.unwrap_or(0) > 0 || session.runtime_ticks.unwrap_or(0) > 0
}

fn playback_auth_headers(launch: &MpvLaunch) -> Vec<HttpHeader> {
    let token = token_from_launch(launch);
    let mut headers = Vec::new();
    let mut seen = HashSet::<String>::new();

    for header in &launch.headers {
        let Some(name) = non_empty(Some(header.name.as_str())) else {
            continue;
        };
        if !is_forwardable_auth_header(name) {
            continue;
        }
        let Some(value) = non_empty(Some(header.value.as_str())) else {
            continue;
        };
        push_unique_header(&mut headers, &mut seen, name, value.to_string());
    }

    if let Some(token) = non_empty(token.as_deref()) {
        if !has_auth_header(&headers) {
            push_unique_header(
                &mut headers,
                &mut seen,
                "Authorization",
                minimal_authorization_header(token),
            );
        }
        if !has_token_header(&headers) {
            push_unique_header(&mut headers, &mut seen, "X-Emby-Token", token.to_string());
        }
    }

    headers
}

fn token_from_launch(launch: &MpvLaunch) -> Option<String> {
    for header_name in ["X-Emby-Token", "X-MediaBrowser-Token"] {
        if let Some(value) = header_value(launch, header_name) {
            return Some(value);
        }
    }

    if let Some(value) = header_value(launch, "Authorization")
        && let Some(token) = token_from_authorization(&value)
    {
        return Some(token);
    }

    for key in [
        "api_key",
        "apikey",
        "access_token",
        "accesstoken",
        "x-emby-token",
        "x-mediabrowser-token",
    ] {
        if let Some(value) = query_param_ci(&launch.media_url, key) {
            return Some(value);
        }
    }

    None
}

fn token_from_authorization(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("bearer ") {
        return non_empty(Some(&trimmed[7..])).map(str::to_string);
    }
    auth_parameter(trimmed, "Token")
}

fn auth_parameter(value: &str, key: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let needle = format!("{}=", key.to_ascii_lowercase());
    let index = lower.find(&needle)? + needle.len();
    let rest = value[index..].trim_start();
    if let Some(rest) = rest.strip_prefix('"') {
        let end = rest.find('"').unwrap_or(rest.len());
        return non_empty(Some(&rest[..end])).map(str::to_string);
    }
    let end = rest
        .find(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .unwrap_or(rest.len());
    non_empty(Some(&rest[..end])).map(str::to_string)
}

fn header_value(launch: &MpvLaunch, name: &str) -> Option<String> {
    launch.headers.iter().find_map(|header| {
        header
            .name
            .eq_ignore_ascii_case(name)
            .then(|| header.value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn has_auth_header(headers: &[HttpHeader]) -> bool {
    headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("Authorization")
            || header.name.eq_ignore_ascii_case("X-Emby-Authorization")
    })
}

fn has_token_header(headers: &[HttpHeader]) -> bool {
    headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("X-Emby-Token")
            || header.name.eq_ignore_ascii_case("X-MediaBrowser-Token")
    })
}

fn minimal_authorization_header(token: &str) -> String {
    format!("MediaBrowser Token=\"{}\"", sanitize_auth_value(token))
}

fn push_unique_header(
    headers: &mut Vec<HttpHeader>,
    seen: &mut HashSet<String>,
    name: &str,
    value: String,
) {
    let name = sanitize_header_name(name);
    let value = sanitize_header_value(&value);
    if name.is_empty() || value.is_empty() {
        return;
    }
    if seen.insert(name.to_ascii_lowercase()) {
        headers.push(HttpHeader { name, value });
    }
}

fn is_forwardable_auth_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "x-emby-authorization"
            | "x-emby-token"
            | "x-mediabrowser-token"
            | "cookie"
    )
}

fn sanitize_header_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect()
}

fn sanitize_header_value(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, '\0' | '\r' | '\n'))
        .collect()
}

fn sanitize_auth_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '\0' | '\r' | '\n' | '"'))
        .collect()
}

pub fn seconds_to_ticks(seconds: f64) -> Option<i64> {
    seconds
        .is_finite()
        .then(|| (seconds.max(0.0) * TICKS_PER_SECOND).round() as i64)
}

fn server_base_url(media_url: &str) -> Option<String> {
    let (scheme, _) = media_url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let scheme_end = scheme.len() + 3;
    let after_scheme = &media_url[scheme_end..];
    let host_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    if host_end == 0 {
        return None;
    }

    let origin_end = scheme_end + host_end;
    let origin = &media_url[..origin_end];
    let path = &media_url[origin_end..];
    let lower_path = path.to_ascii_lowercase();
    let base_path_end = lower_path
        .find("/videos/")
        .or_else(|| lower_path.find("/audio/"))
        .unwrap_or(0);
    let base_path = path[..base_path_end].trim_end_matches('/');

    Some(format!("{origin}{base_path}"))
}

fn join_api_url(base_url: &str, endpoint: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        endpoint.trim_start_matches('/')
    )
}

fn query_param_ci(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next().unwrap_or_default();
    query.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        percent_decode(raw_key)
            .eq_ignore_ascii_case(key)
            .then(|| percent_decode(raw_value))
            .filter(|value| !value.trim().is_empty())
    })
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
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

fn fill_string(target: &mut Option<String>, value: Option<&str>) {
    if non_empty(target.as_deref()).is_none()
        && let Some(value) = non_empty(value)
    {
        *target = Some(value.to_string());
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn display_opt(value: Option<&str>) -> &str {
    non_empty(value).unwrap_or("unknown")
}

fn ticks_summary(value: Option<i64>) -> String {
    value
        .map(|ticks| format!("{ticks} ({:.3}s)", ticks as f64 / TICKS_PER_SECOND))
        .unwrap_or_else(|| "unknown".to_string())
}

fn unix_now_ticks() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().saturating_mul(10_000) as i64)
        .unwrap_or_default()
}

#[cfg(not(target_os = "windows"))]
pub fn cleanup_ipc_path(path: &str) {
    let _ = std::fs::remove_file(path);
}

#[cfg(target_os = "windows")]
pub fn cleanup_ipc_path(_path: &str) {}

#[cfg(test)]
mod tests {
    use super::{
        MpvPlaybackState, PlaybackSession, auth_parameter, playback_progress_body,
        playback_stop_body, server_base_url, token_from_authorization, token_from_launch,
    };
    use crate::mpv::{HttpHeader, MpvLaunch};
    use serde_json::json;

    #[test]
    fn extracts_server_base_with_subpath() {
        assert_eq!(
            server_base_url("https://example.test/jellyfin/Videos/abc/stream.mkv?api_key=x")
                .as_deref(),
            Some("https://example.test/jellyfin")
        );
    }

    #[test]
    fn extracts_token_from_mediabrowser_authorization() {
        assert_eq!(
            token_from_authorization(
                "MediaBrowser Client=\"Jellyfin Web\", DeviceId=\"dev\", Token=\"secret\""
            )
            .as_deref(),
            Some("secret")
        );
        assert_eq!(
            auth_parameter("MediaBrowser DeviceId=dev, Token=secret", "DeviceId").as_deref(),
            Some("dev")
        );
    }

    #[test]
    fn extracts_token_from_jellyfin_apikey_query() {
        let launch = MpvLaunch::new("https://example.test/Videos/item/stream.mkv?ApiKey=secret");
        assert_eq!(token_from_launch(&launch).as_deref(), Some("secret"));
    }

    #[test]
    fn playback_bodies_include_richer_mpv_and_jellyfin_fields() {
        let mut launch = MpvLaunch::new("https://example.test/Videos/item/stream.mkv");
        launch.item_id = Some("item".to_string());
        launch.media_source_id = Some("media".to_string());
        launch.play_session_id = Some("session".to_string());
        launch.headers = vec![HttpHeader {
            name: "X-Emby-Token".to_string(),
            value: "secret".to_string(),
        }];
        launch.audio_stream_index = Some(2);
        launch.subtitle_stream_index = Some(5);
        launch.play_method = Some("DirectStream".to_string());
        launch.runtime_ticks = Some(120_000_000);
        launch.queue = Some(json!([
            {
                "Id": "item",
                "PlaylistItemId": "playlist-item"
            }
        ]));
        launch.details = Some(json!({ "NotAQueue": true }));

        let session = PlaybackSession::from_launch(&launch).expect("session");
        let state = MpvPlaybackState {
            position_ticks: 10_000_000,
            pause: true,
            duration_ticks: Some(120_000_000),
            volume: Some(77),
            mute: Some(true),
            eof_reached: false,
        };
        let progress = playback_progress_body(&session, &state);
        assert_eq!(progress["ItemId"], "item");
        assert_eq!(progress["MediaSourceId"], "media");
        assert_eq!(progress["PlaySessionId"], "session");
        assert_eq!(progress["AudioStreamIndex"], 2);
        assert_eq!(progress["SubtitleStreamIndex"], 5);
        assert_eq!(progress["PlayMethod"], "DirectStream");
        assert_eq!(progress["VolumeLevel"], 77);
        assert_eq!(progress["IsMuted"], true);
        assert_eq!(progress["IsPaused"], true);
        assert_eq!(progress["CanSeek"], true);
        assert_eq!(progress["RepeatMode"], "RepeatNone");
        assert_eq!(progress["BufferedRanges"].as_array().unwrap().len(), 0);
        assert_eq!(progress["NowPlayingQueue"].as_array().unwrap().len(), 1);

        let stopped = playback_stop_body(&session, &state, true);
        assert_eq!(stopped["Failed"], true);
        assert_eq!(stopped["PositionTicks"], 10_000_000);
        assert_eq!(stopped["NowPlayingQueue"].as_array().unwrap().len(), 1);
        assert!(stopped.get("AudioStreamIndex").is_none());
        assert!(stopped.get("SubtitleStreamIndex").is_none());
        assert!(stopped.get("CanSeek").is_none());
        assert!(stopped.get("VolumeLevel").is_none());
        assert!(stopped.get("NotAQueue").is_none());
    }

    #[test]
    fn playback_bodies_drop_invalid_queue_shapes() {
        let mut launch = MpvLaunch::new("https://example.test/Videos/item/stream.mkv");
        launch.item_id = Some("item".to_string());
        launch.headers = vec![HttpHeader {
            name: "X-Emby-Token".to_string(),
            value: "secret".to_string(),
        }];
        launch.queue = Some(json!({ "bad": true }));

        let session = PlaybackSession::from_launch(&launch).expect("session");
        let state = MpvPlaybackState::default();
        assert!(
            playback_progress_body(&session, &state)
                .get("NowPlayingQueue")
                .is_none()
        );
        assert!(
            playback_stop_body(&session, &state, false)
                .get("NowPlayingQueue")
                .is_none()
        );
    }

    #[test]
    fn mpv_playback_state_summary_is_stable() {
        let state = MpvPlaybackState {
            position_ticks: 10_000_000,
            pause: true,
            duration_ticks: Some(120_000_000),
            volume: Some(77),
            mute: Some(false),
            eof_reached: false,
        };
        assert_eq!(
            state.to_string(),
            "position=10000000 (1.000s) duration=120000000 (12.000s) paused=true volume=77 muted=false eof=false"
        );
    }
}
