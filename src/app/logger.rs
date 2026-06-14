use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;

use crate::app::settings::config_dir;
use crate::mpv::{HttpHeader, MpvLaunch};

const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_BACKUPS: usize = 3;

const SECRET_QUERY_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "access_token",
    "accesstoken",
    "x-emby-token",
    "x-mediabrowser-token",
    "token",
];

const SECRET_JSON_KEYS: &[&str] = &[
    "access_token",
    "accesstoken",
    "apikey",
    "api_key",
    "authorization",
    "cookie",
    "token",
    "x-emby-authorization",
    "x-emby-token",
    "x-mediabrowser-token",
];

const SECRET_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "x-emby-authorization",
    "x-emby-token",
    "x-mediabrowser-token",
];

pub struct LogGuard {
    _console_guard: WorkerGuard,
    _file_guard: Option<WorkerGuard>,
}

pub fn default_log_file_path() -> PathBuf {
    config_dir().join("mediaflick-desktop.log")
}

pub fn init(path: PathBuf, filter: &str) -> LogGuard {
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "Failed to create app log directory {}: {error}",
            parent.display()
        );
    }

    let (console_writer, console_guard) = NonBlockingBuilder::default()
        .lossy(true)
        .finish(io::stderr());

    let (file_writer, file_guard) = match RotatingFile::open(path.clone()) {
        Ok(file) => {
            let (writer, guard) = NonBlockingBuilder::default().lossy(true).finish(file);
            (Some(writer), Some(guard))
        }
        Err(error) => {
            eprintln!("Failed to open app log file {}: {error}", path.display());
            (None, None)
        }
    };

    let env_filter = EnvFilter::try_new(non_empty(filter).unwrap_or("debug"))
        .unwrap_or_else(|_| EnvFilter::new("debug"));
    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_names(true)
        .with_writer(console_writer);

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer);

    if let Some(file_writer) = file_writer {
        let file_layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_target(true)
            .with_thread_names(true)
            .with_writer(file_writer);
        let _ = tracing::subscriber::set_global_default(subscriber.with(file_layer));
    } else {
        let _ = tracing::subscriber::set_global_default(subscriber);
    }

    tracing::info!(
        target: "main",
        path = %path.display(),
        filter = %non_empty(filter).unwrap_or("debug"),
        "app logging initialized"
    );

    LogGuard {
        _console_guard: console_guard,
        _file_guard: file_guard,
    }
}

struct RotatingFile {
    path: PathBuf,
    file: File,
    bytes_written: u64,
}

impl RotatingFile {
    fn open(path: PathBuf) -> io::Result<Self> {
        rotate_backups(&path)?;
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self {
            path,
            file,
            bytes_written: 0,
        })
    }

    fn maybe_rotate(&mut self, incoming: usize) -> io::Result<()> {
        if self.bytes_written + incoming as u64 <= MAX_FILE_BYTES {
            return Ok(());
        }
        self.file.flush()?;
        rotate_backups(&self.path)?;
        self.file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.path)?;
        self.bytes_written = 0;
        Ok(())
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.maybe_rotate(buf.len())?;
        let written = self.file.write(buf)?;
        self.bytes_written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn rotate_backups(path: &Path) -> io::Result<()> {
    let _ = std::fs::remove_file(backup_path(path, MAX_BACKUPS));
    for index in (1..MAX_BACKUPS).rev() {
        let source = backup_path(path, index);
        if source.exists() {
            std::fs::rename(source, backup_path(path, index + 1))?;
        }
    }
    if path.exists() {
        std::fs::rename(path, backup_path(path, 1))?;
    }
    Ok(())
}

fn backup_path(path: &Path, index: usize) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(format!(".{index}"));
    PathBuf::from(value)
}

pub fn launch_summary(launch: &MpvLaunch) -> String {
    format!(
        "item={} media_source={} play_session={} start={} audio={} subtitle={} url={} headers=[{}]",
        display_opt(launch.item_id.as_deref()),
        display_opt(launch.media_source_id.as_deref()),
        display_opt(launch.play_session_id.as_deref()),
        launch
            .start_seconds()
            .map(|seconds| format!("{seconds:.3}s"))
            .unwrap_or_else(|| "none".to_string()),
        launch
            .audio_stream_index
            .map(|index| format!("{index}/mpv:{}", display_i64_opt(launch.audio_mpv_id)))
            .unwrap_or_else(|| "none".to_string()),
        launch
            .subtitle_stream_index
            .map(|index| {
                if launch
                    .subtitle_url
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|url| !url.is_empty())
                {
                    format!("{index}/external")
                } else {
                    format!("{index}/mpv:{}", display_i64_opt(launch.subtitle_mpv_id))
                }
            })
            .unwrap_or_else(|| "none".to_string()),
        redact_url_secrets(&launch.media_url),
        header_names(&launch.headers)
    )
}

