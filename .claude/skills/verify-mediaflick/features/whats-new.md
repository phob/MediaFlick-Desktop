# What's new after a 0.1.6 update

A device that ran MediaFlick 0.1.6 (its config dir holds 0.1.6's `config.json`) shows a one-time `What's new since MediaFlick 0.1.6` dialog after sign-in: MediaFlick's own interface, signing in again, the server dashboard in the browser, MPC-HC's removal, the Jellyfin versions, and music, audiobooks and books, with links to the matching settings pages. A Jellyfin administrator whose server has no MediaFlick Companion also gets `Add MediaFlick Companion` with a `Companion install guide` link to `https://flick.media/companion/`; a non-administrator gets one line saying their server administrator can add it. Closing the dialog in any way writes `whats_new_seen: true` to `settings.json`; a fresh profile never shows it. Settings → MediaFlick Companion shows an `Install` row with the same guide link whenever the server has no Companion.

## Sub-features

- `whats-new-dialog`: the dialog, its entries and settings links, `Got it`, the close button, Escape, and following a link, each of which records it as seen.
- `companion-offer`: the administrator-only offer inside the dialog, and the one line for everyone else.
- `companion-install-row`: the `Install` row on Settings → MediaFlick Companion, with wording for administrators and for everyone else.

## How to get to it (user POV)

- Start a 0.1.6 profile and sign in; the dialog opens over Home.
- Settings → `Settings navigation` → Integrations → `MediaFlick Companion`, on a server without the Companion.

## Driving it with ctx (just verify)

Preconditions: a server without the Companion. The demo server's `demo` is a non-administrator; an administrator needs your own server, read from `MEDIAFLICK_VERIFY_WHATS_NEW_SERVER`, `_USER` and `_PASSWORD`.

- **Fresh profile.** `just verify whats-new` signs in and opens Companion settings: no dialog, `/api/settings/whats-new` answers `pending: false`, and `settings.json` records nothing. *Proven by `whats-new` (`01-companion-settings`).*
- **0.1.6 profile.** `--seed .claude/skills/verify-mediaflick/seeds/upgrade-0-1-6`. `{ role: "dialog", name: "What's new since MediaFlick 0.1.6" }` opens after sign-in, never on the sign-in screen. While it is open the rest of the page is hidden from the AX tree, so `ctx.signIn` (which waits for the sidebar) cannot be used; fill the form and wait for the dialog. The links inside are `Appearance settings`, `Player settings`, `Companion settings` and `Home settings`. After closing, `settings.json` holds `whats_new_seen: true`, a `ctx.relaunch()` shows no dialog, and `config.json` is unchanged. *Proven by `whats-new` (`01-whats-new`, `01b-whats-new-end`, `03-after-relaunch`).*
- **Offer.** `{ role: "heading", name: "Add MediaFlick Companion" }` and `{ role: "link", name: "Companion install guide" }` (read its `href`) for an administrator without the Companion; the text `Your server administrator can add MediaFlick Companion` otherwise. *Proven by `whats-new` against a local Jellyfin 12.4.0 administrator and the demo server.*
- **Install row.** `{ role: "link", name: "Companion install guide" }` on the Companion page, with `The guide shows how to add the plugin repository…` for an administrator and `Your server administrator can add MediaFlick Companion…` otherwise. *Proven by `whats-new` (`02-companion-settings`).*

## Gotchas

- Never press `Companion install guide`: it opens the user's default browser on the visible desktop.
- A relaunched session reads the administrator flag from Jellyfin a moment after start, so the Companion page's administrator wording can follow its first paint.
- With the Companion installed the dialog shows neither the offer nor the line; that case is not driven yet.
