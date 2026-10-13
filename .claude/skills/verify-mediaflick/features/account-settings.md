# Account settings and integrations

The Account group in Settings (`Viewing`, `Home`, `Appearance`, `Collections`) and Integrations → `Letterboxd` hold the signed-in account's own preferences, stored locally in `accounts.json` under that account's server and user IDs. Every shelf edits a draft with `Save`, `Discard` and `Restore defaults`, and leaving a dirty shelf asks first, as in [Client settings](./client-settings.md). Home's shelves are covered in [Home](./home.md) and Collections in [Collections](./collections.md).

## Sub-features

- `viewing`: Spoiler protection, Next episode (with Countdown seconds), Episode limit, Resume rewind, audio and subtitle languages, Prefer original audio, Subtitles, Text size, Poster width, Startup destination, Remember library filters, Hide watched by default.
- `appearance`: Accent, Density, card previews and their delay, media info on cards, rating sources (see [Companion](./companion.md)), artwork and backdrop intensity, Reduce motion.
- `letterboxd`: add a public profile by username or URL, enable or disable it, refresh, open, remove; up to 16 profiles. Linked profiles add a `Letterboxd` section with reviews to movie pages.

## How to get to it (user POV)

Signed in: sidebar `Settings` → `Settings navigation` → `Viewing`, `Appearance` or `Letterboxd`. Signed out, these links are `aria-disabled` and the shelves ask to sign in.

## Driving it with ctx (just verify)

Preconditions: signed in; `just verify account-settings --account SERVER`. All writes go to the disposable profile's `accounts.json`.

- **Viewing.** `{ role: "switch", name: "Hide watched by default" }` toggled and `Discard` puts it back without touching `accounts.json` (same mtime). Toggled again, with `{ role: "textbox", name: "Audio languages" }` set to `EN, de`, `Save` writes `viewing.hideWatched: true` and `viewing.audioLanguages: ["en","de"]` (lower-cased). Leaving for another shelf and coming back shows the switch still on. `Restore defaults` and `Save` put it back. Other handles: switches `Spoiler protection`, `Prefer original audio`, `Remember library filters`; comboboxes `Next episode`, `Resume rewind`, `Subtitles`, `Text size percent`, `Poster width`, `Startup destination`; spinbuttons `Countdown seconds` (enabled only with `Ask with countdown`) and `Episode limit`; textbox `Subtitle languages`. *Viewing proven by `account-settings` (`01-viewing-saved`).*
- **Leave guard.** With a dirty Viewing draft, pressing `Appearance` opens `{ role: "dialog", name: "Leave without saving?" }`; `Keep editing` stays on `/settings/viewing`. *Proven by `account-settings`.*
- **Appearance.** `ctx.choose({ name: "Accent" }, "Cobalt")` and `Save` set `data-accent="cobalt"` on `<html>` at once and `appearance.accent: "cobalt"` in `accounts.json`; `Restore defaults` and `Save` drop the key again. *Proven by `account-settings` (`02-appearance-cobalt`).* Other handles: `Density`, switches `Show pop-out previews on cards`, `Show media info on cards`, `Reduce motion`, number fields `Card preview delay` (saved as `viewing.previewDelayMs`), `Artwork intensity`, `Backdrop intensity`.
- **Letterboxd.** The input is `#letterboxd-profile` (`Username or profile URL`); `Add profile` queues it and `Save` verifies it by fetching `https://letterboxd.com/<user>/rss/` and writes `letterboxd_profiles[]` (`davidehrlich` came back `verified`, `https://letterboxd.com/davidehrlich/`). The saved profile has `Enable <name>`, `Refresh profile`, `Open profile` and `Remove profile`; `Remove profile` and `Save` empty the list. *Proven by `account-settings` (`03-letterboxd-saved`).*

## Gotchas

- Appearance is listed under Account and stored per account in `accounts.json`, not in `settings.json`; keys at their default are left out.
- A save writes `accounts.json` before the save bar settles. A drive that ends right after a save makes teardown hit `Leave settings?`/`Saving settings…` and stop the app by force; wait until `Save` is disabled first.
- A Sonner toast after `Save` sits where the pointer was and pauses while hovered; `press` now moves the pointer away when a toast covers its target.
- Letterboxd `Save` and `Refresh profile` call letterboxd.com; offline, the profile is still saved as `unverified`. `Refresh profile` takes effect at once, outside the draft. Never press `Open profile` (it opens the user's browser).
- A list or review URL is rejected on Save with a toast and `Could not save all changes…` (source-only).
