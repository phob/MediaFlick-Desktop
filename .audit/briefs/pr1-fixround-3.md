Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\agents\poteto-agent.md in full and act as it says. The poteto-mode skill file is C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\poteto-mode\SKILL.md. You are the fix-round-3 owner for PR 1, the last fix round. Keep the diff small. Do not spawn subagents. Do not push, open a PR, or deploy.

## Where you work
Only C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr1 (branch feat/library-registry, HEAD 0d7f4c1). Run every command there; edit by absolute path. Never write elsewhere. Follow AGENTS.md. Build: `$env:CARGO_TARGET_DIR = "C:\Users\pho\Source\MediaFlick-Desktop\build\cargo-target"` then `just build`. One `just verify` at a time.

## Context (consolidated)
- Design: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\jellyfin-libraries.design.md. Earlier briefs: ...\.audit\briefs\pr1-library-registry.md, pr1-fixround-1.md, pr1-fixround-2.md.
- Fix round 2 (`git log --oneline e02c5bd..HEAD`) added per-library `membership_owed`, `membershipPending`, an owed sweep that clears only when deletion ran, and the rule "a Known and empty registry plus an empty identity walk empties the catalog". Tests live in `src/library/sync/cycle/tests.rs`. A verifier passed everything at 0d7f4c1.
- Review round 3 (two models) found the items below.

## Test rule (owner's hard rule)
Never write tests after the code exists. For items 1, 2 and 4 (behavior changes), first write down every way the change could fail (top of your report and the test commit's body), write loopback cycle tests through the real cycle, commit them failing (make the test commit compile; stub only inside the test commit if you must, and say so), then implement. No back-filled tests for unchanged code; list gaps.

## Act on
1. **Removed last catalog library while other libraries remain** (both reviewers). The empty-catalog rule only fires when the registry lists no library at all. A user who loses their last movies/tvshows library but still sees Music, Collections or Live TV keeps the removed titles in Search, Favorites and Home forever, and `sync.sweep_owed` stays set so a sweep runs every cycle. Change the rule to "this cycle's `/UserViews` answer lists no catalog library" (use `catalog_title()` / the profile table, not a new list). Base it on the answer fetched in this same cycle, not on the stored registry: if this cycle's `/UserViews` call failed, an empty identity answer stays a skipped sweep. Add a test that removes the last movies library while a `music` view remains.
2. **One library that keeps timing out starves the libraries after it** (both). `membership_pass` ends the pass on `Transport` and walks in server order, so one library whose id walk always times out keeps every later library pending. On `Transport`, skip only that library and continue; end the pass only on `RateLimited` (server-wide). Update the existing rate-limit / lost-connection test to the new rule.
3. **Schema 20 changed in place** (reviewer B). A dev cache created by an earlier schema-20 build lacks `membership_owed` and fails without self-healing. Bump `SCHEMA_VERSION` to 21 so it is recreated through the existing path. No migration code.
4. **Boundary for the delete-everything path** (reviewer B). `ItemsResponse` and the views response default missing fields, so a 200 `{}` from a misbehaving proxy reads as an empty answer. The empty-catalog deletion must require the identity answer to carry an explicit `TotalRecordCount` of 0 (keep `#[serde(default)]` elsewhere if other callers rely on it; scope the strictness to this decision, e.g. `total_record_count: Option<i64>` read only here, or a dedicated check). Add the `{}` case to the item 1 tests.

## Batched nits (do them)
- `ui/src/lib/api/types.ts`: say what `membershipPending` really means (a membership walk is owed or running, so it turns true for every catalog library during a full walk), not "an empty grid is not yet an empty library".
- Rename the local `deleted` in `sweep_identity` to `deletion_ran`.
- Remove the `Walk` enum if it simplifies: owe all catalog libraries in `cycle.rs` when a trigger calls for it, then call `membership_pass` without a mode; drop the duplicate pending filter in `cycle.rs`.
- Rename the shadowed `counted` binding in `member_ids`.
- The `item_libraries` / `membership_owed` comments: state that an empty answer settles the walk and keeps the stored rows.
- Changelog: check against `fix/search-favorites-top-level-only` (the PR base) whether `/api/items` ever read an invalid `favorite` value as "not a favorite"; keep the line only if the behavior really changed versus the base, and keep entries user-facing.

## Dismissed (do not do)
A backoff for a library whose walk always ends short; splitting `MediaLibrary` into server and stored shapes.

## Then
- Checks: `just rust-quality`, `just test` (rerun once on a loopback stall and report both), `pnpm --dir ui lint`, `pnpm --dir ui test`, `pnpm --dir ui build`, `git diff --check fix/search-favorites-top-level-only...HEAD`.
- Drives on the final HEAD, one at a time: `just verify libraries-api` (demo and `--account https://jellyfin.ph0c.de`), `just verify top-level-titles` (demo and `--account`), `just verify browse` (demo only). Never sign out of the account. On a stall or CDP attach failure, `just verify-doctor` and rerun once. Use fresh run ids.
- Small ordered commits (failing tests before code). No AI attribution.

## Report
Failure-mode list; per item what changed; deviations and why; test names with before/after results; verbatim drive results and artifact paths; checks; commit SHAs; gaps.
