# jellyfin-mpv

A small Rust + CEF Jellyfin shell, based on the shape of `D:/users/pho/Documents/Source/jellyfin-desktop` but intentionally minimal. It opens a native CEF window with a first-run server/mpv setup screen, then loads your Jellyfin server and hands direct-play streams to an external `mpv` process.

## Requirements

- Rust toolchain
- `just`
- CMake + Ninja (required by `cef-dll-sys`)
- A CEF cache. The `justfile` defaults to the upstream checkout cache at `D:/users/pho/Documents/Source/jellyfin-desktop/.cache/cef`. Override with `CEF_PATH=...` if needed.

## Build

```sh
just build
```

For a non-debug build, use:

```sh
just non-debug
```

The staged app lands in `build/`:

```sh
build/jellyfin-mpv.exe
```

Run through `just`:

```sh
just run
```

The initial welcome screen asks for:

- Jellyfin server URL
- `mpv.exe` path, with a native Browse button

The app saves both values to:

```text
%APPDATA%\jellyfin-mpv\config.json
```

When both `jellyfin_url` and `mpv_path` are present, the welcome screen is skipped and Jellyfin opens directly. You can also seed or override the config from the CLI:

```sh
just run --url http://localhost:8096 --mpv-path "C:/Program Files/mpv/mpv.exe"
```

The app injects a small Jellyfin Web bridge that watches PlaybackInfo/direct-play stream URLs. Direct-play media requests are handed to external `mpv` with captured auth headers, Jellyfin resume position, and item/session metadata. While `mpv` is running, the app reports Jellyfin playback start/progress/stopped events so resume position is saved when playback ends.

mpv input bindings can be customized in:

```text
%APPDATA%\jellyfin-mpv\input.json
```

By default, `w` marks the current item watched and starts the next queued item. To change it:

```json
{
  "bindings": {
    "mark_watched_next": "W"
  }
}
```

Set the binding to an empty string to disable it.
