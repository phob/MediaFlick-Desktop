# Sign in and sign out

A signed-out user connects MediaFlick to a Jellyfin server with a server address and either a username and password or Quick Connect. The session and account are stored in the profile, and signing out from the user menu returns to the sign-in screen.

## Sub-features

- `signin-form` signs in with server address, username and password (empty password allowed).
- `signin-prefill` shows a server address given on the command line (`--url`) or remembered from the last session.
- `signin-error` shows the server's error under the form when sign-in fails.
- `signin-quickconnect` shows a Quick Connect code to approve in another Jellyfin client.
- `signout` signs out from the sidebar's user menu, without confirmation.

## How to get to it (user POV)

- Launch MediaFlick with a fresh profile: the sign-in screen is the whole window.
- Sidebar → user menu (avatar, user name and server host at the bottom) → `Sign out`.
- A server that rejects the stored token also returns the app to the sign-in screen.

## Driving it with ctx (just verify)

Preconditions:

- A fresh profile from `just verify`. Internet access for the demo server.
- For `signin-prefill`, start with `just verify <drive> --url https://demo.jellyfin.org/stable`.

- **Fresh state.** `await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen")`. `{ role: "button", name: "Sign in" }` is disabled until the server field has a value, and `ctx.readConfig("accounts.json")` is `null`. *Proven by `smoke`.*
- **Sign in.** `ctx.fill({ role: "textbox", name: "Server" }, url)`, `ctx.fill({ role: "textbox", name: "Username" }, "demo")`, optionally `{ role: "textbox", name: "Password" }`, then `ctx.press({ role: "button", name: "Sign in" })`. The button reads `Signing in…` while pending. Success shows the sidebar: `{ role: "link", name: "Settings" }` appears. `ctx.signIn({ server, username, password })` does exactly this. *Proven by `browse`, `application-settings`.*
- **Stored session.** `ctx.readConfig("accounts.json").accounts` has one entry, and `ctx.api("/api/status")` returns `authenticated: true, userName: "demo"`. *Proven by `browse`.*
- **Error.** Sign in with a wrong password against a server you own. The error appears as a `p.text-destructive` under the form (no `role="alert"`); read it with `ctx.app.evaluate`. Not yet driven.
- **Quick Connect.** After the server field holds a reachable server, `{ role: "button", name: "Use Quick Connect" }` enables when the server supports it (the `role="status"` help text explains otherwise). Pressing it replaces the button with a code and `Waiting for approval…`. Approval must happen in another Jellyfin client, so stop at the code. Not yet driven.
- **Sign out.** `ctx.press({ role: "button", name: "D demo demo.jellyfin.org" })` opens the user menu, then `ctx.press({ role: "menuitem", name: "Sign out" })`. The sign-in form returns (`{ role: "textbox", name: "Server" }`) and `/api/status` reports `authenticated: false`. *Proven by `browse`.*
- **Proof.** Screenshot and snapshot the signed-out and signed-in states; keep `profile/config/accounts.json` from the evidence folder.

## Gotchas

- The welcome text is a card title, not a heading. Identify the screen by `document.title` on a fresh launch, or by the `Server` textbox.
- After signing out, `document.title` keeps the previous page's title (for example `Pioneer One — MediaFlick`) while the sign-in form shows. Do not wait on the title after sign-out.
- The user-menu trigger's accessible name is the avatar initial, user name and server host joined with spaces (`D demo demo.jellyfin.org`). Two elements match its CSS (`[data-sidebar="footer"] [data-sidebar="menu-button"]`), so use the name.
- The sign-in screen has no link to Settings. Device settings are only reachable signed out by URL, which is not a user path; sign in first to reach them.
- The demo server is public and occasionally slow or down. A sign-in timeout there is an environment failure, not an app regression; retry once before investigating.
