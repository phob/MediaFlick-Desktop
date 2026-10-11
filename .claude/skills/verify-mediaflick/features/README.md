# MediaFlick Desktop verification map

This directory is the maintained source for verifying the user-facing behavior of MediaFlick Desktop. Read this index before driving the app, then use the matching feature file as the recipe. `SKILL.md` covers launch, the harness and evidence.

## Baseline preconditions

- Every run starts through `just verify <drive>`: a fresh disposable profile, the staged app on a private desktop (Windows) or Xvfb display (Linux), CDP on a free loopback port.
- A fresh profile is signed out on the sign-in screen (`document.title` is `Sign in — MediaFlick`).
- Signed-in features use Jellyfin's public demo server: `ctx.signIn({ server: "https://demo.jellyfin.org/stable", username: "demo" })` (empty password). It needs internet access. The account is shared by everyone on the internet, so its data changes under you: watch progress, favorites and Continue Watching belong to strangers.
- The MediaFlick Companion is not installed on the demo server. Releases, Discover, Requests, ratings and Companion settings cannot be verified against it (see Unmapped surfaces).
- `just verify-doctor` reports `ready` and no leftovers before you start.

## Driving conventions

- Find elements by role and accessible name (`ctx.find({ role, name })`). Use `{ css }` only for a control without a stable name, and say why in a comment.
- Act through `ctx.press`, `ctx.fill`, `ctx.choose` and `ctx.hover`, which are real CDP mouse and keyboard input. Use `ctx.api` only to read results, never to perform the action under test.
- Reach pages the way a user does: sidebar links, settings navigation, cards. Do not jump with `ctx.app.route()` when the feature is the navigation itself.
- Never write to the shared demo account: no favorites, watched marks, playback, or Letterboxd/Companion changes. Use a server you own for write paths (pass `--url` and credentials through your own drive; never commit credentials).
- Never press Play, Resume, From start, or an episode's Play: playback starts real audio and video (on Linux through the system libmpv), and the private desktop does not isolate audio on Windows.

## Proof and skip reporting

- Each run leaves `build/verify/<run-id>/` with `steps.log` (actions and checks), `doctor.json`, `result.json`, `guard.json`, screenshots, `.ax.txt` accessibility snapshots, `app.log`, and `profile/config/` (the files the app wrote).
- Capture the action and the resulting state: a snapshot or screenshot after each meaningful state, not only at the end.
- A mutation is proven by the stored side effect (`ctx.readConfig`, file modification times, `/api/*` read-back) and a second visit to the page.
- Report an entry point that cannot be driven as not verified, naming the unmet precondition. Do not substitute another path.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the user-visible behavior, then exactly four H2 sections: `Sub-features`, `How to get to it (user POV)`, `Driving it with ctx (just verify)`, and `Gotchas`. Handles marked *proven* were exercised by a shipped drive; the rest come from the UI source and need their first live run before you rely on them.

## Features

- [Sign in and sign out](./sign-in.md): server address, username and password, `--url` prefill, Quick Connect, the user menu, sign-out. Proven by `smoke`, `browse`, `sign-in`, `labels`, `sidebar-user-menu` and `library-filters`; the error path needs a server you own.
- [Client settings](./client-settings.md): Player, Playback and Application shelves with Save, Discard, Restore defaults and restart notices, the leave-without-saving guard, the Delete local account data dialog, and what closing the window does (Close behavior and the leave guard on a window close). Proven by `application-settings`, `client-settings`, `player-shelf`, `labels`, `close-minimize` and `close-guard`.
- [Home](./home.md): billboard, shelves of poster cards, arrows, cards to detail, and the empty states. Proven by `browse`, `home-library` and `home-settings`; the load-error state is not reachable on the demo server.
- [Library and search](./library-search.md): Movies, Series and Favorites from the sidebar, sort, filters and chips, sidebar search, the empty result. Proven by `browse`, `home-library` and `library-filters`.
- [Library loading and the retry screen](./bootstrap-retry.md): the startup cover after sign-in, the retry screen when the first catalog page fails (reason, next attempt, Retry now, Sign out), and the window on relaunch. Proven by `bootstrap-retry` with a failing loopback proxy.
- [Item detail](./item-detail.md): movie, series and episode pages, the Cast and Crew rails, More info, seasons, episodes, breadcrumbs, favorite and watched toggles. Read-only parts proven by `browse`, `item-detail`, `detail-crew` and `labels`; toggles need a server you own.

## Unmapped surfaces

- Account settings (Viewing, Home, Appearance, Collections) and the Letterboxd integration: reachable after sign-in in `Settings navigation` under Account and Integrations, same save-bar workflow as client settings. Not yet mapped, except the Home shelf's `Show <shelf>` checkboxes that `home-settings` uses for Home's empty states (see [Home](./home.md)).
- Collections (Movie Franchises, My Collections, Jellyfin Collections): the sidebar entry depends on the collection mode; the demo server has no BoxSets.
- Releases (`/calendar`), Discover and Requests, ratings, the Releases shelf on Home, Companion settings: need a server with the MediaFlick Companion and Seerr. Not drivable against the demo server.
- Playback (built-in libmpv and external mpv): starts real audio. Out of scope until a muted or loopback-media path exists. On Linux the Built-in player is the default and loads the system `libmpv.so.2`; `just build` does not stage libmpv.
- The update banner (GitHub release check): never press its install action; it downloads and runs an installer.
