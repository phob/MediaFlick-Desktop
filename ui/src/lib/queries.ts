import { useInfiniteQuery, useMutation, useQueries, useQuery } from "@tanstack/react-query"
import { toast } from "sonner"
import {
  ApiError,
  PAGE_SIZE,
  api,
  type ExternalProvider,
  type ItemQuery,
  type PlayerCommand,
  type MediaInfoResponse,
  type PlaybackTrackPreferenceWrite,
  type PersonResolveQuery,
  type QuickConnectStart,
  type SeerrDiscoverRow,
  type SeerrDiscoverFilters,
  type SeerrMediaType,
  type StreamingQualityId,
} from "./api"
import {
  invalidateMediaSurfaces,
  invalidateSeerrSurfaces,
  queryClient,
  queryKeys,
} from "./query-client"
import {
  discoveryResultSetKey,
  type DiscoveryAvailability,
} from "./discovery"

/**
 * Shared mutation failure handler. Without this a failed action is completely
 * silent — which reads as "nothing happened" and gets clicked again.
 */
function reportError(error: Error) {
  toast.error(error.message)
  // The server rejected the token: re-read status so the shell falls back to
  // the sign-in view instead of leaving dead controls on screen.
  if (error instanceof ApiError && (error.expired || error.status === 401)) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.status })
  }
}

export function useStatus() {
  return useQuery({
    queryKey: queryKeys.status,
    queryFn: api.status,
    // A new account is gated only until its first catalog page commits. Keep a
    // slower status pulse while background phases are active so the compact
    // sidebar indicator advances without invalidating media queries per item.
    refetchInterval: (query) => {
      const status = query.state.data
      if (status?.authenticated && !(status.libraryReady ?? status.bootstrapped)) return 500
      return status?.authenticated && status.syncProgress?.active ? 2_000 : false
    },
  })
}

export function useSettings() {
  return useQuery({ queryKey: queryKeys.settings, queryFn: api.settings })
}

export function useRatingsStatus() {
  return useQuery({
    queryKey: queryKeys.ratingsStatus,
    queryFn: api.ratings.status,
    staleTime: 5 * 60_000,
    retry: false,
  })
}

export function useHome(enabled = true) {
  return useQuery({ queryKey: queryKeys.home, queryFn: api.home, enabled })
}

export function useHomeResume(enabled = true) {
  return useQuery({
    queryKey: queryKeys.homeResume,
    queryFn: api.homeResume,
    enabled,
  })
}

export function useCompanion() {
  return useQuery({
    queryKey: queryKeys.companion,
    queryFn: api.companion.info,
    staleTime: 5 * 60_000,
    retry: false,
  })
}

export function useReleaseCalendar(start: string, end: string) {
  return useQuery({
    queryKey: queryKeys.calendar(start, end),
    queryFn: ({ signal }) => api.calendar(start, end, signal),
    staleTime: 5 * 60_000,
    retry: false,
  })
}

export function useBillboard(enabled = true) {
  return useQuery({
    queryKey: queryKeys.billboard,
    queryFn: api.billboard,
    enabled,
    // Keep one random selection stable while the user moves around the app.
    staleTime: 10 * 60_000,
  })
}

export function useGenres() {
  return useQuery({
    queryKey: queryKeys.genres,
    queryFn: api.genres,
    // Genres change only when the library does; no point re-asking per mount.
    staleTime: 30 * 60_000,
  })
}

export function useItems(query: ItemQuery, enabled = true) {
  return useQuery({
    queryKey: queryKeys.items(query),
    queryFn: ({ signal }) => api.items(query, signal),
    enabled,
  })
}

export function usePersonResolution(query: PersonResolveQuery, enabled = true) {
  return useQuery({
    queryKey: queryKeys.personResolution(
      query.jellyfinId ?? "",
      query.tmdbId ?? null,
      query.name ?? "",
    ),
    queryFn: ({ signal }) => api.resolvePerson(query, signal),
    enabled,
    retry: false,
  })
}

