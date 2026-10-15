Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\agents\poteto-agent.md in full and act as it says. The poteto-mode skill file is C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\poteto-mode\SKILL.md. You are the fix-round-1 owner for PR 1 of a stacked Feature delivery. Do not spawn subagents. Do not push, open a PR, or deploy.

## Where you work
Only C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr1 (branch feat/library-registry, HEAD 7584bd9, stacked on fix/search-favorites-top-level-only 4b6c7ab). Run every command there; edit by absolute path. Never write elsewhere. Follow AGENTS.md. Build: `$env:CARGO_TARGET_DIR = "C:\Users\pho\Source\MediaFlick-Desktop\build\cargo-target"` then `just build`. One `just verify` at a time.

## Context (consolidated)
- Design contract: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\jellyfin-libraries.design.md (the "Synthesis decision" amendments at the end override earlier sections). Brief the previous owner worked from: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\briefs\pr1-library-registry.md.
- The previous owner built PR 1 (`git log --oneline fix/search-favorites-top-level-only..HEAD`): registry, schema 19, `refresh_libraries` + `membership_pass` in `src/library/sync/membership.rs`, `/api/libraries` routes in `src/shell/cef/api/libraries.rs` and `src/library/browse.rs`, `CatalogKind`/`KindFilter`, TS contract types, drive `libraries-api.mjs`. A verifier passed all checks and five drives on both servers at 7584bd9.
- A two-model review found the problems below. Fix exactly these.

## Test rule (owner's hard rule)
Never write tests for code after the code exists. For every behavior this round CHANGES or ADDS (items 1-3), first write down every way it could fail (put that list in `.audit`-free form at the top of your report and in the commit body of the test commit), then write loopback cycle tests through the real `run_cycle_inner` path using the existing fake-Jellyfin `serve()` harness in `src/library/sync/cycle.rs` tests, then implement. Commit the failing tests before the implementation so history shows it. Do not add tests for unchanged existing code; list those gaps in your report instead.

## Act on
1. **Library steps must not break catalog sync** (both reviewers). Today `refresh_libraries(...)?` runs first in the cycle and membership walks use `?`, so a 5xx/timeout/decode error from `/UserViews` or a membership page aborts the whole cycle, sets `META_LATEST_FAILURE`, blocks `ownership_available`, and on a fresh sign-in blocks bootstrap. Make both best-effort: on any error except unauthorized and cancellation, log it, keep the stored registry and membership, and continue; they must not set the latest-failure flag. Only catalog steps gate ownership.
2. **Persist owed work** (both). `replace_libraries` commits the library-set change at the top of the cycle, but the follow-up (membership pass, forced identity sweep for a removed library) is decided from in-memory state, so a cycle that fails afterwards loses it. Persist `membership owed` and `sweep owed` meta flags in the same transaction as the library-set change; set `membership owed` again when a membership walk fails or is truncated; clear each only after its work succeeded; gate the passes on them (plus the existing triggers: catalog changes and `Requested`). Remove the duplicate first-sync walk, rename the misleading `!libraries.is_empty()` predicate, and do not treat a primary-image-tag or position change as a reason to re-walk membership (rename `renamed` to what it means). Drop the unused `libraries.synced_at` column.
3. **Membership walk completeness** (both). `member_ids` treats any short page as the end. Record `TotalRecordCount` from the first page; if the walk ends with fewer distinct ids than that total, treat it as truncated (keep previous membership, set membership owed). Add a stable tie-breaker to the sort (e.g. `SortBy=DateCreated,SortName` plus `Id` if Jellyfin accepts it; check the existing identity sweep for what it uses).
4. **UI forwards a raw `kind`** (both). `ui/src/lib/library-filters.ts` `libraryItemQuery` sends the raw URL `kind`, which `/api/items` now rejects with 400. Send only validated catalog kinds (same normalization as `libraryViewKinds`).
5. **"Not loaded yet" is not "not found"** (reviewer A). Before the first `/UserViews`, `LibraryRegistry::Unknown` makes every `/api/libraries/{id}/*` route answer 404. Add a distinct error mapped to 503 with a clear message; keep 404 for a known registry that lacks the id. `Library::libraries()` must read the `sync.libraries_at` flag and the table in one snapshot so a concurrent clear never yields `Known(vec![])`.
6. **One query parser, one option query** (both on options, B on parser). `BrowseQuery` copies ten `ItemQuery` fields and `browse_query` re-parses what `library_query` parses. Replace with one parser that builds `ItemQuery` with a strict/lenient switch (lenient keeps `/api/items` behavior), run the capability check on `ItemQuery`, delete `BrowseQuery`. Give the existing `genres()` and `official_ratings()` an optional library scope and delete `library_filter_options`'s own SQL. Clamp limits in one layer.
7. **Drive compares members, not counts** (reviewer B). `libraries-api.mjs` compares totals only. Compare sorted id sets per catalog library against Jellyfin `/Items?ParentId=<id>&Recursive=true&IncludeItemTypes=<kind>&Fields=` (paged), and enforce `letterIndex` (letters route answers 400 when the library's capabilities say no).

## Batched nits (do them)
- Narrow types: `GridSource::Catalog { title }` holds only Movie or Series (a `TitleKind` that converts into `CatalogKind`), add `MediaLibrary::catalog_title()` and use it at the three call sites; `KindFilter::Only` holds a non-empty list by type.
- Rename the UI filter-side `LibraryKind` (`"Movie" | "Series"` in `library-filters.ts`) to `TitleKind` so it no longer clashes with the API's `LibraryKind`.
- Move `refresh_libraries` next to the library cache code and the shared `cancelled`/`commit`/`storage_error` helpers into `sync.rs`; add one line on libraries and membership to the sync module doc.

## Dismissed (do not do)
Keeping rows for views with unparsable ids (Jellyfin ids are always hex); a `sqlite_master` check for `CATALOG_TABLES`.

## Then
- Checks: `just rust-quality`, `just test`, `pnpm --dir ui lint`, `pnpm --dir ui test`, `pnpm --dir ui build`, `git diff --check fix/search-favorites-top-level-only...HEAD`.
- Drives, one at a time: `just verify libraries-api` (demo) and `--account https://jellyfin.ph0c.de`, `just verify top-level-titles` (demo) and `--account`, `just verify browse` (demo only). Never sign out of the account. On a stall, run `just verify-doctor` and rerun once; report both.
- Small ordered commits (failing tests before the code they cover). No AI attribution.

## Report
The failure-mode list; per item what changed; any deviation and why; test names and results; verbatim drive results and artifact paths; checks; commit SHAs; gaps you did not cover.
