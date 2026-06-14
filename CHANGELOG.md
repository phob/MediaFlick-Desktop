# Changelog

## [Unreleased]

### Breaking Changes

### Added

- Added the initial Jellyfin-MPV desktop app that embeds Jellyfin Web in a CEF window while routing direct-play media to an external mpv player.
- Added first-run setup for the Jellyfin server URL and mpv executable path, including a native browse action for selecting `mpv.exe` and persisted settings in `%APPDATA%\jellyfin-mpv\config.json`.
- Added command-line options for launching with a Jellyfin server URL and mpv path.
- Added the JavaScript/Rust Jellyfin bridge that intercepts Jellyfin Web playback, resolves stream playback info, launches mpv, and reports Jellyfin playstate updates.
- Added an external mpv IPC controller with command/event pipes, playback progress observation, pause/seek/stop support, and fullscreen mpv launch behavior.
- Added native Jellyfin Web player-control integration so play/pause, seeking, and playback state can control and reflect external mpv playback.
- Added bidirectional playback-state synchronization from mpv back into Jellyfin Web, including progress, pause state, stop/end handling, and saved resume position updates.
- Added bridge logging and synthetic media-readiness events so intercepted browser playback remains visible and debuggable.
- Added configurable mpv input bindings in `%APPDATA%\jellyfin-mpv\input.json`, including the default `w` binding to mark the current item watched and start the next queued item.
- Added an **Exit application** action to the Jellyfin Web user menu for cleanly closing the desktop app and external mpv controller.
- Added persistent Jellyfin Web window sizing between launches.
- Added platform application resources, including Windows icons/resources, a macOS app icon and `Info.plist` template, and Linux desktop/metainfo/icon files.
- Added build recipes for debug, release, non-debug run, Windows distribution staging, and Windows installer creation.
- Added Windows release packaging that stages the app with CEF runtime files, locales, and an optional bundled mpv tree.
- Added an Inno Setup installer definition and packaging script for creating `JellyfinMPV-Setup-<version>.exe`.
- Added changelog-driven draft release automation that promotes `CHANGELOG.md` `[Unreleased]` entries into the requested version and creates a draft GitHub release from those notes.
- Added automatic Windows installer and zip artifact builds to the draft release workflow.
- Added project changelog rules and a `/cl` prompt for auditing unreleased entries before release.
- Added playback regression guard documentation covering the known-good startup/resume behavior.
- Added user-facing README documentation for installation, first launch, usage, mpv configuration, command-line options, local builds, and release packaging.

### Changed

- Reorganized the Rust and bridge sources into `app`, `cef`, `jellyfin`, `mpv`, `ui`, and `windows` modules.
- Updated the app to use bundled/default app icons and set the Windows window icon.
- Expanded the README from developer build notes into end-user installation and packaging documentation.

### Fixed

- Fixed resume/startup behavior by waiting for mpv `file-loaded` before seeking to Jellyfin resume positions.
- Fixed Jellyfin Web playback state getting out of sync when mpv stops or reaches the end of an item.
- Fixed watched-state and next-item flow by adding explicit mpv stop handling and a watched-next binding path.
- Fixed unwanted Windows console windows from helper script launches by hiding spawned script consoles.
- Fixed app shutdown so the external mpv controller is closed when exiting from the Jellyfin Web UI.

### Removed
