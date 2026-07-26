import { useMutation, useQueries, useQuery } from "@tanstack/react-query"
import { toast } from "sonner"
import {
  ApiError,
  PAGE_SIZE,
  api,
  type ExternalProvider,
  type ItemQuery,
  type PlayerCommand,
  type QuickConnectStart,
  type StreamingQualityId,
} from "./api"
import { invalidateMediaSurfaces, queryClient, queryKeys } from "./query-client"

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
  return useQuery({ queryKey: queryKeys.status, queryFn: api.status })
}

export function useSettings() {
  return useQuery({ queryKey: queryKeys.settings, queryFn: api.settings })
}

export function useHome(enabled = true) {
  return useQuery({ queryKey: queryKeys.home, queryFn: api.home, enabled })
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
    },
  })
}
