# Discover and Requests

When the MediaFlick Companion provides Seerr, the sidebar gains Discover and Requests; both pages then need the Jellyfin account mapped to a Seerr user, and otherwise show `Your Seerr account is not mapped` (or `Seerr is unavailable`) with `Back to Home` (`SeerrGate`). Discover browses and searches Seerr's TMDB catalogue, marks titles the library already owns, and requests the rest; Requests lists the user's own requests with their status and cancels one while it awaits approval. Request buttons also appear on partly owned series and on Seerr sections of library search and person pages.

## Sub-features

- `discover-rows`: tabs `Trending`, `Popular movies`, `Popular series`, `Upcoming movies`, `Upcoming series`, loading more titles as you scroll.
- `discover-refine`: Trending's Format and Window, the Popular rows' genre strip, Sort, `Filter by release decade` and Minimum score, and a Library filter on every view. These depend on the Companion's `seerr-discovery-v2`…`v4` capabilities: without them Discover has three tabs and its Refine bar holds only the Library filter, and the decade filter needs `v4` (`ui/src/routes/Discover.tsx`).
- `discover-search`: a Seerr search (2 characters or more) with its own results, Format and Library filters, and `Clear search`.
- `discover-detail`: a title page with status badges, `Watch trailer`, `Open in library`, `Request options`, the external-links menu, Letterboxd reviews, Seasons, Cast, Crew, Release schedule and Details.
- `request-dialog`: seasons (with partial requests), `Request in 4K` (with permission), Download quality (with advanced requests), quota, `Cancel` and `Request`.
- `requests-list`: tabs `All`, `Awaiting approval`, `In progress`, `In your library`; badges; `Open in library`; `Cancel` on a pending request.

## How to get to it (user POV)

- Sidebar `Discover` (`/discover`) and `Requests` (`/requests`), shown only when the Companion is compatible and advertises `seerr`. Off Home, hover the sidebar first.
- A Discover card opens `/discover/movie|tv/<tmdbId>`; `Back to discovery` returns.
- A library series with seasons Seerr lacks: `Request season N` / `Request seasons` (see [Item detail](./item-detail.md)).

## Driving it with ctx (just verify)

Preconditions: your own server with the Companion and Seerr, `just verify discover-requests --account SERVER`, after the first sync (owned titles are matched against the local catalog). `ctx.api("/api/seerr/status")` must answer `linked: true`; its `capabilities` and `instance.partialRequestsEnabled` decide what the dialog shows.

- **Rows.** `{ role: "textbox", name: "Search Seerr" }` and the five tabs; `Trending` is selected first, under `{ role: "heading", name: "What everyone is watching" }`. The count reads `20 loaded / 1,000`, upper-cased by CSS, so read `textContent`, not `innerText`. *Proven by `discover-requests`.*
- **Genre.** `Popular movies` writes `row=movies`; a genre strip button (`Action`) turns `aria-pressed` true and adds `genre=` to the URL. *Proven by `discover-requests`.*
- **Owned title.** Searching `28 Days Later` shows `{ role: "heading", name: "Results for “28 Days Later”" }` and a card whose text includes `In your library`. Its title page has `Open in library`, and `Back to discovery` returns to `/discover`. *Proven by `discover-requests` (`03-search-owned`, `04-discover-owned-detail`).*
- **Request.** A card's quick `Request <title>` button is in the DOM but transparent and unclickable until the card is hovered or holds keyboard focus, and exists only when Seerr offers a request path: `ctx.hover` the card's link (`a[href^="/discover/movie/<tmdbId>"]`), then press the button. `{ role: "dialog", name: "Request <title> (<year>)" }` opens; while it is open the page is hidden, so `{ role: "button", name: "Request" }` is the dialog's. Success shows the toast `Requested` (`Approved` with auto-approve), and `/api/seerr/requests?filter=all` holds the request with `status: "pending"`. *Proven by `discover-requests` (request 457, `05-request-dialog`).*
- **Cancel.** `Requests` → `Awaiting approval` lists it in a `li`; its `Cancel` (one per pending request, so pick the one inside that `li`) shows `Request cancelled`, and the request leaves `/api/seerr/requests`. The drive registers `ctx.cancelSeerrRequestAfterRun(id)` first, so a failure in between still cancels it after the app exits. *Proven by `discover-requests` (`06-requests-pending`, `07-requests-after-cancel`).*
- **Read-back endpoints.** `/api/seerr/requests?filter=all|pending|approved|processing|available|failed&take=40`, `/api/seerr/media/movie/<tmdbId>`, `/api/seerr/discover/<row>?page=N` (rows `trending`, `movies`, `tv`, `upcoming-movies`, `upcoming-tv`; `Popular series` writes `row=tv`, and an unknown row answers 404), `/api/seerr/search?q=&page=`, `/api/seerr/genres/movie|tv`, `/api/seerr/request-options/<movie|tv>?is4k=`. Without `take`, requests default to 20.

## Gotchas

- A request writes to the server owner's Seerr and can notify its admin before it is cancelled. Only `discover-requests` submits, and only a movie with Seerr status `unknown` that nobody requested.
- `Cancel` exists only while a request is pending; an admin approval in between leaves it in Seerr, and the after-run cancel then fails the run with the request id.
- Owned titles show `In your library` only once the catalog is synced; during the first sync the card shows Seerr's own badge (`Available`).
- While the page is in use, feeds and searches reload only after a request or cancel; an unused feed is dropped after 10 minutes. The Requests list refreshes every 30 s and on window focus. Seerr status is cached for 5 minutes.
- Do not press `Watch trailer`, `Open website` (Details, under `Official site`) or the hero's external-links menu items: they open the user's browser.
- `/discover/:type/:id` is not gated by Seerr; without it the page falls back to a collection snapshot or an error.
