# Changelog

## [Unreleased]

### Breaking Changes

### Added

- Added infinite scroll to the Jellyfin library card/poster grid: scrolling toward the bottom now lazy-loads and appends the next page of items in place and hides the pagination controls, instead of requiring the Next/Previous page buttons. It reuses Jellyfin Web's own paged fetch and card rendering (so cards, images, and auth match exactly) by intercepting the grid container's content updates and appending rather than replacing, so it works across the different library controllers (Movies, TV Shows, and other paged grids). It applies only to the card grid layout (the list/table view keeps its native pager), only takes over once the full pager is present, and degrades to normal pagination if the expected Jellyfin Web DOM is not found.

- Added a `CI` GitHub Actions workflow that runs on every pull request and on pushes to `main`, checking formatting (`cargo fmt --check`) on Linux and running clippy (`-D warnings`), the test suite, and a binary build on both Linux and Windows so dependency and code changes are validated on every PR.
- Enabled Renovate auto-merge for non-major dependency updates and lock-file maintenance: once the `CI` checks pass, Renovate merges these PRs itself (`platformAutomerge` disabled), while major updates still open a PR for manual review.

### Changed

- Demoted the high-frequency Jellyfin playstate log lines (state send, state sent, and progress-report-due) from `debug` to `trace` so the default `debug` log level is no longer dominated by them.
- Expanded the `sending mpv command` log line so `set_property` commands now report the property and its value inline (for example `set_property pause=true`), summarizing large array/object values like `chapter-list` as an item/field count instead of dumping them.
- Changed the Client Settings "Log level" control from a free-text input with suggestions to a proper dropdown listing Error, Warn, Info, Debug, and Trace.

### Fixed

### Removed


## [0.1.5] - 2026-06-24

### Added

