# Building MediaFlick Desktop

Building is mainly intended for developers and advanced users.

## Requirements

- Rust toolchain
- `just`
- CMake and Ninja, required by `cef-dll-sys`
- A CEF cache. By default, `just` uses `%LOCALAPPDATA%/MediaFlick/cache/cef` on Windows and `$XDG_CACHE_HOME/mediaflick/cef` or `~/.cache/mediaflick/cef` elsewhere; set `CEF_PATH=...` to override it.
- Node and pnpm for the embedded React UI.
- The .NET 10 SDK selected by `global.json` for the optional server-side
  Companion plugin, which targets and tests on `net10.0` for Jellyfin 12.

## Build a local debug app

```sh
just build
```

The staged app is created in `build/`:

```text
build/mediaflick-desktop.exe
```

Run it with:

```sh
just run --url http://localhost:8096
```

## Build and test the Companion plugin

The plugin has a separate toolchain and is not part of `cargo build`:

```sh
just plugin-test
just plugin
```

The publish output is written to `plugin/bin/Release/publish`. Maintainers with
access to the configured Tailscale development host can install it and restart
Jellyfin with `just plugin-deploy`.

## Build a release app

```sh
just release
```

## Build a Windows release package

Windows packages require the MediaFlick libmpv build. From Linux or WSL, install
the prerequisites in
[`distribution/libmpv/windows/README.md`](distribution/libmpv/windows/README.md), then
build the runtime and its corresponding-source archive:

```sh
bash distribution/libmpv/windows/build.sh
```

Then stage a Windows release payload with the app, CEF runtime, locales, and
`libmpv-2.dll`:

```powershell
just windows-dist
```

The staged payload is created in:

```text
dist/windows/MediaFlickDesktop/
```

The source archive under `build/libmpv-windows-x64/` must be published beside
the installer and zip. Linux can use a system libmpv runtime as described below;
macOS continues to use external system mpv.

## Build the Windows installer

Install Inno Setup 6, then run:

```powershell
$env:ISCC = "C:\path\to\ISCC.exe" # optional if ISCC.exe is on PATH
just windows-installer
```

The installer is created in:

```text
dist/windows/MediaFlickDesktop-Setup-<version>.exe
```

## Build Linux and macOS release packages

For Linux built-in playback, install libmpv with client API major 2 and the
OpenGL render API (the runtime SONAME is `libmpv.so.2`), plus working X11 and
EGL/OpenGL 3.3 drivers, using your distribution's package manager. MediaFlick
loads it dynamically, so headers and link-time libmpv configuration are not required. Select Built-in player in Settings → Player,
save, and restart. Wayland sessions need XWayland and a
valid `DISPLAY`. External mpv remains the Linux default. AppImages use the
host's libmpv instead of bundling it. See
[the Linux integration notes](docs/libmpv-integration.md#integrated-linux-rendering).

An opt-in runtime test can load a local video through libmpv and its IPC server:

```sh
CEF_PATH="$(just --evaluate CEF_PATH)" \
CARGO_TARGET_DIR="$(just --evaluate CARGO_TARGET_DIR)" \
MEDIAFLICK_DESKTOP_LIBMPV_PATH=/path/to/libmpv.so.2 \
MEDIAFLICK_DESKTOP_LIBMPV_MEDIA_PATH=/path/to/test-video.mkv \
cargo test configured_library_initializes_its_ipc_server -- --ignored
```

The Linux browser-focus regression test uses an isolated X11 display and does
not require a Jellyfin account or media file. With Xvfb installed:

```sh
CEF_PATH="$(just --evaluate CEF_PATH)" \
CARGO_TARGET_DIR="$(just --evaluate CARGO_TARGET_DIR)" \
xvfb-run -a cargo test internal_focus_transfers_and_grabs_keep_browser_focus -- --ignored
```

Linux AppImage packaging requires `appimagetool` or network access so the script can download it:

```sh
just linux-appimage
```

macOS DMG packaging creates an unsigned/ad-hoc signed `.app` bundle:

```sh
just macos-dmg
```

Packages are written to `dist/windows/`, `dist/linux/`, or `dist/macos/`.