/**
 * The windowed grid's data source: one query per `PAGE_SIZE` page, so a page
 * already in the cache renders without a round trip and pages scrolled past
 * stay cached under the normal `gcTime`. Callers pass only the pages they can
 * actually see (plus page 0, which anchors `total`).
 */
export function useItemPages(query: ItemQuery, pages: number[]) {
  return useQueries({
    queries: pages.map((page) => {
      const paged = { ...query, limit: PAGE_SIZE, offset: page * PAGE_SIZE }
      return {
        queryKey: queryKeys.items(paged),
        queryFn: ({ signal }: { signal: AbortSignal }) => api.items(paged, signal),
      }
    }),
  })
}

export function useItem(id: string | undefined) {
  return useQuery({
    queryKey: queryKeys.item(id ?? ""),
    queryFn: () => api.item(id!),
    enabled: Boolean(id),
  })
}

/** Live billboard prose without the detail page's cast and facts payload. */
export function useItemSynopsis(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: queryKeys.itemSynopsis(id ?? ""),
    queryFn: () => api.itemSynopsis(id!),
    enabled: Boolean(id) && enabled,
    staleTime: 10 * 60_000,
    retry: false,
  })
}

/**
 * The live rich-metadata record — synopsis, cast, critic rating, tags,
 * studios — behind the instant cached row. Never persisted natively, so the
 * query cache is its only memory; a modest staleTime keeps detail navigation
 * from re-asking the server on every mount.
 */
export function useItemAbout(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: queryKeys.itemAbout(id ?? ""),
    queryFn: () => api.itemAbout(id!),
    enabled: Boolean(id) && enabled,
    staleTime: 5 * 60_000,
    retry: false,
  })
}

/** Public profile activity is independent of the local item response, so a
 * slow or unavailable RSS feed never delays the usable detail page. */
export function useLetterboxdReviews(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: queryKeys.letterboxdReviews(id ?? ""),
    queryFn: () => api.itemLetterboxd(id!),
    enabled: Boolean(id) && enabled,
    staleTime: 30 * 60_000,
    retry: false,
  })
}

export function useChildren(id: string | undefined) {
  return useQuery({
    queryKey: queryKeys.children(id ?? ""),
    queryFn: () => api.children(id!),
    enabled: Boolean(id),
  })
}

/**
 * Container and track detail. This is the one detail-page query that always
 * costs a request to the server — nothing about codecs is cached locally — so
 * it is only enabled for the kinds that have streams of their own.
 */
export function useMediaInfo(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: queryKeys.media(id ?? ""),
    queryFn: () => api.media(id!),
    enabled: Boolean(id) && enabled,
    staleTime: 30 * 60_000,
    retry: false,
  })
}

/** Saves source, audio, and subtitle together so indices never cross sources. */
export function useSetPlaybackPreference(itemId: string) {
  return useMutation({
    mutationFn: (preference: PlaybackTrackPreferenceWrite) =>
      api.setPlaybackPreference(itemId, preference),
    onSuccess: ({ playbackPreference }) => {
      queryClient.setQueryData(
        queryKeys.media(itemId),
        (current: MediaInfoResponse | undefined) =>
          current ? { ...current, playbackPreference } : current,
      )
    },
    onError: (error: Error) => {
      reportError(error)
      // A 409 means the file changed between reading and saving the controls.
      // Re-fetching restores safe current choices instead of keeping stale UI.
      if (error instanceof ApiError && error.status === 409) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.media(itemId) })
      }
    },
  })
}

/** Resolves only the small trailer record; video bytes stay lazy until mounted. */
export function useTrailer(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: queryKeys.trailer(id ?? ""),
    queryFn: () => api.trailer(id!),
    enabled: Boolean(id) && enabled,
    staleTime: 30 * 60_000,
    retry: false,
  })
}

/** The episode a series should resume with; server-side logic, series only. */
export function useNextUp(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: queryKeys.nextUp(id ?? ""),
    queryFn: () => api.nextUp(id!),
    enabled: Boolean(id) && enabled,
  })
}

