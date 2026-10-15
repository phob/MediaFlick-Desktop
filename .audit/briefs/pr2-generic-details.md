Read C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\agents\poteto-agent.md in full and act as it says. The poteto-mode skill file is C:\Users\pho\.claude\plugins\cache\t3-pstack\t3-pstack\2.0.0-rc.3\skills\poteto-mode\SKILL.md. You own PR 2 in a stacked Feature delivery. Do not spawn subagents. Do not push, open a PR, or deploy.

## Where you work
Only C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\lib-pr2 (branch feat/generic-item-details, based on feat/library-registry at c2d45df = PR 1, which is stacked on fix/search-favorites-top-level-only = PR 0). Run every command there; edit by absolute path. Never write elsewhere. Read AGENTS.md first and follow it (shared shadcn components in `ui/src/components/ui/`, theme tokens, Sonner, lucide icons, changelog fragment, checks table, contract changes carried through every boundary).

Build (Windows): `$env:CARGO_TARGET_DIR = "C:\Users\pho\Source\MediaFlick-Desktop\build\cargo-target"`, `pnpm --dir ui install --frozen-lockfile`, then `just build`. Cargo may wait on a shared lock. For `just verify`, read `.claude/skills/verify-mediaflick/SKILL.md`; one verify at a time; never sign out of a `--account` session; on a stall run `just verify-doctor` (and `just verify-cleanup` if it reports leftovers) and rerun once. This machine has known loopback flakiness.

## The design (the contract)
Read C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09\.audit\jellyfin-libraries.design.md in full, including the "Synthesis decision" amendments at the end, which override earlier sections. Grounding with measured Jellyfin facts: ...\.audit\jellyfin-libraries.grounding.md. Read `git log --oneline main..feat/library-registry` and the PR 1 description in ...\.audit\briefs\pr1-body.md to see what PR 1 built (registry, `CatalogKind`, `TitleKind`, `KindFilter`, `item_libraries`, `/api/libraries*`, `BrowseError`). PR 1's deviations from the design are real: build on the code, not the design's sketch, where they differ, and report it.

## Owner's product decisions that apply here
- Every item type gets a details page. Movie, Series, Season and Episode keep today's full title page from the cache. Every other Jellyfin type (MusicAlbum, Audio, MusicArtist, Book, AudioBook, Photo, PhotoAlbum, Video, MusicVideo, BoxSet, Playlist, Folder, TvChannel, Trailer, ...) gets a generic page: artwork, name, year, runtime, overview, genres (plain chips, no links), studios/people via the existing `/api/item/{id}/about`, and for folders/containers a grid of their children. No TMDB, Letterboxd, ratings, collection chip, MediaInfo, external links, Play or Favorite on generic pages (design amendment 8: Play comes later).
- A Movie or Series is a title page wherever it lives, including mixed libraries.
- Cards keep the 2:3 frame; add `primaryAspect` (`PrimaryImageAspectRatio`) to `ItemSummary` and use `object-contain` when `primaryAspect >= 0.8` so square album covers are not cropped (design amendment 3).

## Your PR: "PR 2: Generic details" from the design's Delivery plan
- `DetailView` with a `layout: "title" | "generic"` discriminant on `/api/item/{id}`; `GenericItemDetail`; `BaseItemDto.is_folder`; the title layout gains `libraryId` (use PR 1's membership; seasons and episodes resolve through their series; add `library_of` in the storage module).
- `/api/item/{id}/children`: catalog containers (Series, Season) keep today's cached reconcile; any other container gets a live, paged list (`ParentId=<id>`, sensible sort by index/name). Bound limits, validate ids.
- UI: `ItemDetail.tsx` switches on `layout`; new `GenericItemPage` built from existing detail components (`DetailHeroLayout`, `DetailFacts`, etc.); `ItemGrid` can render a container's children.
- Remove the "not Movie means Series" collapses in `ui/src/lib/navigation.ts` (`defaultDetailNavigationState`) and `ui/src/components/detail/DetailHero.tsx` (`genreHref`); back-navigation without state goes to the item's library when known (route `/library?library=<id>` does not exist yet, so for now go Home for generic items and keep the current Movies/Series destination for title items with a known title kind), and the "not Movie => Series" fallback must disappear. Do not build the library route or sidebar (PR 3).
- Changelog fragment (user-facing).

## Test rule (owner's hard rule)
Never write unit tests after the code they cover. Prefer E2E. If you add any isolated test, first write down every way the behavior could fail, commit the failing test, then the code. Keep existing tests passing; update them for contract changes.

## Verification (E2E, artifact required)
Write `.claude/skills/verify-mediaflick/drives/generic-detail.mjs` per the design's PR 2 "Proof": on the demo server, read the Music library's first MusicAlbum id from Jellyfin, open `/item/<id>`, assert no TMDB/Letterboxd/Play/Favorite/ratings controls, assert the track children render and their count equals Jellyfin's `ChildCount`, assert the album cover is not cropped (object-contain applied); then open a Movie and assert the title page still renders as before (collection chip / ratings slots / Play). On `--account https://jellyfin.ph0c.de`, also open one BoxSet from the Collections library and assert the generic page lists its movies and that clicking one opens the full movie title page. Produce `generic-detail.json` plus screenshots. Document the drive in SKILL.md. Also rerun `browse`, `top-level-titles` (demo and account) and `libraries-api` (demo) to prove nothing regressed.

Checks: `just rust-quality`, `just test`, `pnpm --dir ui lint`, `pnpm --dir ui test`, `pnpm --dir ui build`, `git diff --check feat/library-registry...HEAD`.

Commit in small ordered commits (contract types, native detail/children, UI, drive). No AI attribution.

## Report
What you built, deviations from the design and why, verbatim drive results with artifact paths, check results, commit SHAs, and what PR 3 (library browse + sidebar + live tier) must know.
