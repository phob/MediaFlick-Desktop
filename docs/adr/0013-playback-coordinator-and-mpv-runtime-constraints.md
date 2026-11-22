# 0013. Playback coordinator actor model and mpv runtime constraints

Status: Accepted (2026-11-22). Consolidated from codebase audit of `src/playback/`, `src/players/`, and `.claude/skills/verify-mediaflick/`.

## Context

Extending [ADR 0001](0001-built-in-libmpv-player.md), [ADR 0002](0002-native-video-composition.md), and [ADR 0003](0003-mpv-json-ipc-and-post-load-resume.md), playback spans CEF UI-thread calls, Jellyfin remote control, audio queues, external/built-in mpv backends, and FFmpeg demuxing across Windows, Linux, and macOS.

## Decision

- **Single-actor `PlaybackCoordinator`.** One actor thread serializes session state, audio queue preparation, UI/remote commands, backend events, and shell event emission. Native window and overlay calls bypass the actor straight to the installed `PlayerBackend` without holding any mutex so presentation never stalls behind Jellyfin negotiation; each backend adapter therefore serializes its own internal state. `PlaybackCoordinator::replace` bumps the `installation` generation so events from a retired backend are ignored and runs `retired.shutdown()` on a detached `playback-retire` thread so CEF's UI thread never blocks on teardown. Because backends emit `BackendEvent::Failed` for refused loads without a follow-up `BackendEvent::Stopped`, the coordinator synthesizes `stopped(StopReason::Error)` when `self.live == Some(load_id)`.
- **Audio vs video playback and command semantics.** `AudioStart::Default` starts music at 0 and resumes audiobooks with rewind; `AudioStart::At(ticks)` applies to audiobooks only. Audio playback passes file-local `vid=no` and `audio-display=no` in `loadfile` options so the next video restores video automatically, cover art does not hijack libmpv's host window, and external mpv stays windowless. `PlayerSnapshot::now_playing` is set strictly by coordinator intent—never inferred from mpv's track list, because MP3 cover art exposes an attached-picture video track. `PlayerCommand::TogglePause` evaluates against mpv's live pause state rather than a potentially stale UI snapshot so rapid presses do not send duplicate `SetPause(true)`.
- **mpv and FFmpeg runtime quirks.**
  - `stream-lavf-o-append` sets `short_seek_size=1048576` (1 MiB) so FFmpeg reads forward across gaps instead of opening a new HTTP/TLS request per `.m4b` chapter title (reducing 285-chapter remote audiobook loads from 45–125 s to 2–5 s) while preserving External mpv user `mpv.conf` options.
  - External Jellyfin subtitles pass `sid=no` in `loadfile` to avoid flashing an embedded subtitle before post-load `sub-add <url> select`.
  - `seek <seconds> absolute+exact` clamps to `>= 0.0` because mpv treats negative values as offsets from EOF.
  - `PlaybackRequest::audio_mpv_id` and `subtitle_mpv_id` are 1-based per-kind embedded track ordinals, distinct from Jellyfin's 0-based global stream indices.
  - Embedded file chapters take precedence over injected Jellyfin media-segment markers at identical timestamps.
  - External mpv spawns with `CREATE_NO_WINDOW` on Windows (suppressing `mpv.com`'s transient console despite `--no-terminal`) and `--focus-on=all` on Linux/macOS (raising a warm idle process's window on file load).
  - On Linux `gpu-next`, default fullscreen waits (`FullscreenGate`) until the opaque UI exposes video and the startup resume seek settles so decoder buffer replacement never overlaps fullscreen render-target resize.
  - On headless Linux, mpv requires an audio output device to emit `reason=eof` instead of `reason=error`; seeking to EOF before any audio plays also ends with `reason=error` (`"no audio or video data played"`), halting audio queues; External mpv 0.41 under Xvfb hangs on a second `loadfile` after `stop` in the same process (reproduced in bare mpv).

## Rejected alternatives

- Synchronizing `PlaybackCoordinator` behind a single mutex shared with CEF UI overlay callbacks: stalls native overlay presentation during Jellyfin playback negotiation and backend teardown.
- Inferring audio vs video mode from mpv's reported track list: misclassifies music files carrying embedded cover art.
- Leaving FFmpeg's default HTTP `short_seek_size`: triggers hundreds of sequential TLS/range requests on chaptered `.m4b` audiobooks, exceeding load timeouts.

## Consequences

- Every `PlayerBackend` implementation must be thread-safe against concurrent coordinator and CEF UI calls.
- Linux E2E playback drives must provide a virtual audio sink for play-to-end assertions and relaunch between multi-file External mpv passes under Xvfb.
- Lean libmpv builds without Lua/JS return `MPV_ERROR_OPTION_NOT_FOUND` (`-5`) for `load-scripts`/`osc`; Desktop and smoke tests tolerate `-5` only when disabling them (`"no"`), keeping failures fatal when SVP tries to enable scripting.
