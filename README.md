<h1 align="center">
  <img src="resources/app-icon.png" alt="MediaFlick Desktop logo" width="240" height="240">
  <br>
</h1>

# MediaFlick Desktop

[![Draft Release](https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml/badge.svg)](https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml)
[![Latest release](https://img.shields.io/github/v/release/phob/mediaflick-desktop?display_name=tag&sort=semver)](https://github.com/phob/mediaflick-desktop/releases/latest)

External mpv playback for Jellyfin.

MediaFlick Desktop opens Jellyfin Web in a desktop CEF window, then hands direct-play media streams to an external `mpv` process instead of playing them inside the browser. It is built for people who want the Jellyfin Web experience while keeping their own mpv setup: `mpv.conf`, scripts, shaders, SVP4 workflows, HDR profiles, input bindings, and other custom playback features.

While mpv is playing, MediaFlick Desktop still reports playstate back to your Jellyfin server so playback starts, progress, watched state, and resume positions continue to work.

## Why

I’ve always wanted an app that offered the convenience and look of the media player desktop apps provided by developers, but with the ability to enjoy SVP4 and SDR-to-HDR content. Almost all desktop media player apps are partially based on libmpv, without being able to fully utilize all of mpv’s capabilities. While there are the well-known mpv shim applications—which I’ve used for a very long time. Now the new Jellyfin desktop app, currently still in development, came with the promise that it would fully read the mpv configuration and thus be highly customizable. This is true in many respects, but especially when it comes to technologies like integrating SVP 4 and custom HDR profiles, I believe the limitation of having MPV within the app is the main factor behind many of these restrictions. And so I had the idea to simply write a desktop app for myself that exclusively connects to and controls an external MPV player.

## Disclosure

**This Project is AI assisted mainly for getting the linux and mac builds created and to review the code.**
**AI was used to bundle the App and for the github actions**

## Install

### Windows installer

1. Download the latest `MediaFlickDesktop-Setup-<version>.exe` from [GitHub Releases](https://github.com/phob/mediaflick-desktop/releases/latest).
2. Run the installer.
3. Launch **MediaFlick Desktop** from the Start menu or the optional desktop shortcut.

The installer installs the app for the current user to:

```text
%LOCALAPPDATA%\Programs\MediaFlick Desktop
```

If the release includes a bundled mpv, the app detects it automatically on first launch. In that case, you only need to enter your Jellyfin server URL.

MediaFlick Desktop checks GitHub Releases for newer Windows installers. When an update is available, an in-app toast lets you download it, shows progress, then runs the installer quietly and restarts the app into the new version.

### Portable / manual install

If you are using a release zip or a manually staged build:

1. Extract the app folder somewhere permanent.
2. Make sure `mediaflick-desktop.exe` stays next to the included CEF runtime files and `locales` folder.
3. Run `mediaflick-desktop.exe`.
4. If mpv is not bundled, select your own `mpv.exe` on the welcome screen.

## First launch

On first launch, MediaFlick Desktop asks for:

- **Jellyfin server URL** — for example `http://localhost:8096` or `https://jellyfin.example.com`
- **mpv.exe path** — the path to the mpv executable you want MediaFlick Desktop to control

You can use the native **Browse** button to select `mpv.exe`.

The app saves these settings here:

```text
%APPDATA%\mediaflick-desktop\config.json
```

## mpv configuration

MediaFlick Desktop uses an external mpv player, so your normal mpv setup can be used. Configure mpv the same way you normally would for your system, such as with `mpv.conf`, scripts, shaders, profiles, and input bindings.

MediaFlick Desktop also has its own small input binding file for app-specific actions:

```text
%APPDATA%\mediaflick-desktop\input.json
```

By default, pressing `w` marks the current item watched, closes the current mpv process, and lets Jellyfin Web's normal autoplay setting decide whether to start the next queued item. To change that binding, create or edit `input.json`:

```json
{
  "bindings": {
    "mark_watched_next": "W"
  }
}
```

## Command-line options

You can also provide the Jellyfin URL and mpv path from the command line:

```powershell
mediaflick-desktop.exe --url http://localhost:8096 --mpv-path "C:\Program Files\mpv\mpv.exe"
```

This is useful for testing, shortcuts, or quickly switching between servers and mpv installations.

## Build it yourself

Building is mainly intended for developers and advanced users.

### Requirements

- Rust toolchain
- `just`
- CMake and Ninja, required by `cef-dll-sys`
- A CEF cache. By default, `just` downloads/caches CEF in this checkout at `.cache/cef`; set `CEF_PATH=...` to override it.

### Build a local debug app

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

### Build a release app

```sh
just release
```

### Build a Windows release package

To stage a Windows release payload with the app, CEF runtime files, locales, and a bundled mpv tree:

```powershell
$env:MEDIAFLICK_DESKTOP_PACKAGE_MPV = "C:\path\to\mpv" # directory, or path to mpv.exe
just windows-dist
```

The staged payload is created in:

```text
dist/MediaFlickDesktop/
```

### Build the Windows installer

Install Inno Setup 6, then run:

```powershell
$env:ISCC = "C:\path\to\ISCC.exe" # optional if ISCC.exe is on PATH
just windows-installer
```

The installer is created in:

```text
dist/installer/MediaFlickDesktop-Setup-<version>.exe
```

### Build Linux and macOS release packages

Linux AppImage packaging requires `appimagetool` or network access so the script can download it:

```sh
just linux-appimage
```

macOS DMG packaging creates an unsigned/ad-hoc signed `.app` bundle:

```sh
just macos-dmg
```

The packages are written to `dist/`.
