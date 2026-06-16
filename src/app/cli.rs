use std::path::PathBuf;

use clap::Parser;

use crate::app::settings::normalize_server_url;

#[derive(Debug, Clone, Parser)]
#[command(name = "mediaflick-desktop")]
#[command(about = "External mpv playback for Jellyfin in a Rust/CEF desktop shell")]
pub struct Cli {
    /// Jellyfin server URL. If omitted, the welcome screen asks for one.
    ///
    /// Examples: http://localhost:8096, https://jellyfin.example.com.
    #[arg(long, env = "JELLYFIN_URL")]
    pub url: Option<String>,

    /// External mpv executable to save into the app config.
    #[arg(long, env = "MEDIAFLICK_DESKTOP_MPV_PATH")]
    pub mpv_path: Option<PathBuf>,

    /// Enable Chromium remote debugging on this port. Use 0 to disable.
    #[arg(
        long,
        env = "MEDIAFLICK_DESKTOP_REMOTE_DEBUGGING_PORT",
        default_value_t = 0
    )]
    pub remote_debugging_port: i32,

    /// Keep the CEF window hidden at startup.
    #[arg(long, default_value_t = false)]
    pub hidden: bool,

    /// Rust app log level/filter. Examples: debug, trace, mpv.ipc=trace,debug.
    #[arg(long, env = "MEDIAFLICK_DESKTOP_LOG_LEVEL")]
    pub log_level: Option<String>,

    /// Rust app log file. Defaults to the app config directory.
    #[arg(long, env = "MEDIAFLICK_DESKTOP_LOG_FILE")]
    pub log_file: Option<PathBuf>,
}

impl Cli {
    pub fn normalized_url(&self) -> Option<String> {
        self.url.as_deref().and_then(normalize_server_url)
    }
}
