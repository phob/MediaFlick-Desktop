---
name: verify-mediaflick
description: Drive the real MediaFlick Desktop app (staged CEF build, build/mediaflick-desktop) the way a user does and capture proof. Runs the app on an inactive private desktop (Windows) or a private Xvfb display (Linux) with a disposable profile, and operates the embedded React UI over CDP with ARIA lookups and real mouse/keyboard input. Use to prove a UI, settings, sign-in, library, detail-page, playback or Companion-feature change works in the shipped binary, against Jellyfin's demo server or your own server and account, or to reproduce a UI bug. Not for the Companion plugin's own code or packaging.
---

# Verify MediaFlick

MediaFlick Desktop is a Rust/CEF shell around an embedded React UI. `just verify <drive>` builds and stages the app, starts it out of the user's sight with a throwaway profile, runs your **drive** (a Node module) against it over the Chrome DevTools Protocol, saves evidence, and shuts it down.

How it works:

- `just verify` runs the per-OS **isolation wrapper**, which runs `node scripts/session.mjs` inside the isolation. Everything the session starts inherits it.
  - Windows (`scripts/isolate-windows.ps1`): a new desktop `MediaFlick.Verify.<pid>.<tick>`, never switched to, and a kill-on-close job.
  - Linux (`scripts/isolate-linux.sh`): a private Xvfb display chosen with `-displayfd`. `WAYLAND_DISPLAY` is unset, `XDG_RUNTIME_DIR` is private, the session bus is private or absent, and the run gets its own session and process group.
- `session.mjs` refuses to launch anything until `platform.mjs` has confirmed that isolation from inside it. On Windows a child process reports its own desktop, which must differ from the visible input desktop. On Linux, `DISPLAY` must belong to the wrapper's Xvfb, with no Wayland and the private runtime dir.
- The disposable profile is `<os tmp>/mediaflick-verify-<run-id>-<session-pid>-<random>`. On Windows the app gets `APPDATA`, `LOCALAPPDATA` and `TEMP` inside it. On Linux it gets `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_STATE_HOME` inside it, and the wrapper's private runtime dir as `TMPDIR`. Chromium's singleton socket and mpv's IPC socket live in `TMPDIR`, and a Unix socket path may not exceed 107 bytes; inside the profile it would. The app resolves its directories from exactly these variables (`src/app/paths.rs`). Its single-instance gate is keyed by `instance.json` in that config dir, so a verify run never collides with the user's own MediaFlick, which can keep running.
- The user's real `accounts.json`, `settings.json`, `collections.json`, `playback-preferences.json`, `pending-deletions.json`, `instance.json` and `library.db` are never opened for writing. The session fingerprints their size and mtime before and after (`guard.json`) and fails the run if they changed while no user instance was running.

Drive scripts, the harness and the session are platform-neutral Node. Only the two wrappers and `scripts/platform.mjs` (isolation check, process listing, tree kill, app environment) are per-OS.

## Launch

Prerequisites: Node 22+ (global `WebSocket`) and `just`. Windows also needs PowerShell 7 from the MSI, not the Store build. Linux also needs `Xvfb`, `setsid` and `timeout`, and optionally `dbus-run-session` and `xwininfo` (for `ctx.windowShown()`).

```sh
just verify-doctor                                   # read-only preflight
just verify application-settings                     # build, stage, run one drive
just verify browse --run-id browse-1 --timeout 300   # explicit run id and drive timeout
just verify path/to/my-drive.mjs --url https://demo.jellyfin.org/stable
```

Arguments after the drive go to `session.mjs`:

