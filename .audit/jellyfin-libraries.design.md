# Every Jellyfin library, in two tiers

Candidate 2. Direction: keep the cached catalog for the video titles it already serves and add library identity and membership on top. Serve every other library type live from Jellyfin through fixed native endpoints that parse into domain types.

## Problem

MediaFlick shows two hardcoded pages, Movies and Series. Both read one server-wide cache that holds only `Movie, Series, Season, Episode` (`src/jellyfin/api/items.rs:16`, `src/library/model.rs:283`). No code reads `/UserViews` or `CollectionType`. The owner wants the following:

- Every library the user can see, each under its Jellyfin name.
- Today's features for movies and tvshows libraries.
- A browse grid and a generic details page for everything else.
- One "Latest in <library>" Home shelf per library.
- The fix for the query with no kind.

Phase A found several constraints that set the shape:

- **Membership needs a separate lookup.** An item's `ParentId` is its physical folder, never the library (0 of 12 measured). Membership can only come from `GET /Items?ParentId=<library>&Recursive=true`, one walk per library.
- **The sync loop is careful, tested code.** It has a resumable bootstrap, a `DateCreated` watermark with tie ids, and an hourly identity sweep whose `retain_ids` is protected against truncated and empty answers (`src/library/sync/cycle.rs`). A design that rewrites this loop takes on that risk again.
- **Some cached queries have no kind predicate.** These are Search, Favorites and My list (`src/library/query.rs:561`), and they already leak Season and Episode rows. Every new kind added to `items` would widen that hazard.
- **accounts.json is durable and strictly validated.** A value the validator rejects makes app initialization fail (`src/app/services.rs:305`). The browsing keys `last|Movie|Series` (`src/preferences/accounts.rs:700`), `StartupDestination::{Movies,Series}` (`src/preferences/viewing.rs:27`) and `HomeElementId` with `deny_unknown_fields` (`accounts.rs:56`) must all keep loading.
- **The UI may not call Jellyfin, and Desktop gets no generic proxy** (AGENTS.md).

## Usage (caller's view)

### README: libraries

> The sidebar lists your Jellyfin libraries in your Jellyfin order, under their Jellyfin names. Movies and TV shows libraries open the full catalog grid: every filter, sort and the A–Z rail. They work offline. Other libraries open a grid served live from your server. It has name, date added and release year sorts, and works only while the server is reachable. Any item opens its details page. Movies, shows, seasons and episodes get the full title page. Everything else gets a simple page: artwork, facts, overview and, for folders and albums, their contents.

### Native API, as the UI calls it

```ts
// Sidebar, Home settings, startup: one cached list, no server round trip.
const { known, libraries, titles } = await api.libraries()
// libraries[0] = { id: "f137…", name: "Movies", kind: "movies",
//                  browse: { sorts: ["name","year","added","rating","played","plays","runtime","random"],
//                            filters: ["genre","decade","watched","parental","favorite"], letterIndex: true } }
// libraries[2] = { id: "7e64…", name: "Music", kind: "music",
//                  browse: { sorts: ["name","added","year"], filters: ["favorite"], letterIndex: false } }

// One grid for every library; the UI never learns which tier answered.
const page = await api.browse({ scope: "library", libraryId: "7e64…", query: { sort: "added" } }, 0, 60)
// page = { items: ItemSummary[], total: 4 }   (MusicAlbum cards, live)

// Home shelf, any library.
const latest = await api.libraryLatest("7e64…")      // { items: ItemSummary[] }

// Details: a discriminated union, chosen by the item, not the library.
const item = await api.item("a1b2…")
if (item.layout === "generic") { /* GenericItemPage */ } else { /* today's title page */ }
```

### Call site 1: sidebar (`ui/src/components/AppSidebar.tsx`)

```tsx
const libraries = useLibraries()
// …
<SidebarGroupLabel>Libraries</SidebarGroupLabel>
{libraries.data?.libraries.map((library) => {
  const Icon = libraryIcon(library.kind)
  return (
    <SidebarMenuItem key={library.id}>
      <SidebarMenuButton asChild isActive={activeLibraryId === library.id} tooltip={library.name}>
        <Link to={libraryPath(library.id)}><Icon /><span>{library.name}</span></Link>
      </SidebarMenuButton>
    </SidebarMenuItem>
  )
})}
```

### Call site 2: library page (`ui/src/routes/Library.tsx`)

```tsx
const scope = useBrowseScope()            // library route or cross-library titles view
const filters = readLibraryFilters(params, scope.browse)
<LibraryFilters value={filters} capabilities={scope.browse} onChange={update} total={total} />
<ItemGrid source={scope.source(params)} jumpRail={scope.browse.letterIndex ? rail : undefined} />
```

### Call site 3: a native handler (`src/shell/cef/api/libraries.rs`)

```rust
["libraries", id, "items"] if request.is("GET") => {
    let id = library_id(id)?;                                   // 400 on a bad id
    let query = BrowseQuery::from_request(request)?;            // 400 on a bad param
    browse::library_page(&services.library, &services.session, &id, &query)
        .map(|page| ApiResponse::ok(json!({ "items": page.items, "total": page.total })))
        .map_err(|error| browse_failure(services, error))
}
```

## Shape

### The tier boundary

**The library decides the grid. The item decides the details page.**

- **Grid by library.** `LibraryKind` comes from `CollectionType` in one function. Its profile picks the grid source:
  - movies and tvshows: the catalog tier, which is library.db joined with `item_libraries`.
  - every other kind, including mixed: the live tier, a fixed `/Items` shape per kind.

  One grid always has exactly one source. Merging a cached and a live page stream under one offset, total and sort order is unsound: SQLite `NOCASE` collation and Jellyfin `SortName` disagree, and offsets drift. Merging is ruled out by construction.
