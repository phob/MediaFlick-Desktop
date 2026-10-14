# Collections

Collections group titles in one of two modes. In MediaFlick mode the sidebar shows `Movie Franchises` (TMDB collections matched against the library, with owned and missing titles) and `My Collections` (per-account collections built from TMDB or MDBList templates). In Jellyfin mode it shows only `Jellyfin Collections`, the server's BoxSets. The mode and My Collections are stored per account in `collections.json`, custom posters in `custom-art/`; franchise and collection results are rebuildable snapshots in `library.db`.

## Sub-features

- `mode-sidebar`: the sidebar entries follow the effective mode; `/collections` and the other mode's routes redirect to the active one.
- `franchises`: the franchise grid (`Finding movie franchises...`, with three ASCII dots, while the first run works), and a franchise page with `Owned` and `Missing` sections, `Show all N` and `Choose edition`.
- `my-collections`: the cards with an `Actions for <title>` menu (`Edit`, `Check for updates`), the empty state with `Choose templates`, and a collection's page.
- `jellyfin-collections`: the BoxSet grid and a BoxSet's `Items`.
- `collections-settings`: Settings → `Collections` with `Collection mode`, `Include unreleased titles`, reorder and delete with `Undo`, the template catalog and the `Add collection` / `Edit collection` dialog (preview, custom poster, `Available on Home`).

## How to get to it (user POV)

- Sidebar `Movie Franchises`, `My Collections` or `Jellyfin Collections`, then a card `Open <name>`.
- Settings → `Settings navigation` → Account → `Collections`; also `My Collections` → `Choose templates`, or a card's menu → `Edit`.

## Driving it with ctx (just verify)

Preconditions: your own server with the Companion (`collection-experience-v1` and TMDB), `just verify collections --account SERVER`, after the first sync.

- **Mode.** With nothing saved, `ctx.api("/api/collections/settings")` reads `effectiveMode: "mediaFlick"`, `modeSelection: null`, `readiness: { tmdb: true, mdblist: true }`, and the sidebar has `Movie Franchises` and `My Collections` but no `Jellyfin Collections`. *Proven by `collections`.*
- **Franchises.** Sidebar `Movie Franchises` opens `/collections/franchises`. Poll `ctx.api("/api/collections/franchises?localDate=YYYY-MM-DD")` until `status` is `ready`; the grid then has one `Open <name>` card per franchise (116 on the test server). A franchise page has `{ role: "heading", name: "Owned" }` and, when titles are missing, `Missing`. *Proven by `collections` (`01-franchises`, `02-franchise-detail`).*
- **My Collections.** A fresh profile shows `{ role: "heading", name: "No collections yet" }` and `{ role: "link", name: "Choose templates" }`, which opens the template catalog in Settings with `{ role: "textbox", name: "Search templates" }`. Template buttons are named by all their text (`Trending todayTMDB`), so mark one with `ctx.app.evaluate` and press it by css. `{ role: "dialog", name: "Add collection" }` opens; its first `Save` runs the preview (`N total · N movies · N series`), the second creates the collection: `collections.json` gains the profile (2.8 s on the test server) and `My Collections` shows `Open <title>`, which opens `/collections/mine/<id>`. *Proven by `collections` (`03-collection-preview`, `04-my-collection`).*
- **Jellyfin mode.** In Settings → `Collections`, `ctx.choose({ name: "Collection mode" }, "Jellyfin")` and `Save` write `modeSelection: "jellyfin"`; the sidebar swaps to `Jellyfin Collections`, whose grid shows one card per BoxSet (85 of 85 rendered). `/collections/franchises` then redirects to `/collections/jellyfin`. *Proven by `collections` (`05-jellyfin-collections`).*
- **Back to defaults.** `Delete <title>`, `Collection mode` → `MediaFlick`, `Save`: `collections.json` holds no profile and no `jellyfin` mode. *Proven by `collections`.*
- **Custom poster.** `Choose poster` opens a native file dialog; set `input[aria-label="Custom poster file"]` with CDP `DOM.setFileInputFiles` instead (PNG, JPEG or WebP up to 10 MB). Source-only.

## Gotchas

- `Collections` in Settings navigation and the sidebar's collection links are different elements; inside Settings, `Collections` is the only link with that name, but `Home` exists twice (sidebar and shelf).
- The first franchise visit can sit on `Finding movie franchises...` for several seconds while the scheduler runs; poll the API, not the text.
- Once, creating a collection stayed on `Saving…` for over 60 s with nothing in `app.log`, then saved; the next runs took under 3 s. The drive allows 240 s and logs the time it took.
- Preview, Save and `Check for updates` read TMDB or MDBList through the Companion and use the server's provider quota; they never write to Jellyfin.
- Never press `Request <title>` on a Missing card: it opens a Seerr request.
- While the `Add collection` dialog is open the page behind it is hidden, so `Save` resolves to the dialog's button.
