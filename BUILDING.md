# Building MediaFlick Desktop

Building is mainly intended for developers and advanced users.

## Requirements

- Rust toolchain
- `just`
- CMake and Ninja, required by `cef-dll-sys`
- A CEF cache. By default, `just` downloads/caches CEF in this checkout at `.cache/cef`; set `CEF_PATH=...` to override it.

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

## Build a release app

```sh
just release
```

## Build a Windows release package

To stage a Windows release payload with the app, CEF runtime files, and locales (mpv is no longer bundled; the app downloads it on first run):

```powershell
just windows-dist
```

The staged payload is created in:

```text
dist/MediaFlickDesktop/
```

## Build the Windows installer

Install Inno Setup 6, then run:

```powershell
$env:ISCC = "C:\path\to\ISCC.exe" # optional if ISCC.exe is on PATH
just windows-installer
```

The installer is created in:

```text
dist/installer/MediaFlickDesktop-Setup-<version>.exe
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

The packages are written to `dist/`.
