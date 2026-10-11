# Library loading and the retry screen

After sign-in, Home waits for the first page of the library catalog. While it loads, the startup cover (logo and progress bar) stays up. If the page cannot be fetched, the cover gives way to a retry screen that says why in plain words, when MediaFlick tries again, and offers `Retry now` and `Sign out`. The same screen appears when MediaFlick starts with a stored session whose catalog never loaded, and the window is shown for it.

## Sub-features

- `bootstrap-loading` keeps the startup cover up until the first catalog page commits (`/api/status` `bootstrap.state` is `loading`).
- `bootstrap-retry-screen` shows the reason (`unreachable`, `server` or `storage`, worded in `ui/src/lib/sync-failure.ts`), `Trying again in N s.` or `Trying again now…`, `Retry now` and `Sign out` (`bootstrap.state` is `retrying`).
- `bootstrap-retry-now` asks the sync worker for an attempt at once (`POST /api/sync`). The screen stays, with `Retry now` disabled, until the attempt ends.
- `bootstrap-sign-out` signs out from the retry screen to the sign-in form.
- `bootstrap-relaunch` reveals the native window with the retry screen on a launch with a stored session and no catalog.

## How to get to it (user POV)

- Sign in to a server that accepts the sign-in but cannot deliver the library: the cover appears, then the retry screen.
- Quit and start MediaFlick again while the server is still failing: the window opens on the retry screen.
- From the retry screen: `Retry now`, or `Sign out`.

## Driving it with ctx (just verify)

Preconditions:

- A fresh profile from `just verify`. Internet access for the demo server.
- A failing server: `const proxy = await ctx.proxy({ upstream: "https://demo.jellyfin.org/stable", fail })` with `fail(url, method)` selecting the bootstrap's catalog pages, `url.pathname === "/Items"` with `SortBy=DateCreated` and `SortOrder=Ascending` (`fetch_items_page` in `src/jellyfin/api/items.rs`). Sign in to `proxy.url`. `proxy.heal()` stops failing; `proxy.stats` counts forwarded and dropped requests.

- **Retry screen.** `ctx.find({ role: "heading", name: "Can't reach your Jellyfin server" })`, then `{ role: "button", name: "Retry now" }` and `{ role: "button", name: "Sign out" }`. `ctx.api("/api/status")` reports `bootstrap: { state: "retrying", reason: "unreachable", retryAt }`. The next-attempt line is plain text; read it from `document.body.innerText`. *Proven by `bootstrap-retry`.*
- **Retry now, server still down.** Press `Retry now`: the text becomes `Trying again now…` and the button is disabled, then the countdown returns once the attempt fails. *Proven by `bootstrap-retry`.*
- **Retry now, server back.** `proxy.heal()`, press `Retry now`, wait for `{ role: "link", name: "Settings" }` and pathname `/`. `bootstrap.state` is `ready`. *Proven by `bootstrap-retry`.*
- **Sign out.** Press `Sign out`; `{ role: "textbox", name: "Server" }` returns and `/api/status` reports `authenticated: false`. *Proven by `bootstrap-retry`.*
- **Relaunch.** `await ctx.relaunch()` while the proxy still fails, then the retry screen handles above and `await ctx.until(() => ctx.windowShown(), …)`. *Proven by `bootstrap-retry` on Linux.*
- **Proof.** Snapshot and screenshot the retry screen on first launch and after relaunch, and Home after `Retry now`.

## Gotchas

- The reason heading is a card title with `role="heading"`; the `server` and `storage` wordings need a server that answers with errors or a broken local database, and are not driven.
- The cover stays up until the first failed attempt ends. The client tries a request 3 times (250 ms, then 500 ms apart), so the retry screen appears about 1 to 2 s after the first dropped request. A server that does not answer at all costs three 10 s connect timeouts, about 31 s (`src/jellyfin/api/client.rs`; not driven).
- The worker's backoff grows across attempts (5, 10, 20, 40, 60 s, each plus up to 20% jitter). A failed `Retry now` advances it one step like any failed attempt; Sign out while the worker waits does not reset it (`src/library/sync/worker.rs`). To prove that `Retry now` caused an attempt, wait until `retryAt` is far enough away before pressing it.
- `Trying again now…` and the disabled `Retry now` also show when the countdown reaches 0, before the worker's own attempt starts.
- The `fail` predicate also matches the identity sweep (`/Items` with `Limit=1000`, `EnableImages=false`), which only runs after a catalog exists. Add `Limit=200` to the match when a drive must fail the bootstrap alone.
- Settings (`/settings/*`) bypasses the cover and the retry screen. A 401 or 403 does not show the retry screen: the session expires and the sign-in form returns.
- A new sign-in starts in `loading`: a failure belongs to the session it happened in.
- The proxy forwards every request it does not drop, so the demo server's read-only rules apply through it too.
