use std::env;
use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Environment::{ExpandEnvironmentStringsW, SetEnvironmentVariableW};
use windows_sys::Win32::System::LibraryLoader::{AddDllDirectory, RemoveDllDirectory};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_EXPAND_SZ, REG_SZ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
};

const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const MAX_REGISTRY_STRING_BYTES: u32 = 64 * 1024;
const MAX_SUBKEY_NAME_LENGTH: usize = 256;

pub(super) struct RuntimeEnvironment {
    dll_directory_cookie: *mut std::ffi::c_void,
}

impl Drop for RuntimeEnvironment {
    fn drop(&mut self) {
        if !self.dll_directory_cookie.is_null() {
            // SAFETY: `AddDllDirectory` returned this cookie, and this owner
            // removes it exactly once after libmpv has been destroyed.
            unsafe {
                RemoveDllDirectory(self.dll_directory_cookie);
            }
        }
    }
}

pub(super) fn runtime_directory() -> Option<PathBuf> {
    registry_runtime_directory().or_else(program_files_runtime_directory)
}

pub(super) fn prepare_runtime_environment() -> io::Result<Option<RuntimeEnvironment>> {
    let Some(directory) = runtime_directory() else {
        tracing::warn!(
            target: "mpv.library",
            "SVP profile is enabled, but the SVP 4 runtime is no longer available"
        );
        return Ok(None);
    };

    prepend_python_path(&directory)?;
    let wide_directory = wide_null(directory.as_os_str());
    // SAFETY: `wide_directory` is null-terminated and remains alive for the call.
    let cookie = unsafe { AddDllDirectory(wide_directory.as_ptr()) };
    if cookie.is_null() {
        return Err(io::Error::other(format!(
            "could not add the SVP runtime directory {}: {}",
            directory.display(),
            io::Error::last_os_error()
        )));
    }
    tracing::info!(
        target: "mpv.library",
        path = %directory.display(),
        "configured the SVP 4 runtime directory"
    );
    Ok(Some(RuntimeEnvironment {
        dll_directory_cookie: cookie,
    }))
}

fn registry_runtime_directory() -> Option<PathBuf> {
    let roots = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE];
    let views = [KEY_WOW64_64KEY, KEY_WOW64_32KEY];
    for root in roots {
        for view in views {
            if let Some(directory) = registry_view_runtime_directory(root, view) {
                return Some(directory);
            }
        }
    }
    None
}

fn registry_view_runtime_directory(root: HKEY, view: u32) -> Option<PathBuf> {
    let uninstall = RegistryKey::open(root, UNINSTALL_KEY, KEY_READ | view)?;
    let mut index = 0;
    while let Some(name) = uninstall.subkey_name(index) {
        index += 1;
        let Some(product) = uninstall.open_subkey(&name, KEY_READ | view) else {
            continue;
        };
        let Some(display_name) = product.string_value("DisplayName") else {
            continue;
        };
        if !is_svp_display_name(&display_name) {
            continue;
        }
        if let Some(directory) = runtime_directory_from_metadata(
            product.string_value("InstallLocation").as_deref(),
            product.string_value("UninstallString").as_deref(),
        ) {
            return Some(directory);
        }
    }
    None
}

fn runtime_directory_from_metadata(
    install_location: Option<&str>,
    uninstall_string: Option<&str>,
) -> Option<PathBuf> {
    install_location
        .and_then(clean_registry_path)
        .and_then(|directory| runtime_directory_in(&directory))
        .or_else(|| {
            uninstall_string
                .and_then(uninstall_directory)
                .and_then(|directory| runtime_directory_in(&directory))
        })
}

fn runtime_directory_in(install_directory: &Path) -> Option<PathBuf> {
    let runtime_directory = install_directory.join("mpv64");
    has_vapoursynth_runtime(&runtime_directory).then_some(runtime_directory)
}

fn clean_registry_path(value: &str) -> Option<PathBuf> {
    let value = value.trim().trim_matches('"').trim();
    (!value.is_empty()).then(|| PathBuf::from(value))
}

fn uninstall_directory(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    let executable = if let Some(quoted) = command.strip_prefix('"') {
        quoted.split_once('"').map(|(path, _)| path)?
    } else {
        let lowercase = command.to_ascii_lowercase();
        let end = lowercase.find(".exe")? + ".exe".len();
        command.get(..end)?
    };
    Path::new(executable.trim()).parent().map(Path::to_path_buf)
}

fn is_svp_display_name(display_name: &str) -> bool {
    let display_name = display_name.trim().to_ascii_lowercase();
    display_name == "svp 4" || display_name.starts_with("svp 4 ")
}

fn program_files_runtime_directory() -> Option<PathBuf> {
    let roots = ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .map(|root| root.join("SVP 4"));
    find_runtime_directory(roots)
}

fn find_runtime_directory(
    install_directories: impl IntoIterator<Item = PathBuf>,
) -> Option<PathBuf> {
    install_directories
        .into_iter()
        .map(|directory| directory.join("mpv64"))
        .find(|directory| has_vapoursynth_runtime(directory))
}

fn has_vapoursynth_runtime(directory: &Path) -> bool {
    directory.join("VSScript.dll").is_file()
}

