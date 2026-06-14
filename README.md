# jellyfin-mpv

I’ve always wanted an app that offered the convenience and look of the media player desktop apps provided by developers, but with the ability to enjoy SVP4 and SDR-to-HDR content. Almost all desktop media player apps are partially based on libmpv, without being able to fully utilize all of mpv’s capabilities. While there are the well-known mpv shim applications—which I’ve used for a very long time—the new Jellyfin desktop app, currently still in development, came with the promise that it would fully read the mpv configuration and thus be highly customizable. This is true in many respects, but especially when it comes to technologies like integrating SVP 4 and custom HDR profiles, I believe the limitation of having MPV within the app is the main factor behind many of these restrictions. And so I had the idea to simply write a desktop app for myself that exclusively connects to and controls an external MPV player.

The result is Jellyfin-MPV, a desktop app that offers the convenience of Jellyfin Web, with the ability to easily and efficiently connect your own MPV player. This is exactly what I’m now offering for download here. This app is not intended to—and will not—replace Jellyfin Desktop or Jellyfin MPV Shim, nor will it offer the full range of features found in both of those apps. Its primary purpose is to bridge the gap between Jellyfin Web and your own MPV player session.

## What this app does

Jellyfin-MPV opens Jellyfin Web in a desktop window, but sends direct-play media to an external `mpv.exe` instead of playing it inside the browser. This lets you use your own mpv setup, including your existing mpv configuration, shaders, scripts, SVP4 workflows, HDR profiles, and other custom playback features.

While mpv is playing, Jellyfin-MPV still talks to your Jellyfin server so playback state works as expected: playback starts, progress is reported, and the resume position is saved when playback stops.

## Install

### Windows installer

1. Download the latest `JellyfinMPV-Setup-<version>.exe` from the project releases.
2. Run the installer.
3. Launch **Jellyfin MPV** from the Start menu or the optional desktop shortcut.

The installer installs the app for the current user to:

```text
%LOCALAPPDATA%\Programs\Jellyfin MPV
```

If the release includes a bundled mpv, the app detects it automatically on first launch. In that case, you only need to enter your Jellyfin server URL.

### Portable / manual install

If you are using a release zip or a manually staged build:

1. Extract the app folder somewhere permanent.
2. Make sure `jellyfin-mpv.exe` stays next to the included CEF runtime files and `locales` folder.
3. Run `jellyfin-mpv.exe`.
4. If mpv is not bundled, select your own `mpv.exe` on the welcome screen.

## First launch

On first launch, Jellyfin-MPV asks for:

- **Jellyfin server URL** — for example `http://localhost:8096` or `https://jellyfin.example.com`
- **mpv.exe path** — the path to the mpv executable you want Jellyfin-MPV to control

You can use the native **Browse** button to select `mpv.exe`.

The app saves these settings here:

```text
%APPDATA%\jellyfin-mpv\config.json
```

After both values are saved, Jellyfin-MPV skips the welcome screen and opens Jellyfin directly on future launches.

## How to use

1. Open Jellyfin-MPV.
2. Log in to your Jellyfin server like you would in Jellyfin Web.
3. Choose a movie or episode and press play.
4. Jellyfin-MPV detects the direct-play stream and opens it in your external mpv player.
5. Control playback in mpv as usual.

Jellyfin-MPV reports playback progress back to Jellyfin, so watched state and resume positions should continue to work.

When you open the Jellyfin user menu inside the app, Jellyfin also shows an **Exit application** action. Use it to close the desktop app cleanly, including the external mpv controller.

## mpv configuration

Jellyfin-MPV uses an external mpv player, so your normal mpv setup can be used. Configure mpv the same way you normally would for your system, such as with `mpv.conf`, scripts, shaders, profiles, and input bindings.

Jellyfin-MPV also has its own small input binding file for app-specific actions:

```text
%APPDATA%\jellyfin-mpv\input.json
```

By default, pressing `w` marks the current item watched and starts the next queued item. To change that binding, create or edit `input.json`:

```json
{
  "bindings": {
    "mark_watched_next": "W"
  }
}
```

Set the binding to an empty string to disable it:

```json
{
  "bindings": {
    "mark_watched_next": ""
  }
}
```

## Command-line options

You can also provide the Jellyfin URL and mpv path from the command line:

```powershell
jellyfin-mpv.exe --url http://localhost:8096 --mpv-path "C:\Program Files\mpv\mpv.exe"
```

This is useful for testing, shortcuts, or quickly switching between servers and mpv installations.

## Build it yourself

Building is mainly intended for developers and advanced users.

### Requirements

- Rust toolchain
- `just`
- CMake and Ninja, required by `cef-dll-sys`
- A CEF cache. Set `CEF_PATH=...` to the CEF cache you want to use.

### Build a local debug app

```sh
just build
```

The staged app is created in `build/`:

```text
build/jellyfin-mpv.exe
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
$env:JELLYFIN_MPV_PACKAGE_MPV = "C:\path\to\mpv" # directory, or path to mpv.exe
just windows-dist
```

The staged payload is created in:

```text
dist/JellyfinMPV/
```

### Build the Windows installer

Install Inno Setup 6, then run:

```powershell
$env:ISCC = "C:\path\to\ISCC.exe" # optional if ISCC.exe is on PATH
just windows-installer
```

The installer is created in:

```text
dist/installer/JellyfinMPV-Setup-<version>.exe
```
