#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod books;
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

use crate::app::cli::Cli;
use crate::app::logger;
use crate::preferences::AppSettings;
use crate::shell::cef::AppConfig;

fn main() {
    let exit_code = run();
    std::process::exit(exit_code);
}

fn run() -> i32 {
    #[cfg(target_os = "linux")]
    if players::mpv::is_player_helper_invocation() {
        return players::mpv::run_player_helper();
    }

    if let Some(exit_code) = windows::run_command_processor_shim() {
        return exit_code;
    }

    jellyfin::bridge::ensure_session_token();

    if is_cef_subprocess() {
        #[cfg(target_os = "linux")]
        configure_linux_cef_gpu_env();
        #[expect(
            clippy::disallowed_methods,
            reason = "a CEF subprocess reads settings.json once; it never opens the services"
        )]
        return crate::shell::cef::run(&AppConfig {
            settings: AppSettings::load_at_startup(),
            title: "MediaFlick Desktop".to_string(),
            remote_debugging_port: 0,
            hidden: false,
        });
    }

    let cli = Cli::parse();
    #[expect(
        clippy::disallowed_methods,
        reason = "startup reads settings.json once, before the services exist"
    )]
    let mut settings = AppSettings::load_at_startup();
    let log_file = cli
        .log_file
        .clone()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(logger::default_log_file_path);
    let effective_log_level = cli.log_level.as_deref().unwrap_or(&settings.log_level);
    let _log_guard = logger::init(&log_file, effective_log_level);

    if cli.library_stats {
        return library::headless::print_stats();
    }
    if cli.library_sync_once {
        return library::headless::sync_once();
    }
    let _instance_guard = match crate::app::instance::InstanceGuard::acquire() {
        Some(guard) => guard,
        None => {
            tracing::info!(
                target: "main",
                "another mediaflick-desktop session is already running; exiting"
            );
            #[cfg(target_os = "windows")]
            crate::app::instance::notify_already_running();
            #[cfg(not(target_os = "windows"))]
            crate::shell::cef::notify_already_running();
            return 0;
        }
    };

    #[cfg(target_os = "linux")]
    if let Err(error) = app::desktop::register() {
        tracing::warn!(target: "main", "failed to register Linux desktop identity: {error}");
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "command-line overrides are saved once, before the services exist"
    )]
    settings.apply_launch_overrides(
        cli.normalized_url(),
        cli.mpv_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        || default_mpv_path().map(|path| path.to_string_lossy().into_owned()),
    );

    let player_path = crate::players::configured_player_path(&settings);
    tracing::info!(
        target: "main",
        "Starting mediaflick-desktop: server={}, player_backend={}, player={}",
        settings.jellyfin_url.as_deref().unwrap_or("not configured"),
        settings.effective_backend().as_str(),
        player_path.as_deref().unwrap_or("not configured")
    );

    crate::shell::cef::run(&AppConfig {
        settings,
        title: "MediaFlick Desktop".to_string(),
        remote_debugging_port: cli.remote_debugging_port,
        hidden: cli.hidden,
    })
}

fn is_cef_subprocess() -> bool {
    std::env::args().any(|arg| arg == "--type" || arg.starts_with("--type="))
}

#[cfg(target_os = "linux")]
fn configure_linux_cef_gpu_env() {
    let is_gpu_process =
        std::env::args().any(|arg| arg == "--type=gpu-process" || arg == "--type=zygote");
    let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|val| !val.is_empty());
    let force_x11 = std::env::var_os("MEDIAFLICK_FORCE_X11")
        .is_some_and(|val| val == "1" || val.eq_ignore_ascii_case("true"));
    if !is_gpu_process || !has_wayland || force_x11 {
        return;
    }
    append_mesa_debug_token("INTEL_DEBUG", "noccs");
    append_mesa_debug_token("AMD_DEBUG", "nodcc");
}

#[cfg(target_os = "linux")]
fn append_mesa_debug_token(var: &str, token: &str) {
    let existing =
        std::env::var_os(var).map_or_else(String::new, |val| val.to_string_lossy().into_owned());
    if existing
        .split(',')
        .any(|part| part.trim().eq_ignore_ascii_case(token))
    {
        return;
    }
    let updated = if existing.trim().is_empty() {
        token.to_string()
    } else {
        format!("{existing},{token}")
    };
    // SAFETY: called at process entry in the single-threaded CEF zygote or GPU subprocess before CEF or Mesa starts threads.
    unsafe { std::env::set_var(var, updated) };
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
