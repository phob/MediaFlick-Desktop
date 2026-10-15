## Why

Only movies, series, seasons and episodes had a details page. To show every Jellyfin library, every item needs one. This PR gives albums, tracks, box sets, books, photos, folders and every other item type a simple details page, and keeps the full title page for movies and shows wherever they live.

## What changed

- `/api/item/{id}` answers `layout: "title"` for movies, series, seasons and episodes (from the local catalog, now with `libraryId`) and `layout: "generic"` for every other type (read live). The generic page shows artwork, year, runtime, overview, genres, credits, studios and, for folders and albums, a paged grid of their contents. It has no Play, favorites, ratings, TMDB or Letterboxd.
- Square art such as album covers is shown whole on cards and on the page. The "anything that isn't a movie is a series" fallbacks in navigation, genre links and the sidebar are gone.
- Watched and favorite changes now refresh open box set and folder grids, including changes made on another device. The hourly and reconnect sync now also reports watched-state changes it picks up, so Home and title pages refresh too.

## Scope

- No library browse page or sidebar list yet; that is the next PR. Generic items go "Back to home" until then.
- Albums, playlists and photo folders don't refresh after a change on another device, because their items aren't in the local catalog. That is planned with the live library grids in the next PR. Playlists show contents sorted by name, not their manual order.
- Books, photos, home videos and mixed libraries were not available on either test server.

## Blast Radius

Movie and series pages keep their look and behaviour; the verifier compared them with `main`'s screenshots. The media, playback-preference and Next Up routes now answer 500 when the local catalog can't be read, instead of treating the item as uncached. Every `/api/item/{id}/…` route now rejects a malformed id with 400.

## Verification

- `just verify generic-detail` on the demo server (65 of 65 checks) and with `--account` on a real server (63 of 63): album, track, folder, box set, series, season, an uncached series, a movie with a collection chip, and a box set card that updates after a favorite and a watched change made in Jellyfin. Two bugs were reproduced in this drive before their fixes.
- `top-level-titles`, `browse`, `home-library`, `library-parity`, `library-filters` and `playback` on both Built-in libmpv and External mpv pass.
- A loopback sync test pins that the sweep reports watched-state changes. `just rust-quality`, `just test` (235 passed) and the UI lint, test and build checks pass.
