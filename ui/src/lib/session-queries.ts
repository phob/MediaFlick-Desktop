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

export function useQuickConnectPoll(
  started: QuickConnectStart | undefined,
  onError?: (error: Error) => void,
) {
  const client = useQueryClient()
  return useQuery({
    ...optionalIdQuery(started, queryKeys.quickConnect(started?.secret ?? ""), async (started, { signal }) => {
      try {
        const result = await api.quickConnectPoll(started.serverUrl, started.secret, signal)
        if (result.authenticated) {
          client.removeQueries({ queryKey: queryKeys.billboard })
          await client.invalidateQueries({ queryKey: queryKeys.status })
          resetAccountQueries(client)
          invalidateMediaSurfaces(client)
        }
        return result
      } catch (error) {
        if (!signal.aborted && error instanceof Error) onError?.(error)
        throw error
      }
    }),
    refetchInterval: (query) =>
      query.state.data?.authenticated || query.state.status === "error" || query.state.error != null
        ? false
        : 2000,
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
