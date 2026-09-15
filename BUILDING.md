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

On Linux, the app registers a fallback desktop entry and icon in the user data
directory (`$XDG_DATA_HOME`, or `~/.local/share`) before opening its window.
This gives direct launches and `just run` the MediaFlick name and icon in the
dock. The fallback does not appear in the application menu and preserves
user-installed or system-installed entries with the same desktop ID.

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

For a disposable packaged test build, run **Draft Release** in GitHub Actions
on the desired branch with a prerelease version such as `0.2.0-test.1` and
`test_draft` enabled. This uses the normal platform builds and smoke tests,
then creates a draft marked as a prerelease. It does not commit a release,
push a version tag, or consume `CHANGELOG.md` or `changelog.d/`. The temporary
version patch is applied only to the build jobs. Keep this test release as a
draft; its downloads are accessible to repository maintainers after signing in.

Use the separate **Draft Companion Release** workflow for server packages;
see [Companion test repository](plugin/README.md#test-repository).

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
the installer and zip. Linux AppImages bundle their own Linux libmpv runtime;
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

Linux AppImages include a dedicated libmpv runtime without DVD, Lua, or
VapourSynth support. For unpackaged Linux developer builds, install mpv 0.41 or
newer with client API major 2, gpu-next, and X11 Vulkan/EGL backends (the runtime
SONAME is `libmpv.so.2`), plus working X11 and Vulkan or EGL/OpenGL drivers, using
your distribution's package manager. MediaFlick
loads it dynamically, so headers and link-time libmpv configuration are not required. Select Built-in player in Settings → Player,
save, and restart. Wayland sessions need XWayland and a
valid `DISPLAY`. External mpv remains the Linux default. See
[the Linux integration notes](docs/libmpv-integration.md#integrated-linux-rendering).

An opt-in runtime test can load a local video through libmpv and its IPC server:

```sh
CEF_PATH="$(just --evaluate CEF_PATH)" \
CARGO_TARGET_DIR="$(just --evaluate CARGO_TARGET_DIR)" \
MEDIAFLICK_DESKTOP_LIBMPV_PATH=/path/to/libmpv.so.2 \
MEDIAFLICK_DESKTOP_LIBMPV_MEDIA_PATH=/path/to/test-video.mkv \
cargo test configured_library_initializes_its_ipc_server -- --ignored
```

The desktop-registration integration test checks icon/name lookup and launcher
execution through GIO in an isolated user data directory. It needs
`desktop-file-validate` and `/usr/bin/python3` with PyGObject, but no display:
The native gpu-next test checks actual displayed pixels while idle and paused,
including transparency, frame ownership, and resizing from browser raster pixels
to X11 coordinates. Test Vulkan under Xvfb, then force a missing Vulkan driver
to verify the OpenGL fallback:

```sh
CEF_PATH="$(just --evaluate CEF_PATH)" \
CARGO_TARGET_DIR="$(just --evaluate CARGO_TARGET_DIR)" \
cargo test desktop_shell_resolves_and_launches_registered_identity -- --ignored
MEDIAFLICK_DESKTOP_LIBMPV_PATH="$PWD/build/libmpv-linux-x86_64/lib/libmpv.so.2" \
MEDIAFLICK_DESKTOP_EXPECT_GPU_CONTEXT=x11vk \
xvfb-run -a cargo test configured_gpu_next_composes -- --ignored --nocapture

CEF_PATH="$(just --evaluate CEF_PATH)" \
CARGO_TARGET_DIR="$(just --evaluate CARGO_TARGET_DIR)" \
MEDIAFLICK_DESKTOP_LIBMPV_PATH="$PWD/build/libmpv-linux-x86_64/lib/libmpv.so.2" \
MEDIAFLICK_DESKTOP_EXPECT_GPU_CONTEXT=x11egl \
VK_DRIVER_FILES=/nonexistent VK_ICD_FILENAMES=/nonexistent \
xvfb-run -a cargo test configured_gpu_next_composes -- --ignored --nocapture
```

The Linux browser-focus regression test uses an isolated X11 display and does
not require a Jellyfin account or media file. With Xvfb installed:

```sh
CEF_PATH="$(just --evaluate CEF_PATH)" \
CARGO_TARGET_DIR="$(just --evaluate CARGO_TARGET_DIR)" \
xvfb-run -a cargo test internal_focus_transfers_and_grabs_keep_browser_focus -- --ignored
```

Linux AppImage packaging builds the dedicated Linux libmpv runtime first.
Install its build dependencies and enable source repositories as described in
[`distribution/libmpv/linux/README.md`](distribution/libmpv/linux/README.md).
Packaging also requires `appimagetool` or network access so the script can download it:

```sh
just linux-appimage
```

Publish `dist/linux/mediaflick-libmpv-linux-<arch>-sources.tar.zst` beside the
AppImage. To rebuild only libmpv, use `just libmpv`. To package an existing
release binary and runtime, run `bash distribution/linux/build-appimage.sh`.

macOS DMG packaging creates an unsigned/ad-hoc signed `.app` bundle:

```sh
just macos-dmg
```

Packages are written to `dist/windows/`, `dist/linux/`, or `dist/macos/`.
