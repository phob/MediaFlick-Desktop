## Why

Search, Favorites, their A-Z rail and the Home Favorites shelf send no item kind. The catalog query then had no kind filter and returned seasons and episodes next to movies and series. Searching "night" on the demo server showed the episode "Alone in the Night" as a title card.

## What changed

- `query_base` treats a query with no kind as movies and series. Every caller that sends no kind gets the same rule. A query that names a kind, such as `kind=Episode`, works as before.
- The unused `parentId` and `seriesId` item filters are removed from the native API and the TypeScript types.
- The verify skill has a new `top-level-titles` drive. With `--account`, it favorites one movie and one episode as the test user, checks the Favorites views, and removes both favorites again. A small journal removes test favorites that a killed run left behind.

## Scope

- Searching for an episode or season by name now finds nothing. Episode search results would need their own section on the search page.
- No other kind behavior changes.

## Blast Radius

Every kindless catalog query changes: Search, Favorites, the A-Z rail and the Home Favorites shelf. Nothing in the app sent `parentId` or `seriesId`. The account drive writes favorites to the server it runs against. It only touches items that were not favorites before the run, and removes them again.

## Verification

- `just verify top-level-titles` on the demo server fails before the fix and passes 12 of 12 checks after it.
- `just verify top-level-titles --account <server>` fails 7 of 16 checks on the old code and passes 19 of 19 on the fix. The test user's favorites are back to 0 afterwards.
- `just verify browse`, `just rust-quality`, `just test`, and the UI lint, test and build checks pass.
