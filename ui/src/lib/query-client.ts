import { QueryClient, queryOptions } from "@tanstack/react-query"
import { ApiError, type ItemQuery, type PlayerState } from "./api"
import type { TitleKind } from "./kinds"
import type { BrowseSource } from "./libraries"

export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: 2 * 60_000,
        gcTime: 10 * 60_000,
        refetchOnWindowFocus: false,
        refetchOnReconnect: true,
        retry: (failureCount, error) => {
          if (error instanceof ApiError && (error.expired || error.status === 401 || error.startupFailed)) return false
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

type MintedQueryKey = readonly unknown[] & { readonly [minted]: true }

declare module "@tanstack/react-query" {
  interface Register {
    queryKey: MintedQueryKey
  }
}

function key<const Parts extends readonly unknown[]>(...parts: Parts) {
  return parts as Parts & MintedQueryKey
}

type QueryRoot =
  | typeof ITEMS_ROOT
  | typeof BROWSE_ROOT
  | typeof LIBRARY_LATEST_ROOT
  | "person-items"
  | "similar"
  | "viewing"
  | "letterboxd"
  | "collections"

export const queryKeys = {
  root: (root: QueryRoot) => key(root),
  seerrScope: (scope: "requests" | "search" | "person" | "discover" | "media") => key("seerr", scope),
  collectionsOf: (account: string) => key("collections", account),
  status: key("status"),
  recovery: key("recovery"),
  companion: key("companion"),
  calendar: (start: string, end: string) => key("calendar", start, end),
  settings: key("settings"),
  whatsNew: key("whats-new"),
  viewing: (account: string) => key("viewing", account),
  browsing: (account: string) => key("browsing", account),
  ratingsStatus: key("ratings", "status"),
  home: key("home"),
  homeResume: key("home", "resume"),
  homeSettings: key("home", "settings"),
  billboard: key("billboard"),
  genres: key("genres"),
  studios: key("studios"),
  tags: key("tags"),
  libraries: key("libraries"),
  libraryOptions: (id: string) => key("library-options", id),
  libraryLatest: (id: string) => key(LIBRARY_LATEST_ROOT, id),
  officialRatings: (kinds: readonly TitleKind[]) => key("official-ratings", kinds),
  serverInfo: (server: string) => key("server-info", server),
  quickConnect: (secret: string) => key("quick-connect", secret),
  items: (query: ItemQuery) =>
    query.personId ? key("person-items", query) : key(ITEMS_ROOT, query),
  browsePage: (source: BrowseSource, offset: number, limit: number) =>
    key(BROWSE_ROOT, source, { offset, limit }),
  browseLetters: (source: BrowseSource) => key(BROWSE_ROOT, source, "letters"),
  personResolution: (jellyfinId: string, tmdbId: number | null, name: string) =>
    key("person", "resolve", jellyfinId, tmdbId, name),
  item: (id: string) => key("item", id),
  book: (id: string, from: "resume" | "start") => key("book", id, from),
  itemSynopsis: (id: string) => key("item", id, "synopsis"),
  itemAbout: (id: string) => key("item", id, "about"),
  letterboxdReviews: (id: string) => key("letterboxd", "item", id),
  letterboxdMovieReviews: (tmdbId: number) => key("letterboxd", "movie", tmdbId),
  letterboxdProfiles: key("letterboxd", "profiles"),
  children: (id: string) => key("item", id, "children"),
  media: (id: string) => key("item", id, "media"),
  trailer: (id: string) => key("item", id, "trailer"),
  nextUp: (id: string) => key("item", id, "nextup"),
  similar: (id: string) => key("similar", id),
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
  seerrCompanies: (term: string) => key("seerr", "companies", term),
  seerrNetworks: key("seerr", "networks"),
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

export function optionalIdQuery<Id, Data>(
  id: Id | null | undefined,
  queryKey: MintedQueryKey,
  fetch: (id: Id, context: { readonly signal: AbortSignal }) => Promise<Data>,
  enabled = true,
) {
  return queryOptions({
    queryKey,
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
  "similar",
  "studios",
  "tags",
])

export function removeAccountQueryData(client: QueryClient) {
  client.removeQueries({
    predicate: (query) => ACCOUNT_QUERY_ROOTS.has(String(query.queryKey[0])),
  })
}

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

export function patchPlayerState(client: QueryClient, patch: Partial<PlayerState>) {
  client.setQueryData<PlayerState>(queryKeys.playerState, (previous) =>
    previous ? { ...previous, ...patch } : previous,
  )
}

export function invalidateMediaSurfaces(
  client: QueryClient,
  ...itemIds: (string | null | undefined)[]
) {
  const active = { refetchType: "active" as const }
  const exactActive = { ...active, exact: true }
  void client.invalidateQueries({ queryKey: queryKeys.home, ...active })
  void client.invalidateQueries({ queryKey: queryKeys.root(ITEMS_ROOT), ...active })
  void client.invalidateQueries({ queryKey: queryKeys.root("person-items"), ...active })
  void client.invalidateQueries({ queryKey: queryKeys.root("similar"), ...active })
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

const LIST_ROOTS = [
  ITEMS_ROOT,
  "genres",
  "studios",
  "tags",
  "libraries",
  "library-options",
  "official-ratings",
  "status",
  "collections",
]

const LIBRARY_CHANGE_REFRESHES: Record<
  LibraryChangeScope,
  {
    roots: ReadonlySet<string>
    home: "all" | "summary" | "none"
    details: "named" | "all"
    grids: (source: BrowseSource) => boolean
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