fn prepend_python_path(directory: &Path) -> io::Result<()> {
    let mut paths = vec![directory.to_path_buf()];
    if let Some(current) = env::var_os("PYTHONPATH") {
        paths.extend(env::split_paths(&current).filter(|path| path != directory));
    }
    let joined = env::join_paths(paths).map_err(io::Error::other)?;
    let wide_name = wide_null(OsStr::new("PYTHONPATH"));
    let wide_value = wide_null(&joined);
    // SAFETY: both UTF-16 buffers are null-terminated and live through the call.
    let updated = unsafe { SetEnvironmentVariableW(wide_name.as_ptr(), wide_value.as_ptr()) };
    if updated == 0 {
        return Err(io::Error::other(format!(
            "could not configure PYTHONPATH for SVP: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(())
}

struct RegistryKey(HKEY);

impl RegistryKey {
    fn open(root: HKEY, path: &str, access: u32) -> Option<Self> {
        let path = wide_null(OsStr::new(path));
        let mut key = std::ptr::null_mut();
        // SAFETY: `path` is null-terminated, and `key` is valid output storage.
        let status = unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, access, &mut key) };
        if status == ERROR_SUCCESS {
            Some(Self(key))
        } else {
            None
        }
    }

    fn open_subkey(&self, name: &str, access: u32) -> Option<Self> {
        Self::open(self.0, name, access)
    }

    fn subkey_name(&self, index: u32) -> Option<String> {
        let mut buffer = [0_u16; MAX_SUBKEY_NAME_LENGTH];
        let mut length = u32::try_from(buffer.len()).ok()?;
        // SAFETY: this key owns a valid handle, and the API receives the exact
        // writable buffer length. The unused optional outputs are null.
        let status = unsafe {
            RegEnumKeyExW(
                self.0,
                index,
                buffer.as_mut_ptr(),
                &mut length,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let length = usize::try_from(length).ok()?;
        Some(String::from_utf16_lossy(buffer.get(..length)?))
    }

    fn string_value(&self, name: &str) -> Option<String> {
        let name = wide_null(OsStr::new(name));
        let mut value_type = 0;
        let mut byte_count = 0;
        // SAFETY: this key owns a valid handle, `name` is null-terminated, and
        // the first query writes only the type and required byte count.
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut value_type,
                std::ptr::null_mut(),
                &mut byte_count,
            )
        };
        if status != ERROR_SUCCESS
            || !matches!(value_type, REG_SZ | REG_EXPAND_SZ)
            || byte_count == 0
            || byte_count > MAX_REGISTRY_STRING_BYTES
        {
            return None;
        }

        let word_count = usize::try_from(byte_count.div_ceil(2)).ok()?;
        let mut buffer = vec![0_u16; word_count];
        // SAFETY: the prior query supplied `byte_count`; `buffer` has at least
        // that many bytes and stays writable for the call.
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut value_type,
                buffer.as_mut_ptr().cast(),
                &mut byte_count,
            )
        };
        if status != ERROR_SUCCESS || !matches!(value_type, REG_SZ | REG_EXPAND_SZ) {
            return None;
        }
        let length = buffer
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(buffer.len());
        let value = String::from_utf16_lossy(&buffer[..length]);
        if value_type == REG_EXPAND_SZ {
            Some(expand_environment_strings(&value))
        } else {
            Some(value)
        }
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: `RegOpenKeyExW` created this owned handle, which is closed once.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn expand_environment_strings(value: &str) -> String {
    let source = wide_null(OsStr::new(value));
    // SAFETY: `source` is null-terminated; a null destination requests the size.
    let required = unsafe { ExpandEnvironmentStringsW(source.as_ptr(), std::ptr::null_mut(), 0) };
    let Ok(buffer_length) = usize::try_from(required) else {
        return value.to_string();
    };
    if required == 0 {
        return value.to_string();
    }
    let mut buffer = vec![0_u16; buffer_length];
    // SAFETY: `buffer` has the exact size requested by the first call.
    let written =
        unsafe { ExpandEnvironmentStringsW(source.as_ptr(), buffer.as_mut_ptr(), required) };
    if written == 0 || written > required {
        return value.to_string();
    }
    let length = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_installation(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "mediaflick-svp-detection-{}-{name}",
            std::process::id()
        ));
        let runtime = root.join("mpv64");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&runtime).expect("create fake SVP runtime");
        std::fs::write(runtime.join("VSScript.dll"), []).expect("write runtime DLL");
        (root, runtime)
    }

    #[test]
    fn custom_install_location_resolves_the_runtime() {
        let (root, runtime) = fake_installation("install-location");

        assert_eq!(
            runtime_directory_from_metadata(root.to_str(), None),
            Some(runtime)
        );

        std::fs::remove_dir_all(root).expect("remove fake SVP runtime");
    }

    #[test]
    fn uninstall_command_resolves_the_installation_directory() {
        let (root, runtime) = fake_installation("uninstall-command");
        let command = format!("\"{}\" /SILENT", root.join("unins000.exe").display());

        assert_eq!(
            runtime_directory_from_metadata(None, Some(&command)),
            Some(runtime)
        );

        std::fs::remove_dir_all(root).expect("remove fake SVP runtime");
    }

    #[test]
    fn invalid_install_location_falls_back_to_the_uninstall_command() {
        let (root, runtime) = fake_installation("metadata-fallback");
        let command = format!("\"{}\" /SILENT", root.join("unins000.exe").display());

        assert_eq!(
            runtime_directory_from_metadata(Some("C:\\Missing SVP"), Some(&command)),
            Some(runtime)
        );

        std::fs::remove_dir_all(root).expect("remove fake SVP runtime");
    }

    #[test]
    fn only_svp_four_product_names_are_accepted() {
        assert!(is_svp_display_name("SVP 4 Pro"));
        assert!(is_svp_display_name("SVP 4"));
        assert!(!is_svp_display_name("SVP 3"));
        assert!(!is_svp_display_name("SVP 40"));
    }
}
