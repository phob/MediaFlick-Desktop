use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/phob/mediaflick-desktop/releases/latest";
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);

pub type UpdaterResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRelease {
    pub version: String,
    pub tag_name: String,
    pub html_url: String,
    pub asset: UpdateAsset,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
}

pub fn check_for_update() -> UpdaterResult<Option<UpdateRelease>> {
    if !automatic_updates_supported() {
        return Ok(None);
    }

    let agent = update_agent();
    let mut response = agent
        .get(GITHUB_LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()?;
    let release = response.body_mut().read_json::<GithubRelease>()?;
    let version = normalized_version(&release.tag_name);
    if !version_is_newer(&version, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }

    let Some(asset) = select_platform_asset(release.assets) else {
        tracing::warn!(
            target: "updater",
            version,
            "latest release does not contain a supported auto-update asset for this platform"
        );
        return Ok(None);
    };

    Ok(Some(UpdateRelease {
        version,
        tag_name: release.tag_name,
        html_url: release.html_url,
        asset: UpdateAsset {
            name: asset.name,
            browser_download_url: asset.browser_download_url,
            size: asset.size,
        },
    }))
}

pub fn download_update<F>(release: &UpdateRelease, mut progress: F) -> UpdaterResult<PathBuf>
where
    F: FnMut(u64, Option<u64>) + Send + 'static,
{
    let download_dir = std::env::temp_dir().join("mediaflick-desktop-updates");
    fs::create_dir_all(&download_dir)?;
    let installer_path = download_dir.join(safe_file_name(&release.asset.name));
    let partial_path = installer_path.with_extension("download");

    let agent = update_agent();
    let mut response = agent
        .get(release.asset.browser_download_url.as_str())
        .header("Accept", "application/octet-stream")
        .call()?;
    let total = content_length(&response).or(release.asset.size);

    let mut reader = response.body_mut().as_reader();
    let mut file = File::create(&partial_path)?;
    let mut downloaded = 0u64;
    let mut last_progress = Instant::now() - PROGRESS_INTERVAL;
    let mut buffer = [0u8; 64 * 1024];

    progress(downloaded, total);
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
        downloaded = downloaded.saturating_add(read as u64);
        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            progress(downloaded, total);
            last_progress = Instant::now();
        }
    }
    file.flush()?;
    drop(file);
    let _ = fs::remove_file(&installer_path);
    fs::rename(&partial_path, &installer_path)?;
    progress(downloaded, total);
    Ok(installer_path)
}

#[cfg(target_os = "windows")]
pub fn start_installer(installer_path: &Path) -> UpdaterResult<()> {
    std::process::Command::new(installer_path)
        .args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CLOSEAPPLICATIONS",
            "/RESTARTAPPLICATIONS",
            "/MEDIAFLICKAUTOSTART=1",
        ])
        .spawn()?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn start_installer(_installer_path: &Path) -> UpdaterResult<()> {
    Err(std::io::Error::other("automatic installation is only supported on Windows").into())
}

pub fn update_available_script(release: &UpdateRelease) -> String {
    let payload = serde_json::to_string(release).unwrap_or_else(|_| "{}".to_string());
    include_str!("update_toast.js").replace("{{update_payload}}", &payload)
}

pub fn update_progress_script(state: &str, payload: serde_json::Value) -> String {
    let payload = serde_json::json!({
        "state": state,
        "payload": payload,
    });
    format!(
        "window.__mediaFlickDesktopUpdateProgress&&window.__mediaFlickDesktopUpdateProgress({payload});"
    )
}

fn automatic_updates_supported() -> bool {
    cfg!(target_os = "windows")
}

fn update_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .user_agent(format!("mediaflick-desktop/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn select_platform_asset(assets: Vec<GithubAsset>) -> Option<GithubAsset> {
    assets.into_iter().find(|asset| {
        let name = asset.name.to_ascii_lowercase();
        cfg!(target_os = "windows")
            && name.starts_with("mediaflickdesktop-setup-")
            && name.ends_with(".exe")
    })
}

fn normalized_version(version: &str) -> String {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .trim()
        .to_string()
}

fn version_is_newer(latest: &str, current: &str) -> bool {
    let latest = version_components(latest);
    let current = version_components(current);
    if latest.is_empty() || current.is_empty() {
        return false;
    }
    let length = latest.len().max(current.len());
    for index in 0..length {
        let left = latest.get(index).copied().unwrap_or(0);
        let right = current.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

fn version_components(version: &str) -> Vec<u64> {
    normalized_version(version)
        .split(['.', '-', '+'])
        .take_while(|part| part.chars().all(|ch| ch.is_ascii_digit()))
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

fn content_length(response: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    response
        .headers()
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

fn safe_file_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|ch| match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '-' | '_' => ch,
            _ => '_',
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "MediaFlickDesktop-Setup.exe".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::version_is_newer;

    #[test]
    fn compares_release_versions() {
        assert!(version_is_newer("v0.1.3", "0.1.2"));
        assert!(version_is_newer("1.0", "0.9.9"));
        assert!(!version_is_newer("0.1.2", "0.1.2"));
        assert!(!version_is_newer("0.1.1", "0.1.2"));
    }
}
