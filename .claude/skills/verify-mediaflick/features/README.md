# MediaFlick Desktop verification map

This directory is the maintained source for verifying the user-facing behavior of MediaFlick Desktop. Read this index before driving the app, then use the matching feature file as the recipe. `SKILL.md` covers launch, the harness and evidence.

## Baseline preconditions

- Every run starts through `just verify <drive>`: a fresh disposable profile, the staged app on a private desktop (Windows) or Xvfb display (Linux), CDP on a free loopback port.
- A fresh profile is signed out on the sign-in screen (`document.title` is `Sign in — MediaFlick`).
- Signed-in features use Jellyfin's public demo server: `ctx.signIn({ server: "https://demo.jellyfin.org/stable", username: "demo" })` (empty password). It needs internet access. The account is shared by everyone on the internet, so its data changes under you: watch progress, favorites and Continue Watching belong to strangers.
- Your own server covers what the demo server cannot: writes, playback, and the MediaFlick Companion (Releases, ratings, Discover, Requests, Movie Franchises, My Collections). Cache a session once with `just verify-login SERVER` (you approve a Quick Connect code), then run drives with `--account SERVER`; see SKILL.md, Your own server. The proofs below name the test server they ran against: Jellyfin 12.2.0 with a non-admin `Testuser` (684 movies, 97 series, 4,371 episodes, 85 BoxSets) and Companion 0.2.2 with Seerr, Sonarr, Radarr, MDBList and TMDB.
- The MediaFlick Companion is not installed on the demo server.
- `just verify-doctor` reports `ready` and no leftovers before you start.
- Every drive except `close-minimize` and `close-guard` (Linux only: `ctx.wmClose`) has passed on Windows 11, and every drive on Linux (all but `login` re-run in the 2026-10-14 maintenance pass).

## Driving conventions

- Find elements by role and accessible name (`ctx.find({ role, name })`). Use `{ css }` only for a control without a stable name, and say why in a comment.
- Act through `ctx.press`, `ctx.fill`, `ctx.choose` and `ctx.hover`, which are real CDP mouse and keyboard input. Use `ctx.api` only to read results, never to perform the action under test.
- Reach pages the way a user does: sidebar links, settings navigation, cards. Do not jump with `ctx.app.route()` when the feature is the navigation itself.
- Never write to the shared demo account: no favorites, watched marks, playback, or Letterboxd/Companion changes. Write paths run on your own server with `--account`; never put credentials in a drive.
- Play only with `--account` on your own server (see [Playback](./playback.md)). Playback writes the test user's playstate and plays real audio; on Windows the private desktop does not isolate it.
- A drive that changes server state puts it back through the user path and registers an after-run safety net (`ctx.restoreItemAfterRun`, `ctx.cancelSeerrRequestAfterRun`) that runs once the app has exited.

## Proof and skip reporting

- Each run leaves `build/verify/<run-id>/` with `steps.log` (actions and checks), `doctor.json`, `result.json`, `guard.json`, screenshots, `.ax.txt` accessibility snapshots, `app.log`, and `profile/config/` (the files the app wrote).
- Capture the action and the resulting state: a snapshot or screenshot after each meaningful state, not only at the end.
- A mutation is proven by the stored side effect (`ctx.readConfig`, file modification times, `/api/*` read-back) and a second visit to the page.
- Report an entry point that cannot be driven as not verified, naming the unmet precondition. Do not substitute another path.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the user-visible behavior, then exactly four H2 sections: `Sub-features`, `How to get to it (user POV)`, `Driving it with ctx (just verify)`, and `Gotchas`. Handles marked *proven* were exercised by a shipped drive; the rest come from the UI source and need their first live run before you rely on them.

## Features

- [Sign in and sign out](./sign-in.md): server address, username and password, `--url` prefill, Quick Connect, the user menu, sign-out (which revokes the token), an expired session. Proven by `smoke`, `browse`, `sign-in`, `labels`, `sidebar-user-menu` and `library-filters` on the demo server, and `account-session` on your own; the wrong-password path is not driven (lockout).
- [Client settings](./client-settings.md): Player, Playback and Application shelves with Save, Discard, Restore defaults and restart notices, the leave-without-saving guard, the Delete local account data dialog, and what closing the window does (Close behavior and the leave guard on a window close). Proven by `application-settings`, `client-settings`, `player-shelf`, `labels`, `close-minimize` and `close-guard`.
- [Home](./home.md): billboard, shelves of poster cards, arrows, cards to detail, the Releases shelf, and the empty states. Proven by `browse`, `home-library` and `home-settings`, and the Releases shelf by `companion`; the load-error state is not reached.
- [Library and search](./library-search.md): Movies, Series and Favorites from the sidebar, sort and direction, filters and chips, the A–Z rail, the unwatched badge, sidebar search, the empty result. Proven by `browse`, `home-library`, `library-filters`, `library-parity` and `punctuation-search`.
- [Library loading and the retry screen](./bootstrap-retry.md): the startup cover after sign-in, the retry screen when the first catalog page fails (reason, next attempt, Retry now, Sign out), and the window on relaunch. Proven by `bootstrap-retry` with a failing loopback proxy.
- [Item detail](./item-detail.md): movie, series and episode pages, the Cast and Crew rails, More info, seasons, episodes, breadcrumbs, ratings, favorite and watched toggles. Read-only parts proven by `browse`, `item-detail`, `detail-crew`, `labels` and `person-discover`; watched by `playback` and ratings by `companion` on your own server; favorites not yet.
- [Playback](./playback.md): Play, the player bar and keys, playstate reports, the resume point, Resume with the startup seek rules, stop, on the Built-in player and External mpv. Proven by `playback` on both backends on Linux and Windows; segment skipping needs a server with media segments.
- [Companion: Releases, ratings and status](./companion.md): the Companion status page, the Releases page and its toggles, MDBList ratings. Proven by `companion`.
- [Discover and Requests](./discover-requests.md): Seerr rows, genres, search, title pages, a real request and its cancellation. Proven by `discover-requests`.
- [Collections](./collections.md): Movie Franchises, My Collections from a template, Jellyfin mode with BoxSets and the route guard. Proven by `collections`.
- [Account settings and integrations](./account-settings.md): Viewing, Appearance, the leave guard between account shelves, Letterboxd. Proven by `account-settings`.

## Unmapped surfaces

- The update banner (GitHub release check): never press its install action; it downloads and runs an installer.
- Media segment skipping and the next-episode countdown: mapped in [Playback](./playback.md) from source; the test server has no media segments, and no drive plays to the end of an episode yet.
