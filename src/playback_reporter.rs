use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Write};
use std::process::Child;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use crate::external_mpv::{HttpHeader, MpvLaunch};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);
const START_GRACE: Duration = Duration::from_secs(2);
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);
const TICKS_PER_SECOND: f64 = 10_000_000.0;

static IPC_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
struct PlaybackSession {
    base_url: String,
    item_id: String,
    media_source_id: Option<String>,
    play_session_id: Option<String>,
    start_position_ticks: i64,
    runtime_ticks: Option<i64>,
    playback_start_time_ticks: i64,
    auth_headers: Vec<HttpHeader>,
}

#[derive(Debug, Clone, Copy, Default)]
struct MpvStatus {
    time_pos: Option<f64>,
    pause: Option<bool>,
    duration: Option<f64>,
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
            r"\\.\pipe\jellyfin-mpv-{}-{timestamp}-{counter}",
            std::process::id()
        )
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::temp_dir()
            .join(format!(
                "jellyfin-mpv-{}-{timestamp}-{counter}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }
}

pub fn monitor_mpv_playback(mut child: Child, launch: MpvLaunch, ipc_path: String) {
    thread::spawn(move || {
        let Some(session) = PlaybackSession::from_launch(&launch) else {
            let _ = child.wait();
            cleanup_ipc_path(&ipc_path);
            return;
        };

        let mut reporter = PlaybackReporter::new(session);
        let process_started = Instant::now();
        let mut last_position_ticks = reporter.session.start_position_ticks.max(0);
        let mut last_pause = false;
        let mut reported_pause = false;
        let mut started = false;
        let mut last_progress_sent: Option<Instant> = None;

        loop {
            let now = Instant::now();
            if let Ok(Some(_status)) = child.try_wait() {
                break;
            }

            if let Ok(status) = query_mpv_status(&ipc_path) {
                if let Some(position) = status.time_pos.and_then(seconds_to_ticks) {
                    last_position_ticks = position;
                }
                if let Some(paused) = status.pause {
                    last_pause = paused;
                }
                if let Some(duration) = status.duration.and_then(seconds_to_ticks)
                    && duration > 0
                    && reporter.session.runtime_ticks.is_none()
                {
                    reporter.session.runtime_ticks = Some(duration);
                }
            } else if !last_pause {
                last_position_ticks = estimate_position_ticks(
                    reporter.session.start_position_ticks,
                    now.saturating_duration_since(process_started),
                );
            }

            if !started && now.saturating_duration_since(process_started) >= START_GRACE {
                reporter.report_start(last_position_ticks, last_pause);
                started = true;
                reported_pause = last_pause;
                last_progress_sent = Some(now);
            }

            if started {
                let should_report_progress = last_progress_sent
                    .map(|instant| now.saturating_duration_since(instant) >= PROGRESS_INTERVAL)
                    .unwrap_or(true)
                    || reported_pause != last_pause;

                if should_report_progress {
                    reporter.report_progress(last_position_ticks, last_pause);
                    reported_pause = last_pause;
                    last_progress_sent = Some(now);
                }
            }

            thread::sleep(POLL_INTERVAL);
        }

        if let Ok(status) = query_mpv_status(&ipc_path) {
            if let Some(position) = status.time_pos.and_then(seconds_to_ticks) {
                last_position_ticks = position;
            }
            if let Some(paused) = status.pause {
                last_pause = paused;
            }
        }

        if !started {
            reporter.report_start(last_position_ticks, last_pause);
        }
        reporter.report_stopped(last_position_ticks);

        let _ = child.wait();
        cleanup_ipc_path(&ipc_path);
    });
}

struct PlaybackReporter {
    session: PlaybackSession,
    agent: ureq::Agent,
}

impl PlaybackReporter {
    fn new(session: PlaybackSession) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .user_agent(format!("jellyfin-mpv/{}", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            session,
            agent: config.into(),
        }
    }

    fn report_start(&self, position_ticks: i64, is_paused: bool) {
        self.post_playstate(
            "Sessions/Playing",
            playback_progress_body(&self.session, position_ticks, is_paused),
        );
    }

    fn report_progress(&self, position_ticks: i64, is_paused: bool) {
        self.post_playstate(
            "Sessions/Playing/Progress",
            playback_progress_body(&self.session, position_ticks, is_paused),
        );
    }

    fn report_stopped(&self, position_ticks: i64) {
        self.post_playstate(
            "Sessions/Playing/Stopped",
            playback_stop_body(&self.session, position_ticks),
        );
    }

    fn post_playstate(&self, endpoint: &str, body: Value) {
        let url = join_api_url(&self.session.base_url, endpoint);
        let mut request = self
            .agent
            .post(url.as_str())
            .header("Accept", "application/json");

        for header in &self.session.auth_headers {
            request = request.header(header.name.as_str(), header.value.as_str());
        }

        if let Err(error) = request.send_json(&body) {
            eprintln!("Failed to report Jellyfin playback state to {endpoint}: {error}");
        }
    }
}

impl PlaybackSession {
    fn from_launch(launch: &MpvLaunch) -> Option<Self> {
        let item_id = non_empty(launch.item_id.as_deref())?.to_string();
        let base_url = server_base_url(&launch.media_url)?;
        let auth_headers = playback_auth_headers(launch);
        if auth_headers.is_empty() {
            eprintln!(
                "Cannot report Jellyfin playback state for item {item_id}: missing auth token"
            );
            return None;
        }

        Some(Self {
            base_url,
            item_id,
            media_source_id: non_empty(launch.media_source_id.as_deref()).map(str::to_string),
            play_session_id: non_empty(launch.play_session_id.as_deref()).map(str::to_string),
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
        })
    }
}

fn playback_progress_body(
    session: &PlaybackSession,
    position_ticks: i64,
    is_paused: bool,
) -> Value {
    let mut body = Map::new();
    body.insert("ItemId".to_string(), json!(session.item_id));
    insert_string_opt(
        &mut body,
        "MediaSourceId",
        session.media_source_id.as_deref(),
    );
    insert_string_opt(
        &mut body,
        "PlaySessionId",
        session.play_session_id.as_deref(),
    );
    body.insert("PositionTicks".to_string(), json!(position_ticks.max(0)));
    body.insert(
        "PlaybackStartTimeTicks".to_string(),
        json!(session.playback_start_time_ticks),
    );
    body.insert(
        "CanSeek".to_string(),
        json!(session.runtime_ticks.unwrap_or(0) > 0),
    );
    body.insert("IsPaused".to_string(), json!(is_paused));
    body.insert("IsMuted".to_string(), json!(false));
    body.insert("PlayMethod".to_string(), json!("DirectPlay"));
    body.insert("VolumeLevel".to_string(), json!(100));
    Value::Object(body)
}

fn playback_stop_body(session: &PlaybackSession, position_ticks: i64) -> Value {
    let mut body = Map::new();
    body.insert("ItemId".to_string(), json!(session.item_id));
    insert_string_opt(
        &mut body,
        "MediaSourceId",
        session.media_source_id.as_deref(),
    );
    insert_string_opt(
        &mut body,
        "PlaySessionId",
        session.play_session_id.as_deref(),
    );
    body.insert("PositionTicks".to_string(), json!(position_ticks.max(0)));
    body.insert("Failed".to_string(), json!(false));
    Value::Object(body)
}

fn insert_string_opt(body: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = non_empty(value) {
        body.insert(key.to_string(), json!(value));
    }
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

fn query_mpv_status(ipc_path: &str) -> io::Result<MpvStatus> {
    let mut stream = connect_ipc(ipc_path)?;
    write_ipc_command(&mut stream, 1, "time-pos")?;
    write_ipc_command(&mut stream, 2, "pause")?;
    write_ipc_command(&mut stream, 3, "duration")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut status = MpvStatus::default();
    let mut seen = 0_u8;
    let mut line = String::new();

    for _ in 0..16 {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(request_id) = value.get("request_id").and_then(Value::as_i64) else {
            continue;
        };
        match request_id {
            1 => {
                status.time_pos = value.get("data").and_then(Value::as_f64);
                seen |= 0b001;
            }
            2 => {
                status.pause = value.get("data").and_then(Value::as_bool);
                seen |= 0b010;
            }
            3 => {
                status.duration = value.get("data").and_then(Value::as_f64);
                seen |= 0b100;
            }
            _ => {}
        }
        if seen == 0b111 {
            break;
        }
    }

    Ok(status)
}

fn write_ipc_command<W: Write>(stream: &mut W, request_id: i64, property: &str) -> io::Result<()> {
    let command = json!({
        "command": ["get_property", property],
        "request_id": request_id
    });
    serde_json::to_writer(&mut *stream, &command)?;
    stream.write_all(b"\n")
}

#[cfg(target_os = "windows")]
type IpcConnection = std::fs::File;

#[cfg(target_os = "windows")]
fn connect_ipc(path: &str) -> io::Result<IpcConnection> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
}

#[cfg(all(unix, not(target_os = "windows")))]
type IpcConnection = std::os::unix::net::UnixStream;

#[cfg(all(unix, not(target_os = "windows")))]
fn connect_ipc(path: &str) -> io::Result<IpcConnection> {
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(stream)
}

fn estimate_position_ticks(start_ticks: i64, elapsed: Duration) -> i64 {
    start_ticks.saturating_add((elapsed.as_secs_f64() * TICKS_PER_SECOND).round() as i64)
}

fn seconds_to_ticks(seconds: f64) -> Option<i64> {
    seconds
        .is_finite()
        .then(|| (seconds.max(0.0) * TICKS_PER_SECOND).round() as i64)
}

fn server_base_url(media_url: &str) -> Option<String> {
    let scheme_end = media_url.find("://")? + 3;
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

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn unix_now_ticks() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().saturating_mul(10_000) as i64)
        .unwrap_or_default()
}

#[cfg(not(target_os = "windows"))]
fn cleanup_ipc_path(path: &str) {
    let _ = std::fs::remove_file(path);
}

#[cfg(target_os = "windows")]
fn cleanup_ipc_path(_path: &str) {}

#[cfg(test)]
mod tests {
    use super::{auth_parameter, server_base_url, token_from_authorization, token_from_launch};
    use crate::external_mpv::MpvLaunch;

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
}
