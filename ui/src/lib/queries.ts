import {
  queryOptions,
  useInfiniteQuery,
  useMutation,
  useQueries,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query"
import { toast } from "sonner"
import {
  ApiError,
  PAGE_SIZE,
  api,
  type ExternalProvider,
  type ItemQuery,
  type ItemSummary,
  type PlayerCommand,
  type PlayerState,
  type MediaInfoResponse,
  type PlaybackTrackPreferenceWrite,
  type PersonResolveQuery,
  type QuickConnectStart,
  type SeerrDiscoverRow,
  type SeerrDiscoverFilters,
  type SeerrMediaType,
  type Status,
  type StreamingQualityId,
} from "./api"
import {
  invalidateMediaSurfaces,
  invalidateSeerrSurfaces,
  patchPlayerState,
  queryKeys,
  removeAccountQueryData,
} from "./query-client"
import {
  discoveryResultSetKey,
  type DiscoveryAvailability,
} from "./discovery"
import type { BrowseSource, ListingSource } from "./libraries"

/**
 * Shared mutation failure handler. Without this a failed action is completely
 * silent — which reads as "nothing happened" and gets clicked again.
 */
function reportError(client: QueryClient) {
  return (error: Error) => {
    toast.error(error.message)
    // The server rejected the token: re-read status so the shell falls back to
    // the sign-in view instead of leaving dead controls on screen.
    if (error instanceof ApiError && (error.expired || error.status === 401)) {
      void client.invalidateQueries({ queryKey: queryKeys.status })
    }
  }
}

/**
 * The session view of `/api/status`. Catalog counts and sync progress change
 * with every committed page, so they are left out here: a poll during a sync
 * then keeps the same `data` reference and does not re-render the observers.
 */
export type SessionStatus = Omit<Status, "library" | "lastSync" | "syncProgress"> & {
  catalogComplete: boolean
}

export function sessionStatus(status: Status): SessionStatus {
  const { library: _library, lastSync: _lastSync, syncProgress, ...session } = status
  return { ...session, catalogComplete: syncProgress.catalog.complete }
}

function selectSyncProgress(status: Status) {
  return status.authenticated && status.syncProgress?.active ? status.syncProgress : null
}

export function useStatus() {
  return useQuery({ queryKey: queryKeys.status, queryFn: api.status, select: sessionStatus })
}

/**
 * The status pulse. Every observer with an interval runs its own timer, so
 * only the root mounts this one. A new account is gated only until its first
 * catalog page commits, and the retry screen waits for the same page. Keep a
 * slower status pulse while background phases are active so the compact
 * sidebar indicator advances without invalidating media queries per item.
 */
export function useStatusPulse() {
  return useQuery({
    queryKey: queryKeys.status,
    queryFn: api.status,
    select: sessionStatus,
    refetchInterval: (query) => {
      const status = query.state.data
      if (status?.authenticated && status.bootstrap.state !== "ready") return 500
      return status?.authenticated && status.syncProgress?.active ? 2_000 : false
    },
  })
}

export function useSyncProgress() {
  return useQuery({ queryKey: queryKeys.status, queryFn: api.status, select: selectSyncProgress })
}

/**
 * Seeds the queries the first frame reads from one `/api/startup` request, so
 * they need not wait on status one round trip at a time. Anything missing or
 * failed is left to its own query.
 */
export async function primeStartupQueries(client: QueryClient, pathname: string) {
  let startup
  try {
    startup = await api.startup(pathname === "/")
  } catch {
    return
  }
  const { status } = startup
  if (!status) return
  client.setQueryData(queryKeys.status, status)
  if (startup.settings) client.setQueryData(queryKeys.settings, startup.settings)
  const account = accountKey(status)
  if (startup.viewing) client.setQueryData(viewingQueryOptions(account).queryKey, startup.viewing)
  if (startup.browsing) client.setQueryData(browsingQueryOptions(account).queryKey, startup.browsing)
  if (startup.home) client.setQueryData(queryKeys.home, startup.home)
  if (startup.billboard) client.setQueryData(queryKeys.billboard, startup.billboard)
}

export function viewingQueryOptions(account: string) {
  return queryOptions({ queryKey: queryKeys.viewing(account), queryFn: api.viewing })
}

export function browsingQueryOptions(account: string) {
  return queryOptions({ queryKey: queryKeys.browsing(account), queryFn: api.browsing })
}

export function useBrowsing() {
  const { data: status } = useStatus()
  return useQuery({
    ...browsingQueryOptions(accountKey(status)),
    enabled: Boolean(status?.authenticated),
  })
}

export function letterboxdProfilesQueryOptions() {
  return queryOptions({
    queryKey: queryKeys.letterboxdProfiles,
    queryFn: api.letterboxd.profiles,
    retry: false,
  })
}

export function useSettings() {
  return useQuery({ queryKey: queryKeys.settings, queryFn: api.settings })
}

export function useRatingsStatus(enabled = true) {
  return useQuery({
    queryKey: queryKeys.ratingsStatus,
    queryFn: api.ratings.status,
    enabled,
    staleTime: 5 * 60_000,
    retry: false,
  })
}

function resetAccountQueries(client: QueryClient) {
  removeAccountQueryData(client)
  void client.resetQueries({ queryKey: queryKeys.settings })
}

export function useHome(enabled = true) {
  return useQuery({ queryKey: queryKeys.home, queryFn: api.home, enabled })
}

export function useHomeSettings(enabled = true) {
  return useQuery({ queryKey: queryKeys.homeSettings, queryFn: api.homeSettings, enabled })
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
    refetchInterval: 5 * 60_000,
    retry: false,
  })
}

