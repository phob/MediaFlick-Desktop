---
name: verify-mediaflick
description: Drive the real MediaFlick Desktop app (staged CEF build, build/mediaflick-desktop) the way a user does and capture proof. Runs the app on an inactive private desktop (Windows) or a private Xvfb display (Linux) with a disposable profile, and operates the embedded React UI over CDP with ARIA lookups and real mouse/keyboard input. Use to prove a UI, settings, sign-in, library or detail-page change works in the shipped binary, or to reproduce a UI bug. Not for playback, the Companion plugin, or packaging.
---

# Verify MediaFlick

MediaFlick Desktop is a Rust/CEF shell around an embedded React UI. `just verify <drive>` builds and stages the app, starts it out of the user's sight with a throwaway profile, runs your **drive** (a Node module) against it over the Chrome DevTools Protocol, saves evidence, and shuts it down.

How it works:

- `just verify` runs the per-OS **isolation wrapper**, which runs `node scripts/session.mjs` inside the isolation. Everything the session starts inherits it.
  - Windows (`scripts/isolate-windows.ps1`): a new desktop `MediaFlick.Verify.<pid>.<tick>`, never switched to, and a kill-on-close job.
  - Linux (`scripts/isolate-linux.sh`): a private Xvfb display chosen with `-displayfd`. `WAYLAND_DISPLAY` is unset, `XDG_RUNTIME_DIR` is private, the session bus is private or absent, and the run gets its own session and process group.
- `session.mjs` refuses to launch anything until `platform.mjs` has confirmed that isolation from inside it. On Windows a child process reports its own desktop, which must differ from the visible input desktop. On Linux, `DISPLAY` must belong to the wrapper's Xvfb, with no Wayland and the private runtime dir.
- The disposable profile is `<os tmp>/mediaflick-verify-<run-id>-<session-pid>-<random>`. On Windows the app gets `APPDATA`, `LOCALAPPDATA` and `TEMP` inside it. On Linux it gets `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME` and `TMPDIR` inside it. The app resolves its directories from exactly these variables (`src/app/paths.rs`). Its single-instance gate is keyed by `instance.json` in that config dir, so a verify run never collides with the user's own MediaFlick, which can keep running.
- The user's real `accounts.json`, `settings.json`, `collections.json`, `playback-preferences.json`, `pending-deletions.json`, `instance.json` and `library.db` are never opened for writing. The session fingerprints their size and mtime before and after (`guard.json`) and fails the run if they changed while no user instance was running.

Drive scripts, the harness and the session are platform-neutral Node. Only the two wrappers and `scripts/platform.mjs` (isolation check, process listing, tree kill, app environment) are per-OS.

## Launch

Prerequisites: Node 22+ (global `WebSocket`) and `just`. Windows also needs PowerShell 7 from the MSI, not the Store build. Linux also needs `Xvfb`, `setsid` and `timeout`, and optionally `dbus-run-session`.

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
- `--seed DIR`: copied into the empty config dir before launch, for example a prepared `settings.json`.
- `--keep-profile`: leaves the profile on disk for inspection. Remove it later with `just verify-cleanup`.

`MEDIAFLICK_VERIFY_HARD_TIMEOUT` (seconds, default 900) bounds the whole wrapper run.

