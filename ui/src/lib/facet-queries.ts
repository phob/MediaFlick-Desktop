import { useQuery } from "@tanstack/react-query"
import { api } from "./api"
import { queryKeys } from "./query-client"

// The genre, studio and tag names of every cached title, for the combined
// library view's filters. A library's own grid reads them from its options.

export function useGenres(enabled = true) {
  return useQuery({
    queryKey: queryKeys.genres,
    queryFn: api.genres,
    staleTime: 30 * 60_000,
    enabled,
  })
}

export function useStudios(enabled = true) {
  return useQuery({
    queryKey: queryKeys.studios,
    queryFn: api.studios,
    staleTime: 30 * 60_000,
    enabled,
  })
}

export function useTags(enabled = true) {
  return useQuery({
    queryKey: queryKeys.tags,
    queryFn: api.tags,
    staleTime: 30 * 60_000,
    enabled,
  })
}