export function useOpenExternal() {
  return useMutation({
    mutationFn: ({ id, provider }: { id: string; provider: ExternalProvider }) =>
      api.openExternal(id, provider),
    onError: reportError,
  })
}

/** Polls only while something is actually playing; idle costs one request. */
export function usePlayerState() {
  return useQuery({
    queryKey: queryKeys.playerState,
    queryFn: api.playerState,
    refetchInterval: (query) => (query.state.data?.active ? 1000 : false),
    staleTime: 0,
  })
}

/**
 * `context` is the id of whatever list the item was toggled from — a season for
 * an episode row, a series for a Next Up card. Without it the row updates but
 * the list it sits in keeps the stale watched state.
 */
interface UserDataMutation {
  id: string
  context?: string | null
}

export function useSetPlayed() {
  return useMutation({
    mutationFn: ({ id, played }: UserDataMutation & { played: boolean }) =>
      api.setPlayed(id, played),
    onSuccess: (_result, { id, context }) => invalidateMediaSurfaces(id, context),
    onError: reportError,
  })
}

export function useSetFavorite() {
  return useMutation({
    mutationFn: ({ id, favorite }: UserDataMutation & { favorite: boolean }) =>
      api.setFavorite(id, favorite),
    onSuccess: (_result, { id, context }) => invalidateMediaSurfaces(id, context),
    onError: reportError,
  })
}

export function usePlay() {
  return useMutation({
    mutationFn: ({
      id,
      resume,
      quality,
    }: {
      id: string
      resume: boolean
      quality?: StreamingQualityId
    }) => api.play(id, resume, quality),
    onSuccess: (started) => {
      // The player is a separate window that can take a moment to come up, so
      // say something: otherwise Play looks like it did nothing at all.
      toast.success(started.playMethod ? `Playing (${started.playMethod})` : "Playing")
      void queryClient.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError,
  })
}

export function usePlayerCommand() {
  return useMutation({
    mutationFn: (command: PlayerCommand) => api.playerCommand(command),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError,
  })
}

export function useLogin() {
  return useMutation({
    mutationFn: ({ server, username, password }: { server: string; username: string; password: string }) =>
      api.login(server, username, password),
    onSuccess: (status) => {
      queryClient.setQueryData(queryKeys.status, status)
      invalidateMediaSurfaces()
    },
  })
}

/**
 * Probes a server address so the sign-in view can name the server and only
 * offer Quick Connect where it is actually enabled.
 */
export function useServerInfo(server: string) {
  return useQuery({
    queryKey: queryKeys.serverInfo(server),
    queryFn: ({ signal }) => api.connect(server, signal),
    enabled: Boolean(server),
    // A typo in the address should surface immediately, not after a retry.
    retry: false,
    staleTime: 60_000,
  })
}

export function useQuickConnectStart() {
  return useMutation({ mutationFn: (server: string) => api.quickConnectStart(server) })
}

/**
 * Polls an approved-yet? Quick Connect request every two seconds. There is no
 * client-side deadline on purpose: the server expires the secret itself and
 * answers 404, which surfaces here as a plain error.
 */
export function useQuickConnectPoll(started: QuickConnectStart | undefined) {
  return useQuery({
    queryKey: queryKeys.quickConnect(started?.secret ?? ""),
    queryFn: async ({ signal }) => {
      const result = await api.quickConnectPoll(started!.serverUrl, started!.secret, signal)
      if (result.authenticated) {
        // The shell gates on `/api/status`; re-reading it is what actually
        // swaps the sign-in view for the app.
        await queryClient.invalidateQueries({ queryKey: queryKeys.status })
        invalidateMediaSurfaces()
      }
      return result
    },
    enabled: Boolean(started),
    refetchInterval: (query) => (query.state.data?.authenticated ? false : 2000),
    staleTime: 0,
    gcTime: 0,
    retry: false,
  })
}

export function useLogout() {
  return useMutation({
    mutationFn: api.logout,
    onError: reportError,
    onSuccess: (status) => {
      queryClient.setQueryData(queryKeys.status, status)
      queryClient.removeQueries({ queryKey: ["items"] })
      queryClient.removeQueries({ queryKey: queryKeys.home })
      // The Seerr link belonged to the account that just went away; the shell
      // has already dropped it, so nothing of it may stay in the cache either.
      queryClient.removeQueries({ queryKey: ["seerr"] })
    },
  })
}

// -------------------------------------------------------------------- seerr

/**
 * Seerr's own failure handler. Deliberately not `reportError`: that invalidates
 * the *Jellyfin* status on a 401, which would bounce the user to the sign-in
 * screen because a Seerr cookie lapsed. Only the Seerr status is re-read, which
 * is what puts the re-link prompt on screen.
 */
function reportSeerrError(error: Error) {
  toast.error(error.message)
  if (error instanceof ApiError && (error.seerrExpired || error.status === 401)) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.seerrStatus })
  }
}