- Added in-app mpv setup: the welcome and Client Settings screens now offer a one-click "Download mpv" on Windows (fetched and extracted from the shinchiro mpv builds) and copyable per-OS install commands on macOS and Linux, plus a link to mpv.io/installation.
- Added native Jellyfin intro and credits skipping in mpv with prompt/always settings and forward-seek prompt acceptance.
- Added Jellyfin recap and commercial segment skipping in mpv with their own prompt/always Client Settings, alongside the existing intro and credits options.
- Added skip-segment markers on the mpv seek bar by injecting chapter ticks at each segment's boundaries, so the timeline shows where intros, credits, recaps, and commercials are skipped. Existing embedded file chapters are preserved and merged with the markers rather than replaced.
- Added an MPC-HC player backend on Windows: a new "Player backend" client setting switches playback between mpv and MPC-HC, driven over slave mode (`WM_COPYDATA` / `MpcApi.h`) — open, resume, play/pause, seek, playback speed, audio/subtitle track selection, segment skipping with on-screen prompts and auto-skip countdowns, Jellyfin progress reporting, and end-of-file auto-advance to the next episode. The setting is a segmented switch that shows only the active backend's options and applies live without restarting the app; MPC-HC launches on first playback rather than at app start. Both players sit behind a new `PlayerBackend` trait with a capability probe. On MPC-HC, external subtitles play by requesting a server-side burned-in transcode, volume and mute are emulated through relative volume steps, and the configured default fullscreen behavior is applied on every load (matching mpv); chapter-marker pips and the mark-watched hotkey remain mpv-only and degrade gracefully. On every file open MediaFlick asserts the selected audio/subtitle track (translating mpv's 1-based track ids to MPC-HC's 0-based indexes) and the resume position. Because MPC-HC services `CMD_SETPOSITION` with a synchronous seek that can block its window for many seconds while it re-fetches a remote stream (the duration depends heavily on the file — sub-second on some, ~15s on high-bitrate 4K), all slave-mode commands are dispatched on a dedicated sender thread so the app stays responsive during the seek, rapid seeks are coalesced to the latest target, and a "Seeking..." on-screen message is shown until MPC-HC reports the seek completed.
- Added unit tests for the CEF bridge origin allowlist, external-link scheme checks, and U+2028/U+2029, HTML, and percent-encoding escaping helpers, locking in recent security hardening.
- Added a single-instance gate so only one MediaFlick session runs at a time: a stable id is persisted in `instance.json` (mirroring Jellyfin Desktop's instance file) and used to name a Windows mutex acquired at startup. A second launch detects the existing session, shows an "already running" message, and exits without starting a duplicate player or WebUI. CEF subprocesses are unaffected.
- Added a reusable in-app error toast that surfaces user-facing failures as a dismissible notification injected into the WebUI (styled to match the update toast), with a Copy button to put the error text on the clipboard. It stays on screen until dismissed, and reports when playback is requested but no media player is configured, and when a player backend fails to start playback — mpv or MPC-HC failing to launch (for example a wrong executable path) or mpv rejecting the video — instead of failing silently in the log.

### Changed

- Redesigned the first-run welcome screen to match the app's dark, Jellyfin-compatible design system: removed the marketing-style gradient background, unified colors and typography with the settings dialog, reserved the violet→cyan gradient for the brand mark, and integrated the "get mpv" setup inline instead of as a nested card.
- Stopped bundling mpv in the Windows installer and zip; the app now downloads mpv on first run or guides the user to install it, producing a much smaller installer.
- Changed automatic intro and credits skipping to show a three-second countdown before seeking.
- Moved app-owned dialog and load-error markup templates out of Rust source files.
- Slimmed the README to a Why, Features, and short Install section, and moved the build instructions into a dedicated `BUILDING.md`.
- Replaced the About and Client Settings dialog brand marks with the app logo.
- Polished the About dialog and redesigned the update notification as a compact pill without installer filename copy.
- Changed the default CEF cache location to the project-local `.cache/cef` directory instead of an upstream Jellyfin Desktop checkout path.
- Updated the Rust `cef` crate to v149.1.0 ([#12](https://github.com/phob/MediaFlick-Desktop/pull/12) by [@renovate](https://github.com/apps/renovate)).
- Updated the draft release workflow to use `actions/cache@v6` ([#14](https://github.com/phob/MediaFlick-Desktop/pull/14) by [@renovate](https://github.com/apps/renovate)).
- Updated the Rust `sevenz-rust2` crate to v0.21.1 ([#15](https://github.com/phob/MediaFlick-Desktop/pull/15) by [@renovate](https://github.com/apps/renovate)).
- Changed the Windows auto-update installer launch to use Inno Setup `/SILENT` instead of `/VERYSILENT`.
- Changed the Windows in-app mpv download to install mpv inside the app installation directory.
- Changed Windows mpv window raising on file load to pulse the `window-minimized` IPC property so the player window takes focus, instead of the `ontop` pulse which only changed z-order.
- Scoped the Jellyfin page `fetch` and `XMLHttpRequest` hooks to PlaybackInfo, play-state report, and direct-stream URLs, so unrelated page requests pass straight through to the native implementation.
- Reworked the Client Settings dialog into Player, Playback, and Application tabs so it no longer grows into one long scroll when a backend's options expand, and the backend-dependent options stay confined to the Player tab. Added a scroll-edge fade and chevron hint that appears when a section overflows the window, so there is still a visual cue that more settings exist below the fold even when scrollbars are hidden app-wide (the `--hide-scrollbars` renderer flag otherwise suppresses the dialog's own scrollbar). The tab strip is keyboard navigable with arrow keys.

### Fixed

- Fixed the Linux and macOS release builds failing to compile because the `warnings = "deny"` lint flagged the Windows-only MPC-HC segment helpers and mpv auto-download phases as dead code on those platforms.
- Fixed every native bridge message (player commands, playback context, play-state reports) being delivered twice because `sendBridgeRequest` fired both a `fetch` and an `<img>` request as a fallback pair; it now sends one and only falls back to the image when `fetch` is unavailable. This was duplicating each play/pause/seek/volume command — and the duplicate seeks compounded MPC-HC's synchronous seek stalls.
- Fixed the external player (mpv or MPC-HC) sometimes being left running after MediaFlick exits — for example after switching player backends at runtime — by binding each spawned player to a Windows job object the OS terminates when the app process ends.
- Hardened the auto-updater to download installers only over HTTPS from GitHub-owned hosts and into a unique per-run directory, preventing redirect-to-untrusted-host and predictable-temp-path attacks.
- Restricted the native `mediaflick-desktop://` bridge to pages from our own local UI or the configured Jellyfin origin, so unrelated page content can no longer drive mpv, settings, or app exit.
- Restricted Jellyfin playback-state reporting to `http(s)` targets and percent-encoded the media-segments item id, closing SSRF and path-injection vectors from page-supplied stream URLs.
- Restricted the configured server URL and externally opened links to `http`/`https` (links also allow `mailto`), rejecting `file:`, `data:`, `about:`, and other schemes.
- Pinned the Windows command-processor shim to the system `cmd.exe` resolved from `SystemRoot` instead of an attacker-settable `COMSPEC` override.
- Surfaced mpv IPC command rejections in the log instead of silently treating every command as successful.
- Fixed an auto-skipped intro/credits segment being consumed even when its seek failed, so the skip can be retried after the mpv session recovers.
- Escaped U+2028/U+2029 in data injected into the Jellyfin page, preventing server-derived playback fields from breaking the injected script.
- Fixed native mpv forward seeks not accepting an active skip-intro/credits prompt when mpv reports the seek event before the `seeking` property.
- Fixed the in-app mpv download deleting a working install before the new archive is validated; the build is now extracted to a staging directory, checked for `mpv.exe`, and swapped into place with the previous install kept until the swap succeeds.
- Fixed segment chapter markers overwriting a file's embedded chapters when a duration or media-segment update raced ahead of mpv's `chapter-list` event; marker injection now waits until the file's own chapters have been captured.
- Fixed Linux and macOS update notifications by linking the updater dialog to the GitHub latest release page instead of offering unsupported automatic installation.
- Fixed Linux and macOS first launch by auto-detecting a system `mpv` executable and using generic mpv executable wording in app UI.
- Fixed Linux AppImage startup aborts with `close symbol missing` by preloading bundled CEF and stripping that preload from spawned mpv processes.
- Fixed the mpv window staying at full-screen size after leaving fullscreen when playback starts fullscreen, by constraining the windowed size with `--autofit=70%`.
- Added a Jellyfin Web integration check that logs a clear console error and shows a dismissible banner when the bridge cannot install its required hooks (for example, after an incompatible Jellyfin Web update), instead of silently failing to drive mpv.
- Required a per-session token on `mediaflick-desktop://` requests originating from the Jellyfin page, so in-origin scripts (such as a rogue Jellyfin plugin or injected content) can no longer forge bridge actions like app exit or playback control.
- Fixed mpv commands silently failing after a half-open IPC connection by detecting a dead command-writer and restarting the mpv session, instead of waiting for the event stream to also disconnect.
## [0.1.4] - 2026-06-19

### Added

- Added a `just test` recipe for running the Rust test suite.

### Changed

- Moved mpv episode transition handling into a dedicated playback transition module.
- Changed mpv lifecycle management to warm a hidden idle IPC process when an executable is configured and to reuse it until the configured mpv path changes or the app exits.
- Changed external mpv raise handling to rely on mpv's own `--focus-on=all` support on Linux/macOS plus a temporary IPC `ontop` pulse instead of Win32 window activation.
- Updated the Rust `cef` crate to v149 ([#10](https://github.com/phob/MediaFlick-Desktop/pull/10) by [@renovate](https://github.com/apps/renovate)).

### Fixed

- Fixed next-episode handoff snapshots reusing the previous episode's final playback position before the new mpv file finished loading.
- Fixed late Jellyfin play-session context not being merged into active mpv playback reports after an external-player handoff.
- Fixed the external-player backdrop remaining above Jellyfin Web after stopping playback from mpv during an auto-started next episode.
- Fixed stale mpv EOF/mark-watched stop events from ending the newly started next episode by correlating WebUI stop handling with playback IDs and Jellyfin item/session identifiers.
- Fixed mpv session supervision so the configured idle IPC process is polled, restarted after process or IPC loss, reconnected once before media handoff commands fail, and cancelled cleanly during app shutdown.
- Fixed cold and slow mpv startup being treated as unavailable too quickly by extending IPC/media readiness waits, logging when mpv exits before creating its IPC pipe, and keeping watched-next handoffs on the existing mpv IPC session while ignoring stale browser stop commands.
- Fixed automatic next-episode playback after mpv reaches EOF by keeping the warm mpv handoff protected and emitting Jellyfin Web's stopped player event with ended stop details.
- Fixed mpv `q`/window-close handling to stop only the current file, keep the warm mpv process alive, and restart the idle process if it exits unexpectedly while the app is running.
- Fixed app shutdown so the controller waits for the warm external `mpv.exe` process to quit or be killed before the app exits.
- Fixed Jellyfin Web playstate synchronization after mpv-driven `q`, `w`, and EOF stops by preserving final player state until Jellyfin handles the stopped event.
- Fixed the external-player backdrop/white-background layering by keeping the synthetic player backdrop above Jellyfin's page background while placing the video OSD above it.
## [0.1.3] - 2026-06-17

### Added

- Added a Client Settings dialog for mpv path browsing, log level, default fullscreen behavior, close behavior, scrollbar visibility, and the mark-watched-next input binding.
- Added Windows auto-update checks with an in-app update toast, download progress, quiet installer launch, and automatic restart into the updated version.
- Added Linux AppImage and macOS DMG packaging scripts and release artifacts to the draft release workflow.

### Changed

- Reduced About and Client Settings dialog copy to keep app-owned surfaces terse.
- Redesigned the About dialog with MediaFlick brand treatment, clearer product copy, metadata grouping, and improved keyboard focus behavior.
- Redesigned the Client Settings dialog with grouped controls, Jellyfin-compatible dark styling, stronger focus states, and clearer save/error feedback.
- Changed draft release automation to build all required platform artifacts before committing and tagging a release.
- Updated draft release workflow actions to `actions/cache@v5` and `actions/checkout@v6` ([#2](https://github.com/phob/mediaflick-desktop/pull/2), [#3](https://github.com/phob/mediaflick-desktop/pull/3), [#5](https://github.com/phob/mediaflick-desktop/pull/5), [#6](https://github.com/phob/mediaflick-desktop/pull/6) by [@renovate](https://github.com/apps/renovate)).
- Updated draft release workflow runners and artifact actions to Ubuntu 24.04, `actions/upload-artifact@v7`, and `actions/download-artifact@v8` ([#7](https://github.com/phob/mediaflick-desktop/pull/7), [#8](https://github.com/phob/mediaflick-desktop/pull/8) by [@renovate](https://github.com/apps/renovate)).
- Updated README rationale and streamlined usage documentation.

### Fixed

- Fixed Client Settings labels sitting above their controls after terse copy removal.
- Fixed packaged macOS CEF startup by resolving bundle resource and framework paths from the app bundle layout.
- Fixed non-Windows CEF compilation by matching platform keyboard event types and normalizing CEF enum IDs.
- Fixed Linux and macOS release builds by compiling the hidden command processor shim only on Windows and making packaging scripts executable.
## [0.1.2] - 2026-06-14

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
