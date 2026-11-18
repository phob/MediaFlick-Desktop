import { queryOptions, useQuery } from "@tanstack/react-query"
import { api } from "./api"
import { optionalIdQuery, queryKeys } from "./query-client"

export function useCompanion() {
  return useQuery({
    queryKey: queryKeys.companion,
    queryFn: api.companion.info,
    staleTime: 5 * 60_000,
    refetchInterval: 5 * 60_000,
    retry: false,
  })
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

export function letterboxdProfilesQueryOptions() {
  return queryOptions({
    queryKey: queryKeys.letterboxdProfiles,
    queryFn: api.letterboxd.profiles,
    retry: false,
  })
}

export function useLetterboxdReviews(id: string | undefined, enabled = true) {
  return useQuery({
    ...optionalIdQuery(id, queryKeys.letterboxdReviews(id ?? ""), (id) => api.itemLetterboxd(id), enabled),
    staleTime: 30 * 60_000,
    retry: false,
  })
}

export function useLetterboxdMovieReviews(tmdbId: number | null, enabled = true) {
  return useQuery({
    ...optionalIdQuery(
      tmdbId,
      queryKeys.letterboxdMovieReviews(tmdbId ?? 0),
      (tmdbId) => api.movieLetterboxd(tmdbId),
      enabled,
    ),
    staleTime: 30 * 60_000,
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
