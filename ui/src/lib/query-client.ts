import { QueryClient, queryOptions } from "@tanstack/react-query"
import { ApiError, type ItemQuery, type PlayerState } from "./api"
import type { TitleKind } from "./kinds"
import type { BrowseSource } from "./libraries"

/**
 * The app's one cache, created by `main.tsx` and handed to the provider. React
 * code reaches it through `useQueryClient()`, and the helpers below take it as
 * a parameter, so a test can render against its own client.
 *
 * `refetchOnWindowFocus` is off because the library cache is local and the
 * sync thread is the source of change; a focus-driven refetch storm buys
 * nothing here.
 */
export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: 2 * 60_000,
        gcTime: 10 * 60_000,
        refetchOnWindowFocus: false,
        refetchOnReconnect: true,
        retry: (failureCount, error) => {
          // An expired token will not resolve itself by retrying — the shell
          // needs to fall back to sign-in instead.
          if (error instanceof ApiError && (error.expired || error.status === 401)) return false
          return failureCount < 1
        },
      },
    },
  })
}

const ITEMS_ROOT = "items"
const BROWSE_ROOT = "browse"
const LIBRARY_LATEST_ROOT = "library-latest"

declare const minted: unique symbol

/**
 * A key from `queryKeys` below. Registered with TanStack Query, so every
 * `queryKey` the app hands a hook, a filter or the cache must come from this
 * registry: a raw `["seerr", ...]` literal no longer type-checks.
 */
type MintedQueryKey = readonly unknown[] & { readonly [minted]: true }

declare module "@tanstack/react-query" {
  interface Register {
    queryKey: MintedQueryKey
  }
}

function key<const Parts extends readonly unknown[]>(...parts: Parts) {
  // SAFETY: the brand is a phantom type with no runtime field. Minting it here,
  // and only here, is the fact it records.
  return parts as Parts & MintedQueryKey
}

/** Roots whose every key one invalidation covers. */
type QueryRoot =
  | typeof ITEMS_ROOT
  | typeof BROWSE_ROOT
  | typeof LIBRARY_LATEST_ROOT
  | "person-items"
  | "viewing"
  | "letterboxd"
  | "collections"

export const queryKeys = {
  root: (root: QueryRoot) => key(root),
  seerrScope: (scope: "requests" | "search" | "person" | "discover" | "media") => key("seerr", scope),
  collectionsOf: (account: string) => key("collections", account),
  status: key("status"),
  companion: key("companion"),
  calendar: (start: string, end: string) => key("calendar", start, end),
  settings: key("settings"),
  viewing: (account: string) => key("viewing", account),
  browsing: (account: string) => key("browsing", account),
  ratingsStatus: key("ratings", "status"),
  home: key("home"),
  homeResume: key("home", "resume"),
  homeSettings: key("home", "settings"),
  billboard: key("billboard"),
  genres: key("genres"),
  libraries: key("libraries"),
  libraryOptions: (id: string) => key("library-options", id),
  libraryLatest: (id: string) => key(LIBRARY_LATEST_ROOT, id),
  officialRatings: (kinds: readonly TitleKind[]) => key("official-ratings", kinds),
  serverInfo: (server: string) => key("server-info", server),
  quickConnect: (secret: string) => key("quick-connect", secret),
  // Live exact-person pages are not local SQLite projections. Keeping their
  // root separate prevents every committed sync batch from re-running the
  // same Jellyfin filmography query.
  items: (query: ItemQuery) =>
    query.personId ? key("person-items", query) : key(ITEMS_ROOT, query),
  browsePage: (source: BrowseSource, offset: number, limit: number) =>
    key(BROWSE_ROOT, source, { offset, limit }),
  browseLetters: (source: BrowseSource) => key(BROWSE_ROOT, source, "letters"),
  personResolution: (jellyfinId: string, tmdbId: number | null, name: string) =>
    key("person", "resolve", jellyfinId, tmdbId, name),
  item: (id: string) => key("item", id),
  itemSynopsis: (id: string) => key("item", id, "synopsis"),
  itemAbout: (id: string) => key("item", id, "about"),
  letterboxdReviews: (id: string) => key("letterboxd", "item", id),
  letterboxdMovieReviews: (tmdbId: number) => key("letterboxd", "movie", tmdbId),
  letterboxdProfiles: key("letterboxd", "profiles"),
  children: (id: string) => key("item", id, "children"),
  media: (id: string) => key("item", id, "media"),
  trailer: (id: string) => key("item", id, "trailer"),
  nextUp: (id: string) => key("item", id, "nextup"),
  playbackNeighbors: (id: string) => key("player", "neighbors", id),
  playerState: key("player", "state"),
  seerrStatus: key("seerr", "status"),
  seerrSearch: (term: string) => key("seerr", "search", term),
  seerrSearchInfinite: (term: string) => key("seerr", "search", term, "infinite"),
  seerrPersonCredits: (tmdbId: number, jellyfinId: string) =>
    key("seerr", "person", tmdbId, jellyfinId, "credits"),
  seerrDiscover: (row: string, resultSetKey: string) =>
    key("seerr", "discover", row, resultSetKey),
  seerrGenres: (mediaType: string) => key("seerr", "genres", mediaType),
  seerrMedia: (mediaType: string, tmdbId: number) => key("seerr", "media", mediaType, tmdbId),
  seerrRequestOptions: (mediaType: string, is4k: boolean) =>
    key("seerr", "request-options", mediaType, is4k),
  seerrRequests: (filter: string) => key("seerr", "requests", filter),
  collectionSettings: (account: string) => key("collections", account, "settings"),
  collectionTemplates: (account: string) => key("collections", account, "templates"),
  collectionProfiles: (account: string) => key("collections", account, "profiles"),
  collectionPublicListSearch: (account: string, term: string) =>
    key("collections", account, "public-list-search", term),
  collectionMine: (account: string) => key("collections", account, "mine"),
  collectionMineDetail: (account: string, id: string) =>
    key("collections", account, "mine", id),
  collectionFranchises: (account: string, localDate: string) =>
    key("collections", account, "franchises", localDate),
  collectionFranchise: (account: string, id: number, localDate: string) =>
    key("collections", account, "franchises", id, localDate),
  collectionTitle: (account: string, mediaType: string, tmdbId: number) =>
    key("collections", account, "title", mediaType, tmdbId),
  collectionJellyfin: (account: string) => key("collections", account, "jellyfin"),
  collectionJellyfinDetail: (account: string, id: string) =>
    key("collections", account, "jellyfin", id),
  movieCollection: (account: string, tmdbId: number) =>
    key("collections", account, "movie", tmdbId),
}

