Inspect only. Change no file.

You are an adversarial code reviewer. Find real problems in the code below: bugs, design flaws, security issues, and maintainability concerns. You are not here to be helpful or encouraging. You are here to stress-test.

## Intent

> MediaFlick Desktop will show every Jellyfin library. This PR is the native groundwork (PR 1 of a stack). It reads the signed-in user's libraries from Jellyfin `/UserViews` every sync cycle into a cached `libraries` table, maps each library's CollectionType to a `LibraryKind` in exactly one function, and decides per kind through one exhaustive table whether its grid comes from the local catalog (movies, tvshows) or will be served live from Jellyfin later (every other type answers 501 in this PR). For catalog libraries it records which movies and series belong to which library in an `item_libraries` table, filled by an id-only walk per library that never replaces a non-empty set with an empty or truncated answer. It adds `GET /api/libraries` and `/api/libraries/{id}/items|letters|options|latest` for catalog libraries, with id validation (404), capability validation (400) and limit clamps. It replaces the ad hoc "no kind means Movie and Series" default with a `CatalogKind` enum and `KindFilter` type. Schema goes to 19, which rebuilds the local cache once. The existing server-wide sync loop (bootstrap, watermark, identity sweep with guarded deletion) must keep working unchanged. No UI rendering changes yet.

The design this PR implements is C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\jellyfin-libraries.design.md (read its "Synthesis decision" amendments at the end; they override earlier sections). Deviations the author reported: no per-kind live item types yet (dead-code lint), `library_of`/`first_of` deferred, `MediaLibrary.kind()` derived from the stored raw CollectionType, a `/Users/{id}/Views` fallback for servers before 10.9, no `librariesChanged` event yet, verify harness `ctx.jellyfin` uses the app's stored session when a run has no `--account`.

Review whether the code achieves this intent well. Do NOT question the intent itself. Assume the goal is correct and challenge the execution.

## Code Under Review

Checkout: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr1 (branch feat/library-registry). The change is `git -C C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr1 diff fix/search-favorites-top-level-only...feat/library-registry` (six commits on top of the already-reviewed search fix). Read surrounding code as needed: src/library/sync/cycle.rs and sync.rs (the existing loop and its guards), src/library/session_store.rs (sign-out and account switch), src/library/db.rs (schema recreation), src/shell/cef/api/ (route conventions), src/jellyfin/api/items.rs. Repo rules: AGENTS.md at the checkout root (lock guards released before network calls, no unwrap/expect in production, input validation, contract changes carried through every boundary). Evidence: build/verify/libraries-api-demo-2, libraries-api-account-1, top-level-titles-account-pr1-b in that checkout (produced before a clean rebase onto the search fix's final commit).

Pay particular attention to: concurrency between the sync worker writing `libraries`/`item_libraries` and API handlers reading them; what happens on account switch, sign-out, and a schema-recreated database; whether a failed or partial `/UserViews` or membership walk can empty a grid or delete catalog rows; SQL built from request input; whether the `KindFilter` change preserved every existing caller's behavior.

## Round 2 scope

This is review round 2. Review ONLY `git -C C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr1 diff 7584bd9..HEAD` (commits 82d6fec..e02c5bd). Round 1 found: library steps (`/UserViews`, membership walks) could fail the whole sync cycle; owed follow-up work (membership walk for a new library, forced sweep for a removed one) was lost when a cycle failed after the library-set commit; membership walks trusted short pages; the UI forwarded a raw `kind` that `/api/items` now rejects; an unloaded registry answered 404; duplicate query parser and option SQL; the drive compared counts not members. The author's fix: a `best_effort()` rule in sync.rs (auth and cancellation still propagate), persisted `sync.sweep_owed` / `sync.membership_owed` flags written in the library-set transaction, completeness checked against `TotalRecordCount` with `SortBy=DateCreated,SortName`, `item_query(request, Params::Strict|Lenient)`, 503 for Unknown, schema bumped to 20 to drop `libraries.synced_at`, nine new loopback cycle tests in `src/library/sync/cycle.rs` committed failing before the code (82d6fec). Reported deviations: no `Id` tie-breaker (measured: Jellyfin ignores `SortBy=Id`), `LibrarySetChange` deleted, letters on a live library answer 400 not 501, invalid `favorite` values on `/api/items` are now ignored instead of filtering to non-favorites.

Check each fix is correct and introduces no new problem. In particular: can `best_effort` swallow an error the old loop treated as fatal for catalog sync; can the owed flags get stuck set (walking every cycle) or get cleared without the work; do the new tests assert behavior against literal expected values rather than mirror the implementation; is the schema 20 bump safe for every existing database. Evidence: build/verify/fr1-* in that checkout. Only report critical or warning for problems that can occur in practice; label the rest nit.

## Review Rubric

Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\interrogate\references\rubric.md in full and apply it.

## Code Quality Lens

Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\interrogate\references\code-quality-review.md in full and apply it.

## Instructions

Review through every lens you find relevant. For each finding give Severity (`critical` | `warning` | `nit`), Finding (specific lines/functions), Evidence (your reasoning), and optionally Suggestion. Only use critical or warning for problems that can occur in practice. Do not restate what the code does without identifying a problem. Do not praise. "No findings" is valid.

## Output

```
## Findings

### 1. [Severity] Short title
**Location**: file:line or function name
**Finding**: What's wrong
**Evidence**: Why this matters
**Suggestion**: (optional) What to do instead
```