A drive is a bare name from `drives/` or a path. Ready means `steps.log` shows `isolation ok: …`, then `launched pid … on CDP port …`, then `doctor ok: …`. Startup takes about 2 s. Teardown is automatic: the session calls `window.close()` in the page, which takes the app's own close path including save-on-exit. It waits up to 20 s and records `exit: coordinated`. Otherwise it stops exactly its PID tree and records `exit: forced`.

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
| `press(target)` | Scroll into view, hit-test, click the centre with the real mouse. A covered target first gets about 0.9 s to settle; then the pointer is parked and it is tested again. Fails if still covered. Fails on disabled elements. |
| `fill(target, text)` | Click into a text field, select all, type `text` (`Input.insertText`). |
| `choose({ name }, option)` | Open a Radix Select (role `combobox`, named by its `label`) and pick the option by name. |
| `hover(target)` | Park the pointer, then move onto the target (fires `pointerenter`; the sidebar expands on it). |
| `park()` | Move the pointer to the top-right corner, collapsing hover-opened UI. |
| `key(name, { code, keyCode, modifiers })` | One key press. |
| `state(target)` | AX state: `checked`, `pressed`, `expanded`, `disabled`, `selected`, `value` (tristate tokens become booleans). |
| `exists(target)`, `count(target)` | Rendered matches right now. |
| `until(exprOrFn, description, timeout)` | Poll a page expression or Node function until truthy; logs `saw <description>`. Never use fixed sleeps for state. |
| `check(condition, message)` | Fail, or log `ok: message`. |
| `signIn({ server, username, password })` | Drive the sign-in form and wait for the sidebar. |
| `pathname()` | `location.pathname + location.search`. |
| `api(path)` | GET the app's own `/api/*` as the UI does, to read results. Never the action under test. |
| `readConfig(name)`, `configStat(name)`, `configFile(name)` | Files in the disposable config dir (`settings.json`, `accounts.json`, …). |
| `screenshot(name)`, `snapshot(name)` | `<name>.png` (after a 300 ms settle) and `<name>.ax.txt`: one line per node, `role 'name' [state]`. |
| `writeEvidence(name, value)` | Any extra artifact into the evidence folder. |
| `step(text)` | A line in `steps.log`. |
| `app` | The raw CDP client from `scripts/website/cdp.mjs` (`send`, `evaluate`, `waitFor`). |

To find handles for a page you have not driven yet, take a `snapshot` there and read the `.ax.txt`. Read the feature map in `features/` before writing a drive. It lists entry points, handles marked *proven* or source-only, and traps.

Shipped drives, all proven on Windows:

- `smoke`: fresh profile, signed-out sign-in screen.
- `application-settings`: Settings → Application, draft, Discard, Save, `settings.json`, read-back.
- `browse`: sign-in, Home, Movies, search, movie and series detail, sign-out. Read-only against the demo server.

Re-run all of them:

```powershell
foreach ($d in 'smoke', 'application-settings', 'browse') { just verify $d --run-id "all-$d-$(Get-Date -Format HHmmss)" }
```

```sh
for d in smoke application-settings browse; do just verify "$d" --run-id "all-$d-$(date +%H%M%S)"; done
```

## Evidence

Each run writes **`build/verify/<run-id>/`**, which cleanup never removes (`just clean` does):

- `steps.log`: a timestamped line per action (`press …`, `fill …`, `hover …`), wait (`saw …`) and check (`ok: …`). This is the action record.
- `doctor.json`: the isolation details (desktop or display), PID, exe and its mtime, git HEAD, Chromium version, `/api/status`, and profile paths and files.
- `result.json`: `passed`, `error` (stack), `exit` (`coordinated`, `forced` or `exited-early`), `profileRemoved`, `durationMs`, and `leftovers` if any process outlived teardown.
- `guard.json`: the real-profile fingerprints before and after, the changed files, and any user MediaFlick PIDs.
- `pids.json`: the session PID, app PID, CDP port and profile path.
- `*.png`, `*.ax.txt`, plus `failure.png` and `failure.ax.txt` when the drive throws.
- `app.log` (Rust log at `debug`), `cef.log`, and `profile/config/`: a copy of everything the app wrote to its config dir.

Proof standards:

- Drive the user path: sidebar links, settings navigation, buttons, switches, cards. Do not perform the action through `/api/*`, `ctx.app.route()`, or `Runtime.evaluate` clicks.
- Capture the action and the resulting state. `steps.log` holds the actions; snapshot or screenshot after each meaningful state, not only at the end.
- Verify side effects alongside the screen. Files in the config dir change only on Save (compare `configStat` mtimes for drafts), and `/api/*` reads back the stored value. Then visit the page a second time.
- The demo server is real and shared. Read-only checks there are genuine end-to-end proof; anything that writes needs a server you own.
- If a mapped entry point cannot be driven, report it as not verified with the reason. Do not substitute a different path.

## Cleanup

Normal runs clean up themselves:

