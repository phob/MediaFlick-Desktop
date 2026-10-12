# Playback

Play, Resume and From start on a movie or episode hand the title to mpv: the Built-in player (libmpv, the default; on Linux the app draws its player bar over the video in the same window) or External mpv in its own window, with the app's player bar at the bottom of the page. MediaFlick reports start, progress, pause and stop to Jellyfin, so the resume point follows the user across devices, and a resumed title starts where the server says, by a seek after mpv has loaded the file.

## Sub-features

- `play-start` plays from the beginning (`Play`) and from the stored point (`Resume`; `From start` ignores it).
- `player-bar` holds Previous/Next episode, `Back 10 seconds`, `Pause`/`Resume`, `Forward 30 seconds`, the `Playback position` slider, the time toggle, `Mute`, `Volume N%`, `Playback settings`, `Toggle fullscreen` and `More playback controls` (with `Stop playback`).
- `player-keys`: Space or `k` pause, ←/`j` back 10 s, →/`l` forward 30 s, `m` mute, `f` fullscreen; Built-in only: ↓/↑ ∓30 s, `q` stop, `v` subtitles, `w` mark watched and play next.
- `playstate` reports `Sessions/Playing`, `Sessions/Playing/Progress` (every 10 s and on each pause change) and `Sessions/Playing/Stopped` to Jellyfin.
- `resume` applies the stored point after mpv's `file-loaded` with a delayed seek, and holds the reported position until mpv reaches it (AGENTS.md, Playback invariants).
- `segments` offers `Skip Intro` / `Skip Credits` / `Skip Recap` / `Skip Commercial` inside a Jellyfin media segment, per the Playback shelf's skip settings.
- `next-episode` plays on at the end of an episode (Viewing `Next episode`: `Automatically play` by default, or a countdown toast with `Play now` / `Stop watching`).

## How to get to it (user POV)

- A movie or episode detail page: `Play`, or `Resume` and `From start` once a resume point exists. A series page's primary `Play S1E2` / `Resume S1E2` plays Next Up; episode cards have their own `Play`/`Resume` on hover.
- Home's billboard `Play`/`Resume`, and `Play <title>` on Releases entries.
- While playing: the player bar (Built-in on Linux: move the pointer near the bottom edge to show it) and the keyboard.

## Driving it with ctx (just verify)

Preconditions:

- Your own server: `just verify playback --account SERVER --log-level "debug,jellyfin.playstate=trace,playback=trace"`. The trace levels put each playstate report's endpoint and position in `app.log` for `ctx.playstates()`. Never play on the demo server.
- The full first sync: `Resume` and the start point come from the local cache. Wait for `bootstrap.state === "ready" && syncProgress.active === false`.
- External mpv: add `--seed .claude/skills/verify-mediaflick/seeds/external-mpv` (`player_backend: mpv`, `mpv_path: /usr/bin/mpv`, `default_fullscreen: windowed`).

- **Pick and restore.** `playback` takes the first unplayed H.264 movie of an hour or more without a resume point (`ctx.jellyfin`), and registers `ctx.restoreItemAfterRun(id)` so the title ends unplayed without a resume point even when the drive fails mid-playback. It opens the title through sidebar search. *Proven by `playback`.*
- **Play.** `Play` starts within a few seconds; `ctx.player()` (`GET /api/player/state`) turns `active` with `itemId`, `playMethod` (`DirectPlay` for a 1080p H.264 MKV), `positionMs` and `durationMs`. `app.log` has `starting playback … start=none`. `ctx.mpv("time-pos")` reads mpv itself over its IPC socket and advances; `current-vo` was `gpu-next` (Built-in, `hwdec` `vaapi-copy` under Xvfb) and `x11` (External). `ctx.displayCapture(name)` saves the X display with the video; CDP screenshots hold only the page. *Proven by `playback` on both backends.*
- **Seek.** Press → (`ArrowRight`, 30 s) one at a time and wait until both `ctx.player().positionMs` and the bar's slider (`[role="slider"][aria-label="Playback position"]` `aria-valuenow`) moved before the next press; mpv's `time-pos` then matches within 3 s. *Proven by `playback` (15–18 presses to 8 % of a 125-minute movie, none lost).*
- **Pause.** `k` pauses: `paused` in the app and `pause` in mpv turn true, the bar shows `Resume`, and a progress report goes out at once. `k` again resumes. *Proven by `playback`.*
- **Stop.** Reveal the bar (`ctx.revealPlayer()` on Built-in), then `More playback controls` → `Stop playback` (the bar's own `Stop` only shows from 1536 px; the default window is 1280). `app.log` has `reporting active playback stopped failed=false reason="stop"`, `ctx.playstates()` holds a `Sessions/Playing/Stopped` report answered 204, and Jellyfin's `UserData.PlaybackPositionTicks` equals the reported position within 2 s. The detail page then offers `Resume` and `From start`. A second stop uses `q` on Built-in and the bar on External. *Proven by `playback`.*
- **Resume.** `Resume` launches at the stored point (`starting playback … start=608.983s`), `app.log` runs `loaded Jellyfin stream in mpv` → `activating pending playback` → `sending delayed mpv startup seek` → `mpv startup seek reached resume range`, the `loadfile` URL carries no `start` option or `#t=`, no `set_property pause=` is sent during startup, the `Sessions/Playing` report holds the resume point, and no report afterwards goes below it. `mpv IPC connected` appears once for the whole run. *Proven by `playback` on both backends.*
- **Watched and unwatched.** `Mark watched`, then `Mark unwatched`, leave the title unplayed without a resume point (Jellyfin's play count and last-played date stay). *Proven by `playback`.*
- **Segments.** Not verified: the test server has no media segments (`GET /MediaSegments/<id>` was empty for 96 sampled series and 400 episodes). It needs a segment provider such as the Intro Skipper plugin.
- **Next episode, `w`, audio and subtitle menus.** Source-only.
- **Proof.** `playback.json` (movie, backend, mpv `vo`/`hwdec`, quick and spaced seeks, stop, resume, cleanup, every playstate report), `02-playing.png`, `03-paused-display.png` and `05-resumed.png` (display captures), `03-paused-bar.png`/`.ax.txt`, and `app.log`.

## Gotchas

- Playback writes the test user's playstate and plays real audio. On Linux the private runtime dir cuts the session off the user's audio server; on Windows it is audible.
- Jellyfin stores no resume point below 5 % of the runtime (or above 90 %), so stop well past 5 % before expecting `Resume`.
- Built-in on Linux: the player bar hides after 3 s and turns `inert`; ARIA lookups fail until a real pointer move near the bottom edge (`ctx.revealPlayer()`). Keys still reach the app's handler.
- External mpv fullscreen covers the app window: the page reports `visibilityState` `hidden`, draws no frames, and CDP mouse events then never complete (a `press` or `hover` hangs until the drive times out). Seed `default_fullscreen: windowed` for External mpv runs.
- The app ignores key auto-repeat. Quick repeated presses of the seek keys lose increments: the handler seeks from the bar's rendered position, which lags behind (six quick → presses moved 122–152 s on Built-in and 33–63 s on External instead of 180 s). `playback` records this as `rapidSeek` and seeks one press at a time.
- `stopReason` in `/api/player/state` stayed `null` after a stop; use the `app.log` line and the Stopped report.
- A drive that fails mid-playback leaves the stop to teardown, which still reports the position; `restoreItemAfterRun` resets the title afterwards.
