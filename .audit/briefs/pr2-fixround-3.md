Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\agents\poteto-agent.md in full and act as it says. The poteto-mode skill file is C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\poteto-mode\SKILL.md. You are the fix-round-3 owner for PR 2, the last fix round. Keep the diff small. Do not spawn subagents. Do not push, open a PR, or deploy.

## Where you work
Only C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr2 (branch feat/generic-item-details, HEAD 8d0feb2; untracked `.audit/` there is yours from earlier rounds). Run every command there; edit by absolute path. Never write elsewhere. Follow AGENTS.md. Build: `$env:CARGO_TARGET_DIR = "C:\Users\pho\Source\MediaFlick-Desktop\build\cargo-target"` then `just build`. One `just verify` at a time; never sign out of the account; on a stall `just verify-doctor` (+ `just verify-cleanup`) and rerun once.

## Context (consolidated)
- Briefs: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\briefs\pr2-generic-details.md, pr2-fixround-1.md, pr2-fixround-2.md. Design: ...\.audit\jellyfin-libraries.design.md.
- Fix round 2 (`git diff 6e25ef0 8d0feb2`) passed its drives. Review round 3 found the items below. A partial verifier at 8d0feb2 passed generic-detail (both servers), top-level-titles (account) and browse (demo).
- Deferred to PR 3, do not do here: a refresh signal for live-only items (albums, playlists, photo folders) after remote changes.

## Test rule (owner's hard rule)
No unit tests after code. Item 1 has no end-to-end path (a real server also pushes the change over the socket, so the sweep-only path cannot be isolated in a drive). So for item 1: first write down every way the change could fail (top of your report and in the test commit's body), then write a loopback cycle test in `src/library/sync/cycle/tests.rs` through the real cycle path that fails on the current code, commit it failing, then implement.

## Act on
1. **The identity sweep updates watched state silently** (reviewer B; predates this PR, fixed here because it is the same refresh path). `identity_sweep` writes user data through `upsert_user_data`, which returns only a count into `report.user_data_refreshed`; no `LibraryChangeBatch` is emitted, so after a reconnect (`Trigger::Requested` forces the sweep) or on the hourly sweep, a watched/favorite change the socket missed never refreshes Home, title pages or open grids. Use `Library::apply_user_data` (which already upserts and returns the changed-item batch with contexts) in the sweep and merge its result into `report.changes`, so the existing `library_changed` fires. The test must assert that a sweep that changes only user data produces a change batch naming the item, and that a sweep with no user-data change produces none.

## Batched nits (do them)
- One classifier: `browse::live_detail(&dto)` returning `Generic(GenericItemDetail) | Title` (or similar), so the handler cannot call `write_live_title` with a generic DTO; `write_live_title` propagates an ingest error as `DetailError::Storage` instead of `let _ =`.
- The live-children debug line: log after the account check passes, worded "fetched a container's children from the server", so a rejected request is never counted; update the drive's matcher.
- `DetailHeroLayout` / `GenericItemPage`: keep one source for the square frame's fit (drop the explicit `fit: "contain"` for square or drop the default; pick one).
- `companion/mod.rs`: log a cache-read error from `catalog_kind` like `remote.rs` does.
- Rename `LIVE_CHILDREN_PAGE_SIZE` to a name that says it is the maximum limit, define it in the library domain next to `DEFAULT_PAGE_SIZE` so `browse.rs` no longer imports the Jellyfin client module for it; keep the client's own clamp (AGENTS keeps request bounds at the client).
- `query-client.ts` `invalidateLibraryChanged`: derive the per-scope refresh rules before the predicate so each scope's set reads in one place.
- `MediaCard`: rename `title` to `catalogItem`.
- One helper for the stale-account check used by `item_detail` and `children`.

## Dismissed (do not do)
400 for an oversized limit (no caller sends one); reopening the jump miss (traced to an older build; 30 clean runs).

## Then
- Checks: `just rust-quality`, `just test` (rerun once on a loopback stall), `pnpm --dir ui lint`, `pnpm --dir ui test`, `pnpm --dir ui build`, `git diff --check feat/library-registry...HEAD`.
- Drives on the final HEAD, fresh run ids prefixed `fr3-`: `generic-detail` (demo and `--account https://jellyfin.ph0c.de`), `top-level-titles --account`, `browse` (demo), `home-library --account`, `library-parity` (demo), `library-filters` (demo only; it signs out), `libraries-api` (demo). If a libmpv runtime is staged in `build/` (copy `build/libmpv-windows-x64` from C:\Users\pho\Source\MediaFlick-Desktop\build if missing), also run `playback --account https://jellyfin.ph0c.de`; otherwise report it as not run and why.
- Small ordered commits, each building (failing test before the code). No AI attribution.

## Report
Failure-mode list for item 1; test name with before/after; per nit what changed; deviations; verbatim drive results and artifact paths; checks; commit SHAs; gaps.