- **Details by item.** A catalog kind (Movie, Series, Season, Episode) always gets the title layout from the cache, which fetches and caches the item on a miss as it does today. Every other Jellyfin type gets the generic layout, live.
- **Mixed libraries.** A mixed library (no `CollectionType`, or `folders`) is live and browsed by folder. Its Movie and Series rows are still cached, because the server-wide catalog walk stays unchanged. Those titles keep the title page, Search, Favorites, Continue Watching and TMDB ownership, wherever they live. A Video in the same library gets the generic page.
- **Global views stay catalog-only.** Search, Favorites, My list, genre shelves, Billboard and Continue Watching read the cache and return top-level titles: Movie and Series. Live-tier items do not appear in them, by design (see [Tradeoffs](#tradeoffs-accepted)).

### Core data

```
libraries        (cache)   id PK, name, collection_type NULL, position, primary_image_tag
item_libraries   (cache)   (library_id, item_id) PK WITHOUT ROWID; index (item_id)
items, user_data (cache)   unchanged
```

- **Membership is a relation keyed by Jellyfin id.** It has no foreign key and no column on `items`, for three reasons:
  - It can be written before the item rows arrive, so the first bootstrap still fills library grids progressively.
  - Rewriting it never fires the `items_fts_update` trigger, which runs on every `UPDATE` of `items`.
  - It allows the many-to-many case that the measurement did not rule out in principle.
- **Only top-level titles get membership rows.** Seasons and episodes inherit membership through `series_id`.
- **`collection_type` is stored raw.** It is mapped on read, so `LibraryKind::from_collection_type` is the only reader of that vocabulary. No second id scheme is persisted.

The dominant access paths through these structures:

| Access | Path | Cost |
| --- | --- | --- |
| Sidebar, Home reconcile, startup | `SELECT … FROM libraries ORDER BY position` | O(libraries) |
| Catalog grid page, sorted by name | walk `items_kind_name` in order, probe `item_libraries` PK `(library_id, item_id)`, stop at LIMIT | same index use as today, plus one PK probe per row |
| Latest in a shows library | Series in the library by `EpisodeAdded` | today's `EpisodeAdded` query, scoped |
| Title page `libraryId` | `item_libraries` by `item_id` (series id for seasons and episodes) | one index seek |
| Membership refresh | `DELETE WHERE library_id=?` and `INSERT` the set, in one transaction | idempotent full replace |
| Live grid page | one `/Items` request, no local state | network |

### Invariants and where they live

- **`CollectionType` is read in one place.** `LibraryKind::from_collection_type`. The wire type `UserViewDto` lives only in `jellyfin::api::model`, and `BaseItemDto` gets no `CollectionType` field, so nothing else can read it.
- **One registry, matched exhaustively.** `LibraryKind::profile()` is a `match`. A new kind without a profile does not compile. The UI gets each library's `BrowseCapabilities` from it, never derives capabilities from kinds itself, and never sees the tier.
- **Catalog kinds come from one enum.** `CatalogKind::ALL` derives `SYNCED_ITEM_TYPES`, `is_synced_kind` and `PERSON_ITEM_TYPES`, which today are three hand-written lists. `CatalogKind::TOP_LEVEL` is the no-kind default. This extends the in-flight fix: that branch writes the rule once, and here it becomes the `Default` of a type.
- **No query reaches Jellyfin from the UI's raw input.**
  - `/api/libraries/{id}/…` accepts only an id that is in the `libraries` table.
  - Item types sent to Jellyfin come from the registry, never from request parameters.
  - Sorts and filters outside the library's capabilities get a 400.
  - `limit` is clamped.
  - This is a native endpoint per operation, not a proxy.
- **"Not loaded yet" is a type.** `LibraryRegistry::{Unknown, Known(Vec<MediaLibrary>)}`. Home reconciliation and startup normalization never treat "not loaded yet" as "the server has no libraries", so a cold start cannot drop saved Library shelves from view.
- **Deletion has one owner.** The server-wide identity sweep still owns item existence through `retain_ids`, unchanged. Membership and library rows are replaced wholesale by their own idempotent passes. A dangling membership row joins to nothing and is replaced on the next pass.

### Interface depth

The UI's new surface is one `useLibraries()` list with capabilities, one `BrowseSource` union for every grid, and one `layout` discriminant on details. Behind it sit the `CollectionType` mapping and type table, tier dispatch, membership sync, catalog SQL versus Jellyfin `SortBy`/`Filters` mapping, `/Items/Latest` grouping, and folder versus recursive grids. The interface stays this small because the UI never asks how a page is produced, only which controls it supports. Promoting music to the catalog tier later changes one registry row and adds catalog kinds; no UI file changes.

Deliberately out: local rows for non-video types, unified search across tiers, offline live libraries, per-type features, and play or favorite actions on generic pages.

## Type sketch: Rust

```rust
// src/library/libraries.rs (new): library identity, kind, registry.
use serde::{Deserialize, Serialize};
use crate::library::model::CatalogKind;
use crate::library::SortKey;

/// A Jellyfin CollectionFolder id: validated hex, so it is safe as a route
/// segment, a SQL argument, and a durable accounts.json value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LibraryId(String);

impl LibraryId {
    pub fn parse(raw: &str) -> Option<Self> {
        todo!("crate::collections::valid_opaque_id(raw.trim()) then lowercase")
    }
    pub fn as_str(&self) -> &str { &self.0 }
}
impl TryFrom<String> for LibraryId {
    type Error = String;
    fn try_from(raw: String) -> Result<Self, String> { todo!("parse or 'invalid library id'") }
}
impl From<LibraryId> for String { fn from(id: LibraryId) -> String { id.0 } }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryKind {
    Movies, Shows, Music, MusicVideos, HomeVideos, Photos, Books,
    BoxSets, Playlists, LiveTv, Trailers, Mixed,
}

impl LibraryKind {
    /// The only reader of Jellyfin's CollectionType. Absent, "folders" and
    /// values a future server adds are Mixed: every library is still shown.
    pub fn from_collection_type(value: Option<&str>) -> Self {
        // movies→Movies, tvshows→Shows, music→Music, musicvideos→MusicVideos,
        // homevideos→HomeVideos, photos→Photos, books→Books, boxsets→BoxSets,
        // playlists→Playlists, livetv→LiveTv, trailers→Trailers, _→Mixed
        todo!()
    }

    /// The per-type table. Exhaustive: a new kind without a row fails to build.
    pub fn profile(self) -> LibraryProfile {
        match self {
            Self::Movies => LibraryProfile::catalog(CatalogKind::Movie, &MOVIE_BROWSE),
            Self::Shows => LibraryProfile::catalog(CatalogKind::Series, &SHOW_BROWSE),
            Self::Music => LibraryProfile::live(LiveGrid::Recursive(&["MusicAlbum"])),
            Self::MusicVideos => LibraryProfile::live(LiveGrid::Recursive(&["MusicVideo"])),
            Self::Books => LibraryProfile::live(LiveGrid::Recursive(&["Book", "AudioBook"])),
            Self::BoxSets => LibraryProfile::live(LiveGrid::Recursive(&["BoxSet"])),
            Self::Trailers => LibraryProfile::live(LiveGrid::Recursive(&["Trailer"])),
            Self::HomeVideos | Self::Photos | Self::Playlists | Self::LiveTv | Self::Mixed => {
                LibraryProfile::live(LiveGrid::Folders)
            }
        }
    }
}

pub struct LibraryProfile {
    pub grid: GridSource,
    pub browse: &'static BrowseCapabilities,
}

pub enum GridSource {
    /// library.db scoped by `item_libraries`; `title` is the one kind its grid shows.
    Catalog { title: CatalogKind },
    /// Jellyfin `/Items` with a shape fixed here.
    Live(LiveGrid),
}

#[derive(Debug, Clone, Copy)]
pub enum LiveGrid {
    /// `ParentId=<library>&Recursive=true&IncludeItemTypes=<types>`.
    Recursive(&'static [&'static str]),
    /// `ParentId=<library or folder>`, direct children, folders browsable.
    Folders,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterKey { Genre, Decade, Watched, Parental, SeriesStatus, Favorite }

/// What a grid can execute. Sent to the UI with each library; a request
/// outside it is a 400, so the UI cannot offer what the backend cannot do.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseCapabilities {
    pub sorts: &'static [SortKey],
    pub filters: &'static [FilterKey],
    pub letter_index: bool,
}

pub static MOVIE_BROWSE: BrowseCapabilities = todo!("today's Movie sorts and filters");
pub static SHOW_BROWSE: BrowseCapabilities = todo!("today's Series sorts and filters, incl. EpisodeAdded, SeriesStatus");
/// Cross-library title views: Search, Favorites, person, genre View all.
pub static TITLE_BROWSE: BrowseCapabilities = todo!("today's Movie+Series intersection");
pub static LIVE_RECURSIVE_BROWSE: BrowseCapabilities = todo!("[Name, DateAdded, ReleaseDate], [Favorite], no letters");
pub static LIVE_FOLDER_BROWSE: BrowseCapabilities = todo!("[Name, DateAdded, ReleaseDate], [], no letters");

impl LibraryProfile {
    const fn catalog(title: CatalogKind, browse: &'static BrowseCapabilities) -> Self { todo!() }
    const fn live(grid: LiveGrid) -> Self { todo!("browse from grid: Recursive→LIVE_RECURSIVE_BROWSE, Folders→LIVE_FOLDER_BROWSE") }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaLibrary {
    pub id: LibraryId,
    pub name: String,
    pub kind: LibraryKind,
    pub primary_image_tag: Option<String>,
}

/// `Unknown` = never fetched for this account (fresh database, signed out).
/// `Known(vec![])` = the server says the user sees no library.
pub enum LibraryRegistry { Unknown, Known(Vec<MediaLibrary>) }

impl LibraryRegistry {
    pub fn get(&self, id: &LibraryId) -> Option<&MediaLibrary> { todo!() }
    /// Legacy `StartupDestination::{Movies,Series}` resolve to this.
    pub fn first_of(&self, kind: LibraryKind) -> Option<&MediaLibrary> { todo!("server order") }
}

// `/api/libraries` serializes each MediaLibrary as LibraryEntry { id, name, kind, imageTag,
// browse: kind.profile().browse }: capabilities derived, never stored.
```

```rust
// src/library/model.rs (changed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatalogKind { Movie, Series, Season, Episode }

impl CatalogKind {
    pub const ALL: [Self; 4] = [Self::Movie, Self::Series, Self::Season, Self::Episode];
    /// What a query that names no kind returns: titles a card can represent.
    pub const TOP_LEVEL: [Self; 2] = [Self::Movie, Self::Series];
    pub fn parse(value: &str) -> Option<Self> { todo!() }
    pub fn as_str(self) -> &'static str { todo!() }
    /// `"Movie,Series,Season,Episode"`; replaces the hand-written const.
    pub fn wire_list(kinds: &[Self]) -> String { todo!() }
}

pub(crate) fn is_synced_kind(kind: &str) -> bool { CatalogKind::parse(kind).is_some() }

/// Replaces `ItemQuery.kinds: Vec<String>`. `TopLevel` is the default, so a
/// caller that forgets the kind cannot get Season or Episode rows.
#[derive(Debug, Clone, Default)]
pub enum KindFilter {
    #[default]
    TopLevel,
    Only(Vec<CatalogKind>), // non-empty; `parse_kind_param` enforces it
}

/// Item detail as served: the layout is chosen by the item kind, never by the
/// library the user came from.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "layout", rename_all = "camelCase")]
pub enum DetailView {
    Title(ItemDetail),        // ItemDetail gains `library_id: Option<LibraryId>`
    Generic(GenericItemDetail),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenericItemDetail {
    #[serde(flatten)]
    pub summary: ItemSummary, // `summary.overview` carries the overview
    pub genres: Vec<String>,
    pub is_folder: bool,
    pub date_created: Option<String>,
}

impl GenericItemDetail {
    pub fn from_dto(dto: &BaseItemDto) -> Self { todo!("summary.overview = dto.overview") }
}
```

```rust
// src/library/mod.rs (changed)
pub struct ItemQuery {
    pub kinds: KindFilter,               // was Vec<String>
    pub library: Option<LibraryId>,      // new: membership scope (catalog tier only)
    // parent_id removed: unused, and it named a physical folder, not a library.
    // search, genre, release_decade, series_id, watched, favorite, official_rating,
    // series_status, sort, direction, offset, limit: unchanged.
}

/// The id half of `ItemSort`, serialized with today's ids ("name", "year", "added",
/// "episode-added", "rating", "played", "plays", "runtime", "random").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SortKey { Name, ReleaseDate, DateAdded, EpisodeAdded, CommunityRating, DatePlayed, PlayCount, Runtime, Random }
```

```rust
// src/library/catalog_libraries.rs (new): the cache side of libraries.
impl Library {
    pub fn libraries(&self) -> rusqlite::Result<LibraryRegistry> {
        todo!("Unknown when meta `sync.libraries_at` is absent, else Known(ordered rows)")
    }

    /// Replaces the library set in one transaction. Also deletes membership of
    /// libraries that disappeared or are no longer catalog tier. Idempotent.
    pub(crate) fn replace_libraries(&self, views: &[MediaLibrary]) -> rusqlite::Result<LibrarySetChange> {
        todo!()
    }

    /// Replaces one library's membership with `item_ids`. Returns whether it changed.
    pub(crate) fn replace_membership(&self, library: &LibraryId, item_ids: &HashSet<String>) -> rusqlite::Result<bool> {
        todo!()
    }

    /// The library a cached title belongs to (series membership for seasons and
    /// episodes), first by server order.
    pub fn library_of(&self, item_id: &str) -> rusqlite::Result<Option<LibraryId>> { todo!() }

    pub fn library_filter_options(&self, library: &LibraryId) -> rusqlite::Result<FilterOptions> {
        todo!("genres and official ratings of member titles")
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LibrarySetChange { pub added: Vec<LibraryId>, pub removed: Vec<LibraryId>, pub renamed: bool }

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptions { pub genres: Vec<String>, pub official_ratings: Vec<String> }
```

```rust
// src/library/browse.rs (new): the one place that dispatches between tiers.
use crate::jellyfin::session::Session;

/// ItemQuery's filter and sort fields without `search`, `kinds` or `library`:
/// on a library route the library supplies those. `check(caps)` rejects
/// anything outside the library's capabilities.
#[derive(Debug, Clone)]
pub struct BrowseQuery { /* sort, direction, genre, release_decade, watched,
                            official_rating, series_status, favorite, offset, limit */ }

#[derive(Debug)]
pub enum BrowseError {
    UnknownLibrary,                 // 404: not in the registry
    Unsupported(&'static str),      // 400: outside the library's capabilities
    NotFound,                       // 404: the server has no such item
    Storage(rusqlite::Error),       // 500
    Server(ApiError),               // mapped through scoped_failure
}

pub fn library_page(
    library: &Library, session: &Session, id: &LibraryId, query: &BrowseQuery,
) -> Result<ItemPage, BrowseError> {
    // let entry = registry.get(id).ok_or(UnknownLibrary)?;
    // let profile = entry.kind.profile();
    // query.check(profile.browse)?;
    // match profile.grid {
    //   Catalog { title } => library.query(&ItemQuery { library: Some(id), kinds: Only(vec![title]), ..from(query) }),
    //   Live(grid) => live::page(session.scope()?, id.as_str(), grid, query),
    // }
    todo!()
}

pub fn library_letters(library: &Library, id: &LibraryId, query: &BrowseQuery)
    -> Result<Vec<(String, i64)>, BrowseError> { todo!("catalog tier only; Unsupported otherwise") }

pub fn library_options(library: &Library, id: &LibraryId) -> Result<FilterOptions, BrowseError> {
    todo!("catalog: library_filter_options; live: empty")
}

/// "Latest in <library>". Movies: DateAdded. Shows: Series by EpisodeAdded.
/// Live: `/Items/Latest?ParentId=` (Jellyfin's own grouping).
pub fn latest(library: &Library, session: &Session, id: &LibraryId, limit: i64)
    -> Result<Vec<ItemSummary>, BrowseError> { todo!() }

/// Cached catalog item → Title. Uncached: fetch; a catalog kind is ingested
/// and served as Title (today's cache-through); anything else → Generic.
pub fn item_detail(library: &Library, session: &Session, item_id: &str)
    -> Result<DetailView, BrowseError> { todo!() }

/// Which source answers `/api/item/{id}/children`. Pure; the handler keeps
/// today's Series/Season reconcile thread for `Catalog`.
pub fn children_source(cached: Option<CatalogKind>) -> ChildrenSource { todo!() }
pub enum ChildrenSource { Catalog, Live }

pub fn live_children(session: &Session, item_id: &str, offset: i64, limit: i64)
    -> Result<ItemPage, BrowseError> { todo!("ParentId=<item>, all types, disc/track/name order") }

mod live {
    /// Maps BrowseQuery to `/Items` and parses into ItemSummary. Random is
    /// never offered live: Jellyfin's Random is unseeded, so pages would repeat.
    pub(super) fn page(scope: SessionScope, parent: &str, grid: LiveGrid, query: &BrowseQuery)
        -> Result<ItemPage, BrowseError> { todo!() }
    fn sort_by(key: SortKey) -> &'static str {
        // Name→"SortName", DateAdded→"DateCreated,SortName", ReleaseDate→"ProductionYear,PremiereDate,SortName"
        todo!()
    }
}
```

```rust
// src/jellyfin/api/items.rs (new fetches; the wire stays here). `C` = &JellyfinClient.
pub fn fetch_user_views(c: C, user: &str) -> Result<Vec<UserViewDto>, ApiError> { todo!("GET /UserViews?userId=") }
/// Identity-only: ParentId, Recursive, IncludeItemTypes, Fields=, no images/user data, Limit=1000.
pub fn fetch_library_member_ids(c: C, user: &str, library: &str, types: &[CatalogKind], start: i64)
    -> Result<ItemsResponse, ApiError> { todo!() }
pub fn fetch_live_page(c: C, user: &str, request: &LivePageRequest) -> Result<ItemsResponse, ApiError> { todo!() }
pub fn fetch_latest(c: C, user: &str, library: &str, limit: i64) -> Result<Vec<BaseItemDto>, ApiError> {
    todo!("/Items/Latest returns a bare array")
}
pub fn fetch_generic_item(c: C, user: &str, id: &str) -> Result<Option<BaseItemDto>, ApiError> { todo!() }

// src/jellyfin/api/model.rs: `UserViewDto { id, name, collection_type, image_tags }`, whose
// collection_type is read only by LibraryKind::from_collection_type; BaseItemDto gains `is_folder: bool`.
```

```rust
// src/preferences/accounts.rs (changed durable format; see "Durable accounts.json formats")
pub enum HomeElementId {
    BuiltIn { id: HomeBuiltIn },
    Genre { id: String },
    Collection { id: String },
    Library { id: LibraryId },            // new
}

const BROWSING_LEGACY_PAGES: [&str; 2] = ["Movie", "Series"];   // accepted, never written
const MAX_BROWSING_PAGES: usize = 256;

enum BrowsingPage { Last, Library(LibraryId) }
impl BrowsingPage {
    fn parse(page: &str) -> Option<Self> { todo!("\"last\" | \"library:<id>\"") }
}

// src/preferences/viewing.rs
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupDestination {
    #[default]
    Home,
    /// Legacy: read as "the first Movies library". Never written by this version.
    Movies,
    /// Legacy: read as "the first TV shows library". Never written by this version.
    Series,
    Calendar,
    Last,
    /// Serde's external tagging: `{"library":"f137…"}`; unit variants stay strings.
    Library(LibraryId),
}
// StartupDestination drops `Copy` (it now owns a String); ViewingSettings was never Copy.
```

```rust
// src/shell/cef/api/settings.rs (read boundary)
/// What the UI sees: legacy Movies/Series resolve to the first library of
/// that kind. A missing library resolves to Home. `Unknown` registry:
/// legacy → Home for this read only, nothing written.
fn startup_view(stored: &StartupDestination, registry: &LibraryRegistry) -> StartupDestination { todo!() }
```

## Type sketch: TypeScript

```ts
// ui/src/lib/api/types.ts (changed)
export type CatalogKind = "Movie" | "Series" | "Season" | "Episode"
type ItemKind = CatalogKind | (string & {})                  // unchanged wire meaning

export type LibraryKind =
  | "movies" | "shows" | "music" | "musicVideos" | "homeVideos" | "photos"
  | "books" | "boxSets" | "playlists" | "liveTv" | "trailers" | "mixed"
export type LibrarySortId = "name" | "year" | "added" | "episode-added" | "rating" | "played" | "plays" | "runtime" | "random"
export type LibraryFilterId = "genre" | "decade" | "watched" | "parental" | "seriesStatus" | "favorite"

export interface BrowseCapabilities { sorts: LibrarySortId[]; filters: LibraryFilterId[]; letterIndex: boolean }
export interface Library { id: string; name: string; kind: LibraryKind; imageTag: string | null; browse: BrowseCapabilities }
export interface LibrariesResponse {
  /** False until the first sync cycle has read the server's library list. */
  known: boolean
  libraries: Library[]
  /** Capabilities of the cross-library title views (Search, Favorites, person, genre). */
  titles: BrowseCapabilities
}

/** `/api/libraries/{id}/items`: no kind, the library decides. */
export interface LibraryQuery {
  genre?: string; decade?: number; watched?: WatchFilter | ""; favorite?: boolean
  parental?: string; seriesStatus?: SeriesStatus | ""; sort?: LibrarySortId; order?: SortOrder; seed?: number
}
// ItemQuery: `parentId` removed; `kind` restricted to CatalogKind lists.

export interface ItemDetail extends ItemSummary {           // the title layout
  layout: "title"
  genres: string[]; originalTitle: string | null
  providerIds: { tmdb: string | null; imdb: string | null; tvdb: string | null }
  parentId: string | null; dateCreated: string | null
  libraryId: string | null
}
export interface GenericItemDetail extends ItemSummary {
  layout: "generic"; genres: string[]; isFolder: boolean; dateCreated: string | null
}
export type AnyItemDetail = ItemDetail | GenericItemDetail

export type HomeElement = { enabled: boolean; label: string; available: boolean
  category: "Built-in" | "Genre" | "My Collection" | "Library" } & (
  | { kind: "builtIn"; id: HomeBuiltInId } | { kind: "genre"; id: string }
  | { kind: "collection"; id: string } | { kind: "library"; id: string })

export interface ViewingSettings { /* … */ startupDestination: "home" | "calendar" | "last" | { library: string } }
```

```ts
// ui/src/lib/libraries.ts (new): every library-shaped decision the UI makes.
export type BrowseSource =
  | { scope: "titles"; query: ItemQuery }
  | { scope: "library"; libraryId: string; query: LibraryQuery }
  | { scope: "children"; itemId: string }

export function browsePagePath(source: BrowseSource, offset: number, limit: number): string {
  throw new Error("not implemented") // /api/items | /api/libraries/{id}/items | /api/item/{id}/children
}
export function libraryPath(id: string, params?: URLSearchParams): string { throw new Error("not implemented") }
export function libraryIdFromPath(pathname: string): string | null { throw new Error("not implemented") }
/** Exhaustive over LibraryKind; presentation only. */
export function libraryIcon(kind: LibraryKind): LucideIcon { throw new Error("not implemented") }
export function browsingKey(libraryId: string): `library:${string}` { throw new Error("not implemented") }
export function startupRoute(destination: ViewingSettings["startupDestination"], libraries: Library[], last?: string): string {
  throw new Error("not implemented") // missing library → "/"
}
/** Genre chips on a title page: its library when that library filters by genre, else the title views. */
export function genreRoute(genre: string, item: ItemDetail, libraries: Library[]): string {
  throw new Error("not implemented")
}
export function defaultDetailNavigationState(item: AnyItemDetail, libraries: Library[]): DetailNavigationState {
  throw new Error("not implemented") // title + libraryId → that library; otherwise Home
}

// ui/src/lib/library-filters.ts: capabilities replace kinds (`caps` = BrowseCapabilities).
export function sortsFor(caps: BrowseCapabilities): readonly LibrarySort[] { throw new Error("not implemented") }
export function readLibraryFilters(params: URLSearchParams, caps: BrowseCapabilities): LibraryFilterState { throw new Error("not implemented") }
export function writeLibraryFilters(previous: URLSearchParams, patch: Partial<LibraryFilterState>, caps: BrowseCapabilities): URLSearchParams { throw new Error("not implemented") }
// Deleted: LibraryKind ("Movie"|"Series"), libraryKindPath, libraryViewKinds, seriesStatusApplies,
// and libraryKind's "default to Movie" branch (no kind now means the native TopLevel default).
// ui/src/lib/queries.ts adds useLibraries(), useBrowsePages(source), useLibraryOptions(id), useLibraryLatest(id).
```

## Sync and schema

**Schema 18 → 19.** Two tables are added:

```sql
CREATE TABLE libraries (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    collection_type   TEXT,            -- raw; NULL = mixed
    position          INTEGER NOT NULL,
    primary_image_tag TEXT,
    synced_at         INTEGER NOT NULL
);
CREATE TABLE item_libraries (
    library_id TEXT NOT NULL,
    item_id    TEXT NOT NULL,
    PRIMARY KEY (library_id, item_id)
) WITHOUT ROWID;
CREATE INDEX item_libraries_item ON item_libraries (item_id);
```

- **Upgrade.** The existing recreation path drops the database and keeps the session (`db.rs:155-197`), so everyone resyncs once. The owner has 5,501 rows, about 28 pages. No migration code is added.
- **Sign-out and account change.** The forget list in `src/library/session_store.rs:94` is a hand-synced list of cache tables. It becomes `db::CATALOG_TABLES`, declared next to `SCHEMA`, which now includes `libraries` and `item_libraries`. The forget path iterates that constant, so a new cache table cannot be missed.

**The cycle.** The changes are additions only. `bootstrap`, `incremental` and `identity_sweep` stay unchanged.

```
run_cycle_inner
  1. refresh_libraries          GET /UserViews → replace_libraries → LibrarySetChange      (every cycle, 1 request)
  2. membership (initial)       if bootstrap not complete: membership_pass                   (keeps the first fill progressive)
  3. bootstrap                  unchanged, server-wide Movie,Series,Season,Episode
  4. incremental                unchanged
  5. identity_sweep             unchanged; also forced when LibrarySetChange.removed is non-empty
  6. membership (settle)        if set changed || report.changed() || identity sweep ran || Trigger::Requested
  7. events                     libraries_changed (set change) and library_changed (membership change)
```

```rust
// src/library/sync/membership.rs (new)
/// For each catalog-tier library: walk member ids of its title kind, then
/// replace_membership. A walk that hits MAX_IDENTITY_PAGES skips that library's
/// replacement, so a truncated answer never shrinks a grid. A successful empty
/// answer is trusted: the cost of a wrong replace is a stale relation until
/// the next pass, not deleted catalog rows.
pub(super) fn membership_pass(library: &Library, session: &Session, scope: &SessionScope,
    control: Option<&SyncHandle>) -> Result<bool, ApiError> { todo!() }

/// GET /UserViews → MediaLibrary list in server order. An empty list from a
/// successful call is the server's answer and is committed as Known(vec![]).
pub(super) fn refresh_libraries(library: &Library, session: &Session, scope: &SessionScope)
    -> Result<LibrarySetChange, ApiError> { todo!() }
```

**What deletion and retain do now:**

| Event | Effect |
| --- | --- |
| Item deleted on server | The identity sweep's `retain_ids` deletes the row, as today. Its membership row dangles, joins to nothing, and is replaced in step 6 of the same cycle. |
| Item moved between libraries | Step 6 replaces both libraries' sets. The item row is untouched. |
| Library removed or access revoked | `replace_libraries` deletes the library and its membership in one transaction. Step 5 is forced, so its titles leave the cache in the same cycle, not within the hour. |
| Library added | `replace_libraries` inserts it, and step 6 fills it if it is catalog tier. A live library needs nothing. |
| Kind change to live (not possible in Jellyfin, handled anyway) | `replace_libraries` deletes membership of non-catalog libraries. |

**Watermark and cursor.** There is no change: the `DateCreated` watermark, tie ids and bootstrap offset stay server-wide, because the catalog walk stays server-wide. Membership has no cursor. It is a bounded, idempotent full replace, and a crash halfway leaves the previous complete set, because each library is one transaction. New meta keys: `sync.libraries_at`, which separates `Unknown` from `Known(vec![])`, and `sync.membership_at`.

**Request cost:**

| Work | Today | Two tiers |
| --- | --- | --- |
| Every 10-minute cycle | incremental (≥1) | +1 `/UserViews` |
| Membership | — | per catalog library, ⌈titles/1000⌉ id-only requests, only when step 2 or 6 fires. Owner: 2 requests, about hourly plus after new arrivals. |
| A 200k-track music library | — | 0 sync requests, 0 rows, 0 FTS entries |
| Live grid | — | 1 request per 60-card page scrolled |
| Home | 0 for library rows | 1 `/Items/Latest` per enabled live shelf per Home load (staleTime 5 min). Catalog shelves cost 0. |
| Generic details | — | item, about, and one children page |

## Native API contract

| Route | Change | Request | Response |
| --- | --- | --- | --- |
| `GET /api/libraries` | new | — | `{ known, libraries: Library[], titles: BrowseCapabilities }` |
| `GET /api/libraries/{id}/items` | new | `LibraryQuery` + `offset`, `limit` (1–200 live, 1–500 catalog) | `{ items: ItemSummary[], total }` |
| `GET /api/libraries/{id}/letters` | new | `LibraryQuery` | `{ letters }`; 400 if `!letterIndex` |
| `GET /api/libraries/{id}/options` | new | — | `{ genres, officialRatings }` (live: empty) |
| `GET /api/libraries/{id}/latest` | new | `limit` (1–40) | `{ items: ItemSummary[] }` |
| `GET /api/items` | changed | `kind` must be a comma list of `CatalogKind`; unknown → 400; absent → `TopLevel` (Movie, Series); `parentId` removed | unchanged |
| `GET /api/item/{id}` | changed | — | `AnyItemDetail`: `layout: "title"` gains `libraryId`; `layout: "generic"` is new. Today's uncached non-catalog fallback, which returned an `ItemSummary` missing `providerIds`, is gone. |
| `GET /api/item/{id}/children` | changed | optional `offset`, `limit` | `{ items, total }`. Catalog containers keep today's reconcile and slice; others are live and paged. |
| `GET/PATCH /api/settings/viewing`, `/api/startup` | changed | `startupDestination` may be `{ library }` | legacy values resolved by `startup_view` |
| `GET/PATCH /api/settings/browsing` | changed | `page`: `last` or `library:<id>` | map may still contain legacy keys until the next write |
| `GET/PATCH /api/settings/home` | changed | elements may be `{ kind: "library", id }` | label `Latest in <name>`, category `Library` |
| shell event | new | — | `librariesChanged` → invalidate `libraries`, `home`, `home-settings`, `viewing` |

**The no-kind rule, extended.** The in-flight branch makes an empty kind mean Movie and Series in one Rust place. Here that place becomes `KindFilter::default()`. `library_query` parses `kind` through `CatalogKind::parse` and returns a 400 on anything else, so it never builds an empty `Only`. Library routes take no `kind` at all, because the profile supplies it. So no route can produce a query that returns seasons or episodes unless it asks for them by name, as `/api/item/{id}/children` and the episode navigation do directly in SQL.

Every change carries through the same PR: Rust handlers, `ui/src/lib/api/{types,client,urls}.ts`, UI callers, and their existing tests and contract fixtures.

## UI

**Routes:**

- `/library/:libraryId` is a library.
- `/library`, with no id, is the cross-library title view: Search, Favorites, person and genre "View all". Its `kind` parameter stays for old links and saved `last` routes such as `/library?kind=Movie`, which still render.
- `/item/:id` is unchanged.

**Sidebar.**

- The "Library" group keeps Home, Favorites, Releases and collections.
- A new "Libraries" group renders `useLibraries()` in server order: name, `libraryIcon(kind)`, link `/library/<id>`. While `known === false`, it shows two skeleton rows.
- A library is active when the path is `/library/<id>`. On a detail page, it is the library in the origin route, else the title page's `libraryId`.
- `AppSidebar.tsx:293-296` (`kind !== "Movie"` means Series) is deleted.

**Library page.**

- `useBrowseScope()` returns `{ title, eyebrow, browse, source(params), rememberKey }`:
  - for a library: name, `library.browse`, `{ scope: "library" }`, and `library:<id>`;
  - for the title view: the old ladder minus the Movie/Series branches, `titles`, `{ scope: "titles" }`, and no remember key.
- `LibraryFilters` takes `capabilities` instead of `kinds`:
  - It renders only the sorts and filters the native listed.
  - Genre and official-rating options come from `useLibraryOptions(id)` on library pages and from today's `/api/genres` and `/api/official-ratings` on the title view.
  - `librarySortLabel` keeps its "Date show added" wording, keyed by `library.kind === "shows"`, which is presentation only.
  - "Hide watched by default" applies only when `browse.filters` includes `watched`.
- The A–Z rail renders only when `browse.letterIndex` is true.
- Remembered filters read and write `library:<id>`.

**How filters degrade.** Live libraries offer name, date added and release year sorts. Recursive live grids such as music and books also offer the favorites filter; folder grids offer no filters. Live libraries have no A–Z rail, no random sort (Jellyfin's Random is unseeded and pages would overlap), and no watched filter (Jellyfin's folder played-state is computed and not what a user means). The UI never shows a disabled control for a live library. It shows only what the library can do.

**Grid.** `ItemGrid` takes `source: BrowseSource` in place of `query`, and its infinite query is `useBrowsePages(source)`. The same grid serves catalog libraries, live libraries, the title view and a generic container's children. Query keys are `["browse", source]`; the existing `library_changed` invalidation now targets the `browse` prefix.

**Details.** `ItemDetail.tsx` switches on `layout`:

- `"title"`: today's page. Its back default and genre links use `libraryId` through `defaultDetailNavigationState` and `genreRoute`. That replaces `navigation.ts:21` and `DetailHero.tsx:51-53`.
- `"generic"`: a new `GenericItemPage` built from the existing `DetailHeroLayout`, `DetailPrimitives` and `DetailFacts` components. It shows backdrop and poster through the image proxy, name, year, runtime and overview. Genres are plain chips with no links, because live grids have no genre filter. Studios and people come from `/api/item/{id}/about`. For `isFolder`, it adds an `ItemGrid` with `{ scope: "children", itemId }`. It has no Play, Favorite, Letterboxd, ratings, TMDB chip or MediaInfo.

**Back navigation.**

- `detailNavigationState` labels `/library/<id>` as "Back to <library name>". Folder-to-folder navigation already inherits its state from `/item/` pages (`navigation.ts:34`), so a drill-down through albums or photo folders returns to the library with its filters.
- Without state (a reload or deep link): a title page with `libraryId` goes back to that library; everything else goes back to Home.

**Collapses removed:** `navigation.ts:21`, `DetailHero.tsx:51`, `AppSidebar.tsx:295`, and in `library-filters.ts` the `"Movie"` default and the `"Movie" | "Series"` filter. `ViewingSync.tsx:30`'s destination table becomes `startupRoute`. In `Home.tsx`, the `EmptyHome` Movies and Series buttons become links to the first two libraries, and `rowViewAll` (`Home.tsx:220-230`) gains `library`.

## Home

**Settings model.**

- `HomeElementId::Library { id }` is added.
- `reconcile_home_elements` gains a `&LibraryRegistry` argument.
  - `Known`: it inserts every missing library after the last built-in element, in server order, and drops elements whose library is gone.
  - `Unknown`: it keeps saved Library elements with `available: false`. A cold start never shrinks the list the user would save.
- First-discovery default: enabled, except for movies and tvshows libraries, whose shelves duplicate the cross-library "Recently Added Movies/Shows" built-ins (open question 1).
- `HomeSettings::fresh` order is built-ins, then libraries, then genres. The existing 409 "Home options changed while the page was open" already covers a library appearing while Settings is open.
- Labels and categories come from `settings_view`: `Latest in <name>`, category `Library`. `available` means the library is in a `Known` registry.

**Rendering.**

- `/api/home` does not build Library rows. Its `rows` stay the built-in, genre and collection rows.
- `Home.tsx` renders each enabled, available `library` element as `<LibraryShelf element />`. That component calls `useLibraryLatest(id)` and draws a `MediaRail` titled with the element label, with View all at `/library/<id>?sort=added`.
- Every library shelf takes the same path, so a slow live shelf never delays Home or the other shelves, and an offline live shelf renders nothing, like an empty row today.

**Server changes.** `librariesChanged` invalidates `home` and `home-settings`. A library added on the server shows up in Home settings and, by the default rule, on Home within one sync cycle. A removed library's shelf disappears and is dropped from the file on the user's next Home save.

## Durable accounts.json formats

The file version stays 1. Every change is additive: a v1 file written by the current app loads unchanged. A file written by this version can contain `{"kind":"library"}`, `{"library":…}` or `library:<id>`. The current app reads such a file through `parse_or_recover(intact=true)`, which reports "written by a different MediaFlick version and was left unchanged" and does not rewrite it. That is the AGENTS.md rule, and the same thing already happens for the `collection` variant.

| Saved value | On load | Shown to the UI | Written back |
| --- | --- | --- | --- |
| `browsing.last = "/library?kind=Movie&…"` | valid (path `/library`) | navigates to the cross-library Movie view | unchanged until the next `last` save |
| `browsing.Movie` / `browsing.Series` | accepted (`BROWSING_LEGACY_PAGES`) | ignored: library pages read `library:<id>` | pruned in the same mutation as the account's next `save_browsing`. The user's old remembered filters do not carry over (changelog says so). |
| `browsing["library:<id>"]` | new; id validated; route must be `/library/<id>?…` | remembered filters for that library | written by library pages |
| `browsing` with more than 256 keys | rejected (new bound) | — | — |
| `startupDestination: "movies"` / `"series"` | accepted | the first Movies or TV shows library; Home if none; Home for that one read while the registry is `Unknown` | unchanged until the user saves Viewing settings, then `{"library":"<id>"}` or whatever they chose |
| `startupDestination: {"library":"<id>"}` | new; id validated | that library; Home if the library is gone | as chosen |
| `home.elements[]` without libraries | valid | Library elements inserted by reconcile (view only) | written on the next Home save |
| `home.elements[{"kind":"library","id":…}]` | new; id validated, duplicates rejected | `Latest in <name>`; `available: false` if gone or unknown | dropped on the next Home save only if the registry is `Known` and lacks it |

Nothing is migrated at load, nothing is rewritten without a user save, and no unknown-variant preservation is added. Legacy acceptance is two names in a const and two enum variants, deleted in the release-notes PR after 1.0 if the owner wants a clean format.

## Module map

```
NEW src/library/libraries.rs          identity, kind, CollectionType mapping, profile table, capabilities, registry
NEW src/library/catalog_libraries.rs  Library::{libraries, replace_libraries, replace_membership, library_of, library_filter_options}
NEW src/library/browse.rs             tier dispatch (library_page/letters/options/latest, item_detail, children); mod live
NEW src/library/sync/membership.rs    refresh_libraries, membership_pass
NEW src/shell/cef/api/libraries.rs    thin /api/libraries routes
CHG src/library/{model,mod,query,db,session_store}.rs, sync.rs, sync/cycle.rs
CHG src/jellyfin/api/{items,model}.rs; src/preferences/{accounts,viewing}.rs
CHG src/shell/cef/api/{catalog,settings}.rs; src/app/services.rs (LibrariesChanged)
NEW ui/src/lib/libraries.ts           BrowseSource, paths, icons, browsingKey, startupRoute, genreRoute, back defaults
NEW ui/src/components/detail/GenericItemPage.tsx
CHG ui/src/lib/{api/types,api/client,api/urls,library-filters,navigation,queries}.ts
CHG ui/src/components/{AppSidebar,ItemGrid,LibraryFilters}.tsx
CHG ui/src/routes/{Library,ItemDetail,Home}.tsx, settings/{Home,Viewing}Settings, App.tsx (/library/:libraryId)
```

A tier question is answered in one file: `browse.rs`. `CollectionType` is read in one file: `libraries.rs`. The UI reads capabilities in one file: `library-filters.ts`. A trace goes from route to `browse.rs`, then to either `query.rs` or `items.rs`: three files.

## Synthesis decision

Base: candidate 2 (two tiers). The cross-judge (claude-haiku-5-5, ran no candidate) picked the same base independently: C2 scored 4/4/3/4/4/4 against C1's 3/4/4/3/3/3. C2 keeps the server-wide sync loop (bootstrap, watermark, guarded `retain_ids`) unchanged. C1 rewrites all three per library, demotes Movies in mixed libraries to the base profile, and caches photos into Search. Losing candidate: `C:\Users\pho\AppData\Local\Temp\arena-libraries\candidate-1\design.md`.

**These amendments override the sections above.**

1. **Home shelves default on for every library**, movies and tvshows included. Owner decision 4 says "on the main page for every library that exists". Open question 1 is closed. Built-in "Recently Added Movies/Shows" stay as they are.
2. **The membership pass never replaces a non-empty set with an empty answer.** `cycle.rs:461` already refuses an empty answer for deletion, and membership follows the same rule. A library that really became empty is emptied by `replace_libraries` when it disappears, or on the next non-empty answer.
3. **Card aspect (graft from C1).** `ItemSummary` gains `primaryAspect: number | null` from `PrimaryImageAspectRatio`, which is measured at 1.0 for albums. Cards keep the 2:3 frame so the virtual grid is unchanged, and use `object-contain` when `primaryAspect >= 0.8`. Open question 6 is closed.
4. **The manual `Deserialize for HomeElement` Wire enum (`accounts.rs:76-91`) gains the Library variant.** The judge found that both designs missed it.
5. **Downgrade behaviour is stated honestly.** A serde rejection leaves the file unchanged. A validator rejection, such as a `library:` browsing key, makes an older build fail `AccountConfigurationService::open`, so that build won't start until it is upgraded. The file is never rewritten. The PR description must say this. It does not violate AGENTS.md, which forbids rewriting or moving a newer file and does not forbid refusing to start.
6. **Remaining item-type literals go into the registry or are listed as deliberate exceptions.** These are `items.rs:194`, `:211` (box-set children) and `catalog.rs:585` (official-ratings default).
7. **The claim that tier promotion needs no UI change is withdrawn.** Promoting music to the catalog tier needs a `CatalogKind` variant, ingest, and a title layout.
8. **Generic items get no Play in this change.** Owner decision 3 is base experience only, and the AGENTS.md playback invariants require tracing every backend before playback reaches new item types. Playing Video and MusicVideo is the first follow-up. `playable` from C1 is deferred with it.
9. **Live TV and Trailers libraries are shown** with the generic folder grid, per owner decision 1, "every library type". Open question 3 is closed.

**Measured after synthesis** (`%TEMP%/jfprobe/latest-report.json`, demo server): `/Items/Latest?ParentId=<library>` returns a bare array grouped as expected (Music gives MusicAlbum, Shows gives Series). Direct children of a library are physical Folders for movies and MusicArtist for music. `SortBy=Random` returns different pages per request, which confirms the design's ban on live random sorting. Risk 7 is retired for music and tvshows. Books, photos, home videos and mixed libraries are still unmeasured, because neither server has one.

**Rejected from C1:**
- Per-library sync cursors. They are the riskiest rewrite, for no user-visible gain.
- `profile` replacing `kind` on the wire. That means about 105 `.kind` reads rewritten, against C2's capability model, which hides the same decision.
- `is_title` and `profile` as stored columns. They are derivations that heal only daily.
- Home rows carrying `sort`. Library shelves are separate components with a fixed `?sort=added`.

## Tradeoffs accepted

- We accept that live libraries need the server in exchange for zero sync cost and zero local modelling of music, photo and book hierarchies. Their pages show the existing error state offline. Movies and TV shows stay fully offline.
- We accept that Search, Favorites, My list and genre shelves cover only cached titles (Movie and Series) in exchange for keeping every existing cached query's kind semantics intact. A favorited album appears in its library's favorites filter, not in Favorites.
- We accept thinner filters on live libraries (three sorts, at most a favorites filter, no A–Z rail) in exchange for never sending Jellyfin a query shape it answers inconsistently, such as unseeded random or folder played-state.
- We accept that a Movie inside a mixed library appears in that library's folder grid, not in any Movies library grid, in exchange for one source per grid. It keeps the title page and every global view.
- We accept two `/Items` round trips per generic details page (item and children) plus the existing about call, in exchange for no cache for those types.
- We accept losing remembered Movies and Series filters once, at upgrade, in exchange for not carrying a legacy key reader into the per-library model, where it would be ambiguous with two movies libraries.
- We accept that on the first launch after the upgrade, a legacy `movies` or `series` startup destination opens Home, because the registry is `Unknown` until the first `/UserViews`. In exchange there is a single writer, the sync worker, for the registry, and no read-through write from an API handler.
- Membership is a separate table and pass, not a column set during ingest. That can look over-built. It is what lets the catalog walk stay server-wide and unchanged.

## Alternatives considered

- **One cached catalog for every type** (sync every type, add `library_id`, walk per library). It hides one query path, but it exposes every kind-less cached query to new kinds, which is decision 5's bug class multiplied. Sync cost and FTS noise grow with music and photo libraries. It means rewriting the bootstrap, watermark and identity loop with per-library cursors, and modelling hierarchies locally for types with no features yet. It lost on depth: it pushes Jellyfin's type hierarchy into our schema before anything uses it.
- **Tier boundary by item type** (catalog kinds cached, others live, a mixed grid merging both). Mixed libraries would look uniform, but only through a merged pager whose offsets, totals and sort order across SQLite `NOCASE` and Jellyfin `SortName` cannot be made consistent. Rejected as unsound, not merely complex.
- **Live for every grid, cache only for global features.** The smallest code, but movies and tvshows lose offline use, the A–Z rail and the locally computed sorts and filters (episode-added, series in progress, unplayed counts). That breaks decision 2.
- **Membership as an `items.library_id` column.** It ties membership to ingest order: the server-wide walk does not know the library, so the first fill would show empty grids. It fires the FTS update trigger on every membership write, and it assumes one library per item.
- **Generic proxy (`/api/jellyfin/items?...`).** Forbidden by AGENTS.md, and it puts Jellyfin's wire format on the UI contract.
- **Library shelves built inside `/api/home`.** One slow or offline live shelf would delay or fail all of Home. Per-shelf requests isolate it.

## Open questions and risks

1. Should movies and tvshows library shelves start disabled because they duplicate "Recently Added Movies/Shows"? I recommend yes, with every other kind enabled. Or should every shelf start enabled, as decision 4 might mean?
2. Should generic pages for `Video`, `MusicVideo` and `Audio` get Play, since the mpv path accepts any video item id? I recommend not in this change: decision 3 says base experience only.
3. Is a `livetv` library meant to show? Folder browsing of its view is unmeasured, and channels may not render usefully. Show it generically, or exclude `LiveTv` from the sidebar until it has a real page?
4. Do Movies and Series inside a mixed library get the full title page? This design says yes, by item. Confirm that this reading of decision 3 is right.
5. Only music, movies, tvshows and boxsets were measured. The registry rows for books, photos, homevideos, musicvideos, playlists, trailers and mixed are informed guesses. Can the owner add one small library of each type on the archlinux server, so that PR 3's drive covers every row before merge?
6. Cards assume a 2:3 poster, so albums and photos are cropped. Is that acceptable for the base experience, or should `PrimaryImageAspectRatio` be added to `ItemSummary` now?
7. Risk: `/Items/Latest` returns a bare array and groups by parent. Its result for books, photos and folder-style libraries is unmeasured. PR 5 should probe it before relying on it, and fall back to `/Items?ParentId&SortBy=DateCreated&Recursive=true` per profile if the result is poor.

## Delivery plan

Each PR is independently mergeable. Each adds a `changelog.d/` fragment and runs the AGENTS.md component checks for what it touches. Each is proven by a `just verify <drive>` run that ends in an artifact under the drive's run directory. New drives live in `.claude/skills/verify-mediaflick/drives/`. PR 0 is the in-flight bug-fix branch `fix/search-favorites-top-level-only` and lands first.

**PR 1: Library registry and catalog membership (native only).**

- Contents:
  - `libraries.rs`, `CatalogKind`, `KindFilter` (absorbs PR 0's rule);
  - schema 19, `CATALOG_TABLES`;
  - `refresh_libraries`, `membership_pass`, cycle steps 1, 2, 5 and 6;
  - `GET /api/libraries`;
  - catalog-tier `/api/libraries/{id}/items|letters|options|latest`. Live libraries answer 501 here; the route is not yet linked from the UI.
- Checks: `just rust-quality`, `just test`.
- Proof: drive `libraries-api.mjs` against the demo server and `--account` on the owner's server. From inside the app page it calls `/api/libraries` and every catalog library's `/items?limit=1`, and from Node it calls Jellyfin's `/UserViews` and `/Items?ParentId&IncludeItemTypes&Limit=0` with the session token.
- Artifact: `libraries-report.json`. It must show that names, order and kinds equal `/UserViews` and that each catalog library's `total` equals Jellyfin's `TotalRecordCount`. The report is regenerable by rerunning the drive.

**PR 2: Generic details.**

- Contents:
  - `DetailView`, `GenericItemDetail`, `BaseItemDto.is_folder`;
  - `browse::item_detail`, paged `/api/item/{id}/children` with `children_source`;
  - `ItemDetail.tsx` layout switch, `GenericItemPage`, `ItemGrid` `source` prop with the `children` scope;
  - `defaultDetailNavigationState` and `genreRoute` (collapse removal in `navigation.ts` and `DetailHero.tsx`).
- Checks: Rust and UI checks.
- Proof: drive `generic-detail.mjs` on the demo server. Node reads the Music library's first `MusicAlbum` id from Jellyfin. The drive navigates to `/item/<id>`, asserts there is no TMDB, Letterboxd or Play control, asserts the track children render, opens a Movie and asserts the title page still renders its collection chip and ratings slots.
- Artifact: two screenshots and `generic-detail.json`, with the asserted controls and children count against Jellyfin's `ChildCount`.

**PR 3: Library browse and sidebar.**

- Contents:
  - live branch of `/api/libraries/{id}/items` and `latest`;
  - the `/library/:libraryId` route, `useBrowseScope`, capability-driven `LibraryFilters`, Libraries sidebar group;
  - `library:<id>` browsing keys with legacy acceptance, pruning and the 256-key bound;
  - `librariesChanged` event;
  - removal of the remaining kind collapses.
- Proof: drive `library-browse.mjs` on demo and owner servers. For every sidebar entry it:
  - clicks it;
  - screenshots the grid;
  - records card count and total;
  - for catalog libraries: applies a sort and a filter and checks that the total matches the API;
  - for live libraries: checks the offered sorts equal the library's `browse.sorts`;
  - opens the first card;
  - presses back and asserts the URL and filters are restored.

  A second run uses `--seed` with an accounts.json holding `browsing.Movie`. It asserts the app starts and the key is pruned after one filter change.
- Artifacts: `library-browse.json`, one screenshot per library, and the before and after `accounts.json`.

**PR 4: Startup destination.**

- Contents:
  - `StartupDestination::Library`, `startup_view`;
  - ViewingSettings select listing Home, each library, Releases and Last;
  - `startupRoute`.
- Proof: drive `startup-library.mjs` with three seeded accounts.json files: `"movies"`, `{"library":"<shows id>"}`, and a removed id. It asserts the landing route each time: the first Movies library, the Shows library, then Home. It also asserts that the file is unchanged until Settings → Save.
- Artifacts: `startup-library.json`, three screenshots, and file hashes before and after.

**PR 5: Home library shelves.**

- Contents:
  - `HomeElementId::Library`, reconcile with `LibraryRegistry`, labels;
  - `/api/libraries/{id}/latest` for live libraries, using a probe-informed fallback (risk 7);
  - `LibraryShelf`, Home settings category, `rowViewAll` for `library`.
- Proof: extend `home-settings.mjs` into `home-library-shelves.mjs`:
  - assert that Home settings lists one `Latest in <name>` per `/UserViews` entry;
  - enable "Latest in Music", Save, assert that Home renders it with the same first ids as Jellyfin's `/Items/Latest`;
  - Discard and Reset round-trip.
- Artifacts: `home-library-shelves.json` and screenshots of Settings and Home.

**After merge.** Consolidate the changelog fragments in the release-notes PR. None of these PRs touches Companion, so no `just plugin-deploy` is needed.

## Next implementation step

Write `src/library/libraries.rs` with `LibraryKind::from_collection_type` and the exhaustive `profile()` table, then `fetch_user_views` and cycle step 1 behind schema 19.
