# Architecture

MediaFlick Desktop is organized around application domains rather than the technologies used to deliver them.

## Dependency direction

```text
main / bootstrap
  -> shell (CEF and app-owned UI)
  -> players (mpv and MPC-HC adapters)
  -> maintenance (updates and player installation)
  -> preferences (model, persistence port, apply policy)
  -> jellyfin (Web bridge and HTTP adapters)

shell / players / jellyfin
  -> playback (backend-neutral contracts and policies)

playback
  -> preferences (settings value types referenced by playback contracts)
```

The `playback` domain must not expose CEF, mpv IPC, MPC-HC slave-mode, or Jellyfin HTTP types. It may depend on `preferences` value types such as `FullscreenBehavior` and `SegmentSkipConfig`; `preferences` must not depend on `playback`. Concrete player construction belongs at the application boundary in `players::build_backend`.

## Domains

### Playback

`src/playback` owns media requests, commands, snapshots, events, segment policy, playback-context correlation, application-lifetime playback IDs, the player port, and `PlaybackCoordinator`. The coordinator gives the shell a stable handle while allowing the configured backend to be replaced without exposing adapter details.

### Players

`src/players/mpv` and `src/players/mpchc` translate playback contracts to native player protocols. Process supervision, IPC paths, mpv JSON commands, MPC-HC `WM_COPYDATA`, and backend-specific behavior remain here.

### Jellyfin

`src/jellyfin` is the Jellyfin adapter boundary. It parses authenticated Web bridge actions, maps direct streams to playback requests, fetches media segments, and reports playstate. Bridge paths are parsed exactly and every privileged request requires the per-session token.

### Preferences

`src/preferences` owns the settings model, the `SettingsStore` persistence port, its filesystem adapter, and `SettingsApplyPlan`, which determines runtime effects such as rebuilding a player or refreshing bridge configuration.

### Maintenance

`src/maintenance` owns update and external-player installation workflows. Platform downloads and installer behavior stay out of the playback domain.

### Shell

`src/shell/cef` adapts CEF callbacks to application actions and marshals resource-request bridge work to CEF's UI thread. `src/shell/ui` contains app-owned templates, scripts, and presentation serialization. CEF browser handles never enter the playback domain.

## Concurrency rules

- CEF browser, frame, window, dialog, and JavaScript operations run on the CEF UI thread.
- `BrowserState` is used only to copy or update shell state. It must not be held while a player is opened, controlled, replaced, warmed, or shut down.
- Player adapters own their worker threads and normalize their public surface through `PlayerBackend`.
- Backend replacement publishes the replacement under the coordinator lock and retires the previous backend on a detached thread so no CEF thread waits on player teardown.
- Playback-context registration (`play-context`) is handled synchronously on the CEF IO thread so a following stream-resource request always observes it; all other bridge actions are marshalled to the UI thread.
- Playback contexts are correlated by `PlaybackContextRegistry`; adapters must reject context that conflicts with the active playback identity.

## CEF subprocess startup

CEF subprocess detection and bridge-token initialization happen before normal CLI parsing, logging, single-instance acquisition, or application-service startup. Renderer processes reload persisted settings before installing the bridge so reused renderers receive current configuration.
