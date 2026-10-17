import { QueryClient } from "@tanstack/react-query"
import { ApiError, type ItemQuery, type PlayerState } from "./api"
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

export const queryKeys = {
  status: ["status"] as const,
  companion: ["companion"] as const,
  calendar: (start: string, end: string) => ["calendar", start, end] as const,
  settings: ["settings"] as const,
  viewing: (account: string) => ["viewing", account] as const,
  browsing: (account: string) => ["browsing", account] as const,
  ratingsStatus: ["ratings", "status"] as const,
  home: ["home"] as const,
  homeResume: ["home", "resume"] as const,
  homeSettings: ["home", "settings"] as const,
  billboard: ["billboard"] as const,
  genres: ["genres"] as const,
  libraries: ["libraries"] as const,
  libraryOptions: (id: string) => ["library-options", id] as const,
  libraryLatest: (id: string) => [LIBRARY_LATEST_ROOT, id] as const,
  officialRatings: (kind: string) => ["official-ratings", kind] as const,
  serverInfo: (server: string) => ["server-info", server] as const,
  quickConnect: (secret: string) => ["quick-connect", secret] as const,
  // Live exact-person pages are not local SQLite projections. Keeping their
  // root separate prevents every committed sync batch from re-running the
  // same Jellyfin filmography query.
  items: (query: ItemQuery) =>
    query.personId ? (["person-items", query] as const) : ([ITEMS_ROOT, query] as const),
  browsePage: (source: BrowseSource, offset: number, limit: number) =>
    [BROWSE_ROOT, source, { offset, limit }] as const,
  browseLetters: (source: BrowseSource) => [BROWSE_ROOT, source, "letters"] as const,
  personResolution: (jellyfinId: string, tmdbId: number | null, name: string) =>
    ["person", "resolve", jellyfinId, tmdbId, name] as const,
  item: (id: string) => ["item", id] as const,
  itemSynopsis: (id: string) => ["item", id, "synopsis"] as const,
  itemAbout: (id: string) => ["item", id, "about"] as const,
  letterboxdReviews: (id: string) => ["letterboxd", "item", id] as const,
  letterboxdMovieReviews: (tmdbId: number) => ["letterboxd", "movie", tmdbId] as const,
  letterboxdProfiles: ["letterboxd", "profiles"] as const,
  children: (id: string) => ["item", id, "children"] as const,
  media: (id: string) => ["item", id, "media"] as const,
  trailer: (id: string) => ["item", id, "trailer"] as const,
  nextUp: (id: string) => ["item", id, "nextup"] as const,
  playbackNeighbors: (id: string) => ["player", "neighbors", id] as const,
  playerState: ["player", "state"] as const,
  seerrStatus: ["seerr", "status"] as const,
  seerrSearch: (term: string) => ["seerr", "search", term] as const,
  seerrSearchInfinite: (term: string) => ["seerr", "search", term, "infinite"] as const,
  seerrPersonCredits: (tmdbId: number, jellyfinId: string) =>
    ["seerr", "person", tmdbId, jellyfinId, "credits"] as const,
  seerrDiscover: (row: string, resultSetKey: string) =>
    ["seerr", "discover", row, resultSetKey] as const,
  seerrGenres: (mediaType: string) => ["seerr", "genres", mediaType] as const,
  seerrMedia: (mediaType: string, tmdbId: number) => ["seerr", "media", mediaType, tmdbId] as const,
  seerrRequestOptions: (mediaType: string, is4k: boolean) =>
    ["seerr", "request-options", mediaType, is4k] as const,
  seerrRequests: (filter: string) => ["seerr", "requests", filter] as const,
  collectionSettings: (account: string) => ["collections", account, "settings"] as const,
  collectionTemplates: (account: string) => ["collections", account, "templates"] as const,
  collectionProfiles: (account: string) => ["collections", account, "profiles"] as const,
  collectionPublicListSearch: (account: string, term: string) =>
    ["collections", account, "public-list-search", term] as const,
  collectionMine: (account: string) => ["collections", account, "mine"] as const,
  collectionMineDetail: (account: string, id: string) =>
    ["collections", account, "mine", id] as const,
  collectionFranchises: (account: string, localDate: string) =>
    ["collections", account, "franchises", localDate] as const,
  collectionFranchise: (account: string, id: number, localDate: string) =>
    ["collections", account, "franchises", id, localDate] as const,
  collectionTitle: (account: string, mediaType: string, tmdbId: number) =>
    ["collections", account, "title", mediaType, tmdbId] as const,
  collectionJellyfin: (account: string) => ["collections", account, "jellyfin"] as const,
  collectionJellyfinDetail: (account: string, id: string) =>
    ["collections", account, "jellyfin", id] as const,
  movieCollection: (account: string, tmdbId: number) =>
    ["collections", account, "movie", tmdbId] as const,
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
  void client.invalidateQueries({ queryKey: ["seerr", "requests"], ...active })
  void client.invalidateQueries({ queryKey: ["seerr", "search"], ...active })
  void client.invalidateQueries({ queryKey: ["seerr", "person"], ...active })
  void client.invalidateQueries({ queryKey: ["seerr", "discover"], ...active })
  void client.invalidateQueries({ queryKey: ["seerr", "media"], ...active })
  void client.invalidateQueries({ queryKey: ["collections"], ...active })
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
  void client.invalidateQueries({ queryKey: [ITEMS_ROOT], ...active })
  void client.invalidateQueries({ queryKey: ["person-items"], ...active })
  void client.invalidateQueries({ queryKey: [BROWSE_ROOT], ...active })
  void client.invalidateQueries({ queryKey: [LIBRARY_LATEST_ROOT], ...active })
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
     * `library-changed` (incremental cycles, pushed changes) refreshes every
     * shelf once per event.
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
  void client.invalidateQueries({ queryKey: [LIBRARY_LATEST_ROOT] })
  void client.invalidateQueries({ queryKey: queryKeys.home, exact: true })
  void client.invalidateQueries({ queryKey: queryKeys.homeSettings, exact: true })
  void client.invalidateQueries({ queryKey: ["viewing"] })
}

export function invalidateLibraryMemberships(client: QueryClient) {
  void client.invalidateQueries({ queryKey: queryKeys.libraries, exact: true })
}
