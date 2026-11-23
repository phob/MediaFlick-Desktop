#[cfg(any(target_os = "windows", test))]
use std::path::Path;
use std::path::PathBuf;

use crate::maintenance::updater::UpdaterResult;

pub const MPV_HELP_URL: &str = "https://mpv.io/installation/";

#[cfg(target_os = "windows")]
const MPV_LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases/latest";

#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    not(target_os = "windows"),
    allow(
        dead_code,
        reason = "only the Windows mpv installer downloads and extracts"
    )
)]
pub enum MpvSetupPhase {
    Downloading { downloaded: u64, total: Option<u64> },
    Extracting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Macos,
    Linux,
    Other,
}

pub fn platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::Macos
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else {
        Platform::Other
    }
}

pub fn supported() -> bool {
    cfg!(target_os = "windows")
        && (cfg!(target_arch = "x86_64")
            || cfg!(target_arch = "aarch64")
            || cfg!(target_arch = "x86"))
}

pub fn installed_mpv_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        install_root().join("mpv.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        crate::preferences::config_dir().join("mpv").join("mpv")
    }
}

#[cfg(target_os = "windows")]
pub fn download_and_install<F, B>(mut progress: F, before_replace: B) -> UpdaterResult<PathBuf>
where
    F: FnMut(MpvSetupPhase),
    B: FnOnce(),
{
    use crate::maintenance::updater;

    let agent = updater::update_agent();
    let mut response = agent
        .get(MPV_LATEST_RELEASE_API_URL)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()?;
    let release = response.body_mut().read_json::<GithubRelease>()?;

    let asset = select_mpv_asset(release.assets)
        .ok_or_else(|| std::io::Error::other("no mpv build is available for this platform"))?;

    let download_dir = updater::unique_download_dir();
    std::fs::create_dir_all(&download_dir)?;
    let archive_path = download_dir.join(updater::safe_file_name(&asset.name));

    updater::download_to_file(
        &asset.browser_download_url,
        asset.size,
        &archive_path,
        |downloaded, total| progress(MpvSetupPhase::Downloading { downloaded, total }),
    )?;

    progress(MpvSetupPhase::Extracting);
    let install_dir = install_root();
    let staging_dir = install_dir.with_extension("new");
    let _ = std::fs::remove_dir_all(&staging_dir);
    std::fs::create_dir_all(&staging_dir)?;
    sevenz_rust2::decompress_file(&archive_path, &staging_dir).map_err(|error| {
        let _ = std::fs::remove_dir_all(&staging_dir);
        std::io::Error::other(format!("failed to extract mpv archive: {error}"))
    })?;
    let _ = std::fs::remove_dir_all(&download_dir);

    if !staging_dir.join("mpv.exe").is_file() {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(
            std::io::Error::other("mpv.exe was not found in the downloaded archive").into(),
        );
    }

    replace_install_dir(&staging_dir, &install_dir, before_replace)?;

    let mpv = install_dir.join("mpv.exe");
    tracing::info!(target: "mpv.setup", path = %mpv.display(), "installed mpv");
    Ok(mpv)
}

#[cfg(not(target_os = "windows"))]
pub fn download_and_install<F, B>(_progress: F, _before_replace: B) -> UpdaterResult<PathBuf>
where
    F: FnMut(MpvSetupPhase),
    B: FnOnce(),
{
    Err(std::io::Error::other("automatic mpv download is only supported on Windows").into())
}