/**
 * Options for a query on an id the caller may not have yet: it runs once the id
 * is there, and every observer of the key carries the real fetcher.
 *
 * Use it instead of `id!` in a fetcher. Do not stand in `skipToken` for the
 * missing id: observers of one key share its options, so a `skipToken`
 * observer leaves the others nothing to refetch with (b2926ab).
 */
export function optionalIdQuery<Id, Data>(
  id: Id | null | undefined,
  queryKey: MintedQueryKey,
  fetch: (id: Id, context: { readonly signal: AbortSignal }) => Promise<Data>,
  enabled = true,
) {
  return queryOptions({
    queryKey,
    // Pass the context through untouched: reading `signal` makes TanStack cancel
    // the query when its last observer unmounts, so only fetchers that read it opt in.
    queryFn: (context) => (id ? fetch(id, context) : Promise.reject(new Error("query ran without its id"))),
    enabled: Boolean(id) && enabled,
  })
}

const ACCOUNT_QUERY_ROOTS = new Set([
  "billboard",
  BROWSE_ROOT,
  "calendar",
  "collections",
  "companion",
  "genres",
  "home",
  "item",
  ITEMS_ROOT,
  "letterboxd",
  "libraries",
  LIBRARY_LATEST_ROOT,
  "library-options",
  "official-ratings",
  "person",
  "person-items",
  "player",
  "ratings",
  "seerr",
])

export function removeAccountQueryData(client: QueryClient) {
  client.removeQueries({
    predicate: (query) => ACCOUNT_QUERY_ROOTS.has(String(query.queryKey[0])),
  })
}

/**
 * Everything that changes when a request is made or cancelled. Kept apart from
 * `invalidateMediaSurfaces`: a Seerr write changes nothing about the local
 * library, and refetching the grid over it would be wasted work.
 */
export function invalidateSeerrSurfaces(client: QueryClient) {
  const active = { refetchType: "active" as const }
  void client.invalidateQueries({ queryKey: queryKeys.seerrScope("requests"), ...active })
  void client.invalidateQueries({ queryKey: queryKeys.seerrScope("search"), ...active })
  void client.invalidateQueries({ queryKey: queryKeys.seerrScope("person"), ...active })
  void client.invalidateQueries({ queryKey: queryKeys.seerrScope("discover"), ...active })
  void client.invalidateQueries({ queryKey: queryKeys.seerrScope("media"), ...active })
  void client.invalidateQueries({ queryKey: queryKeys.root("collections"), ...active })
  void client.invalidateQueries({ queryKey: queryKeys.seerrStatus, ...active })
}

/**
 * Applies a control's expected outcome to the player snapshot right away, so a
 * button does not look like it did nothing until mpv pushes the new state.
 */
export function patchPlayerState(client: QueryClient, patch: Partial<PlayerState>) {
  client.setQueryData<PlayerState>(queryKeys.playerState, (previous) =>
    previous ? { ...previous, ...patch } : previous,
  )
}

/**
 * The user-state invalidation entry point. Watched/favorite/playback changes
 * refresh only projections that display that state; live prose, credits,
 * trailers, and technical media facts are independent and stay cached.
 *
 * Takes several ids because the item whose state changed is often not the one
 * on screen: marking an episode watched from a season page has to refresh the
 * season's child list, and the series' Next Up, as well as the episode itself.
 */
