# Home

Home opens after sign-in: a rotating billboard of up to five random movies and series with backdrop or thumb art (chosen once per session; `Featured`, `Continue watching` and `Watch it again` are only its kicker text), then horizontal shelves of poster cards that open the detail page. The shelves follow the account's Home settings, stored locally in `accounts.json`. The default order is Watching (or Continue Watching and Next Up), Because you watched <title> (settings checkbox `Show Because You Watched`), Recently Added Movies, Recently Added Shows, Releases, Latest Movies, Latest Shows, Favorites, then genre shelves, ordered by a preferred list (Action, Comedy, Drama, Science Fiction, Thriller, Documentary, Animation, …) and then the server's other genres; the first six are on, the rest off, and genres that appear later are added off (`src/preferences/accounts.rs`). Releases is on by default and renders when the release calendar has entries in its window (30 days back to 90 ahead): the Companion's calendar where it advertises `calendar`, otherwise Jellyfin's own upcoming episodes (`calendar` and `fallback_calendar` in `src/companion/mod.rs`), so it can appear on a server without the Companion. In MediaFlick collection mode a My Collections entry can become its own shelf: its `Available on Home` switch (off by default) offers it in Home settings, where `Show <collection>` turns it on; the shelf lists only owned titles and its `All` opens `/collections/mine/<id>`. Jellyfin BoxSets never become shelves. `Because you watched <title>` needs a played movie with a genre; its seed is picked once per account per run. A shelf with no items is not rendered.

## Sub-features

- `home-billboard` shows a featured title with Play/Resume, Details and the favorites toggle, and slide ticks when there are two or more titles.
- `home-shelves` shows each enabled shelf with previous/next arrows and, for most shelves, an `All` link.
- `home-card` opens a title's detail page from its poster card; resting the pointer on a card opens a preview panel.
- `home-empty` shows `No titles available` (with `Browse movies` and `Browse series`), `No shelves enabled` (with `Configure Home`), or `Could not load your home page` (with `Try again`) instead of shelves.

## How to get to it (user POV)

- Sign in: Home is the first page.
- Sidebar `Home`, or the `MediaFlick` brand link (named only while the sidebar is expanded).

## Driving it with ctx (just verify)

Preconditions:

- Signed in to the demo server with `ctx.signIn`.

- **Arrive.** `ctx.pathname()` is `/` right after sign-in, and the sidebar is pinned open: `{ role: "link", name: "MediaFlick" }` is named. *Proven by `browse`, `home-library`.*
- **Shelves.** Each shelf is a `section` labelled by its `h2` plus an inner scroller with `role="region"` and the same name, so a shelf name matches two regions and the billboard adds one more. Read shelf titles from `section[aria-labelledby] h2`, and wait for poster cards with `ctx.count({ css: 'a[aria-label^="Open details for"]' }) > 0`. *Proven by `browse`, `home-library`.*
- **Open a card.** Park the pointer, take a card's `aria-label` from `main a[aria-label^="Open details for"]`, and `ctx.press({ role: "link", name }, { nth: 0 })` (the same title can sit on several shelves). `ctx.pathname()` starts with `/item/`. *Proven by `home-library`.*
- **Billboard.** Its title is `h1#billboard-title` (screen-reader-only once a logo loads). `Details` is a link; the favorites toggle is `Add to favorites` / `Remove from favorites` with `aria-pressed`. Slide ticks are buttons named `Show <title>`, with `aria-current` on the shown one; pressing one changes the `h1` text. *Proven by `home-library`.*
- **Shelf arrows.** Buttons `Previous <shelf title>` and `Next <shelf title>`, disabled at the edges and shown on hover. Hover `Next`, press it, and `Previous` enables. *Proven by `home-library`.*
- **All links.** An `All` link sits in the shelf's `section`, named by its text. Watching, Continue Watching, Next Up, Recently Added Shows and Because you watched have none. *Observed by `home-library`*; scope the lookup to the section. Releases' `All` opens `/calendar`; genre shelves open `/library?…&genre=`.
- **Releases shelf (Companion).** On a server with the Companion's calendar, the `Releases` section holds up to 24 cards around a `{ role: "separator", name: "Today" }`, past ones first (about 30 %). Cards are named `Open <title>[, S01E02][, Downloaded|Missing]`; a season premiere, past or upcoming, drops its episode code and shows `NEW SEASON`. A failed calendar request hides the shelf without an error. *Proven by `companion` against Jellyfin 12.2 (24 cards, `04-home-releases.png`).*
- **Rating badges (Companion).** With rating sources picked in Appearance, every card carries `dl[aria-label="Ratings for <title>"]`; with none picked (the default), nothing renders. *Proven by `companion` on library cards (see [Companion](./companion.md)).*
- **Billboard trailers.** A billboard title with a trailer plays it muted behind the slide after a moment: a local Jellyfin trailer, or a YouTube-nocookie embed (`data-trailer-source=local|youtube` on the hidden stage). Reduced motion turns this off. A playing trailer changes how long a slide stays up. Source and `cef.log` (YouTube embed messages) on the real server; not asserted.
- **Empty states.** Home settings are local (`accounts.json`), so a drive may change them. In Settings → `Home` (the second `Home` link, in `Settings navigation`), uncheck `Show billboard` and every `Show <shelf>` except `Show Releases` and Save: Home shows `{ role: "heading", name: "No titles available" }` with links `Browse movies` and `Browse series` (Releases renders nothing there because the demo server has no upcoming episodes for Jellyfin's fallback calendar; if it ever gets some, the shelf renders and this state is no longer reached). Uncheck `Show Releases` too: `No shelves enabled`, whose `Configure Home` link opens `/settings/home`. End with `Restore defaults` and Save. *Proven by `home-settings`.* `Because you watched <title>`'s checkbox is `Show Because You Watched` (observed in a maintenance run). The Watching sub-options are checkboxes without the `Show` prefix: `Continue Watching`, `Next Up`, `Combine shelves` (source-only). `Could not load your home page` (`Try again`) needs a failed home request with no cached data; not verified.
- **Proof.** `01-home.png` and `01-home.ax.txt` show the shelves; `02-billboard.png` and `03-shelf-scrolled.png` show the billboard tick and arrows; `02-no-titles.png` and `03-no-shelves.png` from `home-settings` show the empty states.

## Gotchas

- Shelf titles and contents on the demo server change with other users' activity (`Because you watched …` comes and goes); assert structure (sections, cards), not specific titles, unless the title is static library content.
- On Home the sidebar is pinned open and pushes content aside. Everywhere else it collapses to icons and overlays content when hovered. Leaving Home therefore slides the content left; `ctx.press` waits for the target to hold still.
- Resting the pointer on a poster card opens a portalled preview panel after a delay (550 ms by default, configurable, or off). The panel adds a second `Open details for <title>` link; park the pointer before name lookups and screenshots.
- Releases shelf cards are named `Open <title>[, S01E02][, Downloaded|Missing]`, not `Open details for …`, and open `/calendar` for titles not in the library.
- Never press the billboard's `Play`/`Resume` (audio), or the favorites toggle on the shared demo account.
