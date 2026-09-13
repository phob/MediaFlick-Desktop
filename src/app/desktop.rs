//! Desktop shells resolve native Wayland icons and names through installed
//! desktop entries, even when CEF supplies a window icon. Register a fallback
//! for unpackaged launches before creating the window.
use std::io;
use std::path::{Path, PathBuf};

use super::build_info::APP_DESKTOP_ID;
use crate::preferences::store::atomic_write;

const ENTRY: &str =
    include_str!("../../distribution/linux/io.github.phob.MediaFlickDesktop.desktop");
const ICON: &[u8] = include_bytes!("../../distribution/app-icon.svg");
const MARKER: &str = "X-MediaFlick-Generated=true";

pub fn register() -> io::Result<()> {
    let data_home = super::paths::platform_data_dir();
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let search_dirs: Vec<_> = std::env::split_paths(&data_dirs)
        .filter(|path| path.is_absolute())
        .collect();
    // APPIMAGE is the persistent archive path; current_exe points into its
    // temporary mount and cannot be used to relaunch it from a pinned icon.
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    let executable = launch_path(appimage.as_deref(), &std::env::current_exe()?);
    register_at(&data_home, &search_dirs, &executable)
}

fn launch_path(appimage: Option<&Path>, executable: &Path) -> PathBuf {
    appimage
        .filter(|path| path.is_absolute() && path.is_file())
        .unwrap_or(executable)
        .to_path_buf()
}

fn register_at(data_home: &Path, search_dirs: &[PathBuf], executable: &Path) -> io::Result<()> {
    let relative = PathBuf::from("applications").join(format!("{APP_DESKTOP_ID}.desktop"));
    let destination = data_home.join(&relative);
    let existing = match std::fs::read_to_string(&destination) {
        Ok(entry) => Some(entry),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    // User/package-managed launchers take precedence. Only refresh our own
    // fallback when a checkout or AppImage moves.
    if existing
        .as_ref()
        .is_some_and(|entry| !entry.lines().any(|line| line == MARKER))
    {
        return Ok(());
    }
    if search_dirs
        .iter()
        .any(|dir| dir != data_home && dir.join(&relative).is_file())
    {
        if existing.is_some() {
            std::fs::remove_file(&destination)?;
        }
        return Ok(());
    }

    let icon_path = data_home.join("mediaflick-desktop/desktop/app-icon.svg");
    let entry = desktop_entry(executable, &icon_path)?;
    // Use an absolute icon path so no icon-theme cache refresh is required.
    write_changed(&icon_path, ICON)?;
    write_changed(&destination, entry.as_bytes())
}

fn desktop_entry(executable: &Path, icon: &Path) -> io::Result<String> {
    let mut arguments = vec!["env".to_owned(), "--".to_owned()];
    if let Some(directory) = executable.parent() {
        let cef = directory.join("libcef.so");
        if cef.is_file() {
            // Match the staged launcher: CEF must precede libc in the loader's
            // link map. Do not persist the launching process's environment.
            arguments.push(format!("LD_LIBRARY_PATH={}", path_text(directory)?));
            arguments.push(format!("LD_PRELOAD={}", path_text(&cef)?));
            arguments.push(format!(
                "MEDIAFLICK_DESKTOP_CEF_PRELOAD={}",
                path_text(&cef)?
            ));
        }
    }
    // Pass the path as data, including paths containing '=' that env would
    // otherwise interpret as another environment assignment.
    arguments.extend(["/bin/sh", "-c", "exec \"$0\""].map(str::to_owned));
    arguments.push(path_text(executable)?.to_owned());
    let command = arguments
        .iter()
        .map(|argument| quote_argument(argument))
        .collect::<Vec<_>>()
        .join(" ");
    let icon = escape_value(path_text(icon)?);
    let mut entry = String::new();
    for line in ENTRY.lines() {
        if line.starts_with("Exec=") {
            entry.push_str(&format!("Exec={command}\n"));
        } else if line.starts_with("Icon=") {
            entry.push_str(&format!("Icon={icon}\n"));
        } else {
            entry.push_str(line);
            entry.push('\n');
        }
    }
    entry.push_str(&format!("NoDisplay=true\n{MARKER}\n"));
    Ok(entry)
}

fn path_text(path: &Path) -> io::Result<&str> {
    path.to_str().filter(|_| path.is_absolute()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "desktop paths must be absolute UTF-8",
        )
    })
}

fn escape_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn quote_argument(argument: &str) -> String {
    let mut quoted = String::from("\"");
    for character in argument.chars() {
        if matches!(character, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        if character == '%' {
            quoted.push('%');
        }
        quoted.push(character);
    }
    quoted.push('"');
    escape_value(&quoted)
}

fn write_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match std::fs::read(path) {
        Ok(existing) if existing == bytes => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic_write(path, bytes)
}

#[cfg(test)]
mod tests;
