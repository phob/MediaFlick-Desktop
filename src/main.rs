#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod cef;
mod jellyfin;
mod mpv;
mod windows;

use clap::Parser;

use crate::app::cli::Cli;
use crate::app::logger;
use crate::app::settings::AppSettings;
use crate::cef::AppConfig;
use crate::mpv::ExternalMpv;

fn main() {
    if let Some(exit_code) = windows::run_command_processor_shim() {
        std::process::exit(exit_code);
    }

    // Do not parse the user CLI in CEF subprocesses. Chromium starts this same
    // executable with its own internal switches (for example `--type=renderer`).
    if is_cef_subprocess() {
        std::process::exit(cef::run(AppConfig {
            settings: AppSettings::default(),
            title: "MediaFlick Desktop".to_string(),
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
    } else if settings.mpv_path.is_none()
        && let Some(mpv_path) = bundled_mpv_path()
    {
        settings.mpv_path = Some(mpv_path.to_string_lossy().into_owned());
    }
    settings.sanitize();

    if should_save_settings && let Err(error) = settings.save() {
        tracing::warn!(target: "main", "failed to save mediaflick-desktop config: {error}");
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
        "Starting mediaflick-desktop: target={}, external mpv={}",
        target,
        mpv.executable().display()
    );

    std::process::exit(cef::run(AppConfig {
        settings,
        title: "MediaFlick Desktop".to_string(),
        remote_debugging_port: cli.remote_debugging_port,
        hidden: cli.hidden,
    }));
}

fn is_cef_subprocess() -> bool {
    std::env::args().any(|arg| arg == "--type" || arg.starts_with("--type="))
}

fn bundled_mpv_path() -> Option<std::path::PathBuf> {
    let exe_path = std::env::current_exe().ok()?;
    let app_dir = exe_path.parent()?;

    #[cfg(target_os = "windows")]
    let candidates = [app_dir.join("mpv").join("mpv.exe"), app_dir.join("mpv.exe")];

    #[cfg(not(target_os = "windows"))]
    let candidates = [app_dir.join("mpv").join("mpv"), app_dir.join("mpv")];

    candidates.into_iter().find(|path| path.is_file())
}
