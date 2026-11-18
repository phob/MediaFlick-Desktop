import { queryOptions, useQuery } from "@tanstack/react-query"
import { api, type SeerrMediaType } from "./api"
import { optionalIdQuery, queryKeys } from "./query-client"
import { accountKey, useStatus } from "./session-queries"

export function useCollectionSettings(enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    queryKey: queryKeys.collectionSettings(account),
    queryFn: () => api.collections.settings(),
    enabled: enabled && Boolean(status?.authenticated),
    retry: false,
  })
}

export function useCollectionTemplates(enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    queryKey: queryKeys.collectionTemplates(account),
    queryFn: ({ signal }) => api.collections.templates(signal),
    enabled: enabled && Boolean(status?.authenticated),
    staleTime: 30 * 60_000,
    retry: false,
  })
}

export function useCollectionProfiles(enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    queryKey: queryKeys.collectionProfiles(account),
    queryFn: ({ signal }) => api.collections.profiles(signal),
    enabled: enabled && Boolean(status?.authenticated),
    retry: false,
  })
}

export function myCollectionsQueryOptions(account: string) {
  return queryOptions({
    queryKey: queryKeys.collectionMine(account),
    queryFn: ({ signal }) => api.collections.mine(signal),
    retry: false,
  })
}

export function useMyCollections(enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    ...myCollectionsQueryOptions(account),
    enabled: enabled && Boolean(status?.authenticated),
  })
}

export function myCollectionQueryOptions(account: string, id: string) {
  return queryOptions({
    queryKey: queryKeys.collectionMineDetail(account, id),
    queryFn: ({ signal }) => api.collections.mineDetail(id, signal),
    retry: false,
  })
}

export function useMyCollection(id: string | null) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    ...myCollectionQueryOptions(account, id ?? ""),
    enabled: id !== null && Boolean(status?.authenticated),
  })
}

export function franchisesQueryOptions(account: string, localDate: string) {
  return queryOptions({
    queryKey: queryKeys.collectionFranchises(account, localDate),
    queryFn: ({ signal }) => api.collections.franchises(localDate, signal),
    staleTime: 6 * 60 * 60_000,
    retry: false,
  })
}

export function useFranchises(localDate: string) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    ...franchisesQueryOptions(account, localDate),
    enabled: Boolean(status?.authenticated),
    refetchOnMount: "always",
  })
}

export function franchiseQueryOptions(account: string, id: number, localDate: string) {
  return queryOptions({
    queryKey: queryKeys.collectionFranchise(account, id, localDate),
    queryFn: ({ signal }) => api.collections.franchise(id, localDate, signal),
    retry: false,
  })
}

export function useFranchise(id: number | null, localDate: string) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    ...franchiseQueryOptions(account, id ?? 0, localDate),
    enabled: id !== null && Boolean(status?.authenticated),
  })
}

export function useCollectionTitle(
  mediaType: SeerrMediaType | undefined,
  tmdbId: number | null,
  enabled = true,
) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    ...optionalIdQuery(
      mediaType && tmdbId ? { mediaType, tmdbId } : null,
      queryKeys.collectionTitle(account, mediaType ?? "", tmdbId ?? 0),
      (title, { signal }) => api.collections.title(title.mediaType, title.tmdbId, signal),
      enabled && Boolean(status?.authenticated),
    ),
    retry: false,
  })
}

export function useJellyfinCollections(enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    queryKey: queryKeys.collectionJellyfin(account),
    queryFn: ({ signal }) => api.collections.jellyfin(signal),
    enabled: enabled && Boolean(status?.authenticated),
    retry: false,
  })
}

export function useJellyfinCollection(id: string | null) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    ...optionalIdQuery(
      id,
      queryKeys.collectionJellyfinDetail(account, id ?? ""),
      (id, { signal }) => api.collections.jellyfinDetail(id, signal),
      Boolean(status?.authenticated),
    ),
    retry: false,
  })
}

export function useMovieCollection(tmdbId: number | null, enabled = true) {
  const { data: status } = useStatus()
  const account = accountKey(status)
  return useQuery({
    ...optionalIdQuery(
      tmdbId,
      queryKeys.movieCollection(account, tmdbId ?? 0),
      (tmdbId) => api.collections.forMovie(tmdbId),
      enabled && Boolean(status?.authenticated),
    ),
    staleTime: 30 * 60_000,
    retry: false,
  })
}
