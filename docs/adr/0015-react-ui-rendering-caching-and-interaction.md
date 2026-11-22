# 0015. React UI caching, virtualization, portal isolation, and interaction patterns

Status: Accepted (2026-11-22). Consolidated from codebase audit of `ui/src/` and `ui/test/`.

## Context

The embedded React 19 UI renders large Jellyfin libraries, horizontal shelves, hover preview panels, live-preview Appearance settings, and randomized Home billboards against a local SQLite backend pushed by native sync events. Naive TanStack Query, virtualization, and DOM/Radix patterns caused refetch storms, scroll jumps, focus traps, and O(N) re-renders.

## Decision

- **TanStack Query caching and invalidation.**
  - `refetchOnWindowFocus` is disabled globally because local SQLite data is updated via native CEF shell push events.
  - Shared query keys never use `skipToken` (TanStack Query overwrites shared options with the last-rendered observer, wiping out active fetchers); optional-ID queries use `optionalIdQuery` (passing `context` untouched so TanStack Query's lazy `context.signal` getter only opts fetchers that read `signal` into unmount cancellation).
  - `useStatus` projects `/api/status` through `sessionStatus` (omitting volatile counts/progress) so structural sharing avoids app-wide re-renders during sync; only the root mounts `useStatusPulse` (500 ms during bootstrap gate, 2 s during background sync) because every observer with `refetchInterval` owns a timer.
  - `/api/billboard` returns a fresh random set per call, so it is pinned with `staleTime: Infinity`, excluded from `invalidateMediaSurfaces`/`invalidateLibraryChanged`, and patched in place by watched/favorite mutations; slides mount lazily (`mounted: Set<number>`) because `opacity: 0` still fetches all 5 backdrops eagerly.
  - `invalidateMediaSurfaces` uses `exact: true` on `item`, `children`, and `nextUp` to preserve cached live prose/credits/trailers/media info; bootstrap `catalog-changed` refreshes only named catalog libraries' Latest shelves while incremental `library-changed`/`live-changed` refreshes every Latest shelf once.
  - Home fans out one query per "Latest in `<library>`" shelf so a slow live library isolates its own failure; new accounts wait only for the first committed catalog page behind `LoadingScreen`, while warm starts also wait for the local Home/billboard snapshot; series detail holds its episode grid skeleton while Next Up resolves for multi-season shows; `CardInlineActions` mounts `useNextUp(id, false)` and refetches lazily on Play click to prevent N+1 storms.
  - Seerr Discover infinite queries use `staleTime: Infinity` + 10 min `gcTime` so Back navigation does not replay every loaded page; `useSeerrRequests` overrides defaults with 30 s polling + focus refetch because Seerr has no push channel; global facet hooks serve the combined view while single-library grids use `/api/libraries/{id}/options`.
  - Playback track preferences always save `(mediaSource, audio, subtitle)` atomically; a non-`accountChanged` HTTP 409 invalidates `mediaInfo` to reload changed server streams.
- **Virtualized grids, batch loaders, and artwork.**
  - `ItemGrid` sizes its canvas upfront from pinned page 0's `total` and reads sparse page queries memoized on `[firstPage, lastPage]` so the scrollbar never jumps and `useQueries` does not churn per scroll tick; only page 0 failure without cache shows `PageErrorState`, while later pages fall back to skeletons.
  - `ItemGrid.tsx` and `CollectionTitleGrid.tsx` disable `react/incompatible-library` in `.oxlintrc.json` because oxlint ignores `"use no memo"` (`TanStack/virtual#1119`); `MediaCard` stays wrapped in `React.memo` relying on React Query structural sharing, omits `loading="lazy"` (pairing overscan prefetch with `decoding="async"`), steps `imageIndex` on `<img>` `onError` to break stale-tag retry loops, and gates live technical metadata on a `160 px` `IntersectionObserver` so off-screen horizontal shelf cards do not flood batches.
  - `useCardBatches` reads `options` through a ref so `register` stays strictly identity-stable (and card hooks depend solely on `context.register`), preventing re-renders from momentarily draining `active` and aborting in-flight batches.
- **Portals, Radix, scrolling, and Signal visual system.**
  - `PreviewContext` lives in `ui/src/lib/preview.ts` (separate from `PreviewCard.tsx`) for Vite Fast Refresh and stores the expanded ID in `useSyncExternalStore` to avoid O(N) card re-renders. `RatingsProvider` and `TechnicalProvider` wrap `AppShell` (not just `<Routes>`) so portalled preview panels read selected-source context.
  - Hover previews ignore non-mouse pointers and `buttons !== 0` (preventing touch-drag popups and mid-drag release clicks), handle straddling `mousedown`-on-card / `mouseup`-on-panel clicks via `onPointerUp`, ignore pointer-initiated focus on Chromium's disabled pending toggles, and listen for `scroll` in capture phase on `window` (since `scroll` does not bubble from nested rails/`.content-viewport`).
  - React bubbles `onFocus`/`onPointerEnter` across portals without matching leave on unmount, so `AppShell` and `use-sidebar-hover` guard with `event.currentTarget.contains(event.target)`. Hover sidebar collapse debounces 250 ms to absorb macOS Screen Sharing's synthetic leave/enter pairs (<= 160 ms max observed), and `UserMenu` holds the sidebar open while Radix's modal `DropdownMenu` disables outside pointer events. `<Command>` inside `DropdownMenuSubContent` stops `keydown` propagation against Radix typeahead; Radix `<SelectItem>` uses `"__settings__"` because empty string values are forbidden.
  - Horizontal rails set `overflow-anchor: none` and reset `scrollLeft = 0` on mount/`resetKey` so prepended synced titles stay visible; scroll restoration saves `scrollTop` during scroll events because React unmount can clamp `scrollTop` to 0 before cleanup; detail backdrops use `overflow-clip` + `h-[100cqh]` + root `isolate` to preserve `position: sticky` and stacking.
  - Appearance live preview re-skins real components via scoped `data-*` attributes and portals its hover preview to a body-level container carrying the same attributes to escape `.content-viewport`'s CSS paint containment. YouTube trailer embeds delay reveal by 2000–2200 ms on every `state === 1` transition (including deferred start-offset seeks) with `0 ms` exit delay to conceal YouTube's center overlay.
  - The "Signal" visual language enforces green-cast near-blacks, 2 px/4 px near-square corners (pills only on progress tracks), uppercase monospace measured facts, corner brackets over hover zoom, and full-bleed hairline shelf rules.

## Rejected alternatives

- Storing hovered preview state in React Context: triggers O(N) re-renders across every card on the page on every hover.
- Append-as-you-go infinite scroll for local SQLite libraries: causes scrollbar thumb jumping and breaks deep A-Z jumps.
- Duplicating a separate CSS theme palette for Appearance live preview: drifts from production theme tokens.

## Consequences

- UI tests under jsdom dispatch cancelable `Event` instances for drag-drop guards (jsdom lacks `DragEvent`) and allow React StrictMode double-fetches on page 1.
- Any new card-batch context or shared query hook must preserve `register` stability and avoid `skipToken`.
