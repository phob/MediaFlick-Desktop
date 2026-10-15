## Why

MediaFlick only knows "movies" and "series" across the whole server. To show every Jellyfin library under its own name, the app first has to know which libraries the user can see and which titles each one holds. This PR adds that, with no visible UI change yet. It is the first step of a stack; browsing, details, startup and Home shelves follow.

## What changed

- Each sync cycle reads the user's libraries from `/UserViews`, maps each `CollectionType` to a library kind in one function, and records which movies and series belong to each movies or TV shows library.
- `GET /api/libraries` lists the libraries. `/api/libraries/{id}/items|letters|options|latest` serve movies and TV shows libraries from the local catalog. Other library types answer 501 until the browse PR.
- The library steps are best-effort: if `/UserViews` or a library walk fails, catalog sync carries on and the missed work is retried next cycle. A walk that comes back short of Jellyfin's count never shrinks a library.

## Scope

- No UI rendering changes. The TypeScript contract types and client methods are in place for the next PR.
- Music, books, photos and other library types are listed but not browsable yet.
- Known gap: if the last movies or TV library with titles is removed while an empty one stays listed, the removed titles stay until the next full re-sync. The follow-up is to delete titles whose only library was removed, using the new membership table.

## Blast Radius

The local catalog schema goes to 21, so the first launch after updating rebuilds the catalog once and keeps the signed-in session. The sync loop gains two request types (one `/UserViews` per cycle, plus id-only walks per movies or TV shows library when membership is owed). A failure in either no longer affects catalog sync. `/api/items` now answers 400 for an unknown `kind` and ignores an invalid `favorite` value; the UI only sends valid values.

## Verification

- `just verify libraries-api` on the demo server and with `--account` on a real server: 26 of 26 checks. The account run lists Collections, Movies and Shows exactly as Jellyfin does, and each grid holds the same ids as Jellyfin (685 movies, 97 series, none missing or extra).
- `top-level-titles` (both servers) and `browse` (demo) still pass.
- Loopback sync tests cover a failed library list, a failed or short membership walk, a lost connection, a rate limit, removed libraries and owed work across failed cycles. `just rust-quality`, `just test` (234 passed) and the UI lint, test and build checks pass.
