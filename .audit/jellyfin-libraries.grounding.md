# Grounding: support every Jellyfin library

Repo: C:\Users\pho\.t3\worktrees\MediaFlick-Desktop\t3-689fae09. Read AGENTS.md first (architecture, persistence, validation rules).

## Product decisions (from the owner, final)

1. Show every Jellyfin library the signed-in user can see, of every type, now. That includes music, books, photos, home videos, music videos, box sets, playlists, mixed-content libraries. Type-specific features come later.
2. Movies and TV shows libraries keep today's features (TMDB, Letterboxd, ratings, seasons, next up...). Any number of each, each in its own sidebar entry under its Jellyfin name.
3. Every other type gets only a base experience: a browse grid for the library and a generic details page. No TMDB, no Letterboxd, no special features. Books need no reading support. They only need to be displayed.
4. Home: one "Latest in <library>" shelf per library, for every library type. Each appears in Home settings (Save/Reset/Discard workflow) and on the Home page for every library that exists.
5. Fix the bug: Search, Favorites and the Home "My list" shelf send no `kind`, so the query has no kind predicate and can return Season and Episode rows (`src/library/query.rs:561`, `ui/src/lib/api/urls.ts:20-28` drops empty values). Confirm it, then fix it.

## Current state (verified)

- Sync: one server-wide walk `GET /Items?userId&Recursive=true&IncludeItemTypes=Movie,Series,Season,Episode` (`src/jellyfin/api/items.rs:16`), bootstrap + incremental by DateCreated watermark + hourly identity sweep with global `retain_ids` deletion (`src/library/sync/cycle.rs`). Client filter `is_synced_kind` (`src/library/model.rs:283`). Other types are dropped.
- Cache: `library.db`, `items` table one row per jellyfin_id, `kind` = Jellyfin `Type` string, `parent_id` = direct parent (`src/library/db.rs:417`). Schema version 18; older versions are dropped and recreated keeping the session (`db.rs:155-197`). It is a rebuildable cache.
- No code reads `/UserViews` or `CollectionType`.
- Browse: `/library?kind=Movie|Series` (`ui/src/lib/library-filters.ts:3,172-177`), static sidebar (`ui/src/components/AppSidebar.tsx:61-66`), title ladder (`ui/src/routes/Library.tsx:201-213`). `ItemQuery` (`src/library/mod.rs:265`, `ui/src/lib/api/types.ts:1173`) carries free-form `kind` and an unused `parentId`.
- Non-Movie collapsed to Series in `ui/src/lib/navigation.ts:21`, `ui/src/components/detail/DetailHero.tsx:51`, `AppSidebar.tsx:295`.
- Durable per-account formats in `accounts.json` tied to the two kinds: browsing memory keys `last|Movie|Series` (`src/preferences/accounts.rs:700`), `StartupDestination::{Movies,Series}` (`src/preferences/viewing.rs:27`), `HomeElementId { BuiltIn | Genre | Collection }` with `deny_unknown_fields` (`accounts.rs:56`).
- Home rows are built natively (`src/shell/cef/api/catalog.rs:269-371`); UI renders whatever rows come back, but `Home.tsx:220-230` hardcodes "View all" URLs.
- Detail: `ui/src/routes/ItemDetail.tsx`, `ui/src/components/detail/*`; native detail handlers in `src/shell/cef/api/catalog.rs` (~900-1010) and `src/shell/cef/api/media.rs`.
- Jellyfin box sets are fetched live (`src/shell/cef/api/collections.rs:112-159`).

## Jellyfin facts (measured, %TEMP%\jfprobe\demo-report.json and own-report.json)

- `GET /UserViews?userId=<id>` lists libraries: `{Id, Name, Type: "CollectionFolder", CollectionType}`. `GET /Users/{id}/Views` is 404 on Jellyfin 12.2.
- CollectionType values: movies, tvshows, music, musicvideos, trailers, homevideos, boxsets, books, photos, livetv, playlists, folders, or absent (mixed content).
- Item `ParentId` is the physical folder, never the library id (0 of 12 top-level items on the demo server). `parent_id` is not a library key.
- `GET /Items?ParentId=<libraryId>&Recursive=true` returns the library's items including nested ones.
- Demo music library holds Audio, MusicAlbum, MusicArtist, Folder. Movies holds Movie + Folder. Shows holds Series, Season, Episode, Folder. Owner's server: boxsets library holds BoxSet x85; Movies 685; Shows 97 series / 345 seasons / 4374 episodes. No item was in two libraries. Sum of library items equals the server-wide synced total, so `/Items?userId=` already applies per-user folder access.
- `GET /Items/Latest?userId&ParentId=<libraryId>&Limit=N` is Jellyfin's per-library "Latest" source (used by jellyfin-web's Latest shelves); it groups episodes into their series.

## Constraints from AGENTS.md worth repeating

- UI talks only to the native API in `src/shell/cef/api/`; contract changes go through Rust types, `ui/src/lib/api.ts`/`ui/src/lib/api/*`, callers and tests in one change.
- library.db is a rebuildable cache: prefer schema recreation (bump SCHEMA_VERSION) over migration code.
- accounts.json is durable user intent: write through existing atomic save paths; reject unsupported formats without rewriting; no speculative compat wrappers.
- Settings shelves use Save/Reset/Discard. Shared shadcn components only.
- No unwrap/expect in production Rust; release locks before network calls.
- Tests: E2E via the `verify-mediaflick` skill (`.claude/skills/verify-mediaflick/SKILL.md`), each producing an artifact. No new unit tests written after code.
