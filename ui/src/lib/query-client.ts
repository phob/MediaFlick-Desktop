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
  items: (query: ItemQuery) => ["items", query] as const,
  item: (id: string) => ["item", id] as const,
  children: (id: string) => ["item", id, "children"] as const,
  media: (id: string) => ["item", id, "media"] as const,
  trailer: (id: string) => ["item", id, "trailer"] as const,
  nextUp: (id: string) => ["item", id, "nextup"] as const,
  playerState: ["player", "state"] as const,
  seerrStatus: ["seerr", "status"] as const,
  seerrSearch: (term: string) => ["seerr", "search", term] as const,
  seerrSearchInfinite: (term: string) => ["seerr", "search", term, "infinite"] as const,
  seerrDiscover: (row: string, filters: object = {}) =>
    ["seerr", "discover", row, filters] as const,
  seerrGenres: (mediaType: string) => ["seerr", "genres", mediaType] as const,
  seerrMedia: (mediaType: string, tmdbId: number) => ["seerr", "media", mediaType, tmdbId] as const,
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
  for (const itemId of itemIds) {
    if (!itemId) continue
    void queryClient.invalidateQueries({ queryKey: queryKeys.item(itemId), ...active })
  }
}
