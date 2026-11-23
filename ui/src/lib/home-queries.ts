import { useQueries, useQuery } from "@tanstack/react-query"
import { api } from "./api"
import { queryKeys } from "./query-client"

export function useHome(enabled = true) {
  return useQuery({ queryKey: queryKeys.home, queryFn: api.home, enabled })
}

export function useHomeSettings(enabled = true) {
  return useQuery({ queryKey: queryKeys.homeSettings, queryFn: () => api.homeSettings(), enabled })
}

export function useHomeResume(enabled = true) {
  return useQuery({
    queryKey: queryKeys.homeResume,
    queryFn: api.homeResume,
    enabled,
  })
}

export function useBillboard(enabled = true) {
  return useQuery({
    queryKey: queryKeys.billboard,
    queryFn: api.billboard,
    enabled,
    staleTime: Infinity,
  })
}

export function useLibraryShelves(libraryIds: readonly string[]) {
  return useQueries({
    queries: libraryIds.map((id) => ({
      queryKey: queryKeys.libraryLatest(id),
      queryFn: () => api.libraryLatest(id),
    })),
    combine: (results) => new Map(libraryIds.map((id, index) => [id, results[index]])),
  })
}
