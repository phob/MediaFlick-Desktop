import { queryOptions, useQuery } from "@tanstack/react-query"
import { api } from "./api"
import { queryKeys } from "./query-client"
import { accountKey, useStatus } from "./session-queries"

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

export function useSettings() {
  return useQuery({ queryKey: queryKeys.settings, queryFn: api.settings })
}
