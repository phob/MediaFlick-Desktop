use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=MEDIAFLICK_DESKTOP_GIT_VERSION");
    println!("cargo:rerun-if-env-changed=MEDIAFLICK_DESKTOP_CREATED_BY");

    let repo_root = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    track_git_refs(&repo_root);
    build_ui(&repo_root);

    let git_version = std::env::var("MEDIAFLICK_DESKTOP_GIT_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| git_version(&repo_root).unwrap_or_else(|| "unknown".to_string()));
    let created_by = std::env::var("MEDIAFLICK_DESKTOP_CREATED_BY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "phob".to_string());

    println!("cargo:rustc-env=MEDIAFLICK_DESKTOP_GIT_VERSION={git_version}");
    println!("cargo:rustc-env=MEDIAFLICK_DESKTOP_CREATED_BY={created_by}");

    #[cfg(target_os = "windows")]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("resources/win/app.ico");
        resource.set("CompanyName", "MediaFlick");
        resource.set("FileDescription", "MediaFlick Desktop");
        resource.set("InternalName", "mediaflick-desktop");
        resource.set("OriginalFilename", "mediaflick-desktop.exe");
        resource.set("ProductName", "MediaFlick Desktop");
        resource.compile().unwrap_or_else(|error| {
            panic!("failed to compile Windows resources: {error}");
        });
    }
}

/// Builds the UI bundle that `src/shell/cef/api.rs` embeds with `include_bytes!`.
///
/// Cargo owns this rather than a separate `just ui` step so `cargo build` stays
/// self-sufficient and can never embed a stale `ui/dist` — the failure mode is
/// silent otherwise, and it would ship in a release binary.
///
/// Set `MEDIAFLICK_DESKTOP_SKIP_UI_BUILD=1` when the bundle was already built
/// out of band (CI splits the steps to cache `node_modules`).
fn build_ui(repo_root: &Path) {
    let ui_dir = repo_root.join("ui");
    if !ui_dir.join("package.json").is_file() {
        panic!("ui/package.json is missing — the UI bundle cannot be built");
    }

    // Any of these changing invalidates the bundle. `ui/src` is watched
    // recursively.
    for path in [
        "src",
        "index.html",
        "package.json",
        "pnpm-lock.yaml",
        "vite.config.ts",
    ] {
        println!("cargo:rerun-if-changed={}", ui_dir.join(path).display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("resources/app-icon.png").display()
    );
    println!("cargo:rerun-if-env-changed=MEDIAFLICK_DESKTOP_SKIP_UI_BUILD");

    if std::env::var("MEDIAFLICK_DESKTOP_SKIP_UI_BUILD").as_deref() == Ok("1") {
        assert!(
            ui_dir.join("dist/app.js").is_file(),
            "MEDIAFLICK_DESKTOP_SKIP_UI_BUILD=1 but ui/dist/app.js does not exist"
        );
    } else {
        run_pnpm(&ui_dir, &["install", "--frozen-lockfile"]);
        run_pnpm(&ui_dir, &["build"]);
    }

    stage_bundle(&ui_dir.join("dist"));
}

/// Copies the built bundle into `OUT_DIR` for `include_bytes!`.
///
/// Embedding straight from `ui/dist` looks simpler but breaks: cargo skips
/// `build.rs` whenever its tracked inputs are unchanged, so a wiped `ui/dist`
/// leaves `include_bytes!` pointing at nothing. `OUT_DIR` is cargo-managed and
/// survives exactly as long as the fingerprint that produced it.
fn stage_bundle(dist: &Path) {
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_else(|| {
        panic!("Cargo did not set OUT_DIR for the MediaFlick build script");
    }));

    for asset in ["app.js", "app.css", "index.html"] {
        let source = dist.join(asset);
        let target = out_dir.join(asset);
        std::fs::copy(&source, &target).unwrap_or_else(|error| {
            panic!(
                "failed to stage {} into OUT_DIR: {error}. Run `just ui` to rebuild the bundle.",
                source.display()
            )
        });
    }
}

fn run_pnpm(ui_dir: &Path, args: &[&str]) {
    // On Windows pnpm is a `.cmd` shim, which `Command::new` will not resolve
    // from the bare name.
    let candidates: &[&str] = if cfg!(windows) {
        &["pnpm.cmd", "pnpm"]
    } else {
        &["pnpm"]
    };

    let mut last_error = None;
    for program in candidates {
        match Command::new(program)
            .args(args)
            .current_dir(ui_dir)
            .status()
        {
            Ok(status) if status.success() => return,
            Ok(status) => panic!("`pnpm {}` failed with {status}", args.join(" ")),
            Err(error) => last_error = Some(error),
        }
    }

    let last_error = last_error.map_or_else(
        || "no pnpm executable candidates were configured".to_string(),
        |error| error.to_string(),
    );
    panic!(
        "could not run `pnpm {}` in {}: {}. Install Node and pnpm, or set \
         MEDIAFLICK_DESKTOP_SKIP_UI_BUILD=1 with a prebuilt ui/dist.",
        args.join(" "),
        ui_dir.display(),
        last_error,
    );
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
