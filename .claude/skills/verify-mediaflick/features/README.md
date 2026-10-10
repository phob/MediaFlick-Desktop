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

- [Sign in and sign out](./sign-in.md): server address, username and password, `--url` prefill, Quick Connect, the user menu, sign-out. Proven by `smoke`, `browse`, `sign-in`; the error path needs a server you own.
- [Client settings](./client-settings.md): Player, Playback and Application shelves with Save, Discard, Reset and restart notices, the leave-without-saving guard, and the Delete local account data dialog. Proven by `application-settings` and `client-settings`.
- [Home](./home.md): billboard, shelves of poster cards, arrows, cards to detail. Proven by `browse` and `home-library`; the empty states are not reachable on the demo account.
- [Library and search](./library-search.md): Movies, Series and Favorites (My List) from the sidebar, sort, filters and chips, sidebar search, the empty result. Proven by `browse` and `home-library`.
- [Item detail](./item-detail.md): movie, series and episode pages, More info, seasons, episodes, breadcrumbs, favorite and watched toggles. Read-only parts proven by `browse` and `item-detail`; toggles need a server you own.

## Unmapped surfaces

- Account settings (Viewing, Home, Appearance, Collections) and the Letterboxd integration: reachable after sign-in in `Settings navigation` under Account and Integrations, same save-bar workflow as client settings. Not yet mapped.
- Collections (Movie Franchises, My Collections, Jellyfin Collections): the sidebar entry depends on the collection mode; the demo server has no BoxSets.
- Releases (`/calendar`), Discover and Requests, ratings, the Release Timeline shelf, Companion settings: need a server with the MediaFlick Companion and Seerr. Not drivable against the demo server.
- Playback (built-in libmpv and external mpv): starts real audio. Out of scope until a muted or loopback-media path exists. On Linux the Built-in player is the default and loads the system `libmpv.so.2`; `just build` does not stage libmpv.
- The update banner (GitHub release check): never press its install action; it downloads and runs an installer.