pub fn mpv_command_summary(command: &Value) -> String {
    let Some(args) = command.get("command").and_then(Value::as_array) else {
        return "unknown command".to_string();
    };
    let name = args.first().and_then(Value::as_str).unwrap_or("unknown");
    match name {
        "loadfile" => {
            let url = args
                .get(1)
                .and_then(Value::as_str)
                .map(redact_url_secrets)
                .unwrap_or_else(|| "unknown".to_string());
            let mode = args.get(2).and_then(Value::as_str).unwrap_or("unknown");
            format!("loadfile mode={mode} url={url}")
        }
        other => other.to_string(),
    }
}

pub fn redacted_json(value: &Value) -> String {
    serde_json::to_string(&redacted_value(value)).unwrap_or_else(|_| "<unserializable>".to_string())
}

pub fn redacted_value(value: &Value) -> Value {
    let mut value = value.clone();
    redact_json_value(None, &mut value);
    value
}

pub fn redact_url_secrets(url: &str) -> String {
    redact_url_query_value(url, SECRET_QUERY_KEYS)
}

pub fn redact_text(input: &str) -> String {
    let mut value = redact_url_query_value(input, SECRET_QUERY_KEYS);
    value = redact_json_text_fields(&value);
    redact_header_text_fields(&value)
}

pub fn redacted_header_summary(headers: &[HttpHeader]) -> String {
    header_names(headers)
}

