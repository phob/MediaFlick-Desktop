# Sign in and sign out

A signed-out user connects MediaFlick to a Jellyfin server with a server address and either a username and password or Quick Connect. The server address is remembered in `settings.json`, the session token in `library.db` and the account's preferences in `accounts.json`. Signing out from the user menu returns to the sign-in screen with the server still filled in.

## Sub-features

- `signin-form` signs in with server address, username and password (empty password allowed).
- `signin-prefill` shows a server address given on the command line (`--url`, also `JELLYFIN_URL`) or remembered from the last session.
- `signin-error` shows the server's error in the form when sign-in fails.
- `signin-quickconnect` shows a Quick Connect code to approve in another Jellyfin client.
- `user-menu` holds `Sync library` and `Sign out`.
- `signout` signs out from the user menu, without confirmation.

## How to get to it (user POV)

- Launch MediaFlick with a fresh profile: the sign-in screen is the whole window.
- Sidebar → user menu (avatar, user name and server host at the bottom) → `Sign out`.
- A server that rejects the stored token also returns the app to the sign-in screen (`/api/status` reports `expired: true`).

## Driving it with ctx (just verify)

Preconditions:

- A fresh profile from `just verify`. Internet access for the demo server.
- For `signin-prefill`, start with `just verify <drive> --url https://demo.jellyfin.org/stable`.

- **Fresh state.** `await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen")`. `{ role: "button", name: "Sign in" }` is disabled until the server field has a value, and `ctx.readConfig("accounts.json")` is `null`. *Proven by `smoke`.*
- **Prefill.** With `--url`, `ctx.find({ role: "textbox", name: "Server" })` returns `properties.value` equal to the URL. *Proven by `sign-in`.*
- **Sign in.** `ctx.fill({ role: "textbox", name: "Server" }, url)`, `ctx.fill({ role: "textbox", name: "Username" }, "demo")`, optionally `{ role: "textbox", name: "Password" }`, then `ctx.press({ role: "button", name: "Sign in" })`. The button reads `Signing in…` while pending. Success shows the sidebar: `{ role: "link", name: "Settings" }` appears. `ctx.signIn({ server, username, password })` does exactly this; omit `server` when `--url` prefilled it. *Proven by `browse`, `application-settings`, `sign-in`.*
- **Stored session.** `ctx.api("/api/status")` returns `authenticated: true, userName: "demo"`, and `ctx.readConfig("accounts.json").accounts` has one entry. The source creates that entry on the first account-scoped write, so wait for it with `ctx.until` rather than reading once. *Proven by `browse`, `sign-in`.*
- **Quick Connect.** The availability probe runs when the Server field loses focus, or at once for a prefilled or remembered server. Its result is the `role="status"` text `#quick-connect-help`, for example `Sign in by approving a code on a device already signed in to this Jellyfin server.` (available) or `Leave the Server field to check Quick Connect availability.` (not probed yet). When available, `{ role: "button", name: "Use Quick Connect" }` enables; pressing it shows a code and `Waiting for approval…`. Editing the server address drops the code. Approval needs another Jellyfin client, so stop at the code. *Proven by `sign-in` (the demo server supports Quick Connect).*
- **Error.** Sign in with a wrong password against a server you own. The error is a `p.text-destructive` inside the form, between Password and `Sign in` (no `role="alert"`); Quick Connect errors use a second `p.text-destructive` above Username. Read the right one with `ctx.app.evaluate`. Not verified: the demo account accepts any password.
- **User menu.** `ctx.press({ role: "button", name: "D demo demo.jellyfin.org" })` opens it with `{ role: "menuitem", name: "Sync library" }` and `{ role: "menuitem", name: "Sign out" }`. *Proven by `sign-in`.*
- **Sign out.** `ctx.press({ role: "menuitem", name: "Sign out" })`. The sign-in form returns with the remembered server in `{ role: "textbox", name: "Server" }`, `/api/status` reports `authenticated: false`, `accounts.json` keeps the account, and `settings.json` keeps `jellyfin_url`. *Proven by `browse`, `sign-in`.*
- **Proof.** Screenshot and snapshot the signed-out and signed-in states; keep `profile/config/accounts.json` and `settings.json` from the evidence folder.

## Gotchas

- The welcome text is a card title, not a heading. Identify the screen by `document.title` on a fresh launch, or by the `Server` textbox.
- After signing out, `document.title` resets to `Sign in — MediaFlick` only when you signed out from Home (`/`). From any other page it keeps that page's title (for example `Pioneer One — MediaFlick`); wait on the `Server` textbox instead.
- Signing out from a `/settings/*` page leaves the shell and Settings on screen, because Settings stays reachable signed out (source; not driven). Sign out from Home or a library or detail page.
- The user-menu trigger's accessible name is the avatar initial, user name and server host (with port, if any) joined with spaces (`D demo demo.jellyfin.org`). Two elements match its CSS (`[data-sidebar="footer"] [data-sidebar="menu-button"]`), so use the name.
- Do not press `Sync library` on the shared demo server; it adds load and proves nothing the sign-in does not.
- The sign-in screen has no link to Settings. Device settings are only reachable signed out by URL, which is not a user path; sign in first to reach them.
- The demo server is public and occasionally slow or down. A sign-in timeout there is an environment failure, not an app regression; retry once before investigating.
