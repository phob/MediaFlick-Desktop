#[cfg(target_os = "linux")]
use std::ffi::{OsStr, OsString};
#[cfg(target_os = "linux")]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
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
    remove_packaged_cef_preload_from(
        command,
        std::env::var_os("MEDIAFLICK_DESKTOP_CEF_PRELOAD").as_deref(),
        std::env::var_os("LD_PRELOAD").as_deref(),
    );
}

#[cfg(target_os = "linux")]
fn remove_packaged_cef_preload_from(
    command: &mut Command,
    cef_preload: Option<&OsStr>,
    current_ld_preload: Option<&OsStr>,
) {
    let Some(cef_preload) = cef_preload.filter(|value| !value.is_empty()) else {
        return;
    };

    let current = current_ld_preload.unwrap_or_else(|| OsStr::new(""));
    match cleaned_ld_preload(current, cef_preload) {
        Some(ld_preload) => command.env("LD_PRELOAD", ld_preload),
        None => command.env_remove("LD_PRELOAD"),
    };
    command.env_remove("MEDIAFLICK_DESKTOP_CEF_PRELOAD");
}

#[cfg(not(target_os = "linux"))]
fn remove_packaged_cef_preload(_command: &mut Command) {}

#[cfg(target_os = "linux")]
fn cleaned_ld_preload(current: &OsStr, cef_preload: &OsStr) -> Option<OsString> {
    let cef_bytes = cef_preload.as_bytes();
    let mut cleaned = Vec::new();
    for entry in current
        .as_bytes()
        .split(|byte| *byte == b':' || byte.is_ascii_whitespace())
        .filter(|entry| !entry.is_empty() && *entry != cef_bytes)
    {
        if !cleaned.is_empty() {
            cleaned.push(b' ');
        }
        cleaned.extend_from_slice(entry);
    }
    (!cleaned.is_empty()).then(|| OsString::from_vec(cleaned))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn configure_focus_on_file_load(command: &mut Command) {
    command.arg("--focus-on=all");
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn configure_focus_on_file_load(_command: &mut Command) {}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::collections::HashMap;
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::OsStrExt;
    use std::process::Command;

    use super::{cleaned_ld_preload, remove_packaged_cef_preload_from};

    fn os(bytes: &[u8]) -> &OsStr {
        OsStr::from_bytes(bytes)
    }

    #[test]
    fn removes_packaged_cef_preload_from_ld_preload() {
        assert_eq!(
            cleaned_ld_preload(
                os(b"/app/libcef.so /usr/lib/libtrace.so"),
                os(b"/app/libcef.so")
            ),
            Some(OsString::from("/usr/lib/libtrace.so"))
        );
        assert_eq!(
            cleaned_ld_preload(
                os(b"/usr/lib/liba.so:/app/libcef.so:/usr/lib/libb.so"),
                os(b"/app/libcef.so")
            ),
            Some(OsString::from("/usr/lib/liba.so /usr/lib/libb.so"))
        );
        assert_eq!(
            cleaned_ld_preload(os(b"/app/libcef.so"), os(b"/app/libcef.so")),
            None
        );
        assert_eq!(cleaned_ld_preload(os(b""), os(b"/app/libcef.so")), None);
        assert_eq!(
            cleaned_ld_preload(os(b": \t\n :/app/libcef.so:"), os(b"/app/libcef.so")),
            None
        );
        assert_eq!(
            cleaned_ld_preload(
                os(b"/app/libcef.so.bak /app/libcef.so"),
                os(b"/app/libcef.so")
            ),
            Some(OsString::from("/app/libcef.so.bak"))
        );
    }

    #[test]
    fn removes_packaged_cef_preload_from_non_utf8_ld_preload() {
        assert_eq!(
            cleaned_ld_preload(
                os(b"/app/libcef.so /usr/lib/lib\xfftrace.so"),
                os(b"/app/libcef.so")
            ),
            Some(os(b"/usr/lib/lib\xfftrace.so").to_os_string())
        );
        assert_eq!(
            cleaned_ld_preload(
                os(b"/usr/lib/lib\x80a.so:/app/libcef.so\t/usr/lib/lib\xffb.so"),
                os(b"/app/libcef.so")
            ),
            Some(os(b"/usr/lib/lib\x80a.so /usr/lib/lib\xffb.so").to_os_string())
        );
        assert_eq!(
            cleaned_ld_preload(
                os(b"/opt/app\xfe/libcef.so:/usr/lib/lib\x81keep.so"),
                os(b"/opt/app\xfe/libcef.so")
            ),
            Some(os(b"/usr/lib/lib\x81keep.so").to_os_string())
        );
        assert_eq!(
            cleaned_ld_preload(os(b"/opt/app\xfe/libcef.so"), os(b"/opt/app\xfe/libcef.so")),
            None
        );
    }

    #[test]
    fn command_strips_cef_preload_even_with_non_utf8_env_values() {
        let mut command = Command::new("mpv");
        remove_packaged_cef_preload_from(
            &mut command,
            Some(os(b"/opt/app\xfe/libcef.so")),
            Some(os(
                b"/usr/lib/lib\x80a.so:/opt/app\xfe/libcef.so /usr/lib/lib\xffb.so",
            )),
        );
        let envs: HashMap<&OsStr, Option<&OsStr>> = command.get_envs().collect();
        assert_eq!(
            envs.get(OsStr::new("LD_PRELOAD")),
            Some(&Some(os(b"/usr/lib/lib\x80a.so /usr/lib/lib\xffb.so")))
        );
        assert_eq!(
            envs.get(OsStr::new("MEDIAFLICK_DESKTOP_CEF_PRELOAD")),
            Some(&None)
        );

        let mut only_cef = Command::new("mpv");
        remove_packaged_cef_preload_from(
            &mut only_cef,
            Some(os(b"/app/libcef.so")),
            Some(os(b"/app/libcef.so")),
        );
        let envs: HashMap<&OsStr, Option<&OsStr>> = only_cef.get_envs().collect();
        assert_eq!(envs.get(OsStr::new("LD_PRELOAD")), Some(&None));
        assert_eq!(
            envs.get(OsStr::new("MEDIAFLICK_DESKTOP_CEF_PRELOAD")),
            Some(&None)
        );

        let mut empty_marker = Command::new("mpv");
        remove_packaged_cef_preload_from(
            &mut empty_marker,
            Some(os(b"")),
            Some(os(b"/usr/lib/lib\xffa.so")),
        );
        assert_eq!(empty_marker.get_envs().count(), 0);
    }
}