1. Coordinated exit through `window.close()`.
2. Otherwise a forced stop of the app's PID tree (`taskkill /T` on Windows, a parent-PID walk on Linux).
3. A sweep that stops any remaining process whose command line names this run's profile. CEF helpers carry `--user-data-dir=<profile>…`, and the random profile path is unique to the run.
4. The profile directory is removed.

Then the Windows wrapper terminates its job and closes the desktop. The Linux wrapper stops its process group and its Xvfb, which also drops any X client.

The hard timeout is proven. A run whose app stalled was terminated with its job at `MEDIAFLICK_VERIFY_HARD_TIMEOUT`; no process survived, and the profile was left for cleanup. After an interrupted run (killed terminal, hard timeout), `just verify-doctor` lists LEFTOVER profiles and the run as `INTERRUPTED`. Then:

```sh
just verify-cleanup
```

It stops only processes whose command line names a dead run's profile, then removes that profile. It skips runs whose session is still alive, including other worktrees' and other agents' runs, and it never touches `build/verify/`. Never stop `mediaflick-desktop` by name: the user's own session and other runs use the same executable.

## Platform status

- **Windows: proven.** All three shipped drives pass on Windows 11 with the inactive desktop, about 15–25 s each including the incremental build. CEF renders there, `requestAnimationFrame` runs, `document.visibilityState` is `visible`, and `Page.captureScreenshot` returns real frames. `window.close()` exits cleanly, `guard.json` shows the real profile untouched, and no process or profile survives.
- **Linux: not verified end to end.** Proven in WSL Arch without Xvfb or a Linux build:
  - the wrapper's syntax and its refusal paths (no Xvfb, nesting);
  - `platform.mjs`: process listing from `/proc`, the parent-PID tree kill, the isolation check refusing a missing wrapper and a forged claim, the XDG environment;
  - `doctor.mjs`.

  Never run:
  - Xvfb startup through `-displayfd`;
  - CEF rendering and CDP screenshots under Xvfb;
  - `dbus-run-session`;
  - `window.close()` and the group kill on Linux;
  - the `LD_LIBRARY_PATH`/`LD_PRELOAD` launch outside `just run`;
  - any shipped drive.

  First run on Linux: `just verify smoke`, then read `doctor.json` and `failure.*` if it fails.
- Headless Wayland is not implemented; the wrapper forces X11 (`WAYLAND_DISPLAY` unset, `XDG_SESSION_TYPE=x11`).

## Gotchas

- **Audio is not isolated on Windows.** The private desktop hides windows and input, not sound. Never press Play, Resume, From start, or an episode's play button. On Linux the private `XDG_RUNTIME_DIR` cuts off the user's audio server.
- **External links reach the visible desktop.** `More info` menu items, `Installation help` and the update banner's release page open the user's default browser. Never press them.
- **Never press the update banner's install action** (it downloads and runs an installer), `Install mpv`, or `Choose mpv executable` (a native file dialog nobody can answer on the private desktop).
- The sidebar expands under the pointer and overlays content off Home. `press` waits for it to settle and parks the pointer when a target stays covered. To use the sidebar's search off Home, `hover({ css: '[data-sidebar="sidebar"]' })` first.
- Sidebar search commits 200 ms after typing with a replacing navigation. Wait for `/library?search=` before pressing a result.
- `role="status"` regions have no accessible name; read their text with `ctx.app.evaluate`.
- After sign-out `document.title` keeps the last page's title. Identify the sign-in screen by its `Server` textbox.
- The sign-in screen has no link to Settings. Device settings are reachable signed out only by URL; drives sign in first.
- CDP attach retries 3 times: loopback connects on this machine occasionally fail with a bare `ErrorEvent`. A failed attach means the app never served CDP; read `app.log`.
- If every step takes seconds instead of milliseconds, the environment is stalling, not the drive. One observed run had an app whose CDP port stopped accepting connections. Let the drive timeout fail it, run `just verify-doctor` (check its loopback line) and re-run. Do not raise timeouts to paper over it.
- The Windows wrapper runs through `pwsh -File` with no parameter binding, so pass session options after the drive (`just verify browse --run-id x`), never `--` before them.
