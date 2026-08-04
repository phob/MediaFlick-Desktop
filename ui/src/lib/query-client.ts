import { QueryClient } from "@tanstack/react-query"
import { ApiError, type ItemQuery, type PlayerState } from "./api"

// Defaults follow SILO Server's `lib/query-client.ts` — see
// .planning/research/silo-server-web.md. `refetchOnWindowFocus` is off because
// the library cache is local and the sync thread is the source of change; a
// focus-driven refetch storm buys nothing here.
export const queryClient = new QueryClient({
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

export const queryKeys = {
  status: ["status"] as const,
  companion: ["companion"] as const,
  calendar: (start: string, end: string) => ["calendar", start, end] as const,
  settings: ["settings"] as const,
  home: ["home"] as const,
  billboard: ["billboard"] as const,
  genres: ["genres"] as const,
  serverInfo: (server: string) => ["server-info", server] as const,
  quickConnect: (secret: string) => ["quick-connect", secret] as const,
  // Live exact-person pages are not local SQLite projections. Keeping their
  // root separate prevents every progressive enrichment batch from re-running
  // the same Jellyfin filmography query.
  items: (query: ItemQuery) =>
    query.personId ? (["person-items", query] as const) : (["items", query] as const),
  personResolution: (jellyfinId: string, tmdbId: number | null, name: string) =>
    ["person", "resolve", jellyfinId, tmdbId, name] as const,
  item: (id: string) => ["item", id] as const,
  children: (id: string) => ["item", id, "children"] as const,
  media: (id: string) => ["item", id, "media"] as const,
  trailer: (id: string) => ["item", id, "trailer"] as const,
  nextUp: (id: string) => ["item", id, "nextup"] as const,
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
}

/**
 * Everything that changes when a request is made or cancelled. Kept apart from
 * `invalidateMediaSurfaces`: a Seerr write changes nothing about the local
 * library, and refetching the grid over it would be wasted work.
 */
export function invalidateSeerrSurfaces() {
  const active = { refetchType: "active" as const }
  void queryClient.invalidateQueries({ queryKey: ["seerr", "requests"], ...active })
  void queryClient.invalidateQueries({ queryKey: ["seerr", "search"], ...active })
  void queryClient.invalidateQueries({ queryKey: ["seerr", "person"], ...active })
  void queryClient.invalidateQueries({ queryKey: ["seerr", "discover"], ...active })
  void queryClient.invalidateQueries({ queryKey: ["seerr", "media"], ...active })
  // The quota moves with every request, and it lives on the status.
  void queryClient.invalidateQueries({ queryKey: queryKeys.seerrStatus, ...active })
}

/**
 * Applies a control's expected outcome to the player snapshot right away. The
 * state is polled once a second, so without this every button spends up to a
 * second looking like it did nothing.
 */
export function patchPlayerState(patch: Partial<PlayerState>) {
  queryClient.setQueryData(queryKeys.playerState, (previous?: PlayerState) =>
    previous ? { ...previous, ...patch } : previous,
  )
}

/**
 * The media-surface invalidation entry point, mirroring SILO's
 * `invalidateMediaSurfaceQueries`. Anything that changes catalog or user state
 * calls this rather than trying to patch individual caches.
 *
 * Takes several ids because the item whose state changed is often not the one
 * on screen: marking an episode watched from a season page has to refresh the
 * season's child list, and the series' Next Up, as well as the episode itself.
 * `queryKeys.item(id)` is a prefix of that item's children, media, and Next Up
 * keys, so one invalidation per id covers all of them.
 */
export function invalidateMediaSurfaces(...itemIds: (string | null | undefined)[]) {
  const active = { refetchType: "active" as const }
  void queryClient.invalidateQueries({ queryKey: queryKeys.home, ...active })
  void queryClient.invalidateQueries({ queryKey: ["items"], ...active })
  // Explicit user-data writes should refresh a live person card too; metadata
  // batches deliberately do not, because that response is already server-live.
  void queryClient.invalidateQueries({ queryKey: ["person-items"], ...active })
  for (const itemId of itemIds) {
    if (!itemId) continue
    void queryClient.invalidateQueries({ queryKey: queryKeys.item(itemId), ...active })
  }
}

/**
 * One committed native batch becomes one React Query invalidation operation.
 * The predicate covers aggregate projections, diagnostics/status, changed
 * details, and parent/series/season contexts while preserving active-only
 * refetch behavior.
 */
export function invalidateLibraryChanged(
  itemIds: readonly string[],
  contextIds: readonly string[] = [],
) {
  const relevant = new Set([...itemIds, ...contextIds])
  void queryClient.invalidateQueries({
    refetchType: "active",
    predicate: (query) => {
      const [root, id] = query.queryKey
      if (["home", "items", "billboard", "genres", "status"].includes(String(root))) return true
      return root === "item" && typeof id === "string" && relevant.has(id)
    },
  })
}
