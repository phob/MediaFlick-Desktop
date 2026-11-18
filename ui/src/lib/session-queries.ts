import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query"
import { api, type QuickConnectStart, type Status } from "./api"
import { reportError } from "./mutation-failure"
import {
  invalidateMediaSurfaces,
  optionalIdQuery,
  queryKeys,
  removeAccountQueryData,
} from "./query-client"

/**
 * The session view of `/api/status`. Catalog counts and sync progress change
 * with every committed page, so they are left out here: a poll during a sync
 * then keeps the same `data` reference and does not re-render the observers.
 */
type SessionStatus = Omit<Status, "library" | "lastSync" | "syncProgress"> & {
  catalogComplete: boolean
}

function sessionStatus(status: Status): SessionStatus {
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

export function accountKey(status: Pick<Status, "serverUrl" | "userId"> | undefined) {
  return `${status?.serverUrl ?? "anonymous"}:${status?.userId ?? "anonymous"}`
}

function resetAccountQueries(client: QueryClient) {
  removeAccountQueryData(client)
  void client.resetQueries({ queryKey: queryKeys.settings })
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
    ...optionalIdQuery(started, queryKeys.quickConnect(started?.secret ?? ""), async (started, { signal }) => {
      const result = await api.quickConnectPoll(started.serverUrl, started.secret, signal)
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
    }),
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
