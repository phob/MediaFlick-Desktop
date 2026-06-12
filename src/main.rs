#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod cef_shell;
mod cli;
mod external_mpv;
mod jellyfin_bridge;
mod logger;
mod mpv_controller;
mod playback_reporter;
mod settings;

use clap::Parser;

use crate::cef_shell::AppConfig;
use crate::cli::Cli;
use crate::external_mpv::ExternalMpv;
use crate::settings::AppSettings;

fn main() {
    // Do not parse the user CLI in CEF subprocesses. Chromium starts this same
    // executable with its own internal switches (for example `--type=renderer`).
    if is_cef_subprocess() {
        std::process::exit(cef_shell::run(AppConfig {
            settings: AppSettings::default(),
            title: "jellyfin-mpv".to_string(),
            remote_debugging_port: 0,
            hidden: false,
        }));
    }

    let cli = Cli::parse();
    let log_file = cli
        .log_file
        .clone()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(logger::default_log_file_path);
    let _log_guard = logger::init(log_file, &cli.log_level);

    let mut settings = AppSettings::load();
    let mut should_save_settings = false;

    if let Some(url) = cli.normalized_url() {
        settings.jellyfin_url = Some(url);
        should_save_settings = true;
    }
    if let Some(mpv_path) = &cli.mpv_path {
        settings.mpv_path = Some(mpv_path.to_string_lossy().into_owned());
        should_save_settings = true;
    }
    settings.sanitize();

    if should_save_settings && let Err(error) = settings.save() {
        tracing::warn!(target: "main", "failed to save jellyfin-mpv config: {error}");
    }

    let mpv = ExternalMpv::new(
        settings
            .mpv_path
            .clone()
            .unwrap_or_else(|| "mpv".to_string()),
    );
    let target = if settings.is_complete() {
        settings.jellyfin_url.as_deref().unwrap_or("welcome screen")
    } else {
        "welcome screen"
    };
    tracing::info!(
        target: "main",
        "Starting jellyfin-mpv: target={}, external mpv={}",
        target,
        mpv.executable().display()
    );

    std::process::exit(cef_shell::run(AppConfig {
        settings,
        title: "jellyfin-mpv".to_string(),
        remote_debugging_port: cli.remote_debugging_port,
        hidden: cli.hidden,
    }));
}

fn is_cef_subprocess() -> bool {
    std::env::args().any(|arg| arg == "--type" || arg.starts_with("--type="))
}
