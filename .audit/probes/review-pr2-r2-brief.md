Inspect only. Change no file.

You are an adversarial code reviewer. Find real problems in the code below: bugs, design flaws, security issues, and maintainability concerns. You are not here to be helpful or encouraging. You are here to stress-test.

## Intent

> MediaFlick Desktop will show every Jellyfin library. This PR (PR 2 of a stack) gives every Jellyfin item a details page. `/api/item/{id}` now answers a discriminated union: `layout: "title"` for Movie, Series, Season and Episode (today's full page from the local cache, plus the item's `libraryId` from PR 1's membership, seasons and episodes resolving through their series), and `layout: "generic"` for every other Jellyfin type (MusicAlbum, Audio, Book, Photo, BoxSet, Playlist, Folder, Video, ...), fetched live. A Movie or Series is a title page wherever it lives, including mixed libraries. `/api/item/{id}/children` is now paged: cached Series and Season keep today's reconcile, any other container is one live Jellyfin page. The generic page shows artwork, name, year, runtime, overview, plain genre chips, people and studios, and a grid of a container's children. It deliberately has no Play, Favorite, ratings, TMDB, Letterboxd, collection chip or media info. Cards get `primaryAspect` and show near-square art whole instead of cropped. The "anything that isn't a Movie is a Series" fallbacks in navigation, the detail hero genre links and the sidebar are removed. No library route or sidebar listing yet (PR 3).

Design: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\jellyfin-libraries.design.md (amendments at the end override). Author's reported deviations: network fetch stays in the API handler (catalog.rs) not `browse.rs`; `TitleDetail { item, library_id }` wraps `ItemDetail`; `ItemGrid` gained `scroll: "own" | "route"`; card actions limited to catalog kinds; one sidebar line changed early; one sort for all live containers (playlists lose manual order); title pages fetch their extras after the item loads; commit 1 alone trips the dead-code lint.

Review whether the code achieves this intent well. Do NOT question the intent itself.

## Code Under Review

Checkout: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr2 (branch feat/generic-item-details). The change is `git -C C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr2 diff feat/library-registry...feat/generic-item-details` (six commits on top of PR 1). Read surrounding code as needed: src/shell/cef/api/catalog.rs (item and children handlers, cache-through, scoped failures), src/jellyfin/api/items.rs and model.rs, src/library/ (ItemSummary, library_of), ui/src/routes/ItemDetail.tsx, ui/src/components/detail/*, ui/src/components/ItemGrid.tsx and MediaCard, ui/src/lib/navigation.ts, ui/src/lib/libraries.ts, ui/src/lib/queries.ts. Repo rules: AGENTS.md (UI must not call Jellyfin directly; shared shadcn components and theme tokens; input validation and bounds; no unwrap/expect in production; contract changes through every boundary; account isolation and sanitized logs). Evidence: build/verify/gd-demo-final and gd-acct-final in that checkout (screenshots and generic-detail.json).

Pay particular attention to: whether any item id or query value reaches Jellyfin unvalidated; whether a live fetch can leak across an account switch or write into the cache for the wrong account; what an uncached Movie/Series fetched by id does now versus before; whether existing title pages, season browser, next-up, playback entry points and Letterboxd/ratings still behave exactly as before; whether any remaining code still assumes non-Movie means Series; and whether the TS discriminated union is narrowed safely everywhere (no type assertions without a reason).

## Round 2 scope

This is review round 2. Review ONLY `git -C C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr2 diff a7d5047 6e25ef0` (the history was rebased, so diff the two trees). Round 1 found: card Watched/Favorite toggles stayed stale in box-set and folder grids; the drive skipped series/season pages, the collection chip, a real clamp and the uncached title path; ItemGrid had a two-mode scroll switch duplicating CollectionTitleGrid; tier logic and paging sat in catalog.rs (over 1,000 lines); only two of the item routes validated ids, one decoded twice; the uncached title was read after the account guard; title pages serialized their requests; the generic hero cropped 4:3/3:2 art and cards letterboxed 16:9; MediaCard checked kind in three places; the item query existed twice. The author's fix: a `child-pages` query root invalidated on every played/favorite change and on sync batches; drive extensions; `ItemGrid` takes its scroll element from context; tier logic in `src/library/browse.rs`, series/season reconcile moved to `src/library/sync/children.rs`, catalog.rs at 1,016 lines; one id check in the router via `app::ids`; write and read-back in one `commit_if_current`; hoisted requests; shared artwork thresholds in `ui/src/lib/artwork.ts`; one card capability object; `exact_items_query` with a fields parameter; plus nits. Reported deviations: `child-pages` instead of `["browse", source]`; the fetch is passed as data, not a closure; `jumpTo` lands at `row * rowHeight` after a measured-margin regression in library-parity; a debug log line for the uncached path.

Check each fix is correct and introduces no new problem. In particular: does the moved reconcile behave exactly as before; can the new child-pages invalidation refetch too much on every sync batch; does the scroll-context grid still restore scroll position and jump correctly on the Library page; does the router id check reject anything the app legitimately sends (non-hex ids anywhere, e.g. person or collection ids under /item?). Evidence: build/verify/fr1-* in that checkout. Only report critical or warning for problems that can occur in practice; label the rest nit.

## Review Rubric

Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\interrogate\references\rubric.md in full and apply it.

## Code Quality Lens

Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\interrogate\references\code-quality-review.md in full and apply it.

## Instructions

For each finding give Severity (`critical` | `warning` | `nit`), Finding (specific lines/functions), Evidence (your reasoning), and optionally Suggestion. Only use critical or warning for problems that can occur in practice. Do not restate what the code does without identifying a problem. Do not praise. "No findings" is valid.

## Output

```
## Findings

### 1. [Severity] Short title
**Location**: file:line or function name
**Finding**: What's wrong
**Evidence**: Why this matters
**Suggestion**: (optional) What to do instead
```
