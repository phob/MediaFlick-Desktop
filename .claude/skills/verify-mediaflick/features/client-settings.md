# Client settings

The Client group in Settings holds this device's Player, Playback and Application preferences. Every shelf edits a draft: changes stay unsaved until `Save`, `Discard` restores the saved values, `Reset` loads defaults into the draft, and leaving a dirty shelf asks before discarding. Saved values land in `settings.json` in the config dir; default values are left out of the file.

## Sub-features

- `app-close` chooses what closing the window does (`Exit MediaFlick` or `Minimize window`).
- `app-scrollbars` shows or hides native scrollbars, applied as soon as it is saved.
- `app-loglevel` sets the log level for the next launch.
- `app-delete-local` deletes this device's data for the signed-in account (typed `DELETE`, then a confirmation dialog).
- `player-backend` picks the built-in player (libmpv) or external mpv. Built-in shows comfort values and shortcut recorders; external shows the mpv path, `Installation help`, and on Windows the mpv installer.
- `player-shortcuts` records player keys, with a conflict alert.
- `playback-quality` and `playback-segments` set the default streaming quality and intro/credits/recap/commercial skipping.
- `savebar` covers Save, Discard, Reset and restart notices; `draft-guard` covers leaving with unsaved changes.

## How to get to it (user POV)

- Signed in: sidebar `Settings` → lands on `Player` (`/settings/client/player`) → `Settings navigation` links `Player`, `Playback`, `Application`. The same navigation lists the Account group (`Viewing`, `Home`, `Appearance`, `Collections`) and Integrations (`MediaFlick Companion`, `Letterboxd`), which this file does not cover.
- Signed out there is no visible entry; see Gotchas.

## Driving it with ctx (just verify)

Preconditions:

- Signed in through the sign-in form (`ctx.signIn`, demo server is fine: these settings are device-local).
- `ctx.readConfig("settings.json")` and `ctx.configStat("settings.json")` recorded before the change.
- Wait for `{ role: "combobox", name: "Player backend" }` before reading the Player shelf; it renders empty while loading.

- **Open the shelf.** `ctx.press({ role: "link", name: "Settings" })`, wait for `ctx.pathname()` to be `/settings/client/player`, then `ctx.press({ role: "link", name: "Application" })`. *Proven by `application-settings`.*
- **Draft.** `ctx.press({ role: "switch", name: "Show scrollbars" })`. `ctx.state(...)` reports `checked: true`, the save bar's status reads `You have unsaved changes.`, `{ role: "button", name: "Save" }` enables, and `settings.json`'s mtime is unchanged. *Proven.*
- **Discard.** `ctx.press({ role: "button", name: "Discard" })`. The switch returns to `checked: false`, the status text empties, and `settings.json` is still untouched. *Proven.*
- **Save.** Change the switch again, then `ctx.press({ role: "button", name: "Save" })`. A `Settings saved` toast appears, `settings.json` gains `"show_scrollbars": true`, `Save` disables, and `ctx.api("/api/settings")` reports `client.application.showScrollbars: true`. The page's `<style id="__mediaFlickDesktopScrollbarStyle">` (present while scrollbars are hidden) disappears without a restart. *Proven by `application-settings`, `client-settings`.*
- **Read back.** Press `Player`, then `Application` again: the switch still reads `checked: true`. *Proven.*
- **Restart notices.** A change that needs a restart replaces `You have unsaved changes.` in the status: `Log-level changes apply after restarting MediaFlick.` after `Log level`, and `Restart MediaFlick to switch player backends.` (or `… to enable the built-in player.`) after `Player backend`. *Proven by `client-settings`.*
- **Reset.** `Reset` loads defaults into the draft; `Save` then removes the keys from `settings.json` instead of writing defaults. *Proven by `client-settings`.*
- **Selects.** Selects are Radix comboboxes named by their `label` prop, not the visible row title: `ctx.choose({ name: "Close behavior" }, "Minimize window")`, `ctx.choose({ name: "Log level" }, "Info")`, `ctx.choose({ name: "Player backend" }, "External mpv")`, `ctx.choose({ name: "Default fullscreen" }, "Windowed")`, `ctx.choose({ name: "Default streaming quality" }, …)` (`Original file`, `Auto`, `120 Mbps` … `1.5 Mbps`), and the segment selects `Intro skipping`, `Credits skipping`, `Recap skipping`, `Commercial skipping` with options `Never`, `Ask me`, `Always skip`. `Log level`, `Player backend` and `Intro skipping` (`settings.json` `skip_intro: "always"`) are *proven by `client-settings`*; the others are source-only.
- **Player, built-in.** `ctx.api("/api/settings")` reports `capabilities.libmpv` (on Linux, whether `libmpv.so.2` loaded) and `capabilities.mpvInstaller` (Windows only). With libmpv available, a fresh profile reads `Built-in player`, with spinbuttons named like `Subtitle size (%)` (each with a `… slider`) and shortcut recorders named like `Pause key` (`aria-pressed` while recording) with `Clear pause key`. A value out of range shows `role="alert"` `Enter a whole number from 50 to 200.` and disables `Save`. *Proven by `client-settings` on Linux*; the shortcut conflict alert is source-only.
- **Player, external.** `External mpv` shows `{ role: "textbox", name: "mpv executable" }`, `{ role: "button", name: "Choose mpv executable" }`, `Installation help`, and the only shortcut recorder left, `Mark watched key`. The `Install mpv` button exists only where `capabilities.mpvInstaller` is true; on Linux the row titled `Install mpv` holds just `Installation help`. *Proven by `client-settings` on Linux.*
- **Leave guard.** With a dirty shelf, press another settings link: `{ role: "dialog", name: "Leave without saving?" }` offers `Keep editing` (stays) and `Discard and leave` (drops the draft and navigates). The guard also fires on sidebar links that leave Settings. *Proven by `client-settings`.*
- **Delete local account data.** Typing `DELETE` into `{ role: "textbox", name: "Type DELETE to confirm local account deletion" }` enables `Delete local account data`; pressing it opens `{ role: "alertdialog", name: "Delete local account data?" }` with `Cancel` and a second `Delete local account data`. Only the dialog's action deletes. *Cancel proven by `client-settings`.*
- **Proof.** Snapshot the draft state (`02-draft.ax.txt` in `application-settings`), screenshot each state, and keep `settings-before.json`, `settings-after.json` and `settings-final.json` via `ctx.writeEvidence`.

## Gotchas

- `role="status"` gets no accessible name from its content, so read the save bar text with `ctx.app.evaluate('document.querySelector(".settings-save-bar [role=status]")?.textContent')`.
- The `Settings saved` toast sits over the save bar's buttons for about 4 s. `ctx.press` waits it out; a hand-rolled click would hit the toast.
- End a drive with the shelf saved or discarded. A dirty draft holds the window on `window.close()`, and teardown has to force the exit.
- Signed out, the app keeps these shelves reachable by URL (`/settings/client/*`), but the sign-in screen has no link to them. That is not a user path; sign in first.
- Never press `Installation help` (opens the user's default browser on the visible desktop), `Install mpv` on Windows (downloads and installs mpv into the profile), `Choose mpv executable` (opens a native file dialog on the private display that no drive can answer), or the dialog's `Delete local account data` (wipes the profile's account and signs out mid-drive).
- `Show scrollbars` is also passed to CEF at launch, but the saved value and the style element are the proof, not what the page's scrollbars look like.
- `Reset` only loads defaults into the draft; it still needs `Save`.
