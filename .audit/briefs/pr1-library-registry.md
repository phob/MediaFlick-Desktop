Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\agents\poteto-agent.md in full and act as it says. The poteto-mode skill file is C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\poteto-mode\SKILL.md. You own one PR in a stacked Feature delivery. Do not spawn subagents. Do not push, open a PR, or deploy.

## Where you work
Only C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr1 (branch feat/library-registry, based on fix/search-favorites-top-level-only at 3c5b92d, which is PR0 and may still get small review fixes). Run every command there and edit by absolute path. Never write in any other worktree or C:\Users\pho\Source\MediaFlick-Desktop. Read AGENTS.md first and follow it.

Build (Windows): `$env:CARGO_TARGET_DIR = "C:\Users\pho\Source\MediaFlick-Desktop\build\cargo-target"`, `pnpm --dir ui install --frozen-lockfile`, then `just build`. Other agents share that target dir; cargo waits on its lock, so be patient and never delete it. For `just verify`, read `.claude/skills/verify-mediaflick/SKILL.md`; one verify at a time in this checkout.

## The design (the contract)
Read C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\jellyfin-libraries.design.md in full, including the "Synthesis decision" amendments at the end, which override the sections above them. Grounding with measured Jellyfin facts: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\jellyfin-libraries.grounding.md. The design is the contract. If the code forces a deviation, make the smallest one, and report it with the reason; do not silently reshape the design.

## Your PR: "PR 1: Library registry and catalog membership (native only)" from the design's Delivery plan
Build exactly that scope:
- `src/library/libraries.rs`: `LibraryId`, `LibraryKind::from_collection_type` (the only reader of CollectionType), the exhaustive `profile()` table, `BrowseCapabilities` statics, `MediaLibrary`, `LibraryRegistry { Unknown, Known }`.
- `CatalogKind` and `KindFilter` in `src/library/model.rs`/`mod.rs`, absorbing PR0's `TOP_LEVEL_KINDS` default. `CatalogKind::ALL` derives `SYNCED_ITEM_TYPES` and `is_synced_kind`. Move the other Movie/Series literals into the registry or leave them with a one-line reason in your report (amendment 6: `items.rs:194`, `:211`, `catalog.rs:585`).
- Schema 19 (`libraries`, `item_libraries`), `db::CATALOG_TABLES` used by the forget path in `session_store.rs`.
- `fetch_user_views` (`GET /UserViews?userId=`) and `fetch_library_member_ids` in `src/jellyfin/api/items.rs`; `UserViewDto` in `jellyfin::api::model`.
- `src/library/sync/membership.rs`: `refresh_libraries` and `membership_pass`, wired into the cycle as steps 1, 2, 5 and 6 of the design. Amendment 2: never replace a non-empty membership set with an empty answer; a truncated walk skips replacement.
- `Library::{libraries, replace_libraries, replace_membership, library_of, library_filter_options}`.
- Routes: `GET /api/libraries` and the catalog-tier `/api/libraries/{id}/items|letters|options|latest` in a thin `src/shell/cef/api/libraries.rs`. Live-tier libraries answer 501 for items/letters/options/latest in this PR. Validate ids against the registry (404), reject sorts/filters outside capabilities (400), clamp limits.
- `librariesChanged` shell event if the design's PR 1 needs it for cache invalidation; add the matching TS event type only.
- UI: add the TS contract types and `api.libraries()`/library route client methods in `ui/src/lib/api/*` only. No UI rendering changes in this PR.
- A changelog.d fragment (user-facing wording; this PR has no visible UI change, so phrase it as groundwork only if needed, or skip the fragment if nothing user-facing changed and say why).

## Verification (E2E, artifact required)
Write the drive `.claude/skills/verify-mediaflick/drives/libraries-api.mjs` exactly as the design's PR 1 "Proof" describes, producing `libraries-report.json` that shows `/api/libraries` names, order and kinds equal `/UserViews`, and each catalog library's `total` equals Jellyfin's `/Items?ParentId=<id>&Recursive=true&IncludeItemTypes=<title kind>&Limit=0` TotalRecordCount. Run it on the demo server (`--url https://demo.jellyfin.org/stable`) and on `--account https://jellyfin.ph0c.de` (cached session exists; never sign out; if it 401s, report and skip). Also rerun the existing `top-level-titles` drive in both modes to prove PR0 behavior survived `KindFilter`. Document the new drive in the skill's SKILL.md drive list.

Checks: `just rust-quality`, `just test`, `pnpm --dir ui lint`, `pnpm --dir ui test`, `pnpm --dir ui build`, `git diff --check`. Keep existing tests passing; update them for contract changes; write no new unit tests.

Commit in small ordered commits (types and schema, sync membership, routes, drive). No AI attribution in messages.

## Report
What you built per bullet, any deviation from the design and why, verbatim pass/fail lines and artifact paths for every drive run, check results, commit SHAs, and anything the next PR (generic details) must know.
