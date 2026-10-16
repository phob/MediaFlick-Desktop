# Account settings and integrations

The Account group in Settings (`Viewing`, `Home`, `Appearance`, `Collections`) and Integrations → `Letterboxd` hold the signed-in account's own preferences, stored locally under that account's server and user IDs: `accounts.json` for Viewing, Home, Appearance and Letterboxd, `collections.json` for Collections. Every shelf edits a draft with `Save`, `Discard` and `Restore defaults`, and leaving a dirty shelf asks first, as in [Client settings](./client-settings.md). Home's shelves are covered in [Home](./home.md) and Collections in [Collections](./collections.md).

## Sub-features

- `viewing`: Spoiler protection, Next episode (with Countdown seconds), Episode limit, Resume rewind, audio and subtitle languages, Prefer original audio, Subtitles, Text size, Poster width, Startup destination, Remember library filters, Hide watched by default.
- `appearance`: Accent, Density, card previews and their delay, media info on cards, rating sources (see [Companion](./companion.md)), artwork and backdrop intensity, Reduce motion.
- `letterboxd`: add a public profile by username or URL, enable or disable it, refresh, open, remove; up to 16 profiles. Linked profiles add a `Letterboxd` section with reviews to movie pages, library and Discover alike, for movies with a TMDB id.

## How to get to it (user POV)

Signed in: sidebar `Settings` → `Settings navigation` → `Viewing`, `Appearance` or `Letterboxd`. Signed out, these links are `aria-disabled` and the shelves ask to sign in.

## Driving it with ctx (just verify)

Preconditions: signed in; `just verify account-settings --account SERVER`. All writes go to the disposable profile's `accounts.json`.

- **Viewing.** `{ role: "switch", name: "Hide watched by default" }` toggled and `Discard` puts it back without touching `accounts.json` (same mtime). Toggled again, with `{ role: "textbox", name: "Audio languages" }` set to `EN, de`, `Save` writes `viewing.hideWatched: true` and `viewing.audioLanguages: ["en","de"]` (lower-cased). Leaving for another shelf and coming back shows the switch still on. `Restore defaults` and `Save` put it back. Other handles: switches `Spoiler protection`, `Prefer original audio`, `Remember library filters`; comboboxes `Next episode`, `Resume rewind`, `Subtitles`, `Poster width`, `Startup destination`; spinbuttons `Countdown seconds` (enabled only with `Ask with countdown`), `Episode limit` and `Text size percent`, each with a `<name> slider`; textbox `Subtitle languages`. *Viewing proven by `account-settings` (`01-viewing-saved`).*
- **Startup destination.** The combobox offers `Home`, one option per library under its Jellyfin name in `/api/libraries` order, `Releases` and `Last browsing page`, and shows the destination native resolved (a legacy `movies` reads as the first movies library, a library the server lacks as `Home`). Saving a library writes `viewing.startupDestination: {"library":"<id>"}`. *Proven by `startup-library`.* A saved library missing from a list that is not loaded yet shows as `Saved library` (source-only).
- **Leave guard.** With a dirty Viewing draft, pressing `Appearance` opens `{ role: "dialog", name: "Leave without saving?" }`; `Keep editing` stays on `/settings/viewing`. *Proven by `account-settings`.*
- **Appearance.** `ctx.choose({ name: "Accent" }, "Cobalt")` and `Save` set `data-accent="cobalt"` on `<html>` at once and `appearance.accent: "cobalt"` in `accounts.json`; `Restore defaults` and `Save` drop the key again. *Proven by `account-settings` (`02-appearance-cobalt`).* Other handles: `Density`, switches `Show pop-out previews on cards`, `Show media info on cards`, `Reduce motion`, spinbutton-and-slider pairs `Card preview delay` (saved as `viewing.previewDelayMs`, disabled while pop-out previews are off), `Artwork intensity`, `Backdrop intensity`. Appearance's `Restore defaults` also resets the preview delay to 550 ms; Viewing's leaves it alone.
- **Letterboxd.** The input is `{ role: "textbox", name: "Letterboxd username or profile URL" }` (`#letterboxd-profile`; `Username or profile URL` is only its placeholder); `Add profile` queues it, with a `Cancel adding <input>` button until Save, and `Save` verifies it by fetching `https://letterboxd.com/<user>/rss/` and writes `letterboxd_profiles[]` (`davidehrlich` came back `verified`, `https://letterboxd.com/davidehrlich/`). The saved profile has `Enable <name>`, `Refresh profile`, `Open profile` and `Remove profile`; `Remove profile` only marks the row (`Will be removed when you save`, with `Undo removal of <name>`, which restores it); `Save` then empties the list. *Proven by `account-settings` (`03-letterboxd-saved`)*; `Refresh profile` and `Open profile` are never pressed.

## Gotchas

- Appearance is listed under Account and stored per account in `accounts.json`, not in `settings.json`; keys at their default are left out.
- A save writes `accounts.json` before the save bar settles. A drive that ends right after a save makes teardown hit `Leave settings?`/`Saving settings…` and stop the app by force; wait until `Save` is disabled first.
- A Sonner toast after `Save` sits where the pointer was and pauses while hovered; `press` now moves the pointer away when a toast covers its target.
- Letterboxd `Save` and `Refresh profile` call letterboxd.com; offline, the profile is still saved as `unverified`. `Refresh profile` takes effect at once, outside the draft. Never press `Open profile` (it opens the user's browser).
- A list or review URL is rejected on Save with a toast and `Could not save all changes…` (source-only).
