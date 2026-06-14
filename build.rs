use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=JELLYFIN_MPV_GIT_VERSION");
    println!("cargo:rerun-if-env-changed=JELLYFIN_MPV_CREATED_BY");

    let repo_root = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    track_git_refs(&repo_root);

    let git_version = std::env::var("JELLYFIN_MPV_GIT_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| git_version(&repo_root).unwrap_or_else(|| "unknown".to_string()));
    let created_by = std::env::var("JELLYFIN_MPV_CREATED_BY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "phob".to_string());

    println!("cargo:rustc-env=JELLYFIN_MPV_GIT_VERSION={git_version}");
    println!("cargo:rustc-env=JELLYFIN_MPV_CREATED_BY={created_by}");

    #[cfg(target_os = "windows")]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("resources/win/jellyfin.ico");
        resource.set("CompanyName", "Jellyfin");
        resource.set("FileDescription", "Jellyfin MPV");
        resource.set("InternalName", "jellyfin-mpv");
        resource.set("OriginalFilename", "jellyfin-mpv.exe");
        resource.set("ProductName", "Jellyfin MPV");
        resource
            .compile()
            .expect("failed to compile Windows resources");
    }
}

fn git_version(repo_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--always", "--dirty=-dirty"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn track_git_refs(repo_root: &Path) {
    let git_dir = repo_root.join(".git");
    if git_dir.is_dir() {
        println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
        println!("cargo:rerun-if-changed={}", git_dir.join("index").display());
        println!(
            "cargo:rerun-if-changed={}",
            git_dir.join("packed-refs").display()
        );
        return;
    }

    if let Ok(git_file) = std::fs::read_to_string(&git_dir)
        && let Some(path) = git_file.trim().strip_prefix("gitdir:")
    {
        let path = repo_root.join(path.trim());
        println!("cargo:rerun-if-changed={}", path.join("HEAD").display());
        println!("cargo:rerun-if-changed={}", path.join("index").display());
        println!(
            "cargo:rerun-if-changed={}",
            path.join("packed-refs").display()
        );
    }
}
