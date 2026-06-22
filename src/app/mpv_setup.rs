use std::path::PathBuf;

use crate::app::updater::UpdaterResult;

/// Where users can read about installing mpv on every platform.
pub const MPV_HELP_URL: &str = "https://mpv.io/installation/";

#[cfg(target_os = "windows")]
const MPV_LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases/latest";

/// A stage of the in-app mpv setup, reported back to the UI while it runs.
#[derive(Debug, Clone, Copy)]
pub enum MpvSetupPhase {
    Downloading { downloaded: u64, total: Option<u64> },
    Extracting,
}

/// Short identifier for the host platform, consumed by the welcome/settings UI.
pub fn platform_id() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

/// Whether this build can download and install mpv on its own.
pub fn supported() -> bool {
    cfg!(target_os = "windows")
        && (cfg!(target_arch = "x86_64")
            || cfg!(target_arch = "aarch64")
            || cfg!(target_arch = "x86"))
}

/// Location of the mpv executable installed by [`download_and_install`].
pub fn installed_mpv_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        install_root().join("mpv.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        crate::app::settings::config_dir().join("mpv").join("mpv")
    }
}

/// JSON config injected into the welcome/settings screens so they can render the
/// appropriate "get mpv" affordance for the current platform.
pub fn ui_config_json() -> String {
    serde_json::json!({
        "platform": platform_id(),
        "canDownload": supported(),
        "helpUrl": MPV_HELP_URL,
    })
    .to_string()
}

/// Builds the JS that pushes an mpv setup status update into the page.
pub fn setup_script(state: &str, payload: serde_json::Value) -> String {
    let payload = serde_json::json!({ "state": state, "payload": payload });
    format!("window.__mediaFlickDesktopMpvSetup&&window.__mediaFlickDesktopMpvSetup({payload});")
}

#[cfg(target_os = "windows")]
pub fn download_and_install<F>(mut progress: F) -> UpdaterResult<PathBuf>
where
    F: FnMut(MpvSetupPhase),
{
    use crate::app::updater;

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

    let backup_dir = install_dir.with_extension("old");
    let _ = std::fs::remove_dir_all(&backup_dir);
    let had_existing = install_dir.exists();
    if had_existing {
        std::fs::rename(&install_dir, &backup_dir)?;
    }
    if let Err(error) = std::fs::rename(&staging_dir, &install_dir) {
        if had_existing {
            let _ = std::fs::rename(&backup_dir, &install_dir);
        }
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(error.into());
    }
    let _ = std::fs::remove_dir_all(&backup_dir);

    let mpv = install_dir.join("mpv.exe");
    tracing::info!(target: "mpv.setup", path = %mpv.display(), "installed mpv");
    Ok(mpv)
}

#[cfg(not(target_os = "windows"))]
pub fn download_and_install<F>(_progress: F) -> UpdaterResult<PathBuf>
where
    F: FnMut(MpvSetupPhase),
{
    Err(std::io::Error::other("automatic mpv download is only supported on Windows").into())
}

#[cfg(target_os = "windows")]
fn install_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(|home| PathBuf::from(home).join("AppData").join("Local"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("mediaflick-desktop").join("mpv")
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