export function useReleaseCalendar(start: string, end: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.calendar(start, end),
    queryFn: ({ signal }) => api.calendar(start, end, signal),
    enabled,
    staleTime: 5 * 60_000,
    retry: false,
  })
}

export function useBillboard(enabled = true) {
  return useQuery({
    queryKey: queryKeys.billboard,
    queryFn: api.billboard,
    enabled,
    // The endpoint returns a new random set on every request. Keep the chosen
    // slides for the session so background syncs, reconnects, and navigation
    // cannot replace the active title or restart its trailer.
    staleTime: Infinity,
  })
}

export function useGenres(enabled = true) {
  return useQuery({
    queryKey: queryKeys.genres,
    queryFn: api.genres,
    staleTime: 30 * 60_000,
    enabled,
  })
}

export function useOfficialRatings(kind: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.officialRatings(kind),
    queryFn: () => api.officialRatings(kind),
    staleTime: 30 * 60_000,
    enabled,
  })
}

export function useBrowseLetters(source: ListingSource, enabled = true) {
  return useQuery({
    queryKey: queryKeys.browseLetters(source),
    queryFn: ({ signal }) =>
      source.scope === "library"
        ? api.libraryLetters(source.libraryId, source.query, signal)
        : api.itemLetters(source.query, signal),
    enabled,
  })
}

/**
 * The sidebar's library list and every grid's capabilities. While a movies or
 * TV shows library's membership walk runs, its grid may lack titles; the list
 * is re-read until the walk lands, because the walk ending without a change
 * sends no event.
 */
export function useLibraries() {
  const { data: status } = useStatus()
  return useQuery({
    queryKey: queryKeys.libraries,
    queryFn: api.libraries,
    enabled: Boolean(status?.authenticated),
    refetchInterval: (query) =>
      query.state.data?.libraries.some((library) => library.membershipPending) ? 3_000 : false,
  })
}

