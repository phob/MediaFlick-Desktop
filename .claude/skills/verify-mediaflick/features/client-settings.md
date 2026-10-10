# Client settings

The Client group in Settings holds this device's Player, Playback and Application preferences. Every shelf edits a draft: changes stay unsaved until `Save`, `Discard` restores the saved values, `Reset` loads defaults into the draft, and leaving a dirty shelf asks before discarding. Saved values land in `settings.json` in the config dir.

## Sub-features

- `app-close` chooses what closing the window does (`Exit MediaFlick` or `Minimize window`).
- `app-scrollbars` shows or hides native scrollbars.
- `app-loglevel` sets the log level for the next launch.
- `app-delete-local` deletes this device's data for the signed-in account (typed `DELETE` confirmation).
- `player-backend` picks the built-in player or external mpv, with the mpv path, installer and comfort values.
- `player-shortcuts` records player keys, with a conflict alert.
- `playback-quality` and `playback-segments` set the default streaming quality and intro/credits/recap/commercial skipping.
- `savebar` covers Save, Discard and Reset, and `draft-guard` covers leaving with unsaved changes.

## How to get to it (user POV)

- Signed in: sidebar `Settings` → lands on `Player` (`/settings/client/player`) → `Settings navigation` links `Player`, `Playback`, `Application`.
- Signed out there is no visible entry; see Gotchas.

## Driving it with ctx (just verify)

Preconditions:

- Signed in through the sign-in form (`ctx.signIn`, demo server is fine: these settings are device-local).
- `ctx.readConfig("settings.json")` and `ctx.configStat("settings.json")` recorded before the change.

- **Open the shelf.** `ctx.press({ role: "link", name: "Settings" })`, wait for `ctx.pathname()` to be `/settings/client/player`, then `ctx.press({ role: "link", name: "Application" })`. *Proven by `application-settings`.*
- **Draft.** `ctx.press({ role: "switch", name: "Show scrollbars" })`. `ctx.state(...)` reports `checked: true`, the save bar's status reads `You have unsaved changes.`, `{ role: "button", name: "Save" }` enables, and `settings.json`'s mtime is unchanged. *Proven.*
- **Discard.** `ctx.press({ role: "button", name: "Discard" })`. The switch returns to `checked: false`, the status text empties, and `settings.json` is still untouched. *Proven.*
- **Save.** Change the switch again, then `ctx.press({ role: "button", name: "Save" })`. `settings.json` gains `"show_scrollbars": true`, `Save` disables, and `ctx.api("/api/settings")` reports `client.application.showScrollbars: true`. *Proven.*
- **Read back.** Press `Player`, then `Application` again: the switch still reads `checked: true`. *Proven.*
- **Selects.** Selects are Radix comboboxes named by their `label` prop, not the visible row title: `ctx.choose({ name: "Close behavior" }, "Minimize window")`, `ctx.choose({ name: "Log level" }, "Debug")`, `ctx.choose({ name: "Player backend" }, "External mpv")`, `ctx.choose({ name: "Default fullscreen" }, "Windowed")`, `ctx.choose({ name: "Default streaming quality" }, …)`, and the segment selects `Intro skipping`, `Credits skipping`, `Recap skipping`, `Commercial skipping` with options `Never`, `Ask me`, `Always skip`. Not yet driven.
- **Player details.** External mpv shows `{ role: "textbox", name: "mpv executable" }` and `{ role: "button", name: "Choose mpv executable" }`. Comfort values are spinbuttons named like `Subtitle size (%)`, each with a `… slider`. Shortcut recorders are buttons named like `Pause key` (`aria-pressed` while recording) with `Clear pause key`; a conflict shows `role="alert"`. Not yet driven.
- **Leave guard.** With a dirty shelf, press another settings link: a dialog `Leave without saving?` offers `Keep editing` and `Discard and leave`. Not yet driven.
- **Proof.** Snapshot the draft state (`02-draft.ax.txt` in the proven run), screenshot each state, and keep `settings-before.json` and `settings-after.json` via `ctx.writeEvidence`.

## Gotchas

- `role="status"` gets no accessible name from its content, so read the save bar text with `ctx.app.evaluate('document.querySelector(".settings-save-bar [role=status]")?.textContent')`.
- Signed out, the app keeps these shelves reachable by URL (`/settings/client/*`), but the sign-in screen has no link to them. That is not a user path; sign in first.
- Never press `Installation help` (opens the user's default browser on the visible desktop), `Install mpv` (downloads and installs mpv into the profile), `Choose mpv executable` (opens a native file dialog on the private desktop that no drive can answer), or `Delete local account data` with `DELETE` typed (wipes the profile's account; fine in a disposable profile, but it signs out mid-drive).
- `Show scrollbars` is applied at launch through a CEF switch; the saved value is the proof, not the page's scrollbars.
- `Reset` only loads defaults into the draft; it still needs `Save`.
