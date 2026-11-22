use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};

use crate::preferences::normalize_server_url;

#[derive(Debug, Clone)]
pub struct Cli {
    pub url: Option<String>,
    pub mpv_path: Option<PathBuf>,
    pub remote_debugging_port: i32,
    pub hidden: bool,
    pub log_level: Option<String>,
    pub log_file: Option<PathBuf>,
    pub library_sync_once: bool,
    pub library_stats: bool,
}

const URL: &str = "url";
const MPV_PATH: &str = "mpv-path";
const REMOTE_DEBUGGING_PORT: &str = "remote-debugging-port";
const HIDDEN: &str = "hidden";
const LOG_LEVEL: &str = "log-level";
const LOG_FILE: &str = "log-file";
const LIBRARY_SYNC_ONCE: &str = "library-sync-once";
const LIBRARY_STATS: &str = "library-stats";

fn command() -> Command {
    let flag = |id: &'static str, help: &'static str| {
        Arg::new(id).long(id).action(ArgAction::SetTrue).help(help)
    };
    Command::new("mediaflick-desktop")
        .about("Native Jellyfin playback in a Rust/CEF desktop shell")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::new(URL)
                .long(URL)
                .env("JELLYFIN_URL")
                .value_name("URL")
                .help("Jellyfin server URL to prefill on the sign-in screen")
                .long_help(
                    "Jellyfin server URL to prefill on the sign-in screen.\n\n\
                     Examples: http://localhost:8096, https://jellyfin.example.com.",
                ),
        )
        .arg(
            Arg::new(MPV_PATH)
                .long(MPV_PATH)
                .env("MEDIAFLICK_DESKTOP_MPV_PATH")
                .value_name("MPV_PATH")
                .value_parser(value_parser!(PathBuf))
                .help("External mpv executable to save into the app config"),
        )
        .arg(
            Arg::new(REMOTE_DEBUGGING_PORT)
                .long(REMOTE_DEBUGGING_PORT)
                .env("MEDIAFLICK_DESKTOP_REMOTE_DEBUGGING_PORT")
                .value_name("REMOTE_DEBUGGING_PORT")
                .value_parser(value_parser!(i32))
                .default_value("0")
                .help("Enable Chromium remote debugging on this port. Use 0 to disable"),
        )
        .arg(flag(HIDDEN, "Keep the CEF window hidden at startup"))
        .arg(
            Arg::new(LOG_LEVEL)
                .long(LOG_LEVEL)
                .env("MEDIAFLICK_DESKTOP_LOG_LEVEL")
                .value_name("LOG_LEVEL")
                .help("Rust app log level/filter. Examples: debug, trace, mpv.ipc=trace,debug"),
        )
        .arg(
            Arg::new(LOG_FILE)
                .long(LOG_FILE)
                .env("MEDIAFLICK_DESKTOP_LOG_FILE")
                .value_name("LOG_FILE")
                .value_parser(value_parser!(PathBuf))
                .help("Rust app log file. Defaults to the app config directory"),
        )
        .arg(flag(
            LIBRARY_SYNC_ONCE,
            "Run one library sync cycle, print the result, and exit",
        ))
        .arg(flag(
            LIBRARY_STATS,
            "Print local library cache statistics and exit",
        ))
}

impl Cli {
    pub fn parse() -> Self {
        Self::from_matches(&command().get_matches())
    }

    fn from_matches(matches: &ArgMatches) -> Self {
        Self {
            url: matches.get_one::<String>(URL).cloned(),
            mpv_path: matches.get_one::<PathBuf>(MPV_PATH).cloned(),
            remote_debugging_port: matches
                .get_one::<i32>(REMOTE_DEBUGGING_PORT)
                .copied()
                .unwrap_or(0),
            hidden: matches.get_flag(HIDDEN),
            log_level: matches.get_one::<String>(LOG_LEVEL).cloned(),
            log_file: matches.get_one::<PathBuf>(LOG_FILE).cloned(),
            library_sync_once: matches.get_flag(LIBRARY_SYNC_ONCE),
            library_stats: matches.get_flag(LIBRARY_STATS),
        }
    }

    pub fn normalized_url(&self) -> Option<String> {
        self.url.as_deref().and_then(normalize_server_url)
    }
}
