# Item detail

A title's detail page shows its artwork, metadata, cast and actions. Movies offer Play or Resume, From start, the watched toggle, favorites and a streaming-quality picker; series add a season rail and an episode list whose primary action plays the next episode.

## Sub-features

- `detail-movie` shows a movie's title, actions, quality picker and cast.
- `detail-series` shows a series with its `Seasons` rail, `Episodes` and `Play SxEy` / `Resume SxEy`.
- `detail-favorite` toggles favorites (`Add to favorites` / `Remove from favorites`, `aria-pressed`).
- `detail-watched` toggles watched (`Mark watched` / `Watched`).
- `detail-episode` opens an episode from the list.

## How to get to it (user POV)

- A poster card anywhere (Home shelves, library grid, search results), named `Open details for <title>`.
- The billboard's `Details` link on Home.
- Breadcrumb links on season and episode pages back to the series.

## Driving it with ctx (just verify)

Preconditions:

- Signed in to the demo server with `ctx.signIn`, then reach the title through search (see library-search).

- **Movie.** After opening `Night of the Living Dead`: `ctx.pathname()` starts with `/item/`, `{ role: "heading", name: "Night of the Living Dead" }` exists (screen-reader-only when a logo is shown), `Play` or `Resume` is present, the favorite toggle reads `Add to favorites` or `Remove from favorites`, and `{ role: "combobox", name: "Streaming quality" }` appears. *Proven by `browse`.*
- **Series.** After opening `Pioneer One`: `{ role: "heading", name: "Pioneer One" }`, `{ role: "list", name: "Seasons" }` with season buttons (`aria-pressed` marks the selected one), `{ role: "heading", name: "Episodes" }`, and a button matching `(Play|Resume) S<n>E<n>` (no zero padding). *Proven by `browse`.*
- **Favorite and watched.** `ctx.press({ role: "button", name: "Add to favorites" })` flips the name and `aria-pressed`; `Mark watched` becomes `Watched`. Only on a server you own; not yet driven.
- **Episode.** Episode image links are named by the episode name; title links read `<n>. <name>`. Not yet driven.
- **Proof.** `03-movie-detail` and `04-series-detail` screenshots and snapshots from `browse`.

## Gotchas

- Other demo users leave progress, so the primary button is `Play` or `Resume` depending on the day; accept both.
- Never press `Play`, `Resume`, `From start`, `Play SxEy` or an episode's play button: playback starts mpv with real audio, and `just build` does not stage libmpv.
- Never toggle favorites or watched on the shared demo account; the change is visible to everyone.
- `More info` opens a menu of external links (IMDb, TMDB, …). Its items open the user's default browser on the visible desktop; never press them. Opening the menu itself is safe.
- The quality picker is absent on series and season pages.
