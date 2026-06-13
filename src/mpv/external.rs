use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Stdio;
use std::process::{Child, Command};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use crate::windows::install_hidden_command_processor_shim;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ExternalMpv {
    executable: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MpvLaunch {
    #[serde(alias = "url")]
    pub media_url: String,
    pub headers: Vec<HttpHeader>,
    pub item_id: Option<String>,
    pub media_source_id: Option<String>,
    pub play_session_id: Option<String>,
    pub device_id: Option<String>,
    #[serde(alias = "startPositionTicks")]
    pub start_time_ticks: Option<i64>,
    pub start_milliseconds: Option<f64>,
    pub runtime_ticks: Option<i64>,
    pub title: Option<String>,
    pub audio_stream_index: Option<i64>,
    pub subtitle_stream_index: Option<i64>,
    pub audio_mpv_id: Option<i64>,
    pub subtitle_mpv_id: Option<i64>,
    pub subtitle_url: Option<String>,
    pub play_method: Option<String>,
    pub playlist_item_id: Option<String>,
    pub queue: Option<Value>,
    pub details: Option<Value>,
}

impl MpvLaunch {
    pub fn new(media_url: impl Into<String>) -> Self {
        Self {
            media_url: media_url.into(),
            ..Default::default()
        }
    }

    pub fn start_seconds(&self) -> Option<f64> {
        if let Some(milliseconds) = self.start_milliseconds.filter(|value| *value > 0.0) {
            return Some(milliseconds / 1000.0);
        }
        self.start_time_ticks
            .filter(|ticks| *ticks > 0)
            .map(|ticks| ticks as f64 / 10_000_000.0)
    }

    pub fn dedupe_key(&self) -> String {
        if let Some(play_session_id) = non_empty(self.play_session_id.as_deref()) {
            return format!("play-session:{play_session_id}");
        }
        if let (Some(item_id), Some(media_source_id)) = (
            non_empty(self.item_id.as_deref()),
            non_empty(self.media_source_id.as_deref()),
        ) {
            return format!("item:{item_id}:source:{media_source_id}");
        }
        redact_url_query_value(
            &self.media_url,
            &[
                "api_key",
                "apikey",
                "access_token",
                "accesstoken",
                "x-emby-token",
                "x-mediabrowser-token",
            ],
        )
    }

    pub fn merge_missing_from(&mut self, other: &Self) {
        if self.media_url.trim().is_empty() {
            self.media_url = other.media_url.clone();
        }
        if self.headers.is_empty() {
            self.headers = other.headers.clone();
        }
        merge_option(&mut self.item_id, &other.item_id);
        merge_option(&mut self.media_source_id, &other.media_source_id);
        merge_option(&mut self.play_session_id, &other.play_session_id);
        merge_option(&mut self.device_id, &other.device_id);
        merge_option(&mut self.start_time_ticks, &other.start_time_ticks);
        merge_option(&mut self.start_milliseconds, &other.start_milliseconds);
        merge_option(&mut self.runtime_ticks, &other.runtime_ticks);
        merge_option(&mut self.title, &other.title);
        merge_option(&mut self.audio_stream_index, &other.audio_stream_index);
        merge_option(
            &mut self.subtitle_stream_index,
            &other.subtitle_stream_index,
        );
        merge_option(&mut self.audio_mpv_id, &other.audio_mpv_id);
        merge_option(&mut self.subtitle_mpv_id, &other.subtitle_mpv_id);
        merge_option(&mut self.subtitle_url, &other.subtitle_url);
        merge_option(&mut self.play_method, &other.play_method);
        merge_option(&mut self.playlist_item_id, &other.playlist_item_id);
        merge_option(&mut self.queue, &other.queue);
        merge_option(&mut self.details, &other.details);
    }
}

impl ExternalMpv {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Build a plain command for a media URL.
    #[allow(dead_code)]
    pub fn command_for_url(&self, media_url: &str) -> Command {
        self.command_for_launch(&MpvLaunch::new(media_url))
    }

    pub fn command_for_launch(&self, launch: &MpvLaunch) -> Command {
        self.command_for_launch_with_ipc(launch, None)
    }

    pub fn command_for_launch_with_ipc(
        &self,
        launch: &MpvLaunch,
        ipc_path: Option<&str>,
    ) -> Command {
        let mut command = self.hidden_command();
        command.arg("--force-window=yes");
        command.arg("--fullscreen=yes");
        command.arg("--no-terminal");
        // Keep user/package mpv scripts available (SVP needs mpvSockets.lua).
        // Windows `os.execute(...)` console flashes from those scripts are hidden
        // by the command processor shim installed on the mpv child environment.
        command.arg("--load-scripts=yes");

        if let Some(ipc_path) = non_empty(ipc_path) {
            command.arg(format!("--input-ipc-server={ipc_path}"));
        }

        for header in mpv_headers(launch) {
            command.arg(format!(
                "--http-header-fields-append={}: {}",
                header.name, header.value
            ));
        }

        if let Some(start_seconds) = launch.start_seconds() {
            command.arg(format!("--start={start_seconds:.3}"));
        }

        if let Some(title) = non_empty(launch.title.as_deref()) {
            command.arg(format!("--force-media-title={}", sanitize_arg_value(title)));
        }

        for (key, value) in launch.script_metadata() {
            command.arg(format!("--script-opts-append=jellyfin_{key}={value}"));
        }

        command.arg(&launch.media_url);
        command
    }

    pub fn command_for_idle_with_ipc(&self, ipc_path: &str) -> Command {
        let mut command = self.hidden_command();
        command.arg("--force-window=yes");
        command.arg("--fullscreen=yes");
        command.arg("--no-terminal");
        // Keep user/package mpv scripts available (SVP needs mpvSockets.lua).
        // Windows `os.execute(...)` console flashes from those scripts are hidden
        // by the command processor shim installed on the mpv child environment.
        command.arg("--load-scripts=yes");
        command.arg("--idle=yes");
        command.arg(format!("--input-ipc-server={ipc_path}"));
        command
    }

    #[allow(dead_code)]
    pub fn spawn(&self, launch: &MpvLaunch) -> std::io::Result<Child> {
        self.command_for_launch(launch).spawn()
    }

    fn hidden_command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        configure_hidden_child_window(&mut command);
        command
    }
}

