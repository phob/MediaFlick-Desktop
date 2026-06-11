set dotenv-load := true
set windows-shell := ["C:\\Program Files\\PowerShell\\7\\pwsh.exe", "-NoLogo", "-ExecutionPolicy", "Bypass", "-Command"]

# Reuse the CEF cache from the upstream jellyfin-desktop checkout when it exists.
# Override with `CEF_PATH=... just build` to use/download a different CEF cache.
export JELLYFIN_DESKTOP_ROOT := env_var_or_default("JELLYFIN_DESKTOP_ROOT", "D:/users/pho/Documents/Source/jellyfin-desktop")
export CEF_PATH := env_var_or_default("CEF_PATH", JELLYFIN_DESKTOP_ROOT / ".cache" / "cef")
export CARGO_TARGET_DIR := env_var_or_default("CARGO_TARGET_DIR", "build/cargo-target")

# List recipes
[private]
list:
    @just --list --unsorted

# Remove build artifacts
[group('maintenance')]
[windows]
clean:
    if (Test-Path build) { Remove-Item -Recurse -Force build }
    if (Test-Path target) { Remove-Item -Recurse -Force target }

# Remove build artifacts
[group('maintenance')]
[unix]
clean:
    rm -rf build target

# Format the Rust crate
[group('lint')]
fmt:
    cargo fmt --all

# Check formatting
[group('lint')]
fmt-check:
    cargo fmt --all -- --check

# Run clippy
[group('lint')]
clippy:
    cargo clippy --all-targets -- -D warnings

# Build and stage the app into ./build
[group('build')]
[windows]
build:
    cargo build --bin jellyfin-mpv
    New-Item -ItemType Directory -Force build | Out-Null
    Get-ChildItem "$env:CARGO_TARGET_DIR/debug" -File | Copy-Item -Destination build -Force
    Remove-Item -Force -ErrorAction SilentlyContinue build/jellyfin-desktop*, build/jellyfin_desktop*
    if (Test-Path "$env:CARGO_TARGET_DIR/debug/locales") { Copy-Item "$env:CARGO_TARGET_DIR/debug/locales" build -Recurse -Force }

# Build and stage the app into ./build
[group('build')]
[unix]
build:
    cargo build --bin jellyfin-mpv
    mkdir -p build
    find "$CARGO_TARGET_DIR/debug" -maxdepth 1 -type f -exec cp {} build/ \;
    rm -f build/jellyfin-desktop* build/jellyfin_desktop*
    if [ -d "$CARGO_TARGET_DIR/debug/locales" ]; then rm -rf build/locales && cp -R "$CARGO_TARGET_DIR/debug/locales" build/locales; fi

# Build and stage a release app into ./build
[group('build')]
[windows]
release:
    cargo build --release --bin jellyfin-mpv
    New-Item -ItemType Directory -Force build | Out-Null
    Get-ChildItem "$env:CARGO_TARGET_DIR/release" -File | Copy-Item -Destination build -Force
    Remove-Item -Force -ErrorAction SilentlyContinue build/jellyfin-desktop*, build/jellyfin_desktop*
    if (Test-Path "$env:CARGO_TARGET_DIR/release/locales") { Copy-Item "$env:CARGO_TARGET_DIR/release/locales" build -Recurse -Force }

# Build and stage a release app into ./build
[group('build')]
[unix]
release:
    cargo build --release --bin jellyfin-mpv
    mkdir -p build
    find "$CARGO_TARGET_DIR/release" -maxdepth 1 -type f -exec cp {} build/ \;
    rm -f build/jellyfin-desktop* build/jellyfin_desktop*
    if [ -d "$CARGO_TARGET_DIR/release/locales" ]; then rm -rf build/locales && cp -R "$CARGO_TARGET_DIR/release/locales" build/locales; fi

# Run the staged app. Example: just run --url http://localhost:8096
[group('run')]
[windows]
run *args: build
    & 'build/jellyfin-mpv.exe' {{args}}

# Run the staged app. Example: just run --url http://localhost:8096
[group('run')]
[unix]
run *args: build
    build/jellyfin-mpv {{args}}

# Run the external mpv binary that will be wired into playback later
[group('run')]
[windows]
run-mpv *args:
    $mpv = if ($env:JELLYFIN_MPV_PATH) { $env:JELLYFIN_MPV_PATH } else { 'mpv' }; & $mpv {{args}}

# Run the external mpv binary that will be wired into playback later
[group('run')]
[unix]
run-mpv *args:
    "${JELLYFIN_MPV_PATH:-mpv}" {{args}}
