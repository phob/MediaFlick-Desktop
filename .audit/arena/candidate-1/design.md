# Every Jellyfin library in one generalized catalog

Candidate 1. Direction: one generalized cached catalog. Every library and every
browsable title of every type lives in `library.db` and is served by one query
path. Grounding: `.audit/jellyfin-libraries.grounding.md`.

## Problem

MediaFlick shows exactly two hard-coded shelves, Movies and Series. Sync walks
the whole server once (`IncludeItemTypes=Movie,Series,Season,Episode`,
`src/jellyfin/api/items.rs:16`), drops every other type (`is_synced_kind`,
`src/library/model.rs:283`), and nothing reads `/UserViews`. Browse routes,
the sidebar, the title ladder, Home "View all" links, three durable
`accounts.json` formats and about 50 UI `kind` reads assume that world. Three
places collapse "not Movie" into "Series".

The owner wants every library the user can see, by name and in server order,
with full features only for movies and TV libraries and a base grid plus
generic details for the rest. Each library also gets a "Latest in <library>"
Home shelf.

Constraints that shape the design:

- Library membership cannot come from `ParentId`, which is the physical
  folder. It is only known from a per-library walk
  (`/Items?ParentId=<libraryId>&Recursive=true`).
- `library.db` is a rebuildable cache: a schema bump is the migration.
  `accounts.json` is durable: old values must keep loading, and an older app
  must reject a newer file without rewriting it (`json_file.rs`).
- A separate fix in flight (`fix/search-favorites-top-level-only`) makes a
  query with no `kind` return Movie and Series only, in one Rust place. This
  design generalizes that rule to "browsable titles of every library".

## Usage (caller's view)

### What a contributor reads

> The catalog is a list of libraries and the titles in them. Every item row
> has a library (`library_id`) and a placement computed once at ingest:
> `is_title` (shown in a library grid) and `profile` (which feature set the
> item gets). Both come from one table, `LibraryKind::policy()` in
> `src/library/kinds.rs`. To change which types a library type shows, or
> which items get movie or series features, edit that `match`. Do not compare
> Jellyfin type names anywhere else. Rust filters on `profile`, and the UI
> switches on `item.profile`. The wire has no `kind` field to compare.

### Rust: the sync cycle and the query layer

```rust
// src/library/sync/cycle.rs: one cycle
let views = views::fetch_user_views(client, user_id)?;           // GET /UserViews
let refresh = commit(session, &scope, || library.refresh_libraries(&views))?;
for gone in &refresh.removed { report.changes.merge(library.forget_library(gone)?); }
for lib in library.libraries()? {                                  // server order
    bootstrap_library(library, session, &scope, client, &lib, &mut budget, control)?;
    incremental_library(library, session, &scope, client, &lib, &mut report.changes)?;
}

// src/shell/cef/api/catalog.rs: a library grid, and Search with no kind
let page = services.library.query(&ItemQuery {
    scope: Some(library_ref.id.clone()),     // validated against the cache
    sort: ItemSort::DateAdded,
    ..ItemQuery::default()                   // no kinds: titles only
})?;
let hits = services.library.query(&ItemQuery {
    search: Some("abbey road".into()),       // finds the album too,
    ..ItemQuery::default()                   // never a Season or Episode
})?;

// Live DTOs become summaries only through the library, which resolves membership
let next_up = services.library.summaries_from_dtos(&response.items);
```

### Native API

```text
GET /api/libraries
  → { "libraries": [ { "id": "f137…", "name": "Films",  "kind": "movies",  "browse": "movies"  },
                      { "id": "a656…", "name": "Shows",  "kind": "tvshows", "browse": "series"  },
                      { "id": "7e64…", "name": "Music",  "kind": "music",   "browse": "generic" } ] }

GET /api/items?library=7e64…&sort=added&limit=60   → that library's titles (MusicAlbum)
GET /api/items?search=matrix                        → titles of every library
GET /api/items?favorite=true                        → titles of every library
GET /api/items?kind=Movie&sort=added                → cross-library, explicit kind
GET /api/genres?library=7e64…                       → that library's genres
GET /api/item/{id}/children                         → cached seasons/episodes for series,
                                                      live children for any other container
item on the wire: { "id": "b1…", "profile": "item", "typeLabel": "Album", "libraryId": "7e64…",
                    "playable": false, "primaryAspect": 1.0, "name": "Abbey Road", … }
```

### UI call sites

```tsx
// AppSidebar.tsx
const libraries = useLibraries().data ?? []
{libraries.map((library) => {
  const Icon = LIBRARY_ICONS[library.kind]
  return <SidebarMenuItem key={library.id}><SidebarMenuButton asChild
    isActive={activeLibraryId === library.id} tooltip={library.name}>
    <Link to={libraryPath(library)}><Icon /><span>{library.name}</span></Link>
  </SidebarMenuButton></SidebarMenuItem>
})}

// routes/ItemDetail.tsx
switch (item.profile) {
  case "movie": case "series": case "season": case "episode":
    return <TitleDetail item={item} />          // today's page, unchanged features
  case "item":
    return <GenericItemDetail item={item} />    // hero, overview, play if playable, children
  default:
    return item.profile satisfies never
}

// routes/Home.tsx
if (row.kind === "library") return `/library/${encodeURIComponent(row.id)}?sort=${row.sort}`
```

## Shape

### Data structures first

**Library identity.** `libraries(jellyfin_id, name, collection_type, position)`
is a cached copy of `/UserViews`. It stores the raw `CollectionType`. The
`LibraryKind` is derived on every read by
`LibraryKind::from_collection_type`, the only place that interprets the
server value. A mapping change takes effect without a resync (derive, don't
sync).

**Membership.** `items.library_id TEXT NOT NULL REFERENCES libraries`. One
item has one library. The foreign key encodes "no item without a library" in
the schema, and deleting a library row without its items fails instead of
orphaning rows. The library walk that ingests an item stamps its
`library_id`. Nothing infers membership from `parent_id`.

