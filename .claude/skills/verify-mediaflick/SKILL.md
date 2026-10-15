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

`MEDIAFLICK_VERIFY_HARD_TIMEOUT` (seconds, default 900) bounds the whole wrapper run.

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

While a drive runs, `ctx.jellyfin(path)` reads the server as the cached account (GET only), for example `/Items/<id>?userId=${ctx.account.userId}` to read back what the app reported. A drive that changes server state puts it back through the user path, and also registers an after-run task that runs once the app has exited, so a failure halfway still cleans up: `ctx.restoreItemAfterRun(itemId)` resets a played title or resume point (`POST /UserItems/<id>/UserData`; play count and last-played date stay), `await ctx.unfavoriteAfterRun(itemId)` removes a favorite the drive is about to add if it is still one (`DELETE /UserFavoriteItems/<id>`, the route the app uses), and `ctx.cancelSeerrRequestAfterRun(id)` cancels a request still in Seerr (`DELETE /MediaFlick/seerr/request/<id>`, the app's own cancel). A failed after-run task fails the run, and so does a failed teardown, which never skips the after-run tasks.

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

The `ctx` helpers come from `scripts/harness.mjs`. Targets are `{ role, name }`, matched through CDP `Accessibility.queryAXTree` (exact accessible name, rendered elements only). `{ css }` is the fallback for controls with no stable name.

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
| `revokeAppSessionAfterRun()` | `--signed-out` only, after sign-in: once the app has exited, sign the app's own token out on the cached account's server unless it already is (a drive that signs in to a proxy cannot sign out through it after the run). |
| `player()` | `GET /api/player/state`: `active`, `itemId`, `playMethod`, `positionMs`, `durationMs`, `paused`, `tracks`, `skipSegments`, `diagnostics`. |
| `mpv(property)` | Reads one mpv property (`time-pos`, `pause`, `current-vo`, `hwdec-current`) over mpv's IPC socket, found through `app.log`'s `mpv IPC connected ipc_path=`, on one connection kept for the drive. On Windows, when a script has moved mpv's IPC server (SVP's `mpvSockets.lua` re-points it to `<TEMP>\mpvSockets\<pid>`), it follows the move and logs `mpv IPC moved by a script`. Reads only: the app stays mpv's single command writer. |
| `playstates()` | The playstate reports in `app.log`: `{ queued: [{ at, endpoint, itemId, positionS }], sent: [{ at, endpoint, itemId, status }] }`. Needs `--log-level "debug,jellyfin.playstate=trace"`. |
| `revealPlayer()` | Moves the pointer near the bottom edge so the Built-in player's hidden, `inert` bar comes back for ARIA lookups. |
| `displayCapture(name, { seconds })` | Linux only: the whole X display with ffmpeg `x11grab` as `<name>.png`, or an MP4 clip with `seconds`. It contains the mpv video, which CDP screenshots (the page alone) do not. |
| `pathname()` | `location.pathname + location.search`. |
| `api(path)` | GET the app's own `/api/*` as the UI does, to read results. Never the action under test. |
| `proxy({ upstream, fail, answer })` | Start a loopback Jellyfin proxy on `127.0.0.1` that forwards to `upstream` (path prefix included), streaming bodies and ranges and passing WebSocket upgrades (the app's `/socket`) through. It drops the connection of each request `fail(url, method)` selects, and answers a request itself when `answer(url, method)` returns `{ status, body }` (JSON); `mediaSegmentsAnswer({ [itemId]: [{ type, startS, endS }] })` from `scripts/proxy.mjs` serves `GET /MediaSegments/<id>` in Jellyfin's format. Returns `{ url, stats, heal() }` (`stats.answered` lists the answered requests); sign in to `url`. Closed after the drive. |
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

Shipped drives. All are proven on Linux, `segments` only in part: its `Ask me` and `Never` cases on a file with chapters pass, and its other cases wait for the `Don't show` build or a marker fix (see [Playback](features/playback.md)). On Windows 11 (2026-10-13) all but `close-minimize`, `close-guard` and `segments` pass, `playback` on both backends; the close drives use `wmClose` and run on Linux only, and `segments` has not run there yet. The last seven need your own server (`--account`; see [Your own server](#your-own-server)):

- `smoke`: fresh profile, signed-out sign-in screen.
- `login`: only through `just verify-login SERVER`. Quick Connect sign-in, approved by the user, then `saveAccount()`.
- `application-settings`: Settings → Application, draft, Discard, Save, `settings.json`, read-back.
- `browse`: sign-in, Home, Movies, search, movie and series detail, sign-out. Read-only against the demo server.
- `sign-in` (needs `--url https://demo.jellyfin.org/stable`): prefill, the Quick Connect probe while the Server field keeps focus, the code (never approved) and its copy button with a clipboard readback, user menu, sign-out from Home, remembered server.
- `home-library`: billboard ticks, shelf arrows, a card from Home, sort, a decade filter and its chip, an empty search, Favorites. Read-only.
- `detail-crew`: a movie's Crew rail (one `Director, Writer` entry with a loaded headshot, no crew lines left in Details), then a crew face to the person page listing the title they directed. Read-only.
- `item-detail`: movie extras and the More info menu (opened only), series seasons and episodes, an episode page and its Season breadcrumb. Read-only.
- `client-settings`: Player shelf per backend, range alert, leave guard, Playback select, restart notices, live scrollbar style, Restore defaults + Save, the Delete local account data dialog (cancelled).
- `labels`: the sign-in screen's Player and app settings link and Back to sign in, the save bar's Restore defaults and muted disabled Save, Favorites, the watched and favorites toggles on a movie and on episode cards, Countdown seconds gating, and the Releases startup option (opened, never chosen). Read-only.
- `close-minimize`: saves `Minimize window`, closes through the window manager and proves the window asked to be minimized while the app kept running; then saves `Exit MediaFlick` and proves the next close exits.
- `close-guard`: an unsaved draft, a window-manager close, the `Leave without saving?` dialog; `Keep editing` keeps the app and the draft, `Discard and leave` exits without writing it.
- `bootstrap-retry`: sign-in through a proxy that drops the first catalog page, the retry screen, relaunch with the window shown, Sign out, Retry now while down, Home after the proxy heals. Read-only.
- `sidebar-user-menu`: off Home, the user menu holds the hover sidebar expanded while it is open, also with the pointer on the menu, and the sidebar collapses after the menu closes. Read-only.
- `library-filters`: the Series view, `Sort: Rating`, the Genre and Watch status filters (the menu closes on a pick and is named after its trigger), chips and `Clear all`, a one-character search that never commits, clearing a search, and signing out from a library page. Read-only.
- `player-shelf`: the shortcut conflict alert, the Player shelf's `Player settings saved` toast, and `Restore defaults` + Save on Player, which keeps `player_backend`, `comfort` and `mark_watched_next` in `settings.json`.
- `library-parity`: sort direction, the per-kind sort lists, a seeded Random order and Shuffle again, the In progress, Parental rating and Series status filters, the A–Z rail in both directions, and series cards' unwatched count, each compared with the demo server's own API. Read-only.
- `home-settings`: Home's `No titles available` and `No shelves enabled` states through the local Home settings, `Configure Home`, then `Restore defaults` + Save. Read-only against the demo server.
- `punctuation-search`: a search of punctuation alone (`!!`) shows `Nothing to show` and `/api/items` answers 200 with no items, while `night!!` still finds titles. Read-only against the demo server.
- `top-level-titles` (demo server, or `--account SERVER`): views without a kind list only movies and series. Both modes: a sidebar search for a word from a season name that zero to six movies or series share, and one for a word from an episode name that one to six share (`night` on the demo server), each show exactly those titles, the empty state when there are none, and no season or episode card or API row. The kindless `favorite=false` count and A–Z letters match `kind=Movie,Series` while seasons and episodes exist, `kind=Episode` still lists episodes, and `/api/home` rows hold only movies and series (Recently Added Shows: series and episodes). Read-only on the demo server, which leaves Favorites unproven. With `--account` it also favorites a movie and an episode that were not favorites on their detail pages, proves Favorites (cards and `/api/items`), its A–Z letters and Home's Favorites shelf list the movie and not the episode, and removes both favorites again (journaled after-run fallback). The Favorites card check reads a virtualized grid, so it assumes the account has few favorites. Every check is recorded in `top-level-titles.json` and the run fails at the end listing each failed one.
- `cleanup-journal` (`--account SERVER`): favorites the most recently added movie that is not a favorite on its page, then fails on purpose; the after-run task must remove the favorite. With `MEDIAFLICK_VERIFY_HARD_EXIT=1` it ends the session process right after the write instead, as a kill would, and the next `--account` run must remove the favorite before launch. Fails by design in both modes; read the server back after each run. Proven on Windows 11 against Jellyfin 12.2 (2026-10-15): the after-run task removed the favorite, and after a hard exit the next `top-level-titles --account` logged `favorites journal: removed leftover test favorite <id>` before launch.
- `person-discover` (demo server, or `--account SERVER`): a person page's Discover section, reached through a movie's Crew, says Seerr is not connected without the Companion and lists Seerr's person credits with it. Read-only.
- `libraries-api` (demo server, or `--account SERVER`; proven on Windows 11): after the first sync, `/api/libraries` lists exactly the ids, names, order and kinds of Jellyfin's `/UserViews` with no library's `membershipPending` still true, and each movies or TV shows library's grid, paged through `/api/libraries/<id>/items`, lists exactly the ids Jellyfin pages through `/Items?ParentId=<library>&Recursive=true&IncludeItemTypes=<Movie or Series>`, with the same total. It also checks the library routes' 404, 400 and limit clamp answers, letters answering 200 only where `browse.letterIndex` is true and 400 elsewhere, and a live library's 501 on its other grid routes. Every check, with counts and a sample of any missing or extra ids, is recorded in `libraries-report.json`. Read-only.
- `generic-detail` (demo server, or `--account SERVER`; proven on Windows 11): generic details pages, opened by URL with no navigation state because nothing links to an album or box set page until the library browse change. The Music library's first album with artwork shows no Play, favorites, watched, More info, quality picker, ratings, collection chip, Letterboxd or Media, goes back to Home, contains its cover in a square hero frame and any square track art in its cards, and its `Contents` grid holds exactly the tracks Jellyfin lists, as many as its `ChildCount`; a track card opens the track's generic page without a grid. The Movies library's own folder page lists exactly its direct children while the page scrolls them in (685 over 12 pages on the owner's server). The top-rated movie, opened from a search, keeps its title page (Play or Resume, quality, watched, favorites, More info, community rating) and reports its movies library; an episode reports its series' TV shows library. `/api/item/<id>` and its children route answer 400 for a malformed id and a negative offset and clamp `limit` to 200. With `--account` it also opens the Collections library's largest box set, whose grid lists exactly its titles, and presses a movie card into the full movie page. Every check is recorded in `generic-detail.json`, with `01-album`, `02-track`, `03-library-folder`, `04-movie` and, with `--account`, `05-box-set` and `06-box-set-title` screenshots. Read-only.
- `account-session` (`--account SERVER --signed-out`): Quick Connect sign-in approved by the harness, Sign out revoking the token on the server, and a token revoked from outside expiring the session.
- `playback` (`--log-level "debug,jellyfin.playstate=trace,playback=trace"`; External mpv with `--seed .claude/skills/verify-mediaflick/seeds/external-mpv`): Play from the start, seeking past the resume minimum, pause, Stop from the bar, the resume point on the server, Resume with the startup seek rules, stop again, then watched and unwatched. Writes the test user's playstate and leaves the title as it found it.
- `companion`: the Companion status page, Releases (Agenda, Month, toggles, month navigation), Home's Releases shelf, and MDBList ratings after picking sources in Appearance (restored).
- `discover-requests`: Discover rows, a genre, an owned title in search and its page, then a real Seerr request from a card, cancelled from Requests.
- `collections`: Movie Franchises and a franchise page, a My Collections entry from a TMDB template, Jellyfin mode with every BoxSet and the route redirect, then defaults again.
- `account-settings`: Viewing Discard and Save with a revisit, the leave guard between account shelves, Appearance's accent applied live, a Letterboxd profile added, verified and removed.
- `segments` (`--account SERVER --signed-out --log-level "debug,mpv.ipc=trace"`; External mpv with `--seed .claude/skills/verify-mediaflick/seeds/external-mpv`): signs in to a loopback proxy that adds an Intro and an Outro to one episode, then per Playback segment setting plays it and checks `skipSegments`, mpv's chapter markers, `Skip Intro`, the OSD prompt and that nothing skips; with External mpv also a mid-playback switch to `Don't show`. `MEDIAFLICK_VERIFY_SEGMENTS=baseline` runs only `Ask me` and `Never`. Writes the test user's playstate and leaves the episode as it found it.

Both close drives also run with External mpv as the window: seed a `settings.json` holding `{"player_backend":"mpv"}` with `--seed DIR`. No mpv executable is needed for them, because they play nothing. `playback` with External mpv needs one (`mpv_path` in `seeds/external-mpv/settings.json`, `/usr/bin/mpv`; on Windows seed a copy with the `mpv.exe` path).
Re-run all of them:

```powershell
foreach ($d in 'smoke', 'application-settings', 'browse', 'home-library', 'detail-crew', 'item-detail', 'client-settings', 'labels', 'bootstrap-retry', 'sidebar-user-menu', 'library-filters', 'library-parity', 'player-shelf', 'home-settings', 'punctuation-search', 'top-level-titles', 'person-discover', 'libraries-api', 'generic-detail') { just verify $d --run-id "all-$d-$(Get-Date -Format HHmmss)" }
just verify sign-in --run-id "all-sign-in-$(Get-Date -Format HHmmss)" --url https://demo.jellyfin.org/stable
```

```sh
for d in smoke application-settings browse home-library detail-crew item-detail client-settings labels close-minimize close-guard bootstrap-retry sidebar-user-menu library-filters library-parity player-shelf home-settings punctuation-search top-level-titles person-discover libraries-api generic-detail; do just verify "$d" --run-id "all-$d-$(date +%H%M%S)"; done
just verify sign-in --run-id "all-sign-in-$(date +%H%M%S)" --url https://demo.jellyfin.org/stable
```

Your own server (one at a time; they share the cached session):

```sh
S=https://jellyfin.example.com
just verify account-session --account "$S" --signed-out
for d in companion discover-requests collections account-settings top-level-titles libraries-api generic-detail; do just verify "$d" --account "$S"; done
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

- **Windows: proven.** Every shipped drive except the two Linux-only close drives passes on Windows 11 with the inactive desktop (2026-10-13), about 10–30 s each including the incremental build (`collections` about 2 min, most of it the first sync and franchise matching). CEF renders there, `requestAnimationFrame` runs, `document.visibilityState` is `visible`, and `Page.captureScreenshot` returns real frames. `window.close()` exits cleanly, `guard.json` shows the real profile untouched, and no process or profile survives. The Built-in player draws its bar over the video as on Linux (`integratedLibmpvOverlay`), starts fullscreen on the private desktop, and `ctx.mpv` reads it over its named pipe (`\\.\pipe\mpvpipe` with the SVP libmpv profile these runs used). External mpv plays in its own window on the private desktop, windowed at 1280 px, so `Stop` sits in the bar's overflow menu there.
- **Linux: proven.** Every shipped drive passes on Ubuntu 26.04 (all but `login` re-run on 2026-10-14, the real-server drives against Jellyfin 12.2.0 with Companion 0.2.2) under Xvfb with the private session bus, about 2–40 s each after the build (the real-server drives 20–120 s, most of it the first sync and playback). CEF renders, `Page.captureScreenshot` returns real frames, `window.close()` exits cleanly (`exit: coordinated`), `guard.json` shows the real profile untouched, and no process, profile or `mediaflick-xvfb.*` dir survives. The app's Linux window is composited through libmpv (`libmpv.so.2`, the default Built-in player): `app.log` shows `mpv IPC connected` and `received first Linux CEF frame` on a healthy start.

  Not yet exercised on Linux: the hard timeout, and a run without `dbus-run-session`.
- Headless Wayland is not implemented; the wrapper forces X11 (`WAYLAND_DISPLAY` unset, `XDG_SESSION_TYPE=x11`).

## Gotchas

- **Playback is real.** Play only in `playback`-style drives with `--account` on your own server: it writes the test user's playstate, and on Windows it is audible, because the private desktop hides windows and input, not sound; `playback` mutes with the player's `m` key about 2 s after the start. On Linux the private `XDG_RUNTIME_DIR` cuts off the user's audio server (`Failed to connect to PipeWire`). Never press Play on the demo server.
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
