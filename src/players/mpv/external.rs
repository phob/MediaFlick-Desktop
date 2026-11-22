use std::path::PathBuf;
use std::process::Command;
#[cfg(windows)]
use std::process::Stdio;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use crate::windows::install_hidden_command_processor_shim;

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
        command.arg("--load-scripts=yes");
        command.arg("--idle=yes");
        command.arg(format!("--input-ipc-server={ipc_path}"));
        command
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

    let current = match std::env::var("LD_PRELOAD") {
        Ok(current) => current,
        Err(std::env::VarError::NotPresent) => String::new(),
        Err(std::env::VarError::NotUnicode(_)) => {
            command.env_remove("MEDIAFLICK_DESKTOP_CEF_PRELOAD");
            return;
        }
    };
    match cleaned_ld_preload(&current, &cef_preload) {
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
    command.arg("--focus-on=all");
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn configure_focus_on_file_load(_command: &mut Command) {}

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
