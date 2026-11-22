# Shell crash recovery

When Chromium's renderer process terminates unexpectedly, MediaFlick reveals the main window if it was still hidden, reloads the app automatically up to twice in a rolling 30-second window, and falls back to the native error page (`Try again` and `Open Settings`) if crashes keep repeating so the window never stays blank or stuck in a reload loop.

## Sub-features

- `renderer-auto-reload` reloads `mediaflick-desktop://app/` up to twice within 30 seconds after a recoverable renderer crash (`action=ReloadApp` in `app.log`) while keeping the main window shown and `/api/status` reachable.
- `renderer-error-page-fallback` loads the styled native `load_error.html` page (`action=ShowErrorPage` in `app.log`) on the third crash within 30 seconds (or immediately on launch/integrity failures) and stops retrying if the error page itself terminates.
- `renderer-error-page-retry` clicking `Try again` on the native error page navigates back to `mediaflick-desktop://app/`, resets the recovery budget, and restores the app UI.
- `root-react-error-boundary` catches unhandled render errors at the root route with `RootErrorScreen`, signals window readiness when the window has not yet been revealed, redacts sensitive tokens from the summary, and offers a reload action (verified by Vitest, not driven).
- `native-reveal-deadline` reveals the main window after a 10-second timeout if `windowReady()` never arrives, unless launched `--hidden` or exiting (verified by Rust unit tests, not driven).

## How to get to it (user POV)

- If Chromium's renderer process crashes once or twice, MediaFlick reloads the app in place in the same window.
- If the renderer crashes three times within 30 seconds, MediaFlick displays `MediaFlick couldn’t load this page.` with `Try again` and `Open Settings`.
- Clicking `Try again` returns to the app and clears the crash counter.

## Driving it with ctx (just verify)

Preconditions:

- A fresh profile from `just verify`. No Jellyfin server or sign-in is required.

- **Single crash auto-reload.** Trigger `Page.crash` over CDP, wait for `action=ReloadApp` in `ctx.appLog()`, call `ctx.reconnect(...)` on `mediaflick-desktop://app`, and confirm the sign-in screen (`{ role: "textbox", name: "Server" }`), `/api/status` 200, and `ctx.windowShown()`. *Proven by `renderer-crash` (`01-after-auto-reload.png`, `01-after-auto-reload.ax.txt`, `renderer-crash.json`).*
- **Crash loop fallback and manual retry.** Trigger a second crash (reloads) and a third crash within 30 s, wait for `action=ShowErrorPage` in `ctx.appLog()`, reconnect CDP to `data:text/html`, and verify `{ role: "heading", name: "MediaFlick couldn’t load this page." }`, `{ role: "link", name: "Try again" }`, `{ role: "link", name: "Open Settings" }`, and `ctx.windowShown()`. Press `Try again`, reconnect CDP to `mediaflick-desktop://app`, and verify the sign-in screen returns with `/api/status` 200. *Proven by `renderer-crash` (`02-error-page.png`, `02-error-page.ax.txt`, `03-recovered-after-retry.png`, `03-recovered-after-retry.ax.txt`, `renderer-crash.json`).*

## Gotchas

- `Page.crash` disconnects the active CDP WebSocket without answering the command; fire it without awaiting its response and call `ctx.reconnect(...)` after `app.log` confirms the recovery transition.
- Clicking `Try again` navigates from an opaque `data:text/html` origin back to `mediaflick-desktop://app/`, which swaps the renderer process; reconnect CDP to `mediaflick-desktop://app` before querying the recovered UI so teardown can also exit cleanly.
