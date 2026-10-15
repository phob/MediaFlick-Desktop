Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\agents\poteto-agent.md in full and act as it says. The poteto-mode skill file is C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\poteto-mode\SKILL.md. You are the fix-round-1 owner for PR 2 of a stacked Feature delivery. Do not spawn subagents. Do not push, open a PR, or deploy.

## Where you work
Only C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr2 (branch feat/generic-item-details, HEAD a7d5047, stacked on feat/library-registry). Run every command there; edit by absolute path. Never write elsewhere. Follow AGENTS.md. Build: `$env:CARGO_TARGET_DIR = "C:\Users\pho\Source\MediaFlick-Desktop\build\cargo-target"` then `just build`. One `just verify` at a time; never sign out of the account; on a stall `just verify-doctor` (+ `just verify-cleanup`) and rerun once.

## Context (consolidated)
- Original brief: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\briefs\pr2-generic-details.md. Design: ...\.audit\jellyfin-libraries.design.md (amendments at the end override).
- PR 2 (`git log --oneline feat/library-registry..HEAD`) added `/api/item/{id}` `layout: title|generic`, paged live children, `GenericItemPage`, `primaryAspect`, and removed the non-Movie-means-Series fallbacks. An independent verifier passed all checks and drives at a7d5047 and compared the movie title page against main's screenshot (identical).
- A two-model review found the items below.

## Test rule (owner's hard rule)
No unit tests written after code. For the user-visible bug in item 1, reproduce it first in the `generic-detail` drive (commit the drive change, run it, keep the failing artifact), then fix, then rerun to green. Other items are refactors or bounds; prove them through drives and the existing checks. If you add any isolated test, list the failure modes first and commit the test failing before the code.

## Act on
1. **Stale card toggles in children grids** (reviewer A). Cards for Movie/Series inside a box set or folder show Watched/Favorite, but after a toggle the card keeps its old state because `invalidateMediaSurfaces` invalidates `children(itemId)` exactly and never the container's child pages. Fix the query keys/invalidation so every active child page refreshes after a played/favorite change (the design's `["browse", source]` prefix is one way). Drive: with `--account`, open the IMDb Top 250 box set, toggle Favorite on one card that is not a favorite, assert the card shows it without navigating away, toggle it back; register `unfavoriteAfterRun` first. Reproduce the stale state before the fix.
2. **Proof gaps** (reviewer A). Extend `generic-detail.mjs`: open a Series page and a Season page and assert the season rail, the episode list and the Play/Next-up label render as before; assert the collection chip on a movie that belongs to a collection (find one via Jellyfin on the account server); run the limit-clamp check against the 685-entry Movies folder; exercise the uncached title path once (pick a Movie/Series id the app's cache lacks, e.g. by asking `/api/item` for an id after removing it from a disposable profile's cache through the app's own mechanism, or report clearly why it cannot be forced); probe a MusicArtist on the demo server and, if `parentId` children come back empty, either query its albums by artist or hide the grid for by-name item types instead of saying "This folder is empty".
3. **One way to scroll a grid** (both reviewers). Remove the `scroll: "own" | "route"` mode from `ItemGrid`: the grid takes its scroll element (Library.tsx passes its own container, the generic page passes the route viewport, ideally provided by `RouteScrollViewport` through React context instead of `document.querySelector(".content-viewport")`). Share the scroll-margin measurement with `CollectionTitleGrid` in one hook, and keep the virtualizer off until the margin is measured.
4. **Tier logic out of the HTTP handler** (both). Move the title-vs-generic decision, cache-through, the catalog-vs-live children choice, the live-children mapping and the paging type into `src/library/browse.rs` (pass the Jellyfin fetch in as a parameter or closure so `browse.rs` stays free of session code, as PR 1 kept it). `catalog.rs` only parses and shapes, and must end below its pre-PR line count (1,162). One owner for paging bounds. Rename so `items::fetch_item_detail` and the handler no longer share a name.
5. **Validate every item id once** (both). All `["item", id, ..]` routes (detail, children, about, synopsis, letterboxd, media and the rest) validate and percent-decode the id once in the router; move the id predicate out of `collections` into a neutral module. Extend the drive's malformed-id check to `about` and `synopsis`.
6. **Read the cached title under the account guard** (reviewer B). In the uncached path, build the title response (including `library_of`) inside the `commit_if_current` closure so the write and the read share one account check.
7. **Title page requests in parallel again** (both). Hoist `useItemAbout(id)` into `ItemDetail` before the early returns and pass it to both layouts; start `useChildren` for titles as early as before.
8. **Artwork shown whole** (both). The generic hero must show 4:3 and 3:2 art whole (contain in every generic frame or pick the frame from the ratio). Cards contain art only in a bounded near-square range (e.g. 0.8 to 1.3) so wide 16:9 stills are not a thin strip. One set of named thresholds shared by card and hero.
9. **One card capability decision** (reviewer B). Derive `{ preview, ratings, inlineActions }` from the kind once in `MediaCard` and read those flags.
10. **One item query shape** (both). Give `exact_items_query` a fields parameter and build `DETAIL_FIELDS` from `CATALOG_FIELDS`, so the cache-through fetch can never lack a catalog field.

## Batched nits (do them)
- `switch (item.layout)` with an exhaustiveness check instead of `=== "generic" ? … : title`.
- `CATALOG_KINDS` as `const [...] as const` and derive the `CatalogKind` type from it.
- `children`: a storage error reading the cached kind must surface as a storage failure, not fall through to the live path.
- Generic pages: pass only the facts the spec lists (drop Plays and Tags there) and correct the `DetailFacts` comment; item-neutral error and empty texts ("Could not load details", "Could not load these items").
- Remove the ~200 px empty gap under the generic hero when there is no overview (most visible on art-less box sets).
- Drive label: in `--account` mode the album skip must not read "the demo server has a Music library".
- `home-library.mjs:79`: wait with `ctx.until` for the chips to disappear instead of checking 11 ms after the URL change.
- History: arrange the commits so each one builds and passes clippy (the first commit currently trips dead-code).

## Dismissed (do not do)
Folding `TitleDetail` into `ItemDetail` (PR 3 consumes `libraryId`); a 400 for unpaged live children; playlist order (follow-up already reported).

## Then
- Checks: `just rust-quality`, `just test` (rerun once on a loopback stall), `pnpm --dir ui lint`, `pnpm --dir ui test`, `pnpm --dir ui build`, `git diff --check feat/library-registry...HEAD`.
- Drives on the final HEAD, fresh run ids: `generic-detail` (demo and `--account https://jellyfin.ph0c.de`), `top-level-titles --account`, `browse` (demo), `home-library --account`, `libraries-api` (demo).
- Small ordered commits, each building. No AI attribution.

## Report
Per item what changed; the failing-then-passing artifact for item 1; deviations and why; verbatim drive results and artifact paths; checks; `wc -l src/shell/cef/api/catalog.rs`; commit SHAs; gaps.