/**
 * Whether Seerr is linked, and what this user may ask for. Every Seerr surface
 * gates on it, so it is fetched once and shared.
 */
export function useSeerrStatus() {
  return useQuery({
    queryKey: queryKeys.seerrStatus,
    queryFn: api.seerr.status,
    // The answer costs a round trip to Seerr, and it only changes when the user
    // links, unlinks, or the session lapses.
    staleTime: 5 * 60_000,
    retry: false,
  })
}

export function useSeerrConnect() {
  return useMutation({
    mutationFn: (server: string) => api.seerr.connect(server),
    onSuccess: () => {
      // Destination ids are local to one Seerr instance. Never carry a
      // Radarr/Sonarr profile list across an instance switch.
      queryClient.removeQueries({ queryKey: ["seerr", "request-options"] })
    },
  })
}

export function useSeerrLink() {
  return useMutation({
    mutationFn: () => api.seerr.link(),
    onSuccess: (result) => {
      if (result.linked) void queryClient.invalidateQueries({ queryKey: queryKeys.seerrStatus })
    },
  })
}

export function useSeerrLinkPassword() {
  return useMutation({
    mutationFn: ({ username, password }: { username: string; password: string }) =>
      api.seerr.linkPassword(username, password),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.seerrStatus })
    },
  })
}

export function useSeerrUnlink() {
  return useMutation({
    mutationFn: api.seerr.unlink,
    onError: reportSeerrError,
    onSuccess: (status) => {
      queryClient.setQueryData(queryKeys.seerrStatus, status)
      queryClient.removeQueries({ queryKey: ["seerr", "requests"] })
      queryClient.removeQueries({ queryKey: ["seerr", "discover"] })
      queryClient.removeQueries({ queryKey: ["seerr", "search"] })
      queryClient.removeQueries({ queryKey: ["seerr", "person"] })
      queryClient.removeQueries({ queryKey: ["seerr", "request-options"] })
    },
  })
}

/**
 * The "not in your library" search, which is a *separate* query from the local
 * one on purpose: local FTS answers at SQLite speed and must never wait on a
 * round trip to Seerr.
 */
export function useSeerrSearch(term: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.seerrSearch(term),
    queryFn: ({ signal }) => api.seerr.search(term, 1, signal),
    enabled: enabled && term.trim().length > 1,
    retry: false,
  })
}

export function useSeerrPersonCredits(
  tmdbId: number | null,
  jellyfinId: string | null,
  enabled = true,
) {
  return useQuery({
    queryKey: queryKeys.seerrPersonCredits(tmdbId ?? 0, jellyfinId ?? ""),
    queryFn: ({ signal }) => api.seerr.personCredits(tmdbId!, jellyfinId, signal),
    enabled: enabled && tmdbId !== null,
    retry: false,
  })
}