#[cfg(windows)]
fn configure_hidden_child_window(command: &mut Command) {
    // Windows mpv builds include both mpv.exe and mpv.com. If the user configures
    // mpv.com, or the bare `mpv` name resolves to that console wrapper, Windows
    // may allocate a transient console window even though mpv is later run with
    // `--no-terminal`. CREATE_NO_WINDOW suppresses that console without changing
    // the mpv window itself.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
    install_hidden_command_processor_shim(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
}

#[cfg(not(windows))]
fn configure_hidden_child_window(_command: &mut Command) {}

impl MpvLaunch {
    fn script_metadata(&self) -> Vec<(&'static str, String)> {
        let mut values = Vec::new();
        push_metadata(&mut values, "item_id", self.item_id.as_deref());
        push_metadata(
            &mut values,
            "media_source_id",
            self.media_source_id.as_deref(),
        );
        push_metadata(
            &mut values,
            "play_session_id",
            self.play_session_id.as_deref(),
        );
        push_metadata(&mut values, "device_id", self.device_id.as_deref());
        if let Some(ticks) = self.start_time_ticks.filter(|ticks| *ticks > 0) {
            values.push(("start_ticks", ticks.to_string()));
        }
        if let Some(ticks) = self.runtime_ticks.filter(|ticks| *ticks > 0) {
            values.push(("runtime_ticks", ticks.to_string()));
        }
        values
    }
}

fn mpv_headers(launch: &MpvLaunch) -> Vec<HttpHeader> {
    let mut headers = Vec::<HttpHeader>::new();
    for header in &launch.headers {
        let name = sanitize_header_name(&header.name);
        let value = sanitize_header_value(&header.value);
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
            .map(|value| sanitize_header_value(&value))
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

fn sanitize_header_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>()
}

fn sanitize_header_value(value: &str) -> String {
    sanitize_arg_value(value.trim())
}

fn sanitize_arg_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '\0' | '\r' | '\n'))
        .collect::<String>()
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn merge_option<T: Clone>(target: &mut Option<T>, source: &Option<T>) {
    if target.is_none() {
        *target = source.clone();
    }
}

fn push_metadata(values: &mut Vec<(&'static str, String)>, key: &'static str, value: Option<&str>) {
    if let Some(value) = non_empty(value) {
        values.push((key, sanitize_arg_value(value)));
    }
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

fn redact_url_query_value(url: &str, keys: &[&str]) -> String {
    let Some((before_query, rest)) = url.split_once('?') else {
        return url.to_string();
    };
    let (query, fragment) = rest
        .split_once('#')
        .map(|(query, fragment)| (query, Some(fragment)))
        .unwrap_or((rest, None));
    let redacted = query
        .split('&')
        .map(|pair| {
            let Some((raw_key, _raw_value)) = pair.split_once('=') else {
                return pair.to_string();
            };
            let decoded_key = percent_decode(raw_key);
            if keys.iter().any(|key| decoded_key.eq_ignore_ascii_case(key)) {
                format!("{raw_key}=REDACTED")
            } else {
                pair.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    match fragment {
        Some(fragment) => format!("{before_query}?{redacted}#{fragment}"),
        None => format!("{before_query}?{redacted}"),
    }
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
