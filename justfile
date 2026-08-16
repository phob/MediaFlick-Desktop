set dotenv-load := true
set windows-shell := ["C:\\Program Files\\PowerShell\\7\\pwsh.exe", "-NoLogo", "-ExecutionPolicy", "Bypass", "-Command"]

# Cache CEF inside this checkout by default.
# Override with `CEF_PATH=... just build` to use/download a different CEF cache.
export CEF_PATH := env_var_or_default("CEF_PATH", justfile_directory() / ".cache" / "cef")
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

# Build the UI bundle into ui/dist (cargo build does this too, via build.rs)
[group('build')]
ui:
    pnpm --dir ui install --frozen-lockfile
    pnpm --dir ui build

# Build the server-side MediaFlick Companion plugin
[group('build')]
plugin:
    dotnet publish plugin/Jellyfin.Plugin.MediaFlick/Jellyfin.Plugin.MediaFlick.csproj --configuration Release --output plugin/bin/Release/publish

# Run the companion plugin unit tests
[group('test')]
plugin-test:
    dotnet test plugin/Jellyfin.Plugin.MediaFlick.Tests/Jellyfin.Plugin.MediaFlick.Tests.csproj --configuration Release

# Deploy the plugin to the configured Jellyfin development host and restart it
[group('run')]
plugin-deploy: plugin
    ssh pho@archlinux 'mkdir -p /opt/jellyfin/library/data/plugins/MediaFlick'
    scp plugin/bin/Release/publish/* pho@archlinux:/opt/jellyfin/library/data/plugins/MediaFlick/
    ssh pho@archlinux 'docker restart jellyfin'

# Run the Vite dev server against the UI bundle
[group('run')]
ui-dev:
    pnpm --dir ui dev

# Format the Rust crate
[group('lint')]
fmt:
    cargo fmt --all

# Check formatting
[group('lint')]
fmt-check:
    cargo fmt --all -- --check

# Run Rust tests
[group('test')]
test:
    cargo test --all-targets

# Run clippy
[group('lint')]
clippy:
    cargo clippy --all-targets -- -D warnings

# Build and stage the app into ./build
[group('build')]
[windows]
build:
    cargo build --bin mediaflick-desktop
    New-Item -ItemType Directory -Force build | Out-Null
    Get-ChildItem "$env:CARGO_TARGET_DIR/debug" -File | Copy-Item -Destination build -Force
    Remove-Item -Force -ErrorAction SilentlyContinue build/jellyfin-desktop*, build/jellyfin_desktop*, build/*mpv*
    if (Test-Path "$env:CARGO_TARGET_DIR/debug/locales") { Copy-Item "$env:CARGO_TARGET_DIR/debug/locales" build -Recurse -Force }

# Build and stage the app into ./build
[group('build')]
[unix]
build:
    cargo build --bin mediaflick-desktop
    mkdir -p build
    find "$CARGO_TARGET_DIR/debug" -maxdepth 1 -type f -exec cp {} build/ \;
    rm -f build/jellyfin-desktop* build/jellyfin_desktop* build/*mpv*
    if [ -d "$CARGO_TARGET_DIR/debug/locales" ]; then rm -rf build/locales && cp -R "$CARGO_TARGET_DIR/debug/locales" build/locales; fi

# Build and stage a release app into ./build
[group('build')]
[windows]
release:
    cargo build --release --bin mediaflick-desktop
    New-Item -ItemType Directory -Force build | Out-Null
    Get-ChildItem "$env:CARGO_TARGET_DIR/release" -File | Copy-Item -Destination build -Force
    Remove-Item -Force -ErrorAction SilentlyContinue build/jellyfin-desktop*, build/jellyfin_desktop*, build/*mpv*
    if (Test-Path "$env:CARGO_TARGET_DIR/release/locales") { Copy-Item "$env:CARGO_TARGET_DIR/release/locales" build -Recurse -Force }

# Build and stage a release app into ./build
[group('build')]
[unix]
release:
    cargo build --release --bin mediaflick-desktop
    mkdir -p build
    find "$CARGO_TARGET_DIR/release" -maxdepth 1 -type f -exec cp {} build/ \;
    rm -f build/jellyfin-desktop* build/jellyfin_desktop* build/*mpv*
    if [ -d "$CARGO_TARGET_DIR/release/locales" ]; then rm -rf build/locales && cp -R "$CARGO_TARGET_DIR/release/locales" build/locales; fi

# Build and stage a non-debug app into ./build
[group('build')]
non-debug: release

# Stop the interactive app that owns the config-wide single-instance gate.
[private]
[windows]
stop-running-app:
    $running = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'mediaflick-desktop.exe' -and $_.CommandLine -notmatch '--type=' }); foreach ($entry in $running) { $process = Get-Process -Id $entry.ProcessId -ErrorAction SilentlyContinue; if (-not $process) { continue }; if ($process.MainWindowHandle -ne 0) { $null = $process.CloseMainWindow(); $null = $process.WaitForExit(5000) }; if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force } }

# Stop the interactive app that owns the config-wide single-instance gate.
[private]
[unix]
stop-running-app:
    pkill -TERM -x mediaflick-desktop 2>/dev/null || true

# Restart the staged development app so the single-instance gate can never
# leave `just run` showing the UI embedded in an older process.
[group('run')]
[windows]
run *args: stop-running-app build
    & 'build/mediaflick-desktop.exe' {{args}}

# Run the staged app. Example: just run --url http://localhost:8096
[group('run')]
[unix]
run *args: stop-running-app build
    build/mediaflick-desktop {{args}}

# Run a non-debug staged app. Example: just run-non-debug --url http://localhost:8096
[group('run')]
[windows]
run-non-debug *args: stop-running-app non-debug
    & 'build/mediaflick-desktop.exe' {{args}}

# Run a non-debug staged app. Example: just run-non-debug --url http://localhost:8096
[group('run')]
[unix]
run-non-debug *args: stop-running-app non-debug
    build/mediaflick-desktop {{args}}

# Run the external mpv binary that will be wired into playback later
[group('run')]
[windows]
run-mpv *args:
    $mpv = if ($env:MEDIAFLICK_DESKTOP_MPV_PATH) { $env:MEDIAFLICK_DESKTOP_MPV_PATH } else { 'mpv' }; & $mpv {{args}}

# Run the external mpv binary that will be wired into playback later
[group('run')]
[unix]
run-mpv *args:
    "${MEDIAFLICK_DESKTOP_MPV_PATH:-mpv}" {{args}}

# Stage a Windows release payload with CEF under ./dist/MediaFlickDesktop
[group('package')]
[windows]
windows-dist: release
    & './scripts/stage-windows-release.ps1'

# Build a per-user Windows setup.exe from the staged release payload
[group('package')]
[windows]
windows-installer: windows-dist
    & './scripts/build-windows-installer.ps1'

# Build a Linux AppImage from the staged release binary and CEF runtime files
[group('package')]
[linux]
linux-appimage: release
    ./scripts/build-linux-appimage.sh

# Build a macOS DMG containing a signed .app bundle and CEF framework
[group('package')]
[macos]
macos-dmg: release
    ./scripts/build-macos-dmg.sh