export function useInfiniteSeerrSearch(term: string, enabled = true) {
  return useInfiniteQuery({
    queryKey: queryKeys.seerrSearchInfinite(term),
    queryFn: ({ pageParam, signal }) => api.seerr.search(term, pageParam, signal),
    initialPageParam: 1,
    getNextPageParam: (lastPage) =>
      lastPage.page < lastPage.totalPages ? lastPage.page + 1 : undefined,
    enabled: enabled && term.trim().length > 1,
    retry: false,
  })
}

export function useSeerrDiscover(
  row: SeerrDiscoverRow,
  filters: SeerrDiscoverFilters = {},
  availability: DiscoveryAvailability = "all",
  enabled = true,
) {
  const resultSetKey = discoveryResultSetKey(row, filters, availability)
  return useInfiniteQuery({
    queryKey: queryKeys.seerrDiscover(row, resultSetKey),
    queryFn: async ({ pageParam, signal }) => ({
      ...(await api.seerr.discover(row, filters, pageParam, signal)),
      resultSetKey,
    }),
    initialPageParam: 1,
    getNextPageParam: (lastPage) =>
      lastPage.page < lastPage.totalPages ? lastPage.page + 1 : undefined,
    enabled,
    retry: false,
    // A filter transition is a replacement, not a place to resurrect an
    // inactive infinite query. Once its observer goes away, abort its fetch
    // through `signal` and discard every page together.
    staleTime: 0,
    gcTime: 0,
    refetchOnMount: "always",
  })
}

export function useSeerrGenres(mediaType: SeerrMediaType, enabled = true) {
  return useQuery({
    queryKey: queryKeys.seerrGenres(mediaType),
    queryFn: ({ signal }) => api.seerr.genres(mediaType, signal),
    enabled,
    staleTime: 30 * 60_000,
    retry: false,
  })
}

/** One title in full. Also how a request card resolves its own title. */
export function useSeerrMedia(
  mediaType: SeerrMediaType | undefined,
  tmdbId: number | null | undefined,
  enabled = true,
) {
  return useQuery({
    queryKey: queryKeys.seerrMedia(mediaType ?? "", tmdbId ?? 0),
    queryFn: () => api.seerr.media(mediaType!, tmdbId!),
    enabled: enabled && Boolean(mediaType) && Boolean(tmdbId),
    // A title's own facts barely move; its availability is what changes, and a
    // request invalidates this key anyway.
    staleTime: 10 * 60_000,
    retry: false,
  })
}

export function useSeerrRequestOptions(
  mediaType: SeerrMediaType,
  is4k: boolean,
  enabled = true,
) {
  return useQuery({
    queryKey: queryKeys.seerrRequestOptions(mediaType, is4k),
    queryFn: () => api.seerr.requestOptions(mediaType, is4k),
    enabled,
    staleTime: 10 * 60_000,
    retry: false,
  })
}

/**
 * The user's own requests. Polled while the view is mounted — Seerr has no push
 * channel, so an approval or a finished download only lands on a refetch — and
 * nothing polls once the view is gone.
 */
export function useSeerrRequests(filter: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.seerrRequests(filter),
    queryFn: ({ signal }) => api.seerr.requests(filter, 40, signal),
    enabled,
    refetchInterval: 30_000,
    refetchOnWindowFocus: true,
    retry: false,
  })
}

export function useSeerrRequest() {
  return useMutation({
    mutationFn: (body: {
      mediaType: SeerrMediaType
      tmdbId: number
      seasons?: number[]
      is4k?: boolean
      serverId?: number
      profileId?: number
    }) => api.seerr.request(body),
    onError: reportSeerrError,
    onSuccess: (created) => {
      // The dialog is kept even for an auto-approving user (chunk 11's resolved
      // default), so the outcome is reported from what Seerr actually did.
      toast.success(created.status === "approved" ? "Approved" : "Requested")
      invalidateSeerrSurfaces()
    },
  })
}

export function useSeerrCancelRequest() {
  return useMutation({
    mutationFn: (id: number) => api.seerr.cancelRequest(id),
    onError: reportSeerrError,
    onSuccess: () => {
      toast.success("Request cancelled")
      invalidateSeerrSurfaces()
    },
  })
}
