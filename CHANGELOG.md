# Changelog

## [Unreleased]

### Breaking Changes

### Added

- Added an About dialog showing the app version, git version, and creator.
- Added WebUI fullscreen toggling from the context menu and F11.

### Changed

- Restyled the About dialog to use a compact dark panel.
- Replaced Jellyfin-logo-based app artwork with original MediaFlick Desktop gradient icon artwork across the app, installer, Linux, macOS, setup screens, and README.
- Updated Windows installer dialogs to show the MediaFlick Desktop logo artwork.
- Rebranded the app, package metadata, documentation, release workflow, and Windows artifacts to MediaFlick Desktop.
- Opened new-window and off-server Jellyfin Web links in the system default browser instead of CEF.

### Fixed

- Fixed a duplicate separator in the Jellyfin Web context menu.

### Removed

- Removed Print and View Source from the Jellyfin Web context menu.

## [0.1.1] - 2026-06-14

### Added

- Added the initial Jellyfin-MPV desktop app that embeds Jellyfin Web in a CEF window while routing direct-play media to an external mpv player.
- Added first-run setup for the Jellyfin server URL and mpv executable path, including a native browse action for selecting `mpv.exe` and persisted settings in `%APPDATA%\jellyfin-mpv\config.json`.
- Added command-line options for launching with a Jellyfin server URL and mpv path.
- Added the JavaScript/Rust Jellyfin bridge that intercepts Jellyfin Web playback, resolves stream playback info, launches mpv, and reports Jellyfin playstate updates.
- Added an external mpv IPC controller with command/event pipes, playback progress observation, pause/seek/stop support, and fullscreen mpv launch behavior.
- Added native Jellyfin Web player-control integration so play/pause, seeking, and playback state can control and reflect external mpv playback.
- Added bidirectional playback-state synchronization from mpv back into Jellyfin Web, including progress, pause state, stop/end handling, and saved resume position updates.
- Added bridge logging and synthetic media-readiness events so intercepted browser playback remains visible and debuggable.
- Added rotating app log files with configurable `--log-level`/`--log-file` options and redacted playback diagnostics.
- Added configurable mpv input bindings in `%APPDATA%\jellyfin-mpv\input.json`, including the default `w` binding to mark the current item watched and start the next queued item.
- Added an **Exit application** action to the Jellyfin Web user menu for cleanly closing the desktop app and external mpv controller.
- Added persistent Jellyfin Web window sizing between launches.
- Added platform application resources, including Windows icons/resources, a macOS app icon and `Info.plist` template, and Linux desktop/metainfo/icon files.
- Added build recipes for debug, release, non-debug run, Windows distribution staging, and Windows installer creation.
- Added Windows release packaging that stages the app with CEF runtime files, locales, and an optional bundled mpv tree.
- Added an Inno Setup installer definition and packaging script for creating `JellyfinMPV-Setup-<version>.exe`.
- Added changelog-driven draft release automation that promotes `CHANGELOG.md` `[Unreleased]` entries into the requested version and creates a draft GitHub release from those notes.
- Added automatic Windows installer and zip artifact builds to the draft release workflow.
- Added Renovate configuration for automated dependency update proposals.
- Added project changelog rules and a `/cl` prompt for auditing unreleased entries before release.
- Added playback regression guard documentation covering the known-good startup/resume behavior.
- Added user-facing README documentation for installation, first launch, usage, mpv configuration, command-line options, local builds, and release packaging.

### Changed

- Reduced playback log noise by keeping frequent mpv position updates (`time-pos`/`playback-time`) out of default debug logs.
- Reorganized the Rust and bridge sources into `app`, `cef`, `jellyfin`, `mpv`, `ui`, and `windows` modules.
- Updated the app to use bundled/default app icons and set the Windows window icon.
- Expanded the README from developer build notes into end-user installation and packaging documentation.

### Fixed

- Fixed resume/startup behavior by waiting for mpv `file-loaded` before seeking to Jellyfin resume positions.
- Fixed mpv handoff URLs by stripping browser-only fragments before sending `loadfile` commands while preserving Jellyfin resume seeks.
- Fixed transient mpv `playback-abort` snapshots so pending loads are not failed before mpv reports `end-file`.
- Fixed Jellyfin Web playback state getting out of sync when mpv stops or reaches the end of an item.
- Fixed packaged CEF startup by wiring subprocess, resource, locale, and Windows GPU runtime settings.
- Fixed watched-state and next-item flow by adding explicit mpv stop handling and a watched-next binding path.
- Fixed watched-next handling to close the current mpv process and let Jellyfin Web's normal autoplay flow decide whether to start the next episode.
- Fixed unwanted Windows console windows from helper script launches by hiding spawned script consoles.
- Fixed app shutdown so the external mpv controller is closed when exiting from the Jellyfin Web UI.