**Placement.** At ingest, `LibraryKind::place(ItemKind) -> Option<Placement>`
decides whether the item is cached for that library at all (`None` drops it,
replacing `is_synced_kind`), whether it is a grid title (`is_title`), and its
`profile`. Both are stored columns. Every feature predicate in SQL becomes
`profile IN (…)` instead of `kind IN ('Movie','Series')`. The upsert's
change check includes both columns, so a registry edit heals on the next
daily re-bootstrap. Bump `SCHEMA_VERSION` when it must apply at once.

**The registry.** `LibraryKind::policy()` is an exhaustive `match`. Adding a
`LibraryKind` without a policy fails the build.

| LibraryKind | CollectionType | Grid titles | Cached children | Browse | Latest shelf sort |
|---|---|---|---|---|---|
| Movies | `movies` | Movie | | movies | added |
| TvShows | `tvshows` | Series | Season, Episode | series | episode-added |
| Music | `music` | MusicAlbum | | generic | added |
| MusicVideos | `musicvideos` | MusicVideo | | generic | added |
| HomeVideos | `homevideos` | Video, Photo | | generic | added |
| Photos | `photos` | Photo | | generic | added |
| Books | `books` | Book, AudioBook | | generic | added |
| BoxSets | `boxsets` | BoxSet | | generic | added |
| Playlists | `playlists` | Playlist | | generic | added |
| Trailers | `trailers` | Trailer | | generic | added |
| LiveTv | `livetv` | TvChannel | | generic | name |
| Mixed | `folders`, absent, unknown | Movie, Series, Video, MusicVideo, MusicAlbum, Book, AudioBook, BoxSet, Photo | | generic | added |

`profile` = `movie` only for Movie in a Movies library; `series`, `season`,
`episode` only for those kinds in a TvShows library; `item` for everything
else. That follows the owner's decision literally: a Movie in a mixed library
gets the base experience. See Open questions.

`playable` is a property of the item type alone (`ItemKind::playable`: Movie,
Episode, Video, MusicVideo, Trailer). Audio, AudioBook, Book and Photo are
display-only in this change. The UI never infers playability from a type.

### Sync and deletion

Sync state moves from global `meta` keys to one `library_sync` row per
library (bootstrap offset, total, done, time; watermark and its ids). Global
meta keeps sweep time, last sync, catalog-ready and latest-failure.
"Bootstrap complete" and `ownership_available` are derived from the rows.

