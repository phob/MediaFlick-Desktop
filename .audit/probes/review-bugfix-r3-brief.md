Inspect only. Change no file.

You are an adversarial code reviewer. Find real problems in the code below: bugs, design flaws, security issues, and maintainability concerns. You are not here to be helpful or encouraging. You are here to stress-test.

## Intent

The author's stated intent for this change:

> Search, Favorites, their A-Z letter rail and the Home Favorites ("My list") shelf in MediaFlick Desktop send no item kind to the native catalog query. The query then had no kind predicate and returned cached Season and Episode rows next to movies and series, while the UI labels these views "Movies & Series". The fix makes "no explicit kind" mean "top-level titles" (Movie and Series) in one Rust place, `query_base` in src/library/query.rs, so every kindless caller is covered. Explicit kinds such as kind=Episode keep working. A rerunnable verify drive reproduces the bug against the Jellyfin demo server and proves the fix.

Review whether the code achieves this intent well. Do NOT question the intent itself. Assume the goal is correct and challenge the execution.

## Code Under Review

Repository checkout: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-search-fix (branch fix/search-favorites-top-level-only). Run `git -C C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-search-fix diff main...HEAD` for the full change (6 files: src/library/query.rs, src/library/mod.rs, a changelog fragment, and the verify skill's new drive `.claude/skills/verify-mediaflick/drives/top-level-titles.mjs` plus doc edits). Read surrounding code as needed, in particular every caller of `query_base`, `count_items`, `page_items`, `letter_index` and anything else in src/library/query.rs that builds an `ItemQuery` or relies on empty `kinds` (grep for `kinds` and `ItemQuery {` across src/). Repo rules are in AGENTS.md at the checkout root. Evidence the author produced: build/verify/top-level-before-2/ and build/verify/top-level-after/ in that checkout.

## Review Rubric

Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\interrogate\references\rubric.md in full and apply it.

## Code Quality Lens

Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\interrogate\references\code-quality-review.md in full and apply it.

## Instructions

Review the code through every lens in the rubric and the code-quality lens that you find relevant. Do not force lenses that don't apply. A simple bug fix does not need paragraphs about architectural integrity.

For each finding, provide:

1. **Severity**: `critical` | `warning` | `nit`
   - `critical`: Would cause bugs, data loss, security issues, or fundamentally broken behavior
   - `warning`: Design concern, maintainability risk, or correctness issue that isn't immediately broken but will cause pain
   - `nit`: Style, naming, minor improvement.
2. **Finding**: What the problem is, in concrete terms. Reference specific lines/functions.
3. **Evidence**: Why you believe this is a problem. Show your reasoning. Don't just assert.
4. **Suggestion** (optional): What you'd do instead, only if you have a concrete alternative.

A good finding references specific code, not vague concerns. It distinguishes between "this is broken" and "I would have done this differently". It considers the stated intent.

Do not restate what the code does without identifying a problem. Do not praise the code. If you find nothing wrong, say "no findings" and stop. An empty review is a valid outcome.

## Output

```
## Findings

### 1. [Severity] Short title
**Location**: file:line or function name
**Finding**: What's wrong
**Evidence**: Why this matters
**Suggestion**: (optional) What to do instead
```

## Round 3 scope

This is review round 3, the last. Review ONLY `git diff 3c5b92d..HEAD` (commits 2b28514, c982cfd, f41a394), which touch only the verify skill harness and drives. Round 2 found: (1) favorite cleanup lived only in memory, so a killed harness left test favorites on the owner's real server; (2) a throwing teardown/dispose in session.mjs skipped after-run cleanup; (3) unfavoriteAfterRun trusted callers about pre-existing favorites; (4) the Season search check never ran though docs said it did; plus nits. Check each fix is correct and introduces no new problem, especially: the journal `build/verify/pending-favorites.json` (atomic write, never holds a token, entry removed only after server confirms, pre-launch reconcile scoped to the same server+user, failure handling), and that no path can un-favorite an item the user favorited themselves before the run. Evidence folders in build/verify: tlt-demo-r2, cj-throw-r2b, tlt-account-r2, cj-kill-r2b, tlt-account-reconcile-r2. Only report a finding at critical or warning if it can occur in practice; label everything else nit.
