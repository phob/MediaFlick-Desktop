# Item detail

A title's detail page (`/item/<id>`) shows its artwork, metadata, actions, cast, a Details panel and a Media panel. Movies and episodes offer Play or Resume, From start, the watched toggle, favorites and a streaming-quality picker. Series add a season rail and an episode list, and a primary `Play SxEy` / `Resume SxEy` for the Next Up episode. Seasons are not pages of their own: a season link opens its series with that season selected.

## Sub-features

- `detail-movie` shows a movie's title, actions, quality picker, cast, Details and Media.
- `detail-series` shows a series with its `Seasons` rail, `Episodes` and, when there is a Next Up episode, `Play SxEy` / `Resume SxEy` with a `Next up:` note.
- `detail-episode` opens an episode page, with breadcrumbs back to the series and its season.
- `detail-favorite` toggles favorites (`Add to favorites` / `Remove from favorites`, `aria-pressed`).
- `detail-watched` toggles watched (`Mark watched` / `Mark unwatched`).
- `detail-more-info` lists external links for the provider ids the title has: IMDb, TMDB, TVDB, Letterboxd (movies), Trakt, and a Rotten Tomatoes search (movies and series). Without any, the button is absent.

## How to get to it (user POV)

- A poster card anywhere (Home shelves, library grid, search results), named `Open details for <title>`.
- The billboard's `Details` link on Home.
- On an episode page, the breadcrumbs `<series>` › `Season N`, and `Back to library`.

## Driving it with ctx (just verify)

Preconditions:

- Signed in to the demo server with `ctx.signIn`, then reach the title through search (see library-search).

- **Movie.** After opening `Night of the Living Dead`: `ctx.pathname()` starts with `/item/`, `{ role: "heading", name: "Night of the Living Dead" }` exists (screen-reader-only when a logo is shown), `Play` or `Resume` is present (`From start` only with Resume), the favorite toggle reads `Add to favorites` or `Remove from favorites`, the watched toggle reads `Mark watched` or `Mark unwatched`, and `{ role: "combobox", name: "Streaming quality" }` appears. Headings `Cast`, `Details` and `Media` follow; cast and crew links are named `Find titles with <name>`. *Proven by `browse`, `item-detail`.*
- **More info.** `ctx.press({ role: "button", name: "More info" })` opens a menu of `View on <provider>` items, one per provider id the title has (`ui/src/lib/api/urls.ts`), plus `Search Rotten Tomatoes` on movies and series. Close it with `ctx.key("Escape", { code: "Escape", keyCode: 27 })`. On the demo server `Night of the Living Dead` lists `View on IMDb`, `View on TMDB`, `View on Letterboxd`, `View on Trakt` and `Search Rotten Tomatoes` (*proven by `item-detail`*); `Pioneer One` lists IMDb, TMDB, TVDB, Trakt and Rotten Tomatoes, and its `Earthfall` episode page has no `More info` button (observed in a maintenance run, not a shipped drive).
- **Series.** After opening `Pioneer One`: `{ role: "heading", name: "Pioneer One" }`, `{ role: "list", name: "Seasons" }` with season buttons (`aria-pressed` marks the selected one), `{ role: "heading", name: "Episodes" }`, and no quality picker. The primary `(Play|Resume) S<n>E<n>` (no zero padding) appears only when Next Up returns an episode, and it loads after the episode list; poll for it a few seconds and accept its absence. *Proven by `browse`, `item-detail`.*
- **Episodes.** Episode title links are named `<n>.<name>` with no space (`1.Earthfall`); the Next Up one adds ` (Next up)` (`2.The Man From Mars (Next up)`). Episode image links are named by the episode name. Each episode card has its own `Play`/`Resume` and `Mark watched`/`Mark unwatched` (`aria-pressed`) buttons, shown on hover. With `Card previews` turned off in Appearance settings, the cards switch to icon buttons named by `aria-label` that add `Add to favorites`/`Remove from favorites`. *Proven by `item-detail`.*
- **Episode page.** Pressing an episode title link opens `/item/<episodeId>` with the episode name as heading, its `S1E1` code, `{ role: "combobox", name: "Streaming quality" }`, and breadcrumb links `Pioneer One` and `Season 1`. `Season 1` links to the season (`/item/<seasonId>`), which redirects to `/item/<seriesId>?season=<seasonId>`, replacing the history entry. *Proven by `item-detail`.*
- **Another season.** Pressing a season button that is not pressed selects it. Not verified: Pioneer One has a single season on the demo server.
- **Favorite and watched.** `ctx.press({ role: "button", name: "Add to favorites" })` flips the name and `aria-pressed`; `Mark watched` becomes `Mark unwatched`. Only on a server you own; not verified.
- **Proof.** `03-movie-detail` and `04-series-detail` from `browse`; `01-movie`, `02-more-info`, `06-episode` and `07-season-breadcrumb` from `item-detail`.

## Gotchas

- Other demo users leave progress, so the primary button is `Play` or `Resume` depending on the day; accept both.
- Never press `Play`, `Resume`, `From start`, `Play SxEy`, or an episode card's `Play`/`Resume`. Playback starts real audio and video: on Linux through the system `libmpv.so.2`, the default Built-in player. The private display does not hide audio on Windows.
- Never toggle favorites or watched on the shared demo account, including the episode cards' watched buttons (and their favorites buttons when card previews are off); the change is visible to everyone.
- Never change the Media panel's `Media source`, `Audio track` or `Subtitle track` selects: each change saves a playback preference.
- Opening `More info` is safe; its items open the user's default browser on the visible desktop. Never press them.
- The quality picker is absent on series pages. A season link never shows a season page; it redirects to the series with `?season=`.
- Series with Seerr show `Request season N` / `Request seasons`, and in MediaFlick collection mode a movie in a TMDB collection shows a `Part of <collection>` link to `/collections/franchises/<id>`. Both need features the demo server lacks; source-only.