export function useLibraryOptions(id: string | null) {
  return useQuery({
    queryKey: queryKeys.libraryOptions(id ?? ""),
    queryFn: () => api.libraryOptions(id!),
    enabled: id !== null,
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
export function useBrowsePages(source: BrowseSource, pages: number[]) {
  return useQueries({
    queries: pages.map((page) => {
      const offset = page * PAGE_SIZE
      return {
        queryKey: queryKeys.browsePage(source, offset, PAGE_SIZE),
        queryFn: ({ signal }: { signal: AbortSignal }) => {
          switch (source.scope) {
            case "children":
              return api.children(source.itemId, { offset, limit: PAGE_SIZE }, signal)
            case "library":
              return api.libraryItems(source.libraryId, source.query, offset, PAGE_SIZE, signal)
            case "titles":
              return api.items({ ...source.query, limit: PAGE_SIZE, offset }, signal)
          }
        },
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

export function useLetterboxdReviews(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: queryKeys.letterboxdReviews(id ?? ""),
    queryFn: () => api.itemLetterboxd(id!),
    enabled: Boolean(id) && enabled,
    staleTime: 30 * 60_000,
    retry: false,
  })
}

export function useLetterboxdMovieReviews(tmdbId: number | null, enabled = true) {
  return useQuery({
    queryKey: queryKeys.letterboxdMovieReviews(tmdbId ?? 0),
    queryFn: () => api.movieLetterboxd(tmdbId!),
    enabled: tmdbId != null && tmdbId > 0 && enabled,
    staleTime: 30 * 60_000,
    retry: false,
  })
}

export function useChildren(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: queryKeys.children(id ?? ""),
    queryFn: () => api.children(id!),
    enabled: Boolean(id) && enabled,
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
  const client = useQueryClient()
  return useMutation({
    mutationFn: (preference: PlaybackTrackPreferenceWrite) =>
      api.setPlaybackPreference(itemId, preference),
    onSuccess: ({ playbackPreference }) => {
      client.setQueryData<MediaInfoResponse>(queryKeys.media(itemId), (current) =>
        current ? { ...current, playbackPreference } : current,
      )
    },
    onError: (error: Error) => {
      reportError(client)(error)
      // A 409 means the file changed between reading and saving the controls.
      // Re-fetching restores safe current choices instead of keeping stale UI.
      if (error instanceof ApiError && error.status === 409) {
        void client.invalidateQueries({ queryKey: queryKeys.media(itemId) })
      }
    },
  })
}

export function useTrailer(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: queryKeys.trailer(id ?? ""),
    queryFn: () => api.trailer(id!),
    enabled: Boolean(id) && enabled,
    staleTime: 30 * 60_000,
    retry: false,
  })
}

export function useNextUp(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: queryKeys.nextUp(id ?? ""),
    queryFn: () => api.nextUp(id!),
    enabled: Boolean(id) && enabled,
  })
}

export function useOpenExternal() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ id, provider }: { id: string; provider: ExternalProvider }) =>
      api.openExternal(id, provider),
    onError: reportError(client),
  })
}

export function usePlaybackNeighbors(itemId: string | undefined, enabled = true) {
  return useQuery({
    queryKey: queryKeys.playbackNeighbors(itemId ?? ""),
    queryFn: () => api.playbackNeighbors(itemId!),
    enabled: Boolean(itemId) && enabled,
    staleTime: 30_000,
    retry: false,
  })
}

export function usePlayerState() {
  return useQuery({
    queryKey: queryKeys.playerState,
    queryFn: api.playerState,
    refetchInterval: (query) => (query.state.data?.active ? 10_000 : false),
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

type BillboardUserDataPatch = Partial<
  Pick<ItemSummary, "favorite" | "played" | "positionTicks">
>

function patchBillboardItem(client: QueryClient, id: string, patch: BillboardUserDataPatch) {
  client.setQueryData<{ items: ItemSummary[] }>(queryKeys.billboard, (previous) => {
    if (!previous) return previous
    const index = previous.items.findIndex((item) => item.id === id)
    if (index < 0) return previous
    const items = [...previous.items]
    items[index] = { ...items[index], ...patch }
    return { ...previous, items }
  })
}

export function useSetPlayed() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ id, played }: UserDataMutation & { played: boolean }) =>
      api.setPlayed(id, played),
    onSuccess: (_result, { id, context, played }) => {
      patchBillboardItem(client, id, { played, positionTicks: 0 })
      invalidateMediaSurfaces(client, id, context)
    },
    onError: reportError(client),
  })
}

export function useSetFavorite() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ id, favorite }: UserDataMutation & { favorite: boolean }) =>
      api.setFavorite(id, favorite),
    onSuccess: (_result, { id, context, favorite }) => {
      patchBillboardItem(client, id, { favorite })
      invalidateMediaSurfaces(client, id, context)
    },
    onError: reportError(client),
  })
}

export function usePlay() {
  const client = useQueryClient()
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
      toast.success(started.playMethod ? `Playing (${started.playMethod})` : "Playing")
      void client.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError(client),
  })
}