fn redact_json_value(key: Option<&str>, value: &mut Value) {
    if key.is_some_and(is_secret_json_key) {
        *value = Value::String("REDACTED".to_string());
        return;
    }

    match value {
        Value::String(text) => {
            if key.is_some_and(|key| key.eq_ignore_ascii_case("http-header-fields")) {
                *text = redact_mpv_header_fields(text);
            } else {
                *text = redact_text(text);
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_json_value(None, value);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                redact_json_value(Some(key), value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn redact_mpv_header_fields(input: &str) -> String {
    split_mpv_string_list(input)
        .into_iter()
        .map(|entry| {
            let Some((name, _value)) = entry.split_once(':') else {
                return redact_text(&entry);
            };
            if is_secret_header(name.trim()) {
                format!("{}: REDACTED", name.trim())
            } else {
                redact_text(&entry)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn split_mpv_string_list(input: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == ',' {
            values.push(current);
            current = String::new();
            continue;
        }
        current.push(ch);
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() || input.ends_with(',') {
        values.push(current);
    }
    values
}

fn redact_url_query_value(input: &str, keys: &[&str]) -> String {
    let mut output = input.to_string();
    for key in keys {
        output = redact_after_patterns(
            &output,
            &[
                format!("{key}="),
                format!("{}=", key.to_ascii_lowercase()),
                format!("{}=", key.to_ascii_uppercase()),
                format!("{}%3D", key),
                format!("{}%3d", key),
            ],
            "&\"' \t\r\n;<>#",
        );
    }
    output
}

fn redact_json_text_fields(input: &str) -> String {
    let mut output = input.to_string();
    for key in SECRET_JSON_KEYS {
        output = redact_after_patterns(
            &output,
            &[
                format!("\"{key}\":\""),
                format!("\"{key}\": \""),
                format!("'{key}': '"),
                format!("{}=\"", key),
                format!("{}='", key),
                format!("{}=", key),
            ],
            "\"'\r\n, }]",
        );
    }
    output
}

fn redact_header_text_fields(input: &str) -> String {
    let mut output = input.to_string();
    for header in SECRET_HEADERS {
        output = redact_header_value(&output, header);
    }
    output
}

fn redact_header_value(input: &str, header: &str) -> String {
    let mut output = input.to_string();
    let needle = format!("{header}:").to_ascii_lowercase();
    let mut start = 0;
    loop {
        let lower = output.to_ascii_lowercase();
        let Some(relative) = lower[start..].find(&needle) else {
            break;
        };
        let mut value_start = start + relative + needle.len();
        if output[value_start..].starts_with(' ') {
            value_start += 1;
        }
        let value_end = output[value_start..]
            .find(['\r', '\n'])
            .map(|index| value_start + index)
            .unwrap_or(output.len());
        if value_start < value_end {
            output.replace_range(value_start..value_end, "REDACTED");
        }
        start = value_start.saturating_add("REDACTED".len());
        if start >= output.len() {
            break;
        }
    }
    output
}

fn redact_after_patterns(input: &str, patterns: &[String], terminators: &str) -> String {
    let mut output = input.to_string();
    for pattern in patterns {
        let pattern_lower = pattern.to_ascii_lowercase();
        let mut start = 0;
        let mut lower = output.to_ascii_lowercase();
        while let Some(relative) = lower[start..].find(&pattern_lower) {
            let value_start = start + relative + pattern.len();
            let value_end = output[value_start..]
                .find(|ch| terminators.contains(ch))
                .map(|index| value_start + index)
                .unwrap_or(output.len());
            if value_start < value_end {
                output.replace_range(value_start..value_end, "REDACTED");
            }
            start = value_start.saturating_add("REDACTED".len());
            if start >= lower.len() {
                break;
            }
            lower = output.to_ascii_lowercase();
        }
    }
    output
}

fn display_i64_opt(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn header_names(headers: &[HttpHeader]) -> String {
    headers
        .iter()
        .map(|header| {
            let name = header.name.trim();
            if is_secret_header(name) {
                format!("{name}: REDACTED")
            } else {
                name.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_secret_json_key(key: &str) -> bool {
    SECRET_JSON_KEYS
        .iter()
        .any(|secret| key.eq_ignore_ascii_case(secret))
}

fn is_secret_header(key: &str) -> bool {
    SECRET_HEADERS
        .iter()
        .any(|secret| key.eq_ignore_ascii_case(secret))
}

fn display_opt(value: Option<&str>) -> &str {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        launch_summary, mpv_command_summary, redact_text, redact_url_secrets, redacted_json,
    };
    use crate::mpv::{HttpHeader, MpvLaunch};

    #[test]
    fn redacts_query_tokens() {
        assert_eq!(
            redact_url_secrets("https://server/Videos/1/stream.mkv?ApiKey=secret&Static=true"),
            "https://server/Videos/1/stream.mkv?ApiKey=REDACTED&Static=true"
        );
    }

    #[test]
    fn redacts_header_and_json_tokens() {
        assert_eq!(
            redact_text("Authorization: MediaBrowser Token=\"secret\"\n"),
            "Authorization: REDACTED\n"
        );
        assert_eq!(
            redact_text("{\"AccessToken\":\"secret\",\"ItemId\":\"item\"}"),
            "{\"AccessToken\":\"REDACTED\",\"ItemId\":\"item\"}"
        );
    }

    #[test]
    fn redacts_mpv_header_fields() {
        let command = json!({
            "command": [
                "loadfile",
                "https://server/Videos/1/stream.mkv?api_key=secret",
                "replace",
                -1,
                {
                    "http-header-fields": "Authorization: MediaBrowser Client=\"Jellyfin\"\\, Token=\"secret\",User-Agent: mediaflick-desktop"
                }
            ],
            "request_id": 1
        });
        let text = redacted_json(&command);
        assert!(text.contains("Authorization: REDACTED"));
        assert!(text.contains("User-Agent: mediaflick-desktop"));
        assert!(!text.contains("secret"));
    }

    #[test]
    fn launch_and_command_summaries_are_sanitized() {
        let mut launch = MpvLaunch::new("https://server/Videos/1/stream.mkv?api_key=secret");
        launch.item_id = Some("item".to_string());
        launch.headers = vec![HttpHeader {
            name: "X-Emby-Token".to_string(),
            value: "secret".to_string(),
        }];
        let summary = launch_summary(&launch);
        assert!(summary.contains("item=item"));
        assert!(summary.contains("api_key=REDACTED"));
        assert!(summary.contains("X-Emby-Token: REDACTED"));
        assert!(!summary.contains("secret"));

        let command = json!({
            "command": ["loadfile", launch.media_url, "replace", -1, {}],
            "request_id": 7
        });
        let command_summary = mpv_command_summary(&command);
        assert!(command_summary.contains("loadfile"));
        assert!(command_summary.contains("api_key=REDACTED"));
        assert!(!command_summary.contains("secret"));
    }
}
