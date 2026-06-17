#[cfg(target_os = "windows")]
use std::ffi::OsString;
#[cfg(target_os = "windows")]
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::process::{Command, Stdio};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(target_os = "windows")]
const SHIM_ENABLED_ENV: &str = "MEDIAFLICK_DESKTOP_COMSPEC_SHIM";
#[cfg(target_os = "windows")]
const REAL_COMSPEC_ENV: &str = "MEDIAFLICK_DESKTOP_REAL_COMSPEC";

/// Run this executable as a hidden command processor shim when `system()` from an
/// mpv script invokes `%COMSPEC% /c ...`.
#[cfg(target_os = "windows")]
pub fn run_command_processor_shim() -> Option<i32> {
    std::env::var_os(SHIM_ENABLED_ENV)?;

    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let mut command = Command::new(real_comspec());
    command.args(args);
    command.creation_flags(CREATE_NO_WINDOW);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    Some(match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(_) => 1,
    })
}

#[cfg(not(target_os = "windows"))]
pub fn run_command_processor_shim() -> Option<i32> {
    None
}

/// Point a child process at this executable for `%COMSPEC%`, while preserving the
/// real `cmd.exe` path for the shim above. This prevents Windows mpv Lua scripts
/// that call `os.execute(...)` from flashing a visible console window.
#[cfg(target_os = "windows")]
pub fn install_hidden_command_processor_shim(command: &mut Command) {
    let Ok(current_exe) = std::env::current_exe() else {
        return;
    };

    command.env(SHIM_ENABLED_ENV, "1");
    command.env(REAL_COMSPEC_ENV, real_comspec());
    command.env("COMSPEC", current_exe);
}

#[cfg(target_os = "windows")]
fn real_comspec() -> OsString {
    std::env::var_os(REAL_COMSPEC_ENV)
        .or_else(|| std::env::var_os("COMSPEC"))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(default_comspec)
}

#[cfg(target_os = "windows")]
fn default_comspec() -> OsString {
    let system_root =
        std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
    PathBuf::from(system_root)
        .join("System32")
        .join("cmd.exe")
        .into_os_string()
}
