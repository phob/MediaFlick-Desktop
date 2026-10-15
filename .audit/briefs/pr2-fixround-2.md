Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\agents\poteto-agent.md in full and act as it says. The poteto-mode skill file is C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\poteto-mode\SKILL.md. You are the fix-round-2 owner for PR 2. Keep the diff small. Do not spawn subagents. Do not push, open a PR, or deploy.

## Where you work
Only C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr2 (branch feat/generic-item-details, HEAD 6e25ef0). Run every command there; edit by absolute path. Never write elsewhere. Follow AGENTS.md. Build: `$env:CARGO_TARGET_DIR = "C:\Users\pho\Source\MediaFlick-Desktop\build\cargo-target"` then `just build`. One `just verify` at a time; never sign out of the account; on a stall `just verify-doctor` (+ `just verify-cleanup`) and rerun once.

## Context (consolidated)
- Briefs: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\briefs\pr2-generic-details.md and pr2-fixround-1.md. Design: ...\.audit\jellyfin-libraries.design.md.
- Fix round 1 (`git diff a7d5047 6e25ef0`) passed an independent verifier (all checks, 7 drives). Review round 2 found the items below.

## Test rule (owner's hard rule)
No unit tests after code. For item 1 and item 2 (behavior), reproduce first in a drive where the real app can show it (commit the drive step, keep the failing artifact), then fix and rerun green. If a reproduction is impossible on the available servers, say exactly why and prove the fix by the closest drive you can.

## Act on
1. **Uncached series loads uncached seasons** (reviewer A). `useChildren(id, item?.layout !== "generic")` starts `/children` before `/api/item` has cached an uncached Series, so `children_source` sees no cached kind, takes the live path, returns at most 60 seasons without overviews, and never runs the catalog reconcile; the season's episodes then go live too. It also fires a wasted live request on every generic page while the item loads. Gate `useChildren` on `item?.layout === "title"`; keep `about` parallel. Reproduce with the drive's existing uncached-title technique on the account server (ask for an uncached Series while the first sync still runs) before the fix if you can.
2. **Remote changes never refresh box set, playlist or album grids** (reviewer A). Sync-batch invalidation only reaches `child-pages` whose container id is in the batch's item or context ids, and a movie's context is its library folder, never a BoxSet or Playlist. In `invalidateLibraryChanged`, invalidate all active `child-pages` when the batch scope is not item-only, as `ITEMS_ROOT` already is. Prove it by marking a box-set movie played/unplayed through Jellyfin directly (the harness may write to the test account with an after-run restore, `restoreItemAfterRun`) while the box set page is open, and asserting the card updates without navigating.
3. **A library jump missed once** (reviewer B). `build/verify/fr1-item3-parity` failed: after "Jump to S" on Movies A–Z descending, `scrollTop` never reached 0 within 10 s, and a rerun passed on the same tree. Find the cause (failure.png, app.log, steps.log), run `library-parity` five times and keep every result. If the app is at fault (a smooth scroll or reconcile moving it after the jump), fix the app. If the drive's strict `=== 0` is at fault, use the `<= 2` tolerance the other branch already uses, and say which.
4. **Account check on live answers** (reviewer B, AGENTS account-isolation rule). The live generic detail and the live children answers return without `scope_is_current`. Add one check before each return using the existing `stale_account_response()`.

## Batched nits (do them)
- `item_detail`: make the uncached write-and-read-back one `session.commit_if_current(&scope, || DetailError::AccountChanged, || browse::write_live_title(..))` returning `Result<TitleDetail, DetailError>`; drop the `dyn FnMut` callback and the `Option<Result<..>>` nesting (both reviewers).
- Delete `Library::kind()`; move its callers (companion/mod.rs, jellyfin/remote.rs, shell/cef/api/media.rs) to `catalog_kind()` and `CatalogKind` matches (both).
- Put the bounds back in `live_children_query` as well (`limit.clamp(1, MAX)`, non-negative start) so the public client is safe on its own; one shared default page size constant used by catalog.rs and browse.rs (both).
- Replace `cardFeatures` with one `const title = isCatalogKind(item.kind)` (both).
- Pass `ItemAbout | undefined` (and the child list data) to page components instead of whole `UseQueryResult`s.
- `DetailHeroLayout`: the `square` frame defaults to contain again.
- Record the intended change that a storage error reading the cached kind now answers 500 (commit message or PR note; no code change).

## Dismissed (do not do)
Correcting the round-1 description of ItemGrid's scroller (it takes a prop, which is the better design).

## Then
- Checks: `just rust-quality`, `just test` (rerun once on a loopback stall), `pnpm --dir ui lint`, `pnpm --dir ui test`, `pnpm --dir ui build`, `git diff --check feat/library-registry...HEAD`.
- Drives on the final HEAD, fresh run ids: `generic-detail` (demo and `--account https://jellyfin.ph0c.de`), `top-level-titles --account`, `browse` (demo), `home-library --account`, `library-parity` (demo, five runs from item 3), `libraries-api` (demo).
- Small ordered commits, each building. No AI attribution.

## Report
Per item what changed; reproduction artifacts before and after for items 1 and 2 (or why not reproducible); the five library-parity results and the cause from item 3; deviations; verbatim drive results and artifact paths; checks; commit SHAs; gaps.