1. **Library refresh**, every cycle, 1 request. It upserts `/UserViews` in
   order. A removed library's items go through the notifying
   `delete_items_from_transaction` path, then its rows. A changed
   `CollectionType` counts as removed plus added. An empty answer removes
   nothing once libraries were known (the sweep's existing rule), but is
   accepted on a fresh cache so a user with no libraries reaches "ready". Any
   change emits `libraries-changed`.
2. **Bootstrap**, per library, resumable, daily. A `Limit=0` count probe
   gives an honest total, then it pages `DateCreated` ascending with the
   library's types. `MAX_BOOTSTRAP_PAGES` becomes a per-cycle budget.
   Libraries fill in sidebar order, and `catalog_ready` is set on the first
   page of any library.
3. **Incremental**, per library, every cycle: the descending walk to the
   library's own watermark.
4. **Identity sweep**, hourly or on request, per library. It mirrors user data
   as today, then `retain_library(L, seen_L)` deletes only rows of L not seen.
   A truncated or empty sweep skips only that library's deletion. Ids seen in
   L but not cached under L (new or moved) are adopted: fetched exactly with
   `CATALOG_FIELDS`, one request per 200 ids, capped per sweep. A moved item
   lands in the right library within one sweep whatever the walk order.

Cost with L libraries: 1 + L requests per idle cycle (today 1), plus at most L
partial pages per sweep and L probes per bootstrap. On the owner's server
(Movies 685, Shows 4,816 rows, BoxSets 85), the bootstrap goes from 28 pages
to 30 plus 3 probes. Photo libraries are the outlier (see Risks).

### Query contract

`ItemQuery.scope` is a library id validated at the API boundary; an unknown
id is a 404 "that library is no longer available". `kinds` becomes
`Vec<ItemKind>`, and an unknown kind is a 400 instead of a silent empty page.
The unused `parent_id` goes from Rust and TS. The default rule lives in
`kind_predicate`: no kinds means `i.is_title = 1`. That replaces the in-flight
fix's `Movie,Series` default, so Search, Favorites and My list return titles
of every library and never a Season or Episode.

Feature predicates:

| Today | Generalized |
|---|---|
| `tmdb_candidates`: `kind IN ('Movie','Series')` | `profile IN ('movie','series')` |
| `random_billboard_titles`, Because You Watched seed | `profile IN ('movie','series')` / `profile = 'movie'` |
| `continue_watching`: `kind IN ('Movie','Episode')` | `kind IN (ItemKind::PLAYABLE)` (home videos resume too) |
| `ids_by_tmdb(kind)`, `played_by_tmdb`, `ids_by_tvdb("Episode")` | take `ItemProfile` |
| unplayed count, `in_progress`: `CASE WHEN i.kind = 'Series'` | `CASE WHEN i.profile = 'series'` |
| letterboxd, ratings media type, `external_url(kind)` | take `ItemProfile`, rich only |

Structural joins such as episode-to-series stay on `kind`. They describe
hierarchy, not features.

### Generic details

`/api/item/{id}` answers every type from the cached row in the same
`ItemDetail` shape, and `/about` already fetches Overview, Tags, Studios and
People live for any type. `/children` keeps cached seasons and episodes, with
the background reconcile, for `series` and `season`. Any other item gets one
live, uncached page of `/Items?ParentId=<id>` mapped through
`summaries_from_dtos`, so a box set's movies open the full movie page. On a
cache miss, `fetch_and_cache_item` also reads `/Items/{id}/Ancestors`. The
first ancestor that is a known library gives membership, and the item is
ingested if the registry places it there, otherwise served live with that
library's profile, or as `item` with no library. That is one extra request,
on misses only.

### UI

- **Sidebar**: Home, one entry per library in server order (icons from the
  exhaustive `LIBRARY_ICONS`), then Favorites, Releases and collections. On
  `/item/:id` the active entry is `item.libraryId`, which replaces the
  collapse at `AppSidebar.tsx:295`.
- **Browse**: `/library/:libraryId` renders `Library.tsx` scoped to the
  library, titled with its name. Sorts and filters are keyed by
  `BrowseProfile`. `movies` and `series` keep today's sets. `generic` gets
  name, release date, date added, rating, date played, random, genre, decade,
  watched, parental rating and favorite. Genres and ratings are per library.
  Plain `/library` is the cross-library view (search, favorites, person, and
  Home View all with `kind=`), always with the `generic` set. The implicit
  `"Movie"` default in `libraryKind()` is deleted. An unknown id shows "This
  library is no longer available".
- **Details**: `ItemDetail.tsx` switches on `profile`. `TitleDetail` is
  today's page. `GenericItemDetail` composes `DetailHeroLayout`,
  `DetailFacts`, `CastRow`, `DetailActions` (Play only when `playable`) and an
  `ItemGrid` of live children, with no Letterboxd, collection chip or external
  links. Genre links go to `/library/<libraryId>?genre=…`, which replaces the
  collapse at `DetailHero.tsx:51`.
- **Back**: `defaultDetailNavigationState(item, libraries)` returns "Back to
  <library name>" at `/library/<libraryId>`, or Home, which replaces the
  collapse at `navigation.ts:21`.
- **Cards**: base cards keep the 2:3 frame so the virtual grid is unchanged,
  and use `object-contain` when `primaryAspect >= 0.8`.

### Home

`HomeElementId::Library { id }` gives a row `{ kind: "library", id, title:
"Latest in <name>", sort, items }` from
`ItemQuery { scope: Some(id), sort: policy.latest }`. A TV library sorts
series by newest episode, the cache equivalent of `/Items/Latest`'s grouping,
with no request and working offline. `sort` lets the UI build View all
without knowing the registry.

The resolved Home view, used by both Settings and rows, adds a missing
library element as enabled, after the last library element or after the
built-ins. It drops elements of libraries that are gone, but only once the
library list is authoritative, the retention model collections use. The file
keeps such an element until the next Home save, so a returning library keeps
its place and switch. The existing 409 guard covers a library appearing
mid-edit, and `libraries-changed` invalidates `home` and `homeSettings`.

### Durable `accounts.json` formats

`ACCOUNT_CONFIG_VERSION` stays 1. An older app reading a newer file hits an
unknown variant or key, takes the existing "written by a different MediaFlick
version and was left unchanged" path, and does not rewrite the file. That is
the AGENTS.md rule, and it needs no version wrapper.

| Stored today | New app loads | Served to UI | Rewritten |
|---|---|---|---|
| `startupDestination: "movies"` / `"series"` | accepted as storage-only variants | `{ "library": <first movies/tvshows library> }`, or `"home"` if none or not known yet | as `{ "library": id }` when the user saves Viewing; PATCH rejects the legacy values |
| `"home"`, `"calendar"`, `"last"` | unchanged | unchanged | never |
| browsing key `Movie` / `Series` | accepted by `validate_browsing_route` | re-keyed to `library:<first movies/tvshows id>` unless that key exists | retired in the same mutation that first saves `library:<that id>` |
| browsing `last` = `/library?kind=Movie…` | accepted | as is. The cross-library route still renders it | by the next navigation |
| `home.elements` | unchanged | plus a library element per library, enabled | on the next Home save |

New writes: `startupDestination: {"library": "<id>"}`, browsing keys
`library:<id>` with routes under `/library/<id>`, and Home elements
`{"kind":"library","id":"<id>","enabled":…}`. All ids are validated as
Jellyfin ids (1–64 chars, `[0-9A-Fa-f-]`). Resolution is a pure function in
`preferences` that takes a small `KnownLibraries` view, so preferences never
depends on the library module.

### Interface depth and non-goals

The public surface is `refresh_libraries`, `libraries`, `ingest_library_page`,
`retain_library`, `forget_library`, `query`, `children`, `summaries_from_dtos`
and `LibraryKind::policy`. Behind it sit per-library cursors, membership,
placement, the title rule, profile derivation, adoption of moved items and
live-DTO membership lookup. `ItemSummary::from_dto` becomes private to
`library`, so no caller can build a summary without a resolved profile. The
UI gets one discriminant instead of `kind` plus library context plus feature
flags.

Out of scope: caching children of base containers, caching any type the
registry does not place (`Folder`, `Person`, `Genre`), migrating
`library.db`, new music, book or photo features, and Collections, Discover
or Seerr changes beyond ownership following `profile`.

## Synthesis decision

Filled in by arena.

## Type sketch

### Rust: `src/library/kinds.rs` (new, the registry)

```rust
/// What a Jellyfin library is for. Derived from `CollectionType` here and
/// nowhere else; stored rows keep the raw string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LibraryKind { Movies, TvShows, Music, MusicVideos, HomeVideos, Photos, Books,
                       BoxSets, Playlists, Trailers, LiveTv, Mixed }

impl LibraryKind {
    /// `folders`, an absent value and any value this build does not know are
    /// mixed content: a base library of every title type.
    pub fn from_collection_type(value: Option<&str>) -> Self { todo!() }

    /// The single table of library behavior. Exhaustive by construction.
    pub const fn policy(self) -> &'static LibraryPolicy { todo!() }

    /// Where an item of `kind` sits in a library of this kind, or `None` when
    /// this library does not cache that type.
    pub fn place(self, kind: ItemKind) -> Option<Placement> {
        // let policy = self.policy();
        // if policy.titles.contains(&kind) { Some(Placement { is_title: true,  profile: policy.browse.profile(kind) }) }
        // else if policy.children.contains(&kind) { Some(Placement { is_title: false, profile: policy.browse.profile(kind) }) }
        // else { None }
        todo!()
    }

    /// `IncludeItemTypes` for this library's walks: titles then children.
    pub fn sync_types(self) -> String { todo!() }
}

pub struct LibraryPolicy {
    pub titles: &'static [ItemKind],
    pub children: &'static [ItemKind],
    pub browse: BrowseProfile,
    /// Order of the "Latest in <library>" shelf and of its View all link.
    pub latest: ItemSort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowseProfile { Movies, Series, Generic }

impl BrowseProfile {
    /// Rich profiles exist only for movie and TV library rows.
    pub const fn profile(self, kind: ItemKind) -> ItemProfile { todo!() }
}

/// The feature set an item gets. Stored in `items.profile`; sent to the UI
/// instead of the Jellyfin type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemProfile { Movie, Series, Season, Episode, #[default] Item }

impl ItemProfile {
    pub const fn as_sql(self) -> &'static str { todo!() }
    pub fn from_sql(value: &str) -> Option<Self> { todo!() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement { pub is_title: bool, pub profile: ItemProfile }

/// Jellyfin item types the catalog understands. Parsed at the DTO boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind { Movie, Series, Season, Episode, MusicAlbum, MusicArtist, Audio,
                    MusicVideo, Video, BoxSet, Book, AudioBook, Photo, PhotoAlbum,
                    Playlist, Trailer, TvChannel, Folder }

impl ItemKind {
    pub const PLAYABLE: &'static [Self] =
        &[Self::Movie, Self::Episode, Self::Video, Self::MusicVideo, Self::Trailer];
    pub fn parse(jellyfin_type: &str) -> Option<Self> { todo!() }
    pub const fn as_str(self) -> &'static str { todo!() }   // the Jellyfin `Type` name
    pub const fn label(self) -> &'static str { todo!() }    // "Album", "Book", "Collection", …
    pub fn playable(self) -> bool { Self::PLAYABLE.contains(&self) }
}
```

### Rust: `src/library/libraries.rs` (new) and changed `library` types

```rust
/// A cached `/UserViews` entry, as the API and policy see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRef {
    pub id: String,
    pub name: String,
    pub kind: LibraryKind,
    pub browse: BrowseProfile,
}

pub struct LibraryRefresh { pub removed: Vec<String>, pub changed: bool }

impl Library {
    /// Upserts the server's list in its order. A kind change counts as
    /// removed plus added. Never removes anything on an empty list when
    /// libraries were known. Idempotent.
    pub fn refresh_libraries(&self, views: &[UserView]) -> rusqlite::Result<LibraryRefresh> { todo!() }
    pub fn libraries(&self) -> rusqlite::Result<Vec<LibraryRef>> { todo!() }
    pub fn library(&self, id: &str) -> rusqlite::Result<Option<LibraryRef>> { todo!() }
    /// Whether at least one `/UserViews` answer has been stored.
    pub fn libraries_known(&self) -> bool { todo!() }
    /// Deletes the library's items through the notifying delete path, then
    /// its sync state and row.
    pub fn forget_library(&self, id: &str) -> rusqlite::Result<LibraryChangeBatch> { todo!() }

    pub(crate) fn library_cursor(&self, id: &str) -> rusqlite::Result<LibraryCursor> { todo!() }
    pub(crate) fn save_library_cursor(&self, id: &str, cursor: &LibraryCursor) -> rusqlite::Result<()> { todo!() }
}

/// Per-library resumable sync position (one `library_sync` row).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LibraryCursor {
    pub bootstrap_offset: i64,
    pub bootstrap_total: Option<i64>,
    pub bootstrap_done: bool,
    pub bootstrap_at: Option<u64>,
    pub watermark: Option<String>,
    pub watermark_ids: HashSet<String>,
}

// src/library/catalog.rs
impl Library {
    /// Replaces `ingest_page`. Items the library's policy does not place are
    /// skipped; placed ones get `library_id`, `is_title` and `profile`.
    pub fn ingest_library_page(&self, library: &LibraryRef, dtos: &[BaseItemDto])
        -> rusqlite::Result<LibraryChangeBatch> { todo!() }
    /// Replaces the global `retain_ids` for sweeps.
    pub fn retain_library(&self, library_id: &str, seen: &HashSet<String>)
        -> rusqlite::Result<LibraryChangeBatch> { todo!() }
    /// Ids the sweep saw in this library that are not cached under it.
    pub fn unknown_in_library(&self, library_id: &str, seen: &HashSet<String>)
        -> rusqlite::Result<Vec<String>> { todo!() }
    /// Children inherit the parent's cached library; unchanged otherwise.
    pub fn reconcile_children(&self, parent_id: &str, dtos: &[BaseItemDto])
        -> rusqlite::Result<LibraryChangeBatch> { todo!() }
}

// src/library/mod.rs
#[derive(Debug, Clone, Default)]
pub struct ItemQuery {
    /// A cached library id, validated by the API before it gets here.
    pub scope: Option<String>,
    /// Empty means "browsable titles" (see `kind_predicate`). Was `Vec<String>`.
    pub kinds: Vec<ItemKind>,
    // search, genre, release_decade, series_id, watched, favorite,
    // official_rating, series_status, sort, direction, offset, limit: unchanged.
    // parent_id: deleted (no caller).
}

// src/library/model.rs
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSummary {
    pub id: String,
    #[serde(skip)]
    pub kind: String,             // Jellyfin type; Rust-internal only
    pub profile: ItemProfile,
    pub type_label: String,
    pub library_id: Option<String>,
    pub playable: bool,
    pub primary_aspect: Option<f64>,
    // … every existing field except `kind` …
}

impl ItemSummary {
    /// Private to `library`: only `Library::summaries_from_dtos` and row
    /// readers construct summaries, so every summary has a resolved profile.
    pub(super) fn from_dto(dto: &BaseItemDto, placement: Option<(String, Placement)>) -> Self { todo!() }
}

// src/library/query.rs
impl Library {
    /// Live DTOs (Next Up, person titles, box-set and generic children)
    /// become summaries with the membership the cache knows: the item's own
    /// row, else its series' row, else none (`profile: item`).
    pub fn summaries_from_dtos(&self, dtos: &[BaseItemDto]) -> Vec<ItemSummary> { todo!() }
    pub fn genres(&self, scope: GenreScope<'_>) -> rusqlite::Result<Vec<String>> { todo!() }
    pub fn official_ratings(&self, scope: Option<&str>, kinds: &[ItemKind]) -> rusqlite::Result<Vec<String>> { todo!() }
}

pub enum GenreScope<'a> { All, Library(&'a str), /** Home genre shelves */ Titles(&'a [ItemProfile]) }

/// The one place the default rule lives. Replaces the in-flight
/// `Movie,Series` default with `is_title`, which covers every library.
fn kind_predicate(kinds: &[ItemKind], conditions: &mut Vec<String>, arguments: &mut Vec<SqlValue>) {
    // if kinds.is_empty() { conditions.push("i.is_title = 1".into()); return }
    // conditions.push(format!("i.kind IN ({})", placeholders(kinds.len())));
    // arguments.extend(kinds.iter().map(|kind| SqlValue::Text(kind.as_str().into())));
    todo!()
}
```

### Rust: Jellyfin adapter

```rust
// src/jellyfin/api/views.rs (new)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UserView { pub id: String, pub name: String, pub collection_type: Option<String> }

/// `GET /UserViews?userId=`. `/Users/{id}/Views` is gone on 12.2.
pub fn fetch_user_views(client: &JellyfinClient, user_id: &str) -> Result<Vec<UserView>, ApiError> { todo!() }

// src/jellyfin/api/items.rs: the walks take the library and its types
pub const CATALOG_FIELDS: &str = "ProviderIds,Genres,DateCreated,OriginalTitle,SortName,\
PremiereDate,OfficialRating,CommunityRating,ChildCount,ParentId,PrimaryImageAspectRatio";
pub fn fetch_library_page(client: &JellyfinClient, user_id: &str, library: &LibraryWalk<'_>,
    start_index: i64, sort_order: &str) -> Result<ItemsResponse, ApiError> { todo!() }
pub fn fetch_library_count(client: &JellyfinClient, user_id: &str, library: &LibraryWalk<'_>)
    -> Result<i64, ApiError> { todo!() }            // Limit=0, no fields, no images
pub fn fetch_library_identity_page(client: &JellyfinClient, user_id: &str, library: &LibraryWalk<'_>,
    start_index: i64, page_size: i64) -> Result<ItemsResponse, ApiError> { todo!() }
pub fn fetch_ancestors(client: &JellyfinClient, user_id: &str, item_id: &str)
    -> Result<Vec<BaseItemDto>, ApiError> { todo!() }
/// Any container's direct children, untyped. Base details only; never cached.
pub fn fetch_live_children(client: &JellyfinClient, user_id: &str, parent_id: &str, limit: i64)
    -> Result<ItemsResponse, ApiError> { todo!() }

pub struct LibraryWalk<'a> { pub id: &'a str, pub include_types: &'a str }
// SYNCED_ITEM_TYPES and fetch_items_page / fetch_identity_page are deleted.
```

### Rust: sync cycle

```rust
// src/library/sync/cycle.rs
pub(super) fn run_cycle_inner(library: &Library, session: &Session, trigger: Trigger,
    control: Option<&SyncHandle>) -> Result<SyncReport, ApiError> {
    // 1. views = fetch_user_views; refresh = commit(refresh_libraries(&views));
    //    forget every refresh.removed; refresh.changed → shell.libraries_changed()
    // 2. count-probe unbootstrapped libraries; budget = MAX_BOOTSTRAP_PAGES
    // 3. for lib in libraries(): bootstrap_library(.., &mut budget); incremental_library(..)
    // 4. sweep due → for lib: identity_library(..) (retain_library + adopt unknown ids)
    // 5. failure and completion bookkeeping as today
    todo!()
}

// Each takes (library, session, scope, client, lib: &LibraryRef, …, control).
fn bootstrap_library(/* …, */ budget: &mut usize) -> Result<usize, ApiError> { todo!() }
fn incremental_library(/* …, */ changes: &mut LibraryChangeBatch) -> Result<usize, ApiError> { todo!() }
/// Mirrors user data, deletes what this library no longer reports, adopts
/// ids it reports that the cache holds elsewhere or not at all.
fn identity_library(/* … */) -> Result<(usize, LibraryChangeBatch), ApiError> { todo!() }

// src/library/sync.rs: derived, not stored
pub fn bootstrap_progress(library: &Library) -> BootstrapProgress {
    // complete = libraries_known && every cursor.bootstrap_done
    // processed = Σ offsets; total = Σ totals, None until every library counted
    todo!()
}
```

### Rust: schema 20 (excerpt)

```sql
CREATE TABLE libraries (
    jellyfin_id     TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    collection_type TEXT,             -- raw server value; kind is derived in Rust
    position        INTEGER NOT NULL, -- /UserViews order
    synced_at       INTEGER NOT NULL
);
CREATE TABLE library_sync (
    library_id       TEXT PRIMARY KEY REFERENCES libraries(jellyfin_id) ON DELETE CASCADE,
    bootstrap_offset INTEGER NOT NULL DEFAULT 0,
    bootstrap_total  INTEGER,
    bootstrap_done   INTEGER NOT NULL DEFAULT 0,
    bootstrap_at     INTEGER,
    watermark        TEXT,
    watermark_ids    TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(watermark_ids))
);
-- items gains:
    library_id     TEXT NOT NULL REFERENCES libraries(jellyfin_id),  -- no cascade: deletes go
                                                                     -- through the notifying path
    is_title       INTEGER NOT NULL,
    profile        TEXT NOT NULL CHECK (profile IN ('movie','series','season','episode','item')),
    primary_aspect REAL,
CREATE INDEX items_library_name  ON items (library_id, is_title, sort_name COLLATE NOCASE, name COLLATE NOCASE);
CREATE INDEX items_library_added ON items (library_id, is_title, date_created DESC);
CREATE INDEX items_library_year  ON items (library_id, is_title, year DESC, premiere_date DESC);
CREATE INDEX items_titles_name   ON items (is_title, sort_name COLLATE NOCASE) WHERE is_title = 1;
-- items_kind_* indexes stay for cross-library kind rows (Home built-ins).
```

### Rust: preferences

```rust
// src/preferences/accounts.rs
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum HomeElementId {
    BuiltIn { id: HomeBuiltIn },
    Genre { id: String },
    Collection { id: String },
    /// "Latest in <library>". Validated as a Jellyfin id.
    Library { id: String },
}

impl HomeSettings {
    pub fn fresh(genres: &[String], libraries: &[&str]) -> Self { todo!() }
    /// Adds enabled elements for unseen libraries after the last library
    /// element (else after the built-ins) and, when `known` is
    /// authoritative, drops elements of libraries that are gone.
    pub fn reconcile_libraries(&mut self, known: &KnownLibraries<'_>) { todo!() }
}

/// Browsing memory keys: `last`, `library:<id>`; legacy `Movie`/`Series`
/// load but are never written.
pub fn resolve_browsing(stored: &BTreeMap<String, String>, known: &KnownLibraries<'_>)
    -> BTreeMap<String, String> { todo!() }

impl AccountConfigurationService {
    /// Saves `library:<id>` and, in the same atomic mutation, drops the
    /// legacy key that resolved to it.
    pub fn save_browsing(&self, key: &AccountKey, page: &str, route: &str,
        retire: Option<LegacyShelf>) -> io::Result<()> { todo!() }
}

// src/preferences/viewing.rs
/// Stored form. `Movies` and `Series` are pre-library values: they load,
/// resolve to the first library of that kind, and are rejected on PATCH.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupDestination { #[default] Home, Movies, Series, Calendar, Last, Library(String) }

impl StartupDestination {
    pub fn resolve(&self, known: &KnownLibraries<'_>) -> Self { todo!() }
    pub fn is_legacy(&self) -> bool { matches!(self, Self::Movies | Self::Series) }
}

/// What `preferences` needs to know about libraries, built by the API layer.
pub struct KnownLibraries<'a> {
    pub authoritative: bool,
    pub ids: &'a [&'a str],
    pub first_movies: Option<&'a str>,
    pub first_tvshows: Option<&'a str>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyShelf { Movie, Series }
```

### TypeScript: contract (`ui/src/lib/api/types.ts`)

```ts
export type LibraryKind =
  | "movies" | "tvshows" | "music" | "musicvideos" | "homevideos" | "photos"
  | "books" | "boxsets" | "playlists" | "trailers" | "livetv" | "mixed"
export type BrowseProfile = "movies" | "series" | "generic"
export interface Library { id: string; name: string; kind: LibraryKind; browse: BrowseProfile }

export type ItemProfile = "movie" | "series" | "season" | "episode" | "item"
export interface ItemSummary {
  id: string
  profile: ItemProfile          // replaces `kind`; switch on this, exhaustively
  typeLabel: string             // display only ("Album", "Book")
  libraryId: string | null
  playable: boolean
  primaryAspect: number | null
  // … every existing field except `kind` …
}

export interface ItemQuery { library?: string; kind?: string /* … minus parentId … */ }
export type StartupDestination = "home" | "calendar" | "last" | { library: string }

export type HomeElement = { enabled: boolean; label: string; available: boolean
  category: "Built-in" | "Library" | "Genre" | "My Collection" } & (
  | { kind: "builtIn"; id: HomeBuiltInId } | { kind: "library"; id: string }
  | { kind: "genre"; id: string } | { kind: "collection"; id: string })

export type HomeRow =
  | { kind: "builtIn" | "genre" | "collection"; id: string; title: string; items: ItemSummary[] }
  | { kind: "library"; id: string; title: string; sort: LibrarySortId; items: ItemSummary[] }

export interface StartupResponse { /* … */ libraries: Library[] | null }
```

### TypeScript: UI modules

```ts
// ui/src/lib/libraries.ts (new)
export function librariesQueryOptions(account: string): UseQueryOptions<Library[]> { throw new Error("not implemented") }
export function useLibraries(): UseQueryResult<Library[]> { throw new Error("not implemented") }
/** `undefined` while loading, `null` when the id is not a current library. */
export function useLibrary(id: string | undefined): Library | null | undefined { throw new Error("not implemented") }
export function libraryPath(library: Pick<Library, "id">, params?: URLSearchParams): string { throw new Error("not implemented") }
export function browsingKey(library: Pick<Library, "id">): `library:${string}` { throw new Error("not implemented") }
/** Exhaustive: a new LibraryKind fails `tsc` until it has an icon. */
export const LIBRARY_ICONS: Record<LibraryKind, LucideIcon> = {
  movies: Film, tvshows: Tv, music: Music, musicvideos: Clapperboard, homevideos: Video,
  photos: Image, books: BookOpen, boxsets: Layers, playlists: ListMusic, trailers: Film,
  livetv: Radio, mixed: Folder,
}

// ui/src/lib/library-filters.ts: keyed by BrowseProfile, not "Movie" | "Series"
export interface LibrarySort { id: LibrarySortId; label: string; seriesLabel?: string
  appliesTo: readonly BrowseProfile[]; natural: SortOrder }
export function librarySortsFor(profile: BrowseProfile): LibrarySort[] { throw new Error("not implemented") }
export function readLibraryFilters(params: URLSearchParams, profile: BrowseProfile): LibraryFilterState { throw new Error("not implemented") }
export function writeLibraryFilters(previous: URLSearchParams, patch: Partial<LibraryFilterState>, profile: BrowseProfile): URLSearchParams { throw new Error("not implemented") }
/** `library: null` is the cross-library view (search, favorites, kind= rows). */
export function libraryItemQuery(params: URLSearchParams, library: Library | null): ItemQuery { throw new Error("not implemented") }
// libraryKind(), libraryViewKinds(), libraryKindPath() and their "Movie" default are deleted.

// ui/src/lib/navigation.ts
export function defaultDetailNavigationState(item: Pick<ItemSummary, "libraryId">, libraries: Library[]): DetailNavigationState { throw new Error("not implemented") }
export function detailNavigationState(location: LocationLike, libraries: Library[]): DetailNavigationState { throw new Error("not implemented") }

// ui/src/lib/startup.ts
export function startupRoute(destination: StartupDestination, libraries: Library[], history: Record<string, string>): string { throw new Error("not implemented") }

// ui/src/components/detail/GenericItemDetail.tsx (new)
export function GenericItemDetail({ item, navigationState }: { item: ItemDetail; navigationState: DetailNavigationState }): JSX.Element { throw new Error("not implemented") }
```

## Module map

| Path | Change |
|---|---|
| `src/library/kinds.rs` | New. LibraryKind, ItemKind, BrowseProfile, ItemProfile, policy table, placement |
| `src/library/libraries.rs` | New. `libraries` and `library_sync` access, refresh, forget, cursors |
| `src/library/db.rs` | Schema 20: new tables, item columns and indexes |
| `src/library/catalog.rs` | `ingest_library_page`, `retain_library`, adoption. `is_synced_kind` deleted |
| `src/library/query.rs` | `scope`, `kind_predicate`, profile predicates, `summaries_from_dtos`, scoped genres and ratings |
| `src/library/model.rs` | `ItemRecord` gains library, placement and aspect. `ItemSummary` loses wire `kind` |
| `src/library/sync.rs`, `sync/cycle.rs` | Per-library loop, derived progress, sweep adoption |
| `src/jellyfin/api/views.rs` | New. `/UserViews` |
| `src/jellyfin/api/items.rs`, `model.rs` | Library walks, count probe, ancestors, live children, `PrimaryImageAspectRatio` |
| `src/shell/cef/api/catalog.rs` | `/libraries`, `library` param, Home library rows, children policy, miss membership |
| `src/shell/cef/api/{media,letterboxd,ratings}.rs`, `src/companion/mod.rs`, `src/library/integrations.rs` | Kind literals become `ItemProfile` |
| `src/shell/cef/api/settings.rs`, `api.rs` | Resolved viewing and browsing, PATCH rules, `libraries` in startup |
| `src/preferences/{accounts,viewing}.rs` | `Library` Home element, browsing keys, `StartupDestination::Library`, `KnownLibraries` |
| `src/app/services.rs` | `ShellBridge::libraries_changed` |
| `ui/src/lib/api/types.ts`, `client.ts`, `queries.ts`, `query-client.ts`, `library-events.ts` | Contract, `api.libraries`, query keys, `libraries-changed` |
| `ui/src/lib/libraries.ts` | New |
| `ui/src/lib/{library-filters,navigation,startup}.ts` | Profile-keyed filters, library-aware back and startup |
| `ui/src/components/AppSidebar.tsx`, `ViewingSync.tsx` | Sidebar from list, startup and browsing keys |
| `ui/src/routes/Library.tsx`, `App.tsx` | `/library/:libraryId` route, scoped title and filters |
| `ui/src/routes/ItemDetail.tsx`, `components/detail/*`, `GenericItemDetail.tsx` | Switch on profile, generic page, collapses removed |
| `ui/src/components/{MediaCard,CardInlineActions,PreviewCard,Billboard}.tsx`, `lib/api/urls.ts` | `profile` and `playable` instead of `kind` |
| `ui/src/routes/Home.tsx`, `routes/settings/{Home,Viewing}Settings.tsx` | Library rows and View all, "Library" category, startup picker |

## Tradeoffs accepted

- We accept 1 + L requests per idle cycle instead of 1, in exchange for exact
  membership and per-library deletion that never touches another library.
- We accept a full re-sync for every user, in exchange for zero migration
  code.
- We accept one library per item (the last walk to see it wins), in exchange
  for no join on grid queries. Measured: 0 items in two libraries.
- We accept stored `is_title` and `profile`, stale for at most one daily
  re-bootstrap after a registry edit, in exchange for one-column predicates.
  It looks like duplicated state, but it is a derivation that heals itself.
- We accept rewriting about 50 UI `kind` reads, in exchange for a wire with no
  Jellyfin type to compare against.
- We accept live, online-only children for base containers, in exchange for
  not syncing tens of thousands of tracks and album photos.
- We accept legacy browsing keys lingering until that library's filters are
  saved, in exchange for no background writes to user files.

## Alternatives considered

- **Join table `item_libraries`.** Many-to-many membership at the cost of a
  join on every grid, count and letter query plus membership-aware deletion.
  Same interface, more hidden complexity, for a case measured at zero.
- **No `library_id`; resolve membership per query (Ancestors, folder paths).**
  Keeps one global walk, but the cache can no longer answer "items of library
  X" without N requests. It fails the dominant access pattern.
- **Keep `kind` on the wire and add feature flags (`hasTmdb`, …).** A wider
  surface that leaves `kind === "Movie"` as the nearest example to copy: two
  ways to do one task.
- **Registry as a SQL table joined at query time.** No stored columns, but
  policy split across Rust and SQL and a join in every predicate. The Rust
  `match` is checked by the compiler and runs once per ingested row.
- **Global walk, membership only for Movies and TV.** Fewer requests, but two
  catalogs and two query paths, the opposite of this direction.

## Open questions and risks

- **Movies in mixed libraries.** Read literally, the owner's rule gives them
  `profile: item`: no TMDB, Letterboxd or ratings, and no longer owned in
  Collections or Discover. Today they get full features. Should `Mixed`
  follow the item type instead? It is one line in `BrowseProfile::profile`.
- **Photo libraries.** 60k photos cost about 300 bootstrap pages a day and 60
  sweep pages an hour. Should photo-bearing libraries skip the daily
  re-bootstrap, or cache albums only and show photos live?
- **Ancestors.** `/Items/{id}/Ancestors` should include the
  `CollectionFolder` (Jellyfin's `TranslateParentItem`), but that is not
  measured. Should a probe run before PR 4? Without it, a miss is served as
  `profile: item` until the next sweep adopts the item.
- **Built-in overlap.** With one movies library, "Recently Added Movies"
  equals "Latest in Films". Should `RecentlyAdded` and `RecentlyAddedShows`
  retire?
- **Live TV and Trailers.** Channels cannot be played here, and a Trailers
  library is mostly remote trailers. Should they appear now, as decision 1
  says, or be hidden?
- **Audio.** Audio and AudioBook are not `playable` here. Is music playback
  part of the base experience, or later?
- **Downgrade.** An older build rejects the new `accounts.json` and leaves it
  unchanged, per the rule. Should the release notes say so?

## Delivery plan

Every PR stands alone and carries a `changelog.d/` fragment. Each runs the
Rust checks (`just rust-quality`, `just test`), the UI checks (`pnpm --dir ui
lint`, `test`, `build`) and `git diff --check`. Existing tests are updated
where contracts change, and no new unit tests are written. Each PR ends with
a verify-mediaflick drive whose evidence folder is the artifact, run against
the demo server and, with `--account`, the owner's server. The in-flight fix
`fix/search-favorites-top-level-only` merges first, and PR 2 replaces its
predicate in the same function.

**PR 1: library registry and list (schema 19).**
- Scope: `kinds.rs`, `views.rs`, the `libraries` table, the refresh at cycle
  start with the empty-list guard, `GET /api/libraries`, `libraries` in
  `/api/startup` and the `libraries-changed` event. The UI adds types and the
  query only.
- Verify: a new `libraries.mjs` reads `/api/libraries` in the page and
  `/UserViews` from the server, and asserts the same ids, names and order and
  the mapped kinds.
- Artifact: `libraries.json` (both lists plus the mapping).

**PR 2: generalized catalog sync (schema 20).**
- Scope: membership, placement and aspect columns; `library_sync`;
  per-library bootstrap, incremental and sweep with adoption; library
  removal; `kind_predicate`; the `library` param with scoped genres and
  ratings; profile predicates in Rust; additive wire fields (`profile`,
  `typeLabel`, `libraryId`, `playable`, `primaryAspect`) with `kind` kept.
  `SYNCED_ITEM_TYPES` and `is_synced_kind` are deleted.
- Verify: `library-parity.mjs` gains a per-library mode. It compares
  `/api/items?library=X` totals with
  `/Items?ParentId=X&Recursive=true&IncludeItemTypes=<titles>&Limit=0`. It
  checks that a known album and book show up in search, and that search,
  favorites and My list hold no `season` or `episode` profile. It re-runs
  `home-library` and `playback` (Built-in and External).
- Artifact: `library-parity-per-library.json` and screenshots.
- Gap to report: library removal and item moves need server changes the
  non-admin verify user cannot make, so they are checked in code and the log
  only.

**PR 3: library navigation and durable formats.**
- Scope: the sidebar from the list, `/library/:libraryId`, `BrowseProfile`
  filters, scoped genres and ratings, browsing keys `library:<id>` with legacy
  resolution and retirement, `StartupDestination::Library` with legacy
  resolution and the PATCH rejection, and the startup picker. The `"Movie"`
  default and the static `NAV` entries are deleted.
- Verify: a new `library-navigation.mjs` runs with `--seed`, holding an
  `accounts.json` with `startupDestination:"series"`,
  `rememberFilters:true` and browsing
  `Series:"/library?kind=Series&sort=year"`. It asserts the app starts on the
  first tvshows library sorted by release date, and that the file is
  byte-identical until a save. It checks sidebar order against `/UserViews`
  and screenshots every library. A TV-library filter change writes
  `library:<id>` and retires `Series`. Saving Viewing with a music library
  writes `{"library":"<id>"}`.
- Artifacts: `accounts-before.json`, `accounts-after.json`,
  `navigation.json` and screenshots.

**PR 4: generic details and profile-driven UI.**
- Scope: `GenericItemDetail`, live children for base containers, ancestors on
  a cache miss, library-aware back navigation and sidebar highlight, and
  `kind` removed from the item wire. Every UI `kind` read moves to `profile`,
  `playable` or `typeLabel`, and the collapses at `navigation.ts:21`,
  `DetailHero.tsx:51` and `AppSidebar.tsx:295` are gone.
- Verify: `item-detail.mjs` opens the first item of each library type and
  records the profile and which sections render. A movie shows Letterboxd,
  the chip and links. A series shows seasons and next up. An album, book or
  box set shows the generic page with children and no TMDB or Letterboxd. A
  box set's child opens the full movie page. Back returns to
  `/library/<id>`, and the sidebar highlights the library. `playback.mjs`
  plays a home video if one exists.
- Artifact: `detail-report.json` and screenshots per type.

**PR 5: Home "Latest in <library>".**
- Scope: `HomeElementId::Library`, `fresh` and `reconcile_libraries`, library
  rows with `sort` and View all, and the "Library" category in Home settings
  using the existing draft, Save, Reset and Discard.
- Verify: `home-settings.mjs` checks for one "Latest in <name>" per
  `/UserViews` library, turns one off and saves. Home shows the enabled rows
  in order, View all opens the library with the row's sort, and the file
  holds `{"kind":"library",…,"enabled":false}`. A seeded run with an unknown
  library id shows that element dropped from Settings while the file is
  unchanged.
- Artifacts: `home.json`, `accounts.json` and screenshots.

## Next implementation step

Write `src/library/kinds.rs`: the `LibraryKind` mapping and the exhaustive
`policy()` table, including the `ItemKind` parser and labels. Every later PR
reads from it.
