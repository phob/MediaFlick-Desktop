use std::fmt;
use std::path::Path;
use std::process::Command;

/// Cargo prints a failed build script's `main` error with `Debug`, so this
/// prints the message as written instead of a quoted, escaped string.
struct BuildError(String);

impl fmt::Debug for BuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn main() -> Result<(), BuildError> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=MEDIAFLICK_DESKTOP_GIT_VERSION");
    println!("cargo:rerun-if-env-changed=MEDIAFLICK_DESKTOP_CREATED_BY");

    let repo_root = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    track_git_refs(&repo_root);
    build_ui(&repo_root)?;

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
        resource.set_icon("distribution/windows/app.ico");
        resource.set("CompanyName", "MediaFlick");
        resource.set("FileDescription", "MediaFlick Desktop");
        resource.set("InternalName", "mediaflick-desktop");
        resource.set("OriginalFilename", "mediaflick-desktop.exe");
        resource.set("ProductName", "MediaFlick Desktop");
        resource
            .compile()
            .map_err(|error| BuildError(format!("failed to compile Windows resources: {error}")))?;
    }
    Ok(())
}

/// Builds the UI bundle that `src/shell/cef/api.rs` embeds with `include_bytes!`.
///
/// Cargo owns this rather than a separate `just ui` step so `cargo build` stays
/// self-sufficient and can never embed a stale `ui/dist` — the failure mode is
/// silent otherwise, and it would ship in a release binary.
///
/// Set `MEDIAFLICK_DESKTOP_SKIP_UI_BUILD=1` when the bundle was already built
/// out of band (CI splits the steps to cache `node_modules`).
fn build_ui(repo_root: &Path) -> Result<(), BuildError> {
    let ui_dir = repo_root.join("ui");
    if !ui_dir.join("package.json").is_file() {
        return Err(BuildError(
            "ui/package.json is missing — the UI bundle cannot be built".to_string(),
        ));
    }

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
        repo_root.join("distribution/app-icon.png").display()
    );
    println!("cargo:rerun-if-env-changed=MEDIAFLICK_DESKTOP_SKIP_UI_BUILD");

    if std::env::var("MEDIAFLICK_DESKTOP_SKIP_UI_BUILD").as_deref() == Ok("1") {
        if !ui_dir.join("dist/app.js").is_file() {
            return Err(BuildError(
                "MEDIAFLICK_DESKTOP_SKIP_UI_BUILD=1 but ui/dist/app.js does not exist".to_string(),
            ));
        }
    } else {
        run_pnpm(&ui_dir, &["install", "--frozen-lockfile"])?;
        run_pnpm(&ui_dir, &["build"])?;
    }

    stage_bundle(&ui_dir.join("dist"))
}

/// Copies the built bundle into `OUT_DIR` for `include_bytes!`.
///
/// Embedding straight from `ui/dist` looks simpler but breaks: cargo skips
/// `build.rs` whenever its tracked inputs are unchanged, so a wiped `ui/dist`
/// leaves `include_bytes!` pointing at nothing. `OUT_DIR` is cargo-managed and
/// survives exactly as long as the fingerprint that produced it.
fn stage_bundle(dist: &Path) -> Result<(), BuildError> {
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or_else(|| {
        BuildError("Cargo did not set OUT_DIR for the MediaFlick build script".to_string())
    })?);

    for asset in ["app.js", "app.css", "index.html"] {
        let source = dist.join(asset);
        let target = out_dir.join(asset);
        std::fs::copy(&source, &target).map_err(|error| {
            BuildError(format!(
                "failed to stage {} into OUT_DIR: {error}. Run `just ui` to rebuild the bundle.",
                source.display()
            ))
        })?;
    }
    Ok(())
}

fn run_pnpm(ui_dir: &Path, args: &[&str]) -> Result<(), BuildError> {
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
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => {
                return Err(BuildError(format!(
                    "`pnpm {}` failed with {status}",
                    args.join(" ")
                )));
            }
            Err(error) => last_error = Some(error),
        }
    }

    let last_error = last_error.map_or_else(
        || "no pnpm executable candidates were configured".to_string(),
        |error| error.to_string(),
    );
    Err(BuildError(format!(
        "could not run `pnpm {}` in {}: {}. Install Node and pnpm, or set \
         MEDIAFLICK_DESKTOP_SKIP_UI_BUILD=1 with a prebuilt ui/dist.",
        args.join(" "),
        ui_dir.display(),
        last_error,
    )))
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

/// Reruns the build when the described commit changes: a checkout moves
/// `HEAD`, and a commit moves the branch `HEAD` names. The index is left out on
/// purpose: it changes on every `git add`, and each rerun rebuilds the embedded
/// UI. A development build's `-dirty` suffix can therefore lag until the next
/// commit or checkout.
fn track_git_refs(repo_root: &Path) {
    let dot_git = repo_root.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let Some(path) = std::fs::read_to_string(&dot_git).ok().and_then(|file| {
            file.trim()
                .strip_prefix("gitdir:")
                .map(|path| path.trim().to_string())
        }) else {
            return;
        };
        repo_root.join(path)
    };
    // Branch refs live in the shared git dir; a worktree names it in `commondir`.
    let common_dir = std::fs::read_to_string(git_dir.join("commondir"))
        .map(|path| git_dir.join(path.trim()))
        .unwrap_or_else(|_| git_dir.clone());

    let head = git_dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", head.display());
    // A packed branch has no loose ref file until it next moves; naming a
    // missing path would rerun the script on every build, and `packed-refs`
    // already covers it.
    if let Ok(head) = std::fs::read_to_string(&head)
        && let Some(reference) = head.trim().strip_prefix("ref:")
        && let branch = common_dir.join(reference.trim())
        && branch.is_file()
    {
        println!("cargo:rerun-if-changed={}", branch.display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        common_dir.join("packed-refs").display()
    );
}
