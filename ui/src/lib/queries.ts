import { useMutation, useQuery } from "@tanstack/react-query"
import { toast } from "sonner"
import { ApiError, api, type ItemQuery } from "./api"
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

/** Polls only while something is actually playing; idle costs one request. */
export function usePlayerState() {
  return useQuery({
    queryKey: queryKeys.playerState,
    queryFn: api.playerState,
    refetchInterval: (query) => (query.state.data?.active ? 1000 : false),
    staleTime: 0,
  })
}

export function useSetPlayed() {
  return useMutation({
    mutationFn: ({ id, played }: { id: string; played: boolean }) => api.setPlayed(id, played),
    onSuccess: (_result, { id }) => invalidateMediaSurfaces(id),
    onError: reportError,
  })
}

export function useSetFavorite() {
  return useMutation({
    mutationFn: ({ id, favorite }: { id: string; favorite: boolean }) => api.setFavorite(id, favorite),
    onSuccess: (_result, { id }) => invalidateMediaSurfaces(id),
    onError: reportError,
  })
}

export function usePlay() {
  return useMutation({
    mutationFn: ({ id, resume }: { id: string; resume: boolean }) => api.play(id, resume),
    onSuccess: () => {
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
