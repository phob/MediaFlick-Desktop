# Handoff: support every Jellyfin library

Written 2026-10-15 to resume this work on another machine. The original T3 thread is "Support All Jellyfin Library Types" on Martin's Windows machine. This file plus the other files in this folder are everything that lived only there. All code is pushed.

## Goal (owner's decisions, final)

1. Show every Jellyfin library the user can see, of every type (movies, tvshows, music, books, photos, home videos, music videos, box sets, playlists, mixed, Live TV, trailers), each under its Jellyfin name in Jellyfin's order.
2. Movies and TV shows libraries keep today's features, in any number of libraries.
3. Every other type gets only a base experience now: a browse grid and a generic details page. No TMDB, Letterboxd, ratings, Play or Favorite on them. Books only need to be displayed. A Movie or Series keeps its full title page wherever it lives, including mixed libraries.
4. Home: one "Latest in <library>" shelf per library, for every library type, listed in Home settings (Save/Reset/Discard) and shown on Home, enabled by default, Movies and Shows included.
5. Fixed separately: Search, Favorites and the Home Favorites shelf showed seasons and episodes.

## Stack (all pushed; each PR targets the one below)

| PR | Branch | State |
|---|---|---|
| https://github.com/phob/MediaFlick-Desktop/pull/275 | `fix/search-favorites-top-level-only` | open, base `main` |
| https://github.com/phob/MediaFlick-Desktop/pull/276 | `feat/library-registry` | open, base #275 |
| https://github.com/phob/MediaFlick-Desktop/pull/277 | `feat/generic-item-details` | open, base #276 |
| PR 3 library browse + sidebar + live tier | not started | next |
| PR 4 startup destination per library | not started | |
| PR 5 Home "Latest in <library>" shelves | not started | |

Each open PR had three review rounds (Claude Opus + Claude Haiku), a fix round after each, and an independent real-app verifier after every fix round. All three are `BLOCKED: independent review` because only Claude models were enabled; enabling Codex or Grok in T3 allows a cross-family review before merge.

## The design

`jellyfin-libraries.design.md` is the contract (two-tier: movies/tvshows cached in library.db with per-library membership; every other type served live through fixed native endpoints). Read its "Synthesis decision" section at the end first; its amendments override earlier sections. `jellyfin-libraries.grounding.md` has the measured Jellyfin facts. The losing design candidate is in `arena/candidate-1/design.md` for reference.

Where the built code differs from the design, the code wins. Key real shapes: `CatalogKind`, `TitleKind`, `KindFilter` (`src/library/model.rs`), `LibraryKind::profile()` table and `LibraryRegistry` (`src/library/libraries.rs`), `item_libraries` + per-library `membership_owed` (schema 21), `src/library/sync/libraries.rs` (list refresh + membership pass, best-effort), `src/library/browse.rs` (catalog vs live dispatch, `LiveDetail`, `ChildPage`), `/api/libraries*` routes (live types answer 501 until PR 3), `/api/item/{id}` with `layout: "title" | "generic"`.

## Decision trail

`jellyfin-libraries.tsv` is the full decision log, one row per decision with evidence. `briefs/` holds every brief sent to an owner or fix round and the three PR bodies. `jellyfin-libraries.todo.md` is the playbook checklist.

## Carry into PR 3 (owed from review rounds)

- Live items (albums, playlists, photo folders) don't refresh after a change on another device: the native side only emits `library-changed` for cached rows. Add a live-change signal when building the live library grids.
- Playlists list contents by name; read them through `/Playlists/{id}/Items` to keep manual order.
- When a user loses access to a library, delete titles whose only membership was that library (use `item_libraries`), instead of relying on the identity sweep's empty-answer rule. Open gap from PR 1: if the last movies/tvshows library with titles disappears while an empty one stays listed, its titles stay until the next full re-sync.
- `membershipPending` turns true for every catalog library during a full walk; the UI must not show it as "empty".
- Back navigation and genre links should go to `/library/<id>` once that route exists (`navigation.ts`, `DetailHero.tsx`); the sidebar highlight should use the library, not the layout.
- Unify the `child-pages` query root with a `["browse", source]` key when library grids land.

## Other open items

- Loopback test servers (`src/library/sync/cycle/tests.rs`) block on `accept()` with no timeout; a local network flake hangs `just test` instead of failing it. Small follow-up; do it first if CI hangs.
- Cosmetic, pre-existing on main: season rail heading says "Episodes"; series meta text is hard to read on bright backdrops; first-column card preview overlaps the sidebar.
- Books, photos, home videos and mixed libraries were never available on either test server. Adding one small library of each type to the test server lets PR 3's drive cover them.

## How this was run (keep it)

- poteto-mode Feature/figure-it-out flow: owner subagent per PR in its own worktree branched from the previous PR's tip, then interrogate (two reviewers), fix round, verifier, max three fix rounds, then `/no-comments` + `/deslop`, then open the PR stacked on the previous one.
- Owner's test rule (CLAUDE.md): no unit tests written after code. E2E drives through `.claude/skills/verify-mediaflick/` are the proof; each ends in an artifact. An isolated test only when no E2E path exists, with failure modes listed first and the failing test committed before the code.
- Drives used: `top-level-titles`, `libraries-api`, `generic-detail`, plus `browse`, `home-library`, `library-parity`, `library-filters`, `playback` (Built-in libmpv and External mpv).

## Setting up the other machine

1. `git fetch origin` and create worktrees for the stack tip, e.g. `git worktree add <path>/lib-pr3 -b feat/library-browse origin/feat/generic-item-details`.
2. Put this folder at `<checkout>/.audit/` (it is git-ignored by convention here, not committed; the repo is public and these notes name the test server).
3. `pnpm --dir ui install --frozen-lockfile`, then `just build`. On Windows, `just verify` with libmpv needs `build/libmpv-windows-x64` (built through WSL by `just libmpv`, or copied from another checkout).
4. The verify account session is machine-local and holds a token, so it is not in this bundle. Run `just verify-login https://jellyfin.ph0c.de` once and approve the Quick Connect code as the test user.
5. External mpv playback drive expects an mpv path (here `C:\mpv\mpv.exe`); adjust for that machine.
6. Model sheet (`~/.config/t3-pstack/models.md`) is per machine; this run used Opus 5.5 for owners, reviewers and verifiers and Haiku 5.5 as the second reviewer.

## First action on resume

Tell the new agent: "Run poteto-mode Session pickup from `.audit/HANDOFF.md`, then start PR 3 (library browse + sidebar + live tier) per the design's Delivery plan and the 'Carry into PR 3' list." If #275 to #277 got review comments meanwhile, handle those first (Babysit playbook).