#[cfg(any(target_os = "windows", test))]
fn replace_install_dir(
    staging_dir: &Path,
    install_dir: &Path,
    before_replace: impl FnOnce(),
) -> std::io::Result<()> {
    before_replace();
    let backup_dir = install_dir.with_extension("old");
    let _ = std::fs::remove_dir_all(&backup_dir);
    let had_existing = install_dir.exists();
    if had_existing {
        std::fs::rename(install_dir, &backup_dir)?;
    }
    if let Err(error) = std::fs::rename(staging_dir, install_dir) {
        if had_existing {
            let _ = std::fs::rename(&backup_dir, install_dir);
        }
        let _ = std::fs::remove_dir_all(staging_dir);
        return Err(error);
    }
    let _ = std::fs::remove_dir_all(&backup_dir);
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_root() -> PathBuf {
    app_install_dir().join("mpv")
}

#[cfg(target_os = "windows")]
fn app_install_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(target_os = "windows")]
fn select_mpv_asset(assets: Vec<GithubAsset>) -> Option<GithubAsset> {
    let prefix = if cfg!(target_arch = "aarch64") {
        "mpv-aarch64-"
    } else if cfg!(target_arch = "x86") {
        "mpv-i686-"
    } else {
        "mpv-x86_64-"
    };
    assets.into_iter().find(|asset| {
        let name = asset.name.to_ascii_lowercase();
        name.starts_with(prefix)
            && name.ends_with(".7z")
            && !name.contains("-dev-")
            && !name.contains("x86_64-v3")
    })
}

#[cfg(target_os = "windows")]
#[derive(serde::Deserialize)]
struct GithubRelease {
    assets: Vec<GithubAsset>,
}

#[cfg(target_os = "windows")]
#[derive(serde::Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: Option<u64>,
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::{GithubAsset, select_mpv_asset};

    fn asset(name: &str) -> GithubAsset {
        GithubAsset {
            name: name.to_string(),
            browser_download_url: format!("https://github.com/x/y/releases/download/z/{name}"),
            size: Some(1),
        }
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn picks_plain_x86_64_over_v3_and_dev() {
        let assets = vec![
            asset("mpv-x86_64-v3-20260610-git-abc.7z"),
            asset("mpv-dev-x86_64-20260610-git-abc.7z"),
            asset("mpv-i686-20260610-git-abc.7z"),
            asset("mpv-x86_64-20260610-git-abc.7z"),
        ];
        let chosen = select_mpv_asset(assets).expect("an asset");
        assert_eq!(chosen.name, "mpv-x86_64-20260610-git-abc.7z");
    }
}

#[cfg(test)]
mod replace_tests {
    use super::replace_install_dir;
    use std::cell::Cell;
    use std::io::Write;
    use std::path::Path;

    fn write_marker(path: &Path, content: &str) {
        let mut file = std::fs::File::create(path).expect("create marker");
        file.write_all(content.as_bytes()).expect("write marker");
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "mediaflick-mpv-setup-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp root");
        root
    }

    #[test]
    fn replace_install_dir_runs_before_replace_hook_before_swapping_directories() {
        let root = temp_root("swap");
        let install_dir = root.join("mpv");
        let staging_dir = install_dir.with_extension("new");
        std::fs::create_dir_all(&install_dir).expect("install dir");
        std::fs::create_dir_all(&staging_dir).expect("staging dir");
        write_marker(&install_dir.join("version.txt"), "old");
        write_marker(&staging_dir.join("version.txt"), "new");

        let hook_saw_old = Cell::new(false);
        replace_install_dir(&staging_dir, &install_dir, || {
            let content = std::fs::read_to_string(install_dir.join("version.txt")).unwrap();
            hook_saw_old.set(content == "old");
        })
        .expect("replace succeeds");

        assert!(hook_saw_old.get());
        assert_eq!(
            std::fs::read_to_string(install_dir.join("version.txt")).unwrap(),
            "new"
        );
        assert!(!staging_dir.exists());
        assert!(!install_dir.with_extension("old").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn replace_install_dir_restores_previous_directory_when_staging_is_missing() {
        let root = temp_root("rollback");
        let install_dir = root.join("mpv");
        let staging_dir = install_dir.with_extension("new");
        std::fs::create_dir_all(&install_dir).expect("install dir");
        write_marker(&install_dir.join("version.txt"), "old");

        let result = replace_install_dir(&staging_dir, &install_dir, || {});

        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(install_dir.join("version.txt")).unwrap(),
            "old"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
