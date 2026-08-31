# Building MediaFlick Desktop

Building is mainly intended for developers and advanced users.

## Requirements

- Rust toolchain
- `just`
- CMake and Ninja, required by `cef-dll-sys`
- A CEF cache. By default, `just` uses `%LOCALAPPDATA%/MediaFlick/cache/cef` on Windows and `$XDG_CACHE_HOME/mediaflick/cef` or `~/.cache/mediaflick/cef` elsewhere; set `CEF_PATH=...` to override it.
- Node and pnpm for the embedded React UI.
- .NET 10 SDK for the optional server-side Companion plugin. The plugin still
  targets `net9.0` because Jellyfin 10.11 runs on that host framework.

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
the installer and zip. Linux and macOS packages continue to use an external
system mpv until equivalent native libmpv bundles are added.

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

Linux AppImage packaging requires `appimagetool` or network access so the script can download it:

```sh
just linux-appimage
```

macOS DMG packaging creates an unsigned/ad-hoc signed `.app` bundle:

```sh
just macos-dmg
```

Packages are written to `dist/windows/`, `dist/linux/`, or `dist/macos/`.
