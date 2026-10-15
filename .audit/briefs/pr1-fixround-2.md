Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\agents\poteto-agent.md in full and act as it says. The poteto-mode skill file is C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\poteto-mode\SKILL.md. You are the fix-round-2 owner for PR 1 of a stacked Feature delivery. Keep the diff small. Do not spawn subagents. Do not push, open a PR, or deploy.

## Where you work
Only C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr1 (branch feat/library-registry, HEAD e02c5bd). Run every command there; edit by absolute path. Never write elsewhere. Follow AGENTS.md. Build: `$env:CARGO_TARGET_DIR = "C:\Users\pho\Source\MediaFlick-Desktop\build\cargo-target"` then `just build`. One `just verify` at a time.

## Context (consolidated)
- Design: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\jellyfin-libraries.design.md (amendments at the end override). Previous briefs: ...\.audit\briefs\pr1-library-registry.md and ...\.audit\briefs\pr1-fixround-1.md.
- Fix round 1 (`git log --oneline 7584bd9..HEAD`) made library steps best-effort, persisted `sync.sweep_owed` and `sync.membership_owed`, checked membership walks against `TotalRecordCount`, added 503 for an unloaded registry, unified the query parser, bumped schema to 20, and added nine loopback cycle tests in `src/library/sync/cycle.rs`. A verifier passed everything at e02c5bd.
- Review round 2 (two models) found the items below.

## Test rule (owner's hard rule)
Never write tests after the code exists. For each behavior this round CHANGES (items 1 and 2), first write down every way it could fail (list it at the top of your report and in the test commit's body), then write the loopback cycle tests through the real cycle path, commit them failing, then implement. Do not back-fill tests for unchanged code; list such gaps.

## Act on
1. **Membership owed per library** (both reviewers). One library whose walk keeps failing keeps the server-wide `sync.membership_owed` set, so every 10-minute cycle re-walks every catalog library. Track owed per library (e.g. a `membership_owed` column on `libraries`, set by `replace_libraries` for a new catalog library and when that library's walk fails or is truncated, cleared only by that library's successful `replace_membership` in the same transaction). Only owed libraries are walked, plus the existing triggers (catalog changes, `Requested`). End the pass early, leaving the rest owed, at the first retryable error (transport, rate limit) instead of hitting the server once per remaining library. Expose `membershipPending: bool` per library on `GET /api/libraries` (and the TS type) so the UI can tell a pending grid from an empty one. Drop the server-wide `sync.membership_owed` key; schema stays a single bump (20 is not released, so change version 20's schema in place rather than adding 21).
2. **A forced sweep that skipped deletion is not done** (reviewer B). `sweep_identity` clears `sync.sweep_owed` and touches the sweep time whenever `identity_sweep` returns Ok, including when it skipped deletion because the walk was truncated or the server listed nothing. Clear the owed flag (and treat the sweep as having run) only when the deletion pass actually ran. Decide one explicit rule for an authoritatively empty server: when the stored registry is Known and empty (the server lists no libraries for this user) and the identity walk returns zero items, the deletion pass runs and empties the catalog; otherwise an empty identity answer stays a skipped sweep. Keep the hourly timer behavior otherwise unchanged.
3. **Layering** (both). Move `refresh_libraries` back under `src/library/sync/` (beside `membership_pass`, e.g. `sync/libraries.rs`), keep `catalog_libraries.rs` storage-only, and put the owed and read-at meta keys next to the storage code that writes them, so the dependency runs one way (sync -> storage). Remove the duplicate `flag` helper.
4. **`cycle.rs` is 1,332 lines** (reviewer B). Move its test module to `src/library/sync/cycle/tests.rs` (or the repo's existing convention for split test modules; check other modules first) and merge the identical `EMPTY_PAGE` / `NO_LIBRARIES` constants.

## Batched nits (do them)
- Add `KindFilter::from_kinds(Vec<CatalogKind>)` (empty gives `TopLevel`) and use it in `kind_filter`.
- Rename so `/api/items` and the library routes' parser names match what each serves (`library_query` vs `item_query` are inverted today).
- Fix the wrong comment in `ui/src/lib/library-filters.ts` `libraryItemQuery` ("The API refuses any other kind" is false; say it sends only top-level title kinds).
- Mention in the existing changelog fragment that `/api/items` ignores an invalid `favorite` value.
- Replace `Option<bool>` + `.flatten()` in `refresh_membership` with a small named enum if it reads clearer after item 1.

## Dismissed (do not do)
A second process (`headless::sync_once`) racing the owed flag; a partial 200 from `/UserViews`; the registry-then-query race in `library_page`; making the drive's oracle unpaged; surfacing library-step failures in `SyncReport`.

## Then
- Checks: `just rust-quality`, `just test` (rerun once on a loopback stall and report both), `pnpm --dir ui lint`, `pnpm --dir ui test`, `pnpm --dir ui build`, `git diff --check fix/search-favorites-top-level-only...HEAD`.
- Drives, one at a time, on the final HEAD: `just verify libraries-api` (demo and `--account https://jellyfin.ph0c.de`), `just verify top-level-titles` (demo and `--account`), `just verify browse` (demo only). Extend `libraries-api.mjs` to assert `membershipPending` is false for every catalog library after the first sync. Never sign out of the account. On a stall, `just verify-doctor` and rerun once.
- Small ordered commits (failing tests before code). No AI attribution.

## Report
Failure-mode lists; per item what changed; deviations and why; test names and results; verbatim drive results and artifact paths; checks; commit SHAs; gaps.
