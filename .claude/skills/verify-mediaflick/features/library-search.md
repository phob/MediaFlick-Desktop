# Library and search

The library shows a virtualized grid of poster cards for Movies, Series or Favorites, with sort and filter controls. Typing in the sidebar search shows matching titles in the same grid; an exact person match adds a "With <name>" section below it.

## Sub-features

- `library-kind` opens Movies, Series or Favorites from the sidebar.
- `library-sort` sorts by name, year, recently added or rating.
- `library-filter` filters by genre, release decade, watch status and favorites, with removable chips.
- `library-search` searches from the sidebar after a short debounce.
- `library-empty` shows `Nothing to show` with a reason.

## How to get to it (user POV)

- Sidebar `Movies` (`/library?kind=Movie`), `Series` (`/library?kind=Series`), `Favorites` (`/library?favorite=true`).
- Sidebar search field `Search the library`. Off Home, pointing at the collapsed sidebar expands it to show the field.
- A shelf's `All` link on Home, and genre badges on detail pages.

## Driving it with ctx (just verify)

Preconditions:

- Signed in to the demo server with `ctx.signIn`.

- **Movies.** `ctx.press({ role: "link", name: "Movies" })`. `ctx.pathname()` is `/library?kind=Movie`, `{ role: "heading", name: "Movies" }` and `{ role: "combobox", name: "Sort by" }` (value `Sort: Name`) appear, and cards `a[aria-label^="Open details for"]` fill the grid. The count is an `aria-live` span reading `N items` or `1 item`. *Proven by `browse`, `home-library`.*
- **Search.** `ctx.fill({ role: "textbox", name: "Search the library" }, "night of the living")`. After about 200 ms `ctx.pathname()` is `/library?search=night%20of%20the%20living` and `{ role: "heading", name: "Results for “night of the living”" }` appears. Enter commits at once. *Proven by `browse`.* A term under 2 characters never commits, and clearing the field (or shortening it below 2 characters) leaves a search at once for `/library`, the Movies view. *Proven by `library-filters`.*
- **Search off Home.** Off Home the sidebar is collapsed to icons and has no search field: `ctx.hover({ css: '[data-sidebar="sidebar"]' })` expands it, then fill as above. *Proven by `browse`, `home-library`.*
- **Open a result.** Wait for the search route first, then `ctx.press({ role: "link", name: "Open details for Night of the Living Dead" })`. *Proven by `browse`, `item-detail`.*
- **No match.** Searching `zzqqxx` shows `Nothing to show` and `Nothing matches “zzqqxx”.` *Proven by `home-library`.*
- **Series.** Off Home, hover the sidebar and press `{ role: "link", name: "Series" }`: `/library?kind=Series` with `{ role: "heading", name: "Series" }`. *Proven by `library-filters`.*
- **Sort.** `ctx.choose({ name: "Sort by" }, "Sort: Year")`; the URL gains `sort=year` and `filters=true` as a new history entry (`name`, `year`, `added`, `rating`). *Proven by `home-library` (`Sort: Year`) and `library-filters` (`Sort: Rating`).*
- **Filters.** `{ role: "button", name: "Filters" }` (name gains `, N active`) opens a menu whose accessible name is the trigger's current name (`Filters`, or `Filters, 1 active` once a filter is on), with the visible label `Filter library`. Its sub-menu triggers are menuitems named with their current value: `Genre Any`, `Release decade Any`, `Watch status Any watch status`, `Favorites Any`, then `Clear all filters`. Options are `menuitemradio`s: `Any genre` then the server's genres (`genre=`), `Any decade` then `2020s` … `1900s` (`decade=1960`), `Any watch status`, `Unwatched`, `Watched` (`watched=false|true`), `All titles`, `Favorites only`. Picking an option closes the whole menu; reopen `Filters` for the next one. *Proven by `home-library` (Release decade) and `library-filters` (Genre, Watch status).*
- **Chips.** Active filters sit in `{ role: "group", name: "Active filters" }`, each a button `Remove <label> filter` (`Remove Released: 1960s filter`, `Remove Genre: Action filter`, `Remove Unwatched filter`, `Remove Favorites filter`), followed by `Clear all`. Removing the last chip, or `Clear all`, removes the group and keeps the sort. *Proven by `home-library`, `library-filters`.*
- **Favorites.** `ctx.press({ role: "link", name: "Favorites" })` lands on `/library?favorite=true` with `{ role: "heading", name: "Favorites" }`, `Filters, 1 active` and the chip `Remove Favorites filter`. *Proven by `home-library`.*
- **Proof.** `05-filtered.png`, `06-empty-search.png` and `07-favorites.png` from `home-library`, `01-series.png`, `05-two-filters.png` and `library-filters.json` from `library-filters`, and the search steps in `browse`'s `steps.log`.

## Gotchas

- The search commits with `navigate(..., { replace: true })` 200 ms after typing stops. Pressing a card before that is overwritten by the search navigation; wait for the exact `/library?search=` route.
- The grid is virtualized: only visible rows exist in the DOM. Search for a title instead of scrolling for it.
- `Night of the Living Dead` also appears in the Movies grid; a press before the search commits can open it from the wrong page.
- The sidebar stays expanded over the grid while the pointer rests on it, which it does after `ctx.fill` into the search field; focus alone does not hold it open. `ctx.press` waits, then parks the pointer if a card is still covered.
- The collapsed sidebar's `Search` icon is a plain link to `/library` (the Movies view), and an invisible group label covers it. Pointing at the sidebar expands it into the search field first, so the field is the pointer path.
- People never appear in the grid. An exact Jellyfin person match adds a `Titles with <name>` section (headed `With <name>`) below it; with a linked Companion, a `Not in your library` section follows. Source-only; the demo server has no Companion.
- The library defaults to `kind=Movie` when no `kind`, `search` or `favorite` is given. Plain Movies and Series views can also apply remembered filters or "hide watched" without showing them in the URL; both preferences are off in a fresh profile.