export function useChangePlaybackQuality() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({
      itemId,
      positionMs,
      quality,
    }: {
      itemId: string
      positionMs: number
      quality: StreamingQualityId
    }) => api.changePlaybackQuality(itemId, Math.round(positionMs * 10_000), quality),
    onSuccess: (started) => {
      toast.success(started.playMethod ? `Playback restarted (${started.playMethod})` : "Playback restarted")
      void client.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError(client),
  })
}

export function usePlayNeighbor() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ itemId, direction }: { itemId: string; direction: "previous" | "next" }) =>
      direction === "previous" ? api.playPrevious(itemId) : api.playNext(itemId),
    onSuccess: (started, { direction }) => {
      if (!started.started) {
        toast.info(direction === "previous" ? "No previous episode" : "No next episode")
        return
      }
      toast.success(direction === "previous" ? "Playing the previous episode" : "Playing the next episode")
      void client.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError(client),
  })
}

export function usePlayerCommand() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (command: PlayerCommand) => api.playerCommand(command),
    onSettled: () => {
      void client.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError(client),
  })
}

export function usePatchPlayerState() {
  const client = useQueryClient()
  return (patch: Partial<PlayerState>) => patchPlayerState(client, patch)
}

export function useLogin() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ server, username, password }: { server: string; username: string; password: string }) =>
      api.login(server, username, password),
    onSuccess: (status) => {
      client.setQueryData(queryKeys.status, status)
      resetAccountQueries(client)
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
  const client = useQueryClient()
  return useQuery({
    queryKey: queryKeys.quickConnect(started?.secret ?? ""),
    queryFn: async ({ signal }) => {
      const result = await api.quickConnectPoll(started!.serverUrl, started!.secret, signal)
      if (result.authenticated) {
        // The shell gates on `/api/status`; re-reading it is what actually
        // swaps the sign-in view for the app. A billboard selection belongs to
        // that authenticated session and must not cross into the new account.
        client.removeQueries({ queryKey: queryKeys.billboard })
        await client.invalidateQueries({ queryKey: queryKeys.status })
        resetAccountQueries(client)
        invalidateMediaSurfaces(client)
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
  const client = useQueryClient()
  return useMutation({
    mutationFn: api.logout,
    onError: reportError(client),
    onSuccess: (status) => {
      client.setQueryData(queryKeys.status, status)
      resetAccountQueries(client)
    },
  })
}

/**
 * Whether Companion mapped this Jellyfin user to Seerr, and what they may ask
 * for. Every Seerr feature gates on this shared answer.
 */
export function useSeerrStatus(enabled = true) {
  return useQuery({
    queryKey: queryKeys.seerrStatus,
    queryFn: api.seerr.status,
    enabled,
    staleTime: 5 * 60_000,
    retry: false,
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

// Keep a browsing snapshot for Back navigation. Refetching a stale infinite
// query walks every loaded page again; only explicit invalidation should do
// that during this visit. Inactive snapshots expire after ten minutes, and
// account changes remove them through the existing Seerr query root.
const seerrBrowseCache = { staleTime: Infinity, gcTime: 10 * 60_000 }

export function useInfiniteSeerrSearch(term: string, enabled = true) {
  return useInfiniteQuery({
    ...seerrBrowseCache,
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
    ...seerrBrowseCache,
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

export function useSeerrMedia(
  mediaType: SeerrMediaType | undefined,
  tmdbId: number | null | undefined,
  enabled = true,
) {
  return useQuery({
    queryKey: queryKeys.seerrMedia(mediaType ?? "", tmdbId ?? 0),
    queryFn: () => api.seerr.media(mediaType!, tmdbId!),
    enabled: enabled && Boolean(mediaType) && Boolean(tmdbId),
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
  const client = useQueryClient()
  return useMutation({
    mutationFn: (body: {
      mediaType: SeerrMediaType
      tmdbId: number
      seasons?: number[]
      is4k?: boolean
      serverId?: number
      profileId?: number
    }) => api.seerr.request(body),
    onError: reportError(client),
    onSuccess: (created) => {
      toast.success(created.status === "approved" ? "Approved" : "Requested")
      invalidateSeerrSurfaces(client)
    },
  })
}

export function useSeerrCancelRequest() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (id: number) => api.seerr.cancelRequest(id),
    onError: reportError(client),
    onSuccess: () => {
      toast.success("Request cancelled")
      invalidateSeerrSurfaces(client)
    },
  })
}

export function accountKey(status: Pick<Status, "serverUrl" | "userId"> | undefined) {
  return `${status?.serverUrl ?? "anonymous"}:${status?.userId ?? "anonymous"}`
}

export function useCollectionSettings(enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    queryKey: queryKeys.collectionSettings(account),
    queryFn: () => api.collections.settings(),
    enabled: enabled && Boolean(status?.authenticated),
    retry: false,
  })
}

export function useCollectionTemplates(enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    queryKey: queryKeys.collectionTemplates(account),
    queryFn: ({ signal }) => api.collections.templates(signal),
    enabled: enabled && Boolean(status?.authenticated),
    staleTime: 30 * 60_000,
    retry: false,
  })
}

export function useCollectionProfiles(enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    queryKey: queryKeys.collectionProfiles(account),
    queryFn: ({ signal }) => api.collections.profiles(signal),
    enabled: enabled && Boolean(status?.authenticated),
    retry: false,
  })
}

export function myCollectionsQueryOptions(account: string) {
  return queryOptions({
    queryKey: queryKeys.collectionMine(account),
    queryFn: ({ signal }) => api.collections.mine(signal),
    retry: false,
  })
}

export function useMyCollections(enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    ...myCollectionsQueryOptions(account),
    enabled: enabled && Boolean(status?.authenticated),
  })
}

export function myCollectionQueryOptions(account: string, id: string) {
  return queryOptions({
    queryKey: queryKeys.collectionMineDetail(account, id),
    queryFn: ({ signal }) => api.collections.mineDetail(id, signal),
    retry: false,
  })
}

export function useMyCollection(id: string | null) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    ...myCollectionQueryOptions(account, id ?? ""),
    enabled: id !== null && Boolean(status?.authenticated),
  })
}

