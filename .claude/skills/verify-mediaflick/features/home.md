# Home

Home opens after sign-in: a rotating billboard of featured or in-progress titles, then horizontal shelves (Watching or Continue Watching and Next Up, Recently added, latest movies and shows, My List, genres, collections) of poster cards that open the detail page.

## Sub-features

- `home-billboard` shows a featured title with Play/Resume, Details and the My List toggle, and slide ticks.
- `home-shelves` shows each enabled shelf as a scrollable region with previous/next arrows and an `All` link.
- `home-card` opens a title's detail page from its poster card.
- `home-empty` shows `No titles available` or `No shelves enabled` instead of shelves.

## How to get to it (user POV)

- Sign in: Home is the first page.
- Sidebar `Home`, or the `MediaFlick` brand link.

## Driving it with ctx (just verify)

Preconditions:

- Signed in to the demo server with `ctx.signIn`.

- **Arrive.** `ctx.pathname()` is `/` right after sign-in. *Proven by `browse`.*
- **Shelves.** Each shelf is a `region` named by its `h2` title; wait for `ctx.count({ role: "region" }) > 0` and for poster cards `ctx.count({ css: 'a[aria-label^="Open details for"]' }) > 0`. *Proven by `browse`.*
- **Open a card.** `ctx.press({ role: "link", name: "Open details for <title>" })`; `ctx.pathname()` starts with `/item/`. Proven on search results (see library-search), not yet from Home.
- **Billboard.** `region`/`section` labelled by `h1#billboard-title`; slide ticks are buttons named `Show <title>` with `aria-current`; `Details` is a link; the My List toggle is `Add to My List` / `Remove from My List` with `aria-pressed`. Not yet driven.
- **Shelf controls.** Arrows are buttons `Previous <shelf title>` and `Next <shelf title>`, disabled at the edges and invisible until hovered. Each `All` link is unnamed beyond its text, so scope it to the shelf. Not yet driven.
- **Proof.** `01-home.png` and `01-home.ax.txt` from `browse` show the shelves as regions with their cards.

## Gotchas

- Shelf titles and contents on the demo server change with other users' activity; assert structure (regions, cards), not specific titles, unless the title is static library content.
- The sidebar is expanded on Home and overlays content near it; `ctx.press` parks the pointer when a target is covered, which collapses the sidebar elsewhere.
- Hovering a poster card opens an expanded preview card (portalled) after a delay; park the pointer before screenshots that should not show it.
- Never press the billboard's `Play`/`Resume` (audio), or the My List toggle on the shared demo account.