export function invalidateMediaSurfaces(
  client: QueryClient,
  ...itemIds: (string | null | undefined)[]
) {
  const active = { refetchType: "active" as const }
  const exactActive = { ...active, exact: true }
  void client.invalidateQueries({ queryKey: queryKeys.home, ...active })
  // Billboard slides come from a randomized endpoint. Their user-data controls
  // are patched by the mutations, so refreshing that query here would replace
  // the active title instead of merely updating it.
  void client.invalidateQueries({ queryKey: queryKeys.root(ITEMS_ROOT), ...active })
  void client.invalidateQueries({ queryKey: queryKeys.root("person-items"), ...active })
  void client.invalidateQueries({ queryKey: queryKeys.root(BROWSE_ROOT), ...active })
  void client.invalidateQueries({ queryKey: queryKeys.root(LIBRARY_LATEST_ROOT), ...active })
  for (const itemId of itemIds) {
    if (!itemId) continue
    void client.invalidateQueries({ queryKey: queryKeys.item(itemId), ...exactActive })
    void client.invalidateQueries({ queryKey: queryKeys.children(itemId), ...exactActive })
    void client.invalidateQueries({ queryKey: queryKeys.nextUp(itemId), ...exactActive })
  }
}

type LibraryChangeScope = "library" | "catalog" | "items" | "live" | "resync"

const LIST_ROOTS = [ITEMS_ROOT, "genres", "libraries", "library-options", "official-ratings", "status", "collections"]

const LIBRARY_CHANGE_REFRESHES: Record<
  LibraryChangeScope,
  {
    roots: ReadonlySet<string>
    home: "all" | "summary" | "none"
    details: "named" | "all"
    grids: (source: BrowseSource) => boolean
    /**
     * Whether every "Latest in <library>" shelf refreshes, live ones included;
     * a library the batch names refreshes either way. `catalog-changed` (the
     * bootstrap's pages) refreshes only the catalog libraries it names;
     * `library-changed` (incremental cycles, pushed changes) and
     * `live-changed` (live and resync scopes) refresh every shelf once per
     * event.
     */
    shelves: boolean
  }
> = {
  library: {
    roots: new Set(LIST_ROOTS),
    home: "all",
    details: "named",
    grids: (source) => source.scope !== "titles" || !source.query.personId,
    shelves: true,
  },
  catalog: {
    roots: new Set(LIST_ROOTS),
    home: "summary",
    details: "named",
    grids: (source) => source.scope === "titles" && !source.query.personId,
    shelves: false,
  },
  items: { roots: new Set(), home: "none", details: "named", grids: () => false, shelves: false },
  live: { roots: new Set(), home: "none", details: "named", grids: (source) => source.scope === "library", shelves: true },
  resync: {
    roots: new Set(),
    home: "none",
    details: "all",
    grids: (source) => source.scope === "library" || source.scope === "children",
    shelves: true,
  },
}

function isBrowseSource(value: unknown): value is BrowseSource {
  if (typeof value !== "object" || value === null || !("scope" in value)) return false
  return value.scope === "titles" || value.scope === "library" || value.scope === "children"
}

/**
 * One committed native batch becomes one React Query invalidation operation.
 * The predicate covers aggregate projections, diagnostics/status, changed
 * details, and parent/series/season contexts while preserving active-only
 * refetch behavior. `libraryIds` names libraries whose grids and Latest
 * shelves refresh beyond the scope's own.
 */
export function invalidateLibraryChanged(
  client: QueryClient,
  itemIds: readonly string[],
  contextIds: readonly string[] = [],
  scope: LibraryChangeScope = "library",
  libraryIds: readonly string[] = [],
) {
  const { roots, home, grids, details, shelves } = LIBRARY_CHANGE_REFRESHES[scope]
  const relevant = new Set([...itemIds, ...contextIds])
  const namedLibraries = new Set(libraryIds)
  void client.invalidateQueries({
    refetchType: "active",
    predicate: (query) => {
      const [root, id, part] = query.queryKey
      if (root === "home") return home === "all" || (home === "summary" && query.queryKey.length === 1)
      if (roots.has(String(root))) return true
      if (root === LIBRARY_LATEST_ROOT) return shelves || namedLibraries.has(String(id))
      if (root === BROWSE_ROOT && isBrowseSource(id)) {
        return grids(id)
          || (id.scope === "children" && relevant.has(id.itemId))
          || (id.scope === "library" && namedLibraries.has(id.libraryId))
      }
      if (root === "item" && details === "all") return query.queryKey.length === 2 || part === "children"
      // The billboard endpoint is random. A committed sync batch may update
      // shelves around it, but must not replace a selected slide or its video.
      return root === "item" && relevant.has(String(id))
    },
  })
}

export function invalidateLibrariesChanged(client: QueryClient) {
  void client.invalidateQueries({ queryKey: queryKeys.libraries })
  void client.invalidateQueries({ queryKey: queryKeys.root(LIBRARY_LATEST_ROOT) })
  void client.invalidateQueries({ queryKey: queryKeys.home, exact: true })
  void client.invalidateQueries({ queryKey: queryKeys.homeSettings, exact: true })
  void client.invalidateQueries({ queryKey: queryKeys.root("viewing") })
}

export function invalidateLibraryMemberships(client: QueryClient) {
  void client.invalidateQueries({ queryKey: queryKeys.libraries, exact: true })
}
