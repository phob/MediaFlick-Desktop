# Library and search

The library shows a virtualized grid of poster cards for Movies, Series or Favorites, with sort and filter controls. Typing in the sidebar search shows matching titles (and people) in the same grid.

## Sub-features

- `library-kind` opens Movies, Series or Favorites from the sidebar.
- `library-sort` sorts by name, year, recently added or rating.
- `library-filter` filters by genre, release decade, watch status and My List, with removable chips.
- `library-search` searches from the sidebar after a short debounce.
- `library-empty` shows `Nothing to show` with a reason.

## How to get to it (user POV)

- Sidebar `Movies` (`/library?kind=Movie`), `Series` (`/library?kind=Series`), `Favorites` (`/library?favorite=true`).
- Sidebar search field `Search the library` (expanded sidebar) or the `Search` icon link (collapsed sidebar).
- A shelf's `All` link on Home.

## Driving it with ctx (just verify)

Preconditions:

- Signed in to the demo server with `ctx.signIn`.

- **Movies.** `ctx.press({ role: "link", name: "Movies" })`. `ctx.pathname()` is `/library?kind=Movie`, `{ role: "heading", name: "Movies" }` and `{ role: "combobox", name: "Sort by" }` appear, and cards `a[aria-label^="Open details for"]` fill the grid. *Proven by `browse`.*
- **Search.** `ctx.fill({ role: "textbox", name: "Search the library" }, "night of the living")`. After about 200 ms `ctx.pathname()` is `/library?search=night%20of%20the%20living` and `{ role: "heading", name: "Results for “night of the living”" }` appears. *Proven by `browse`.*
- **Search off Home.** Off Home the sidebar is collapsed to icons and has no search field: `ctx.hover({ css: '[data-sidebar="sidebar"]' })` expands it, then fill as above. *Proven by `browse`.*
- **Open a result.** Wait for the search route first, then `ctx.press({ role: "link", name: "Open details for Night of the Living Dead" })`. *Proven by `browse`.*
- **Sort.** `ctx.choose({ name: "Sort by" }, "Sort: Year")`; the URL gains `sort=year` (`name`, `year`, `added`, `rating`). Not yet driven.
- **Filters.** `{ role: "button", name: "Filters" }` (name gains `, N active`) opens the `Library filters` menu with sub-menus `Genre`, `Release decade`, `Watch status`, `My List` and `Clear all filters`. Chips sit in `{ role: "group", name: "Active filters" }`, each a button `Remove <label> filter`. The count is an `aria-live` span reading `N items`. Not yet driven.
- **Proof.** `02-movies.png` and the search steps in `browse`'s `steps.log`.

## Gotchas

- The search commits with `navigate(..., { replace: true })` 200 ms after typing stops. Pressing a card before that is overwritten by the search navigation; wait for the `/library?search=` route.
- The grid is virtualized: only visible rows exist in the DOM. Search for a title instead of scrolling for it.
- `Night of the Living Dead` also appears in the Movies grid; a press before the search commits can open it from the wrong page.
- Typing focuses the sidebar, which keeps it expanded over the grid. `ctx.press` waits, then parks the pointer if a card is still covered.
- The library defaults to `kind=Movie` when no `kind`, `search` or `favorite` is given.