- `--run-id ID`: letters, digits, `.`, `_` and `-`. It must be new. The default is `<drive>-<timestamp>`.
- `--timeout SECONDS`: drive timeout, default 300.
- `--url SERVER`: prefills the sign-in screen through the app's `--url`.
- `--account SERVER`: starts the app signed in to your own server with the session `just verify-login` cached. See [Your own server](#your-own-server). Not combinable with `--url`.
- `--signed-out` (with `--account`): starts on that server's sign-in screen instead; `ctx.signIn({})` signs in with Quick Connect, approved by the harness as the cached account, so the app gets its own token and may sign out.
- `--log-level FILTER`: the app's `--log-level`, an `EnvFilter`, default `debug`. Playback drives use `debug,jellyfin.playstate=trace,playback=trace` for the playstate reports.
- `--seed DIR`: copied into the empty config dir before launch, for example a prepared `settings.json`.
- `--keep-profile`: leaves the profile on disk for inspection. Remove it later with `just verify-cleanup`.
- `--services-down`: the in-session doctor expects `/api/status` to answer 503 instead of 200, for a drive whose seed makes the app's services fail to open.

`MEDIAFLICK_VERIFY_HARD_TIMEOUT` (seconds, default 900) bounds the whole wrapper run.

`MEDIAFLICK_VERIFY_SCREEN` (Linux, `WIDTHxHEIGHTxDEPTH`, default `1920x1080x24`) sizes the private Xvfb screen, such as `3840x2160x24` for a 4K screen at 100% scaling. Each side must be 100 to 8192 pixels and the depth 8, 16 or 24. The app window still opens at 1280x720; a drive that needs the whole screen sizes it with `xdotool`, as `reader-mouse` does.

A drive is a bare name from `drives/` or a path. Ready means `steps.log` shows `isolation ok: …`, then `launched pid … on CDP port …`, then `doctor ok: …`. Startup takes about 2 s. Teardown is automatic: the session calls `window.close()` in the page, which takes the app's own close path including save-on-exit. It waits up to 20 s and records `exit: coordinated`. Otherwise it stops exactly its PID tree and records `exit: forced`.

Run one `just verify` at a time per checkout. Each starts with `just build`, which copies every staged file into `build/` with `cp`, even when nothing changed. That rewrites the executable and `libcef.so` under a run that is still using them: the other run fails with `spawn ETXTBSY` or its app dies silently and the drive hangs until its timeout (seen 2026-10-12). Runs in other worktrees have their own `build/` and are safe.

## Your own server

The demo server only proves read-only behavior. To verify against a real server and account without anyone handing over a password, approve a Quick Connect sign-in once and let runs reuse the session:

```sh
just verify-login https://jellyfin.example.com             # once; prints a code, waits up to 10 min
just verify home-library --account https://jellyfin.example.com
```

1. `verify-login` runs the `login` drive. It opens the sign-in screen on that server, presses `Use Quick Connect`, and writes `QUICK CONNECT CODE <code> for <server>` to `steps.log` and stdout, with `quick-connect-code.png`. The user approves the code in Jellyfin (user menu → Quick Connect) while signed in as the account to use. Jellyfin expires a code after 10 minutes. When an agent runs it, run it in the background, watch `steps.log` for that line, and send the user the code with a push notification.
2. Once the app is signed in, `ctx.saveAccount()` reads the session the app stored in the profile's `library.db` (server URL and ID, user ID and name, device ID, token) and caches it in `~/.config/mediaflick-verify/accounts/<server>.json`, or `%APPDATA%\mediaflick-verify\accounts\` on Windows. The file is 0600 in a 0700 directory on Unix, outside the repo, `build/` and the real MediaFlick profile. A new login for the same server replaces it.
3. `--account SERVER` first asks the server `GET /Users/Me` with only the cached token. Jellyfin then takes the client and device from the token's own device record, so the check changes nothing. A 401 fails the run before launch and names the `verify-login` command to run. Then the session writes a `library.db` holding only that session at schema version 1. The app's pre-1.0 recreation path (`migrate` in `src/library/db.rs`) builds the current schema and keeps the session, as an upgrade does, so the app starts signed in. `ctx.signIn()` then only confirms the account and ignores its arguments, so demo drives run unchanged unless they assert demo data.

4. `--account SERVER --signed-out` seeds nothing and prefills the server. `ctx.signIn({})` presses `Use Quick Connect` and approves the code itself with `POST /QuickConnect/Authorize` as the cached account. The app then holds a token of its own on a new device, which a drive may sign out or revoke (`ctx.appSession()`); signing out removes that device again. The sign-out block below does not apply. `account-session` uses this.

While a drive runs, `ctx.jellyfin(path)` reads the server as the cached account (GET only), for example `/Items/<id>?userId=${ctx.account.userId}` to read back what the app reported. A drive that changes server state puts it back through the user path, and also registers an after-run task that runs once the app has exited, so a failure halfway still cleans up: `ctx.restoreItemAfterRun(itemId)` resets a played title or resume point, also one `ctx.setPlayedOnServer` set in Jellyfin directly (`POST /UserItems/<id>/UserData`; play count and last-played date stay), `await ctx.unfavoriteAfterRun(itemId)` removes a favorite the drive is about to add if it is still one (`DELETE /UserFavoriteItems/<id>`, the route the app uses), and `ctx.cancelSeerrRequestAfterRun(id)` cancels a request still in Seerr (`DELETE /MediaFlick/seerr/request/<id>`, the app's own cancel). A failed after-run task fails the run, and so does a failed teardown, which never skips the after-run tasks.

Favorites also survive a killed run (Ctrl+C, a closed terminal, a crash). `unfavoriteAfterRun` reads `IsFavorite` first and throws if the item already is a favorite, so a drive never takes a favorite that was set before the run. It then records `{ server, userId, itemId }` (never the token) in `build/verify/pending-favorites.json`, written atomically, before the drive writes the favorite. An entry goes only once its cleanup removed the favorite, the server shows it is not one, or the server answers 404 for the item (a rename or an upgraded file can change its id; logged as `favorites journal: <id> no longer exists; dropped its entry`). Every `--account` run first reconciles the entries for its server and user: it removes each favorite the server still shows as set and logs `favorites journal: removed leftover test favorite <id>, which the server showed as set`; one it cannot remove fails the run before launch. Gaps remain. Cleanup removes the favorite whoever set it, so a favorite someone sets by hand on that item while a run holds it is lost. After a killed run, the leftover entry is reconciled only at the next `--account` run in this checkout, whenever that is, and that run removes the favorite even if someone favorited the item by hand in between. The journal lives in this checkout's `build/`, so `just clean` or removing the worktree after a killed run loses the entry and leaves the test favorite on the server; check the test account's favorites by hand then. Another worktree never sees the journal. Keep `--account` runs in one checkout one at a time, since a starting run reconciles another live run's entries. `restoreItemAfterRun` and `cancelSeerrRequestAfterRun` have no journal, so a killed run can leave a played title or a Seerr request. Cleanup uses Jellyfin 10.9+ routes (`/UserFavoriteItems`, `/UserItems`); proven on Jellyfin 12.2.

Safeguards in both modes:

- **Sign out is blocked.** The app's Sign out revokes the token on the server (`POST /Sessions/Logout`), which would kill the cached session. CDP's `Fetch` domain does not see the app's `mediaflick-desktop://` requests, so the session wraps the page's `fetch` to refuse `/api/auth/logout` and report it through a CDP binding. The drive then fails at once with `the drive tried to sign out of a cached Jellyfin account`. `browse`, `sign-in`, `library-filters` and `bootstrap-retry` sign out and cannot run with `--account`.
- **The token never reaches evidence.** After every run the session scans every evidence file for it, replaces any occurrence with `<redacted>`, and fails the run naming the files (`tokenScan` in `result.json`). The app's own log already redacts tokens. `--keep-profile` leaves the profile's `library.db`, token included, in the temp dir until `just verify-cleanup`.
- An agent running the session can still read the cache file. It holds a revocable token, not a password: revoke it under Jellyfin Dashboard → Devices (the `MediaFlick Desktop` device whose ID is in `account.json`), or delete the file.

Use a dedicated non-admin Jellyfin user with access to a small library. Drives that write (favorites, watched state, synced settings) change that user's real data.

Proven on Linux. Against the demo server, with a test-only script approving the code as `demo`: login and cache, `home-library` on the seeded session, a Sign out blocked with the token still valid afterwards, a token planted in evidence redacted and the run failed, and a revoked token rejected before launch. Against a self-hosted Jellyfin 12.2.0 with Companion 0.2.2 (2026-10-12): a session approved by the user and cached, then `account-session`, `playback` (Built-in and External mpv), `companion`, `discover-requests`, `collections` and `account-settings`, each passing with `token scan ok`. On Windows 11 against the same server (2026-10-13): `login`, then `account-session`, `playback` (Built-in, and External mpv with an SVP mpv bundle at `C:\mpv\mpv.exe`), `companion`, `discover-requests`, `collections` and `account-settings`, each passing with `token scan ok`.


## Doctor

Run this first, and whenever anything looks off:

```sh
just verify-doctor
```

It changes nothing. It reports:

- the Node version;
- whether the staged app exists and whether `src/`, `ui/src/`, `Cargo.toml`, `build.rs` or `ui/package.json` are newer than it (`just verify` rebuilds anyway);
- the per-OS prerequisites: on Windows, the MSI PowerShell 7 at `%ProgramFiles%\PowerShell\7\pwsh.exe`, which the justfile's shell, the wrapper and `platform.mjs` all use, because a Store (MSIX) `pwsh` that comes first on `PATH` runs its children outside the job;
- loopback health: the slowest of three connects to a throwaway `127.0.0.1` listener. Above 1 s it is a problem, because CDP then crawls or times out;
- every verify profile in the temp dir, as live (its `session.mjs` PID is running, in any worktree) or LEFTOVER, with the processes still using it;
- whether your own MediaFlick is running (never touched);
- the real profile path the guard watches;
- the sessions cached by `just verify-login` (server, user, when), never their tokens;
- the last run's result, or `INTERRUPTED` when its evidence folder has no `result.json` (it was killed by the hard timeout or a closed terminal).

Exit code 1 lists the problems.

Each session also runs an in-session doctor and writes `doctor.json`. It requires:

- the CDP page at `mediaflick-desktop://app`;
- `/api/status` answering 200;
- `instance.json` created in the disposable config dir, which proves the app really resolved its profile there;
- our PID alive as an app process.

## Drive

A drive is an ES module whose default export receives `ctx`:

```js
// build/verify-drives/scrollbars.mjs  (build/ is git-ignored; put reusable drives in drives/)
export default async function (ctx) {
  await ctx.signIn({ server: "https://demo.jellyfin.org/stable", username: "demo" })
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "Application" })
  await ctx.press({ role: "switch", name: "Show scrollbars" })
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => ctx.readConfig("settings.json")?.show_scrollbars === true, "settings.json to record the switch")
  await ctx.snapshot("saved")
  await ctx.screenshot("saved")
}
```

Run it with `just verify build/verify-drives/scrollbars.mjs`. Any thrown error fails the run and captures `failure.png` and `failure.ax.txt`.

The `ctx` helpers come from `scripts/harness.mjs`. Targets are `{ role, name }`, matched through CDP `Accessibility.queryAXTree` (exact accessible name, rendered elements only). Add `within: { role, name }` to match only inside a container, such as `{ role: "link", name: "Collections", within: { role: "navigation", name: "Settings navigation" } }` when the sidebar has a link of the same name. `{ css }` is the fallback for controls with no stable name.

| Helper | Use |
|---|---|
| `find(target, { timeout, nth })` | Wait for exactly one rendered match; more than one is an error unless `nth` is given. Returns `{ backendNodeId, properties, rect }`. |
| `press(target)` | Scroll into view, wait until the target holds still, hit-test, click the centre with the real mouse. A toast over it is waited out (up to about 9 s). Any other cover first gets about 0.3 s to settle; then the target is scrolled to the middle of the viewport (a sticky bar such as the settings save bar can cover the viewport's edge), and after about 0.3 s more the pointer is parked and it is tested again. Fails if still covered. If the click closes the app, `press` returns once the app has exited. Fails on disabled elements. |
| `fill(target, text)` | Click into a text field, select all, type `text` (`Input.insertText`). |
| `choose({ name }, option)` | Open a Radix Select (role `combobox`, named by its `label`) and pick the option by name. |
| `hover(target)` | Park the pointer, then move onto the target (fires `pointerenter`; the sidebar expands on it). |
| `park()` | Move the pointer to the top-right corner, collapsing hover-opened UI. |
| `key(name, { code, keyCode, modifiers })` | One key press. |
| `state(target, { timeout, nth })` | Wait for exactly one rendered match, as `find` does, then return its AX state: `checked`, `pressed`, `expanded`, `disabled`, `selected`, `value` (tristate tokens become booleans). |
| `exists(target)`, `count(target)` | Rendered matches right now. |
| `until(exprOrFn, description, timeout)` | Poll a page expression or Node function until truthy; logs `saw <description>`. Never use fixed sleeps for state. |
| `check(condition, message)` | Fail, or log `ok: message`. |
| `signIn({ server, username, password })` | Drive the sign-in form and wait for the sidebar. Under `--account`, only confirm that the app started signed in as the cached user; under `--account … --signed-out`, sign in with Quick Connect approved by the harness. |
| `quickConnect({ timeout, approve })` | From the sign-in screen, press `Use Quick Connect`, log the code (`QUICK CONNECT CODE …`) and wait up to `timeout` ms (default 600000) for someone to approve it. `approve: true` (only with `--signed-out`) approves it as the cached account. |
| `saveAccount()` | Under `just verify-login` only: cache the signed-in session for `--account` runs and write `account.json`. |
| `account` | `{ server, userName, userId, login: false, signedOut }` under `--account`, `{ server, login: true }` under `verify-login`, else `null`. Never holds the token. |
| `jellyfin(path)` | GET on the server to read back server state (`/Items/<id>?userId=${ctx.account.userId}`): as the cached account under `--account`, otherwise as the session the app stored when the drive signed in (the demo account, read-only). |
| `appSession()` | `--signed-out` only: the token the app holds now, as `{ status(), revoke() }`: the HTTP status of `GET /Users/Me` with it, and a sign-out from outside the app. |
| `restoreItemAfterRun(id)`, `unfavoriteAfterRun(id)`, `cancelSeerrRequestAfterRun(id)` | `--account` only: after-run cleanup once the app has exited (see [Your own server](#your-own-server)). `unfavoriteAfterRun` is async: await it before the favorite write. |
| `createPlaylistAfterRun(name, itemIds)` | `--account` only: creates a private video playlist of the cached account (`POST /Playlists`) and deletes it after the run unless it is already gone. Returns `{ id, items(), add(ids), move(playlistItemId, index), remove(), exists() }`, which read and change it as another device would. No journal: a killed run can leave the playlist. |
| `setPlayedOnServer(id, played)` | `--account` only, after `restoreItemAfterRun(id)`: marks the item played or unplayed in Jellyfin itself (`POST /UserItems/<id>/UserData`), as another client would, so the app learns of it only through the server's push. Throws on its first call for an item that is already played or has a resume point, since the restore puts it back to unplayed. |
| `setResumeOnServer(id, ticks)` | `--account` only, after `restoreItemAfterRun(id)`: sets the item's resume point in Jellyfin itself (`POST /UserItems/<id>/UserData` with `PlaybackPositionTicks`), as another client would. The same first-call rule as `setPlayedOnServer`. |
| `revokeAppSessionAfterRun()` | `--signed-out` only, after sign-in: once the app has exited, sign the app's own token out on the cached account's server unless it already is (a drive that signs in to a proxy cannot sign out through it after the run). |
| `player()` | `GET /api/player/state`: `active`, `itemId`, `playMethod`, `positionMs`, `durationMs`, `paused`, `tracks`, `skipSegments`, `diagnostics`. |
| `mpv(property)` | Reads one mpv property (`time-pos`, `pause`, `current-vo`, `hwdec-current`) over mpv's IPC socket, found through `app.log`'s `mpv IPC connected ipc_path=`, on one connection kept for the drive. On Windows, when a script has moved mpv's IPC server (SVP's `mpvSockets.lua` re-points it to `<TEMP>\mpvSockets\<pid>`), it follows the move and logs `mpv IPC moved by a script`. Reads only: the app stays mpv's single command writer. |
| `playstates()` | The playstate reports in `app.log`: `{ queued: [{ at, endpoint, itemId, positionS }], sent: [{ at, endpoint, itemId, status }] }`. Needs `--log-level "debug,jellyfin.playstate=trace"`. |
| `revealPlayer()` | Moves the pointer near the bottom edge so the Built-in player's hidden, `inert` bar comes back for ARIA lookups. |
| `displayCapture(name, { seconds })` | Linux only: the whole X display with ffmpeg `x11grab` as `<name>.png`, or an MP4 clip with `seconds`. It contains the mpv video, which CDP screenshots (the page alone) do not. |
| `pathname()` | `location.pathname + location.search`. |
| `api(path)` | GET the app's own `/api/*` as the UI does, to read results. Never the action under test. |
| `proxy({ upstream, fail, answer, delay })` | Start a loopback Jellyfin proxy on `127.0.0.1` that forwards to `upstream` (path prefix included), streaming bodies and ranges and passing WebSocket upgrades (the app's `/socket`) through. It drops the connection of each request `fail(url, method)` selects, answers a request itself when `answer(url, method)` returns `{ status, body }` (JSON), and holds a request for the milliseconds `delay(url, method)` returns before forwarding it (`stats.delayed`). Either hook may return a Promise instead, which holds the request on a gate the drive opens: `answer` then replies with the `{ status, body }` it resolves to, or forwards on null, and `delay` forwards once it resolves. Each request held on a Promise is recorded in `stats.held` (`{ at, hook, method, path, search, releasedAt }`) when it arrives, so a drive can wait until the app is blocked upstream; `mediaSegmentsAnswer({ [itemId]: [{ type, startS, endS }] })` from `scripts/proxy.mjs` serves `GET /MediaSegments/<id>` in Jellyfin's format. Returns `{ url, stats, heal(), resumeFailing(), cutSockets(), restoreSockets() }` (`stats.answered` lists the answered requests). `cutSockets()` drops every open WebSocket and refuses new upgrades (`stats.refusedUpgrades`) until `restoreSockets()`. Sign in to `url`. Closed after the drive. |
| `remoteCommand(command, query)` | `--account` only: remote control as another client of the cached account, aimed at the app's own Jellyfin session: `POST /Sessions/<its id>/<command>?<query>`, such as `remoteCommand("Playing", { playCommand: "PlayNow", itemIds: "a,b", startIndex: 1 })` or `remoteCommand("Playing/NextTrack")`. Returns the target session as Jellyfin listed it (its `PlayableMediaTypes` included). |
| `audioSink()` | Linux only: starts a private PipeWire with one null sink (`mediaflick-verify-sink`) and a WirePlumber that watches no ALSA, Bluetooth or camera device, both in the wrapper's private runtime dir, so mpv has an audio device under Xvfb. Without it an audio-only file fails to load (`Could not open/initialize audio device -> no sound`, then `Errors when loading file`); a video plays silently. Start it before the first Play: mpv opens its audio output per file. Returns `{ sink, capture(name, seconds) }`; `capture` records the sink into `<name>.wav` and returns its `meanDb` and `maxDb` from ffmpeg's `volumedetect` (silence reads -91 dB). Stopped after the drive. |
| `relaunch()` | Close the app through its own close path and start it again on the same profile, as a user quitting and reopening. Writes `doctor-relaunch.json`. |
| `windowShown()` | Whether the native main window is shown: `xwininfo` map state on Linux, `MainWindowHandle` on Windows. |
| `readConfig(name)`, `configStat(name)`, `configFile(name)` | Files in the disposable config dir (`settings.json`, `accounts.json`, …). |
| `appLog()` | The app's Rust log so far, as text (`app.log` in the evidence). |
| `wmClose({ watchMs })` | Linux only. Sends the app's top-level window the `WM_PROTOCOLS`/`WM_DELETE_WINDOW` message a window manager sends for the close button, then listens `watchMs` (default 2000) for the window's `WM_CHANGE_STATE` IconicState request. Returns `{ window, iconifyRequested }`. Under Xvfb no window manager acts on that request, so `iconifyRequested` is the proof of a minimize. |
| `appExit(timeout)` | Waits for the app process to exit. Returns `{ code, signal }`, or `null` if it is still running after `timeout` ms. |
| `screenshot(name)`, `snapshot(name)` | `<name>.png` (after a 300 ms settle) and `<name>.ax.txt`: one line per node, `role 'name' [state]`. |
| `writeEvidence(name, value)` | Any extra artifact into the evidence folder. |
| `step(text)` | A line in `steps.log`. |
| `app` | The raw CDP client from `scripts/website/cdp.mjs` (`send`, `evaluate`, `waitFor`). |

To find handles for a page you have not driven yet, take a `snapshot` there and read the `.ax.txt`. Read the feature map in `features/` before writing a drive. It lists entry points, handles marked *proven* or source-only, and traps.

The shipped drives, with what each proves, the flags it needs and the evidence it writes, are listed in [drives/README.md](drives/README.md). Read it before you pick or write a drive.

Re-run all of them:

```powershell
foreach ($d in 'smoke', 'application-settings', 'browse', 'home-library', 'detail-crew', 'item-detail', 'client-settings', 'labels', 'bootstrap-retry', 'sidebar-user-menu', 'sidebar-stable-icons', 'library-filters', 'library-parity', 'player-shelf', 'home-settings', 'home-library-shelves', 'punctuation-search', 'top-level-titles', 'person-discover', 'libraries-api', 'generic-detail', 'live-libraries', 'library-browse', 'library-pending') { just verify $d --run-id "all-$d-$(Get-Date -Format HHmmss)" }
just verify sign-in --run-id "all-sign-in-$(Get-Date -Format HHmmss)" --url https://demo.jellyfin.org/stable
node .claude/skills/verify-mediaflick/scripts/startup-destination-seed.mjs https://demo.jellyfin.org/stable build/verify-seeds/startup-demo
foreach ($c in 'movies', 'library', 'missing') { just verify startup-library --run-id "all-startup-$c-$(Get-Date -Format HHmmss)" --seed "build/verify-seeds/startup-demo/$c" }
```

```sh
for d in smoke application-settings browse home-library detail-crew item-detail client-settings labels close-minimize close-guard bootstrap-retry sidebar-user-menu sidebar-stable-icons library-filters library-parity player-shelf home-settings home-library-shelves punctuation-search top-level-titles person-discover libraries-api generic-detail live-libraries library-browse library-pending; do just verify "$d" --run-id "all-$d-$(date +%H%M%S)"; done
just verify sign-in --run-id "all-sign-in-$(date +%H%M%S)" --url https://demo.jellyfin.org/stable
node .claude/skills/verify-mediaflick/scripts/startup-destination-seed.mjs https://demo.jellyfin.org/stable build/verify-seeds/startup-demo
for c in movies library missing; do just verify startup-library --run-id "all-startup-$c-$(date +%H%M%S)" --seed "build/verify-seeds/startup-demo/$c"; done
```

Your own server (one at a time; they share the cached session):

```sh
S=https://jellyfin.example.com
just verify account-session --account "$S" --signed-out
for d in companion discover-requests collections account-settings top-level-titles libraries-api generic-detail live-libraries library-browse home-library-shelves library-studio-tag; do just verify "$d" --account "$S"; done
node .claude/skills/verify-mediaflick/scripts/legacy-browsing-seed.mjs "$S" build/verify-seeds/legacy-browsing
just verify library-browse --account "$S" --seed build/verify-seeds/legacy-browsing
node .claude/skills/verify-mediaflick/scripts/startup-destination-seed.mjs "$S" build/verify-seeds/startup-account
for c in movies library missing; do just verify startup-library --account "$S" --seed "build/verify-seeds/startup-account/$c"; done
just verify live-changes --account "$S" --timeout 600
just verify books --account "$S" --timeout 1500
just verify catalog-refresh --account "$S" --signed-out
just verify live-changes --account "$S" --signed-out --timeout 900
just verify libraries-changed --account "$S" --timeout 600
just verify playback --account "$S" --log-level "debug,jellyfin.playstate=trace,playback=trace"
just verify playback --account "$S" --log-level "debug,jellyfin.playstate=trace,playback=trace" --seed .claude/skills/verify-mediaflick/seeds/external-mpv
just verify segments --account "$S" --signed-out --timeout 600 --log-level "debug,mpv.ipc=trace"
just verify segments --account "$S" --signed-out --timeout 600 --log-level "debug,mpv.ipc=trace" --seed .claude/skills/verify-mediaflick/seeds/external-mpv
```

## Evidence

Each run writes **`build/verify/<run-id>/`**, which cleanup never removes (`just clean` does):

- `steps.log`: a timestamped line per action (`press …`, `fill …`, `hover …`), wait (`saw …`) and check (`ok: …`). This is the action record.
- `doctor.json`: the isolation details (desktop or display), PID, exe and its mtime, git HEAD, Chromium version, `/api/status`, and profile paths and files.
- `result.json`: `passed`, `error` (stack), `exit` (`coordinated`, `forced`, or `exited-early` when the app was gone before teardown, including on purpose after a drive closed it), `profileRemoved`, `durationMs`, and `leftovers` if any process outlived teardown.
- `guard.json`: the real-profile fingerprints before and after, the changed files, and any user MediaFlick PIDs.
- `pids.json`: the session PID, app PID, CDP port and profile path.
- `account.json` under `--account` or `verify-login`: the cached account without its token, and the `/Users/Me` check. `result.json` then also has `tokenScan`.
- `*.png`, `*.ax.txt`, plus `failure.png` and `failure.ax.txt` when the drive throws.
- `app.log` (Rust log at `debug`), `cef.log`, and `profile/config/`: a copy of everything the app wrote to its config dir.

Proof standards:

- Drive the user path: sidebar links, settings navigation, buttons, switches, cards. Do not perform the action through `/api/*`, `ctx.app.route()`, or `Runtime.evaluate` clicks.
- Capture the action and the resulting state. `steps.log` holds the actions; snapshot or screenshot after each meaningful state, not only at the end.
- Verify side effects alongside the screen. Files in the config dir change only on Save (compare `configStat` mtimes for drafts), and `/api/*` reads back the stored value. Then visit the page a second time.
- The demo server is real and shared. Read-only checks there are genuine end-to-end proof; anything that writes needs a server you own (`--account`).
- If a mapped entry point cannot be driven, report it as not verified with the reason. Do not substitute a different path.

## Cleanup

Normal runs clean up themselves:

1. Coordinated exit through `window.close()`.
2. Otherwise a forced stop of the app's PID tree (`taskkill /T` on Windows, a parent-PID walk on Linux).
3. A sweep that stops any remaining process whose command line names this run's profile. CEF helpers carry `--user-data-dir=<profile>…`, and the random profile path is unique to the run.
4. The profile directory is removed.

Then the Windows wrapper terminates its job and closes the desktop. The Linux wrapper stops its process group and its Xvfb, which also drops any X client, and removes its work dir `<os tmp>/mediaflick-xvfb.*` (Xvfb's display pipe and the private runtime dir). That name deliberately lacks the `mediaflick-verify-` profile prefix, so doctor and cleanup never mistake a live run's work dir for a leftover profile.

The hard timeout is proven. A run whose app stalled was terminated with its job at `MEDIAFLICK_VERIFY_HARD_TIMEOUT`; no process survived, and the profile was left for cleanup. After an interrupted run (killed terminal, hard timeout), `just verify-doctor` lists LEFTOVER profiles and the run as `INTERRUPTED`. Then:

```sh
just verify-cleanup
```

It stops only processes whose command line names a dead run's profile, then removes that profile. It skips runs whose session is still alive, including other worktrees' and other agents' runs, and it never touches `build/verify/`. Never stop `mediaflick-desktop` by name: the user's own session and other runs use the same executable.

## Platform status

- **Windows: proven.** Every drive shipped by 2026-10-13 except the two Linux-only close drives passes on Windows 11 with the inactive desktop, about 10–30 s each including the incremental build (`collections` about 2 min, most of it the first sync and franchise matching). CEF renders there, `requestAnimationFrame` runs, `document.visibilityState` is `visible`, and `Page.captureScreenshot` returns real frames. `window.close()` exits cleanly, `guard.json` shows the real profile untouched, and no process or profile survives. The Built-in player draws its bar over the video as on Linux (`integratedLibmpvOverlay`), starts fullscreen on the private desktop, and `ctx.mpv` reads it over its named pipe (`\\.\pipe\mpvpipe` with the SVP libmpv profile these runs used). External mpv plays in its own window on the private desktop, windowed at 1280 px, so `Stop` sits in the bar's overflow menu there.
- **Linux: proven.** Every shipped drive passes on Ubuntu 26.04 (all but `login` re-run on 2026-10-14, the real-server drives against Jellyfin 12.2.0 with Companion 0.2.2) under Xvfb with the private session bus, about 2–40 s each after the build (the real-server drives 20–120 s, most of it the first sync and playback). CEF renders, `Page.captureScreenshot` returns real frames, `window.close()` exits cleanly (`exit: coordinated`), `guard.json` shows the real profile untouched, and no process, profile or `mediaflick-xvfb.*` dir survives. The app's Linux window is composited through libmpv (`libmpv.so.2`, the default Built-in player): `app.log` shows `mpv IPC connected` and `received first Linux CEF frame` on a healthy start.

  Not yet exercised on Linux: the hard timeout, and a run without `dbus-run-session`.
- Headless Wayland is not implemented; the wrapper forces X11 (`WAYLAND_DISPLAY` unset, `XDG_SESSION_TYPE=x11`).

## Gotchas

- **Playback is real.** Play only in `playback`-style drives with `--account` on your own server: it writes the test user's playstate, and on Windows it is audible, because the private desktop hides windows and input, not sound; `playback` mutes with the player's `m` key about 2 s after the start. On Linux the private `XDG_RUNTIME_DIR` cuts off the user's audio server (`Failed to connect to PipeWire`), so video plays silently and audio-only files need `ctx.audioSink()`. Never press Play on the demo server.
- **External mpv fullscreen freezes CDP mouse input.** mpv's fullscreen window covers the app window; the page turns `hidden`, draws no frames, and `Input.dispatchMouseEvent` never completes, so `press` and `hover` hang until the drive times out (keys still work). Seed `default_fullscreen: "windowed"` for External mpv runs, as `seeds/external-mpv` does.
- **Linux stderr noise is expected.** The private session bus activates `xdg-desktop-portal`, which logs `Choosing gtk.portal … as a last-resort fallback` and `Failed to connect to PipeWire`; `A connection to the bus can't be made` follows teardown. None of it affects the run. `steps.log` and `result.json` are the result.
- **Linux sockets need a short `TMPDIR`.** CEF dies with `SIGTRAP` (`Socket path too long` in `cef.log`) when its singleton socket path passes 107 bytes, and the libmpv shell falls back after 20 s with `path must be shorter than SUN_LEN` in `app.log` when mpv's IPC socket does. `session.mjs` refuses to launch when the wrapper's runtime dir is too long for them; run from a shorter `TMPDIR`.
- **A drive that ends with an unsaved settings draft exits forced.** The draft guard answers teardown's `window.close()` with its `Leave without saving?` dialog, so teardown stops the app after 20 s (`exit: forced`). Save or Discard before the drive returns.
- **A drive that saves `Minimize window` exits forced with External mpv.** There, teardown's `window.close()` takes the window's close policy and minimizes. Save `Exit MediaFlick` before the drive returns, as `close-minimize` does.
- **`wmClose` needs its sender.** The Linux `just verify` recipe builds `examples/verify-wm-close.rs` into `$CARGO_TARGET_DIR/debug/examples/`; running `session.mjs` some other way leaves it missing. It needs exactly one viewable top-level window of class `io.github.phob.MediaFlickDesktop`.
- **External links reach the visible desktop.** `More info` menu items, `Installation help` and the update banner's release page open the user's default browser. Never press them.
- **Never press the update banner's install action** (it downloads and runs an installer), `Install mpv`, or `Choose mpv executable` (a native file dialog nobody can answer on the private desktop).
- The sidebar expands under the pointer and overlays content off Home. `press` waits for it to settle and parks the pointer when a target stays covered. To use the sidebar's search off Home, `hover({ css: '[data-sidebar="sidebar"]' })` first.
- Sidebar search commits 200 ms after typing with a replacing navigation. Wait for `/library?search=` before pressing a result.
- `role="status"` regions have no accessible name; read their text with `ctx.app.evaluate`.
- After sign-out `document.title` keeps the last page's title. Identify the sign-in screen by its `Server` textbox.
- Signed out, the sign-in screen's `Player and app settings` link opens Player settings, and Settings navigation shows `Back to sign in`. Account-owned shelves still need sign-in.
- CDP attach retries 3 times: loopback connects on this machine occasionally fail with a bare `ErrorEvent`. A failed attach means the app never served CDP; read `app.log`.
- If every step takes seconds instead of milliseconds, the environment is stalling, not the drive. One observed run had an app whose CDP port stopped accepting connections. Let the drive timeout fail it, run `just verify-doctor` (check its loopback line) and re-run. Do not raise timeouts to paper over it.
- The Windows wrapper runs through `pwsh -File` with no parameter binding, so pass session options after the drive (`just verify browse --run-id x`), never `--` before them.
