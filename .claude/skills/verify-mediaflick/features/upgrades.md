# Updating from MediaFlick 0.2.0

A user who updates from MediaFlick 0.2.0, the first release with a `library.db`, starts signed in on the same Jellyfin device, with the library, franchises, collections and ratings it had, and every choice they made: the Player, Application and Viewing settings, the accent, Letterboxd profiles, My Collections with their custom posters, and per-title track choices. Starting the new build changes none of those choices: the JSON files and posters stay as they were, except the page `accounts.json` records as last shown (`browsing`), which every navigation saves. Going back to the real 0.2.0 afterward works, and so does updating again. A `library.db` that a newer MediaFlick wrote, or whose schema change failed and rolled back, is refused once and left as it was; the services stay down for that run, and the window shows a startup error screen with the reason instead of the sign-in screen.

## Sub-features

- `update-keeps-library`: the first start of this build on a 0.2.0 profile opens `library.db` without recreating it, and every table keeps every row, value for value, before any sync could refill it.
- `update-keeps-choices`: the user's JSON files and posters stay as they were (`accounts.json` apart from `browsing`), and every choice shows in the UI.
- `way-back`: the released 0.2.0 AppImage starts signed in on the profile this build left, shows the same choices and completes a sync; this build then takes the profile back.
- `newer-database-refused`: a `library.db` stamped with a newer schema is refused with a reason, tried only once, and left byte for byte.
- `migration-failed`: a schema change that fails rolls back; the file is refused the same way.
- `startup-error-screen`: while the services are down, every API answer is a 503 marked `startupFailed`, and the window shows `MediaFlick could not start` with the reason, never the sign-in screen.

## How to get to it (user POV)

- Install this build over 0.2.0 and start it.
- Install 0.2.0 again over this build and start it, then this build again.
- Start an older build on a profile a newer one used: the window says `MediaFlick could not start` and why.

## Driving it with ctx (just verify)

Preconditions: Linux, a session cached by `just verify-login SERVER` on a server with the MediaFlick Companion, and a profile the released 0.2.0 left signed in to that session on the same device (`fixtures/library-db/README.md` says how to make one). The drives take their expectations from environment variables the orchestrator sets, so run `just upgrade-e2e-0-2-0 --account SERVER --profile DIR`, not `just verify` alone. It writes `build/e2e/upgrade-0-2-0/report.json`.

- **Before any sync.** For each start, the orchestrator first runs the same binary with `--library-stats` on a copy of the profile: the real `Library::open`, migration included, then an exit before any network, so a lost table cannot be refilled by a sync. Every table's rows, projected onto the columns 0.2.0 created, must hash the same as before, with `quick_check` ok and no broken foreign key; the drive records each comparison as a `before any sync:` check. *Proven by `upgrade-0-2-0` (`upgrade-0-2-0.json`).*
- **Update.** `--account SERVER --seed DIR --seed-data DIR`, where the session puts the cached token into the profile's own credentials row (it refuses a profile signed in as another session). The app starts on Home without a sign-in screen as the profile's user on its device id, at the build's schema, with no `recreating the` and at most one `library schema migrated` line in `app.log`. `settings.json`, `accounts.json` (without `browsing`, which every navigation saves), `collections.json`, `playback-preferences.json` and each `custom-art/` file hash as before. `document.documentElement.dataset.accent`, `{ role: "combobox", name: "Default fullscreen" }`, `{ role: "switch", name: "Show scrollbars" }`, `{ role: "switch", name: "Hide watched by default" }`, the Letterboxd shelf's text, `{ role: "link", name: "Open <title>" }` with its poster image's bytes, and the movie's `{ role: "combobox", name: "Subtitle track" }` show what the files say. A sync then completes with `sync.latest_failure` 0, and after `ctx.relaunch()` nothing migrates again. *Proven by `upgrade-0-2-0` (`01-home`, `02-application`, `03-letterboxd`, `04-my-collections`, `05-playback-choice`, `06-after-relaunch`).*
- **The way back and forward again.** `MEDIAFLICK_DESKTOP_EXE` set to the released 0.2.0 AppImage (SHA-256 checked) on the profile the update left, then this build on what 0.2.0 left: the same checks, with no migration expected from 0.2.0. *Proven by `upgrade-0-2-0`.*
- **A newer database.** `--services-down --seed-data DIR` with `library.db` stamped one schema newer. Every `/api/status` answers 503 marked `startupFailed: true`, naming a newer MediaFlick and saying the file was left as it was, with the reason `app.log` gave; after six requests `app.log` still has one `could not open` line; the window shows `{ role: "heading", name: "MediaFlick could not start" }` with that reason and no `Sign in` button; `library.db` keeps its bytes and version. *Proven by `library-refused` (scenario `4-newer-refused`: `library-refused.json`, `01-refused`).*
- **A failed schema change.** The same, with `library.db` stamped schema 0 while it holds 0.2.0's tables, so creating the schema fails on the first table and rolls back (no released step from 23 exists yet to fail). The reason says `could not update library.db from schema 0` and that the file was left at schema 0; it quotes the failed SQL over several lines, which the screen shows with the line breaks collapsed. *Proven by `library-refused` (scenario `5-migration-failed`: `library-refused.json`, `01-refused`).*

## Gotchas

- A read-only open of a WAL `library.db` still writes its `-shm` file: read it from a copy, as `scripts/profile-state.mjs` does.
- The profile holds a real server's library and, once a session ran on it, may hold a token in `library.db`. The orchestrator nulls the token in every copy it keeps and scans `build/e2e/upgrade-0-2-0/` for it; never commit such a profile.
- The startup error screen offers no way to reset or delete `library.db`: the reason says what to do, and a refused file stays as it was.
- `bootstrap_at` and other sync keys change once a sync runs, and a day-old profile gets a full re-bootstrap: compare rows only at the pre-sync point.
