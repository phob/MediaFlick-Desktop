import { useQuery } from "@tanstack/react-query"
import { api } from "./api"
import { queryKeys } from "./query-client"

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
