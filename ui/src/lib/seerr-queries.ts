import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query"
import { toast } from "sonner"
import {
  api,
  type SeerrDiscoverRow,
  type SeerrDiscoverFilters,
  type SeerrMediaType,
} from "./api"
import { discoveryResultSetKey, type DiscoveryAvailability } from "./discovery"
import { reportError } from "./mutation-failure"
import { invalidateSeerrSurfaces, optionalIdQuery, queryKeys } from "./query-client"

export function useSeerrStatus(enabled = true) {
  return useQuery({
    queryKey: queryKeys.seerrStatus,
    queryFn: api.seerr.status,
    enabled,
    staleTime: 5 * 60_000,
    retry: false,
  })
}

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
    ...optionalIdQuery(
      tmdbId,
      queryKeys.seerrPersonCredits(tmdbId ?? 0, jellyfinId ?? ""),
      (tmdbId, { signal }) => api.seerr.personCredits(tmdbId, jellyfinId, signal),
      enabled,
    ),
    retry: false,
  })
}

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
    ...optionalIdQuery(
      mediaType && tmdbId ? { mediaType, tmdbId } : null,
      queryKeys.seerrMedia(mediaType ?? "", tmdbId ?? 0),
      (title) => api.seerr.media(title.mediaType, title.tmdbId),
      enabled,
    ),
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