export function franchisesQueryOptions(account: string, localDate: string) {
  return queryOptions({
    queryKey: queryKeys.collectionFranchises(account, localDate),
    queryFn: ({ signal }) => api.collections.franchises(localDate, signal),
    staleTime: 6 * 60 * 60_000,
    retry: false,
  })
}

export function useFranchises(localDate: string) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    ...franchisesQueryOptions(account, localDate),
    enabled: Boolean(status?.authenticated),
    refetchOnMount: "always",
  })
}

export function franchiseQueryOptions(account: string, id: number, localDate: string) {
  return queryOptions({
    queryKey: queryKeys.collectionFranchise(account, id, localDate),
    queryFn: ({ signal }) => api.collections.franchise(id, localDate, signal),
    retry: false,
  })
}

export function useFranchise(id: number | null, localDate: string) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    ...franchiseQueryOptions(account, id ?? 0, localDate),
    enabled: id !== null && Boolean(status?.authenticated),
  })
}

export function useCollectionTitle(
  mediaType: SeerrMediaType | undefined,
  tmdbId: number | null,
  enabled = true,
) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    queryKey: queryKeys.collectionTitle(account, mediaType ?? "", tmdbId ?? 0),
    queryFn: ({ signal }) => api.collections.title(mediaType!, tmdbId!, signal),
    enabled: enabled && Boolean(status?.authenticated) && Boolean(mediaType) && tmdbId !== null,
    retry: false,
  })
}

export function useJellyfinCollections(enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    queryKey: queryKeys.collectionJellyfin(account),
    queryFn: ({ signal }) => api.collections.jellyfin(signal),
    enabled: enabled && Boolean(status?.authenticated),
    retry: false,
  })
}

export function useJellyfinCollection(id: string | null) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    queryKey: queryKeys.collectionJellyfinDetail(account, id ?? ""),
    queryFn: ({ signal }) => api.collections.jellyfinDetail(id!, signal),
    enabled: id !== null && Boolean(status?.authenticated),
    retry: false,
  })
}

export function useMovieCollection(tmdbId: number | null, enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    queryKey: queryKeys.movieCollection(account, tmdbId ?? 0),
    queryFn: () => api.collections.forMovie(tmdbId!),
    enabled: enabled && tmdbId !== null && Boolean(status?.authenticated),
    staleTime: 30 * 60_000,
    retry: false,
  })
}
