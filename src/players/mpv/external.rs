use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Stdio;
use std::process::{Child, Command};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use crate::windows::install_hidden_command_processor_shim;

use crate::playback::{HttpHeader, PlaybackRequest};
use crate::preferences::FullscreenBehavior;

const WINDOWED_AUTOFIT: &str = "70%";

#[derive(Debug, Clone)]
pub struct ExternalMpv {
    executable: PathBuf,
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
        self.command_for_launch(&PlaybackRequest::new(media_url))
    }

    pub fn command_for_launch(&self, launch: &PlaybackRequest) -> Command {
        self.command_for_launch_with_ipc(launch, None)
    }

    pub fn command_for_launch_with_ipc(
        &self,
        launch: &PlaybackRequest,
        ipc_path: Option<&str>,
    ) -> Command {
        let mut command = self.hidden_command();
        command.arg("--force-window=yes");
        command.arg("--fullscreen=yes");
        command.arg(format!("--autofit={WINDOWED_AUTOFIT}"));
        configure_focus_on_file_load(&mut command);
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

    #[allow(dead_code)]
    pub fn command_for_idle_with_ipc(&self, ipc_path: &str) -> Command {
        self.command_for_idle_with_ipc_and_fullscreen(ipc_path, FullscreenBehavior::Fullscreen)
    }

    pub fn command_for_idle_with_ipc_and_fullscreen(
        &self,
        ipc_path: &str,
        fullscreen: FullscreenBehavior,
    ) -> Command {
        let mut command = self.hidden_command();
        command.arg("--force-window=no");
        command.arg(format!("--fullscreen={}", fullscreen.fullscreen_arg()));
        command.arg(format!("--autofit={WINDOWED_AUTOFIT}"));
        configure_focus_on_file_load(&mut command);
        command.arg("--no-terminal");
        command.arg("--input-default-bindings=yes");
        command.arg("--input-vo-keyboard=yes");
        // Keep user/package mpv scripts available (SVP needs mpvSockets.lua).
        // Windows `os.execute(...)` console flashes from those scripts are hidden
        // by the command processor shim installed on the mpv child environment.
        command.arg("--load-scripts=yes");
        command.arg("--idle=yes");
        command.arg(format!("--input-ipc-server={ipc_path}"));
        command
    }

    #[allow(dead_code)]
    pub fn spawn(&self, launch: &PlaybackRequest) -> std::io::Result<Child> {
        self.command_for_launch(launch).spawn()
    }

    fn hidden_command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        configure_hidden_child_window(&mut command);
        remove_packaged_cef_preload(&mut command);
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

#[cfg(target_os = "linux")]
fn remove_packaged_cef_preload(command: &mut Command) {
    let Ok(cef_preload) = std::env::var("MEDIAFLICK_DESKTOP_CEF_PRELOAD") else {
        return;
    };
    if cef_preload.is_empty() {
        return;
    }

    match cleaned_ld_preload(
        &std::env::var("LD_PRELOAD").unwrap_or_default(),
        &cef_preload,
    ) {
        Some(ld_preload) => command.env("LD_PRELOAD", ld_preload),
        None => command.env_remove("LD_PRELOAD"),
    };
    command.env_remove("MEDIAFLICK_DESKTOP_CEF_PRELOAD");
}

#[cfg(not(target_os = "linux"))]
fn remove_packaged_cef_preload(_command: &mut Command) {}

#[cfg(target_os = "linux")]
fn cleaned_ld_preload(current: &str, cef_preload: &str) -> Option<String> {
    let entries = current
        .split(|ch: char| ch == ':' || ch.is_ascii_whitespace())
        .filter(|entry| !entry.is_empty() && *entry != cef_preload)
        .collect::<Vec<_>>();
    (!entries.is_empty()).then(|| entries.join(" "))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn configure_focus_on_file_load(command: &mut Command) {
    // Ask mpv to focus/raise its own window when a warm idle process creates
    // the video output for a loaded file. mpv documents this for X11 and macOS;
    // on unsupported compositors it is accepted as best-effort/no-op behavior.
    command.arg("--focus-on=all");
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn configure_focus_on_file_load(_command: &mut Command) {}

impl PlaybackRequest {
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

fn mpv_headers(launch: &PlaybackRequest) -> Vec<HttpHeader> {
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::cleaned_ld_preload;

    #[test]
    fn removes_packaged_cef_preload_from_ld_preload() {
        assert_eq!(
            cleaned_ld_preload("/app/libcef.so /usr/lib/libtrace.so", "/app/libcef.so"),
            Some("/usr/lib/libtrace.so".to_string())
        );
        assert_eq!(
            cleaned_ld_preload(
                "/usr/lib/liba.so:/app/libcef.so:/usr/lib/libb.so",
                "/app/libcef.so"
            ),
            Some("/usr/lib/liba.so /usr/lib/libb.so".to_string())
        );
        assert_eq!(cleaned_ld_preload("/app/libcef.so", "/app/libcef.so"), None);
    }
}
