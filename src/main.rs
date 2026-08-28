#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod collections;
mod companion;
mod integrations;
mod jellyfin;
mod library;
mod maintenance;
mod playback;
mod players;
mod preferences;
mod seerr;
mod shell;
mod windows;

use clap::Parser;

use crate::app::cli::Cli;
use crate::app::logger;
use crate::preferences::{FileSettingsStore, PlayerBackend, SettingsStore};
use crate::shell::cef::AppConfig;

fn main() {
    if let Some(exit_code) = windows::run_command_processor_shim() {
        std::process::exit(exit_code);
    }

    jellyfin::bridge::ensure_session_token();

    // Do not parse the user CLI in CEF subprocesses. Chromium starts this same
    // executable with its own internal switches (for example `--type=renderer`).
    if is_cef_subprocess() {
        std::process::exit(crate::shell::cef::run(&AppConfig {
            settings: FileSettingsStore.load(),
            title: "MediaFlick Desktop".to_string(),
            remote_debugging_port: 0,
            hidden: false,
        }));
    }

    let cli = Cli::parse();
    let settings_store = FileSettingsStore;
    let mut settings = settings_store.load();
    let log_file = cli
        .log_file
        .clone()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(logger::default_log_file_path);
    let effective_log_level = cli.log_level.as_deref().unwrap_or(&settings.log_level);
    let _log_guard = logger::init(&log_file, effective_log_level);

    // Headless checks open no window and share the database through WAL, so
    // they run before the single-instance guard and work alongside a running app.
    if cli.library_stats {
        std::process::exit(library::headless::print_stats());
    }
    if cli.library_sync_once {
        std::process::exit(library::headless::sync_once());
    }
    let instance_id = crate::app::instance::instance_id();
    let _instance_guard = match crate::app::instance::InstanceGuard::acquire(&instance_id) {
        Some(guard) => guard,
        None => {
            tracing::info!(
                target: "main",
                "another mediaflick-desktop session is already running; exiting"
            );
            crate::app::instance::notify_already_running();
            std::process::exit(0);
        }
    };

    let mut should_save_settings = false;

    if let Some(url) = cli.normalized_url() {
        settings.jellyfin_url = Some(url);
        should_save_settings = true;
    }
    if let Some(mpv_path) = &cli.mpv_path {
        settings.mpv_path = Some(mpv_path.to_string_lossy().into_owned());
        settings.player_backend = Some(PlayerBackend::Mpv);
        should_save_settings = true;
    } else if settings.effective_backend() == PlayerBackend::Mpv
        && settings.mpv_path.is_none()
        && let Some(mpv_path) = default_mpv_path()
    {
        settings.mpv_path = Some(mpv_path.to_string_lossy().into_owned());
    }
    settings.sanitize();

    if should_save_settings && let Err(error) = settings_store.save(&settings) {
        tracing::warn!(target: "main", "failed to save mediaflick-desktop config: {error}");
    }

    let player_path = crate::players::configured_player_path(&settings);
    tracing::info!(
        target: "main",
        "Starting mediaflick-desktop: server={}, player_backend={}, player={}",
        settings.jellyfin_url.as_deref().unwrap_or("not configured"),
        settings.effective_backend().as_str(),
        player_path.as_deref().unwrap_or("not configured")
    );

    std::process::exit(crate::shell::cef::run(&AppConfig {
        settings,
        title: "MediaFlick Desktop".to_string(),
        remote_debugging_port: cli.remote_debugging_port,
        hidden: cli.hidden,
    }));
}

fn is_cef_subprocess() -> bool {
    std::env::args().any(|arg| arg == "--type" || arg.starts_with("--type="))
}

fn default_mpv_path() -> Option<std::path::PathBuf> {
    bundled_mpv_path()
        .or_else(downloaded_mpv_path)
        .or_else(system_mpv_path)
}

fn downloaded_mpv_path() -> Option<std::path::PathBuf> {
    let path = crate::maintenance::player_setup::installed_mpv_path();
    path.is_file().then_some(path)
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

#[cfg(target_os = "windows")]
fn system_mpv_path() -> Option<std::path::PathBuf> {
    None
}

#[cfg(not(target_os = "windows"))]
fn system_mpv_path() -> Option<std::path::PathBuf> {
    let mut search_dirs = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        search_dirs.extend(std::env::split_paths(&path));
    }

    #[cfg(target_os = "macos")]
    {
        search_dirs.extend([
            std::path::PathBuf::from("/opt/homebrew/bin"),
            std::path::PathBuf::from("/usr/local/bin"),
            std::path::PathBuf::from("/usr/bin"),
            std::path::PathBuf::from("/Applications/mpv.app/Contents/MacOS"),
        ]);
        if let Some(home) = std::env::var_os("HOME") {
            search_dirs
                .push(std::path::PathBuf::from(home).join("Applications/mpv.app/Contents/MacOS"));
        }
    }

    #[cfg(target_os = "linux")]
    search_dirs.extend([
        std::path::PathBuf::from("/usr/local/bin"),
        std::path::PathBuf::from("/usr/bin"),
        std::path::PathBuf::from("/bin"),
        std::path::PathBuf::from("/snap/bin"),
    ]);

    search_dirs
        .into_iter()
        .map(|dir| dir.join("mpv"))
        .find(|path| path.is_file())
}
