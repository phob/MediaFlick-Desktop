### Added

- Native Linux Wayland zero-copy composition for the built-in player (`gpu-next` on `waylandvk` stacked under a DMA-BUF Chromium UI subsurface, with automatic `wl_shm` UI fallback and `MEDIAFLICK_FORCE_X11=1` escape hatch), alongside Wayland and PipeWire support in the bundled Linux `libmpv` runtime.
- Out-of-process Linux `libmpv` player helper that isolates playback crashes (`SIGSEGV`) so the interface stays responsive and recovers cleanly, and bounds application shutdown when a media or GPU driver hangs.
- Host display-sleep inhibition during active playback (`systemd-inhibit` / ScreenSaver on Linux verified; Windows and macOS power-assertion runtime verification is still outstanding).
- Native OS media controls and hardware media keys across platforms (MPRIS2 verified on GNOME/Linux with D-Bus seek, play/pause, stop, and cleanup; Windows SMTC and macOS Now Playing runtime verification is still outstanding).
- Trickplay thumbnail previews while scrubbing on the player timeline.
- Optional audio passthrough setting (`Audio passthrough` in Player settings) and host hardware/codec capability probing for Jellyfin device profiles and bitstream negotiation.
- Account-scoped durable SQLite playstate outbox (`library.db` schema v24) that retains and replays queued Jellyfin progress and stop reports across server outages or restarts without losing watch position or leaking auth tokens.
- Expanded pipeline telemetry in the player diagnostics panel showing active video output, GPU context, hardware decoder/interop, pixel format, colorimetry/HDR peak, audio format, decoder/VO drops, mistimed frames, A/V sync, and vsync jitter.

### Changed

- Built-in Windows playback now configures `vo=gpu-next` with non-blocking DirectComposition commits and bounded `libmpv` teardown (Windows runtime verification is still outstanding).
- Playback startup skips redundant Jellyfin metadata fetches when stream sources are known and schedules resume seeks immediately on `playback-restart` while preserving resume-position accuracy.
- Upgraded local `library.db` cleanly to schema v24 (expand-only) with FTS5 local search and trigger-maintained `series_aggregates` for fast browsing, facet loading, and search across 100,000+ item catalogs, keeping full compatibility with 0.2.0 downgrades.
- Updated local search documentation in `README.md` and the What's New dialog to accurately list searchable fields (`titles`, `original titles`, and `genres`).

### Fixed

- Eliminated full-frame CPU `overlay-add` UI copies and XWayland swapchain overhead when running built-in `libmpv` under Linux Wayland compositors.
