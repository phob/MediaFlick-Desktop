import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query"
import {
  ApiError,
  api,
  type ExternalProvider,
  type ItemSummary,
  type MediaInfoResponse,
  type PlaybackTrackPreferenceWrite,
} from "./api"
import { reportError } from "./mutation-failure"
import { invalidateMediaSurfaces, optionalIdQuery, queryKeys } from "./query-client"

export function useItem(id: string | undefined) {
  return useQuery(optionalIdQuery(id, queryKeys.item(id ?? ""), (id) => api.item(id)))
}

export function useItemSynopsis(id: string | undefined, enabled = true) {
  return useQuery({
    ...optionalIdQuery(id, queryKeys.itemSynopsis(id ?? ""), (id) => api.itemSynopsis(id), enabled),
    staleTime: 10 * 60_000,
    retry: false,
  })
}

export function useItemAbout(id: string | undefined, enabled = true) {
  return useQuery({
    ...optionalIdQuery(id, queryKeys.itemAbout(id ?? ""), (id) => api.itemAbout(id), enabled),
    staleTime: 5 * 60_000,
    retry: false,
  })
}

export function useChildren(id: string | undefined, enabled = true) {
  return useQuery(optionalIdQuery(id, queryKeys.children(id ?? ""), (id) => api.children(id), enabled))
}

export function useMediaInfo(id: string | undefined, enabled = true) {
  return useQuery({
    ...optionalIdQuery(id, queryKeys.media(id ?? ""), (id) => api.media(id), enabled),
    staleTime: 30 * 60_000,
    retry: false,
  })
}

export function useSetPlaybackPreference(itemId: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (preference: PlaybackTrackPreferenceWrite) =>
      api.setPlaybackPreference(itemId, preference),
    onSuccess: ({ playbackPreference }) => {
      client.setQueryData<MediaInfoResponse>(queryKeys.media(itemId), (current) =>
        current ? { ...current, playbackPreference } : current,
      )
    },
    onError: (error: Error) => {
      reportError(client)(error)
      if (error instanceof ApiError && error.status === 409 && !error.accountChanged) {
        void client.invalidateQueries({ queryKey: queryKeys.media(itemId) })
      }
    },
  })
}

export function useTrailer(id: string | undefined, enabled = true) {
  return useQuery({
    ...optionalIdQuery(id, queryKeys.trailer(id ?? ""), (id) => api.trailer(id), enabled),
    staleTime: 30 * 60_000,
    retry: false,
  })
}

export function useNextUp(id: string | undefined, enabled = true) {
  return useQuery(optionalIdQuery(id, queryKeys.nextUp(id ?? ""), (id) => api.nextUp(id), enabled))
}

export function useOpenExternal() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ id, provider }: { id: string; provider: ExternalProvider }) =>
      api.openExternal(id, provider),
    onError: reportError(client),
  })
}

interface UserDataMutation {
  id: string
  context?: string | null
}

type BillboardUserDataPatch = Partial<
  Pick<ItemSummary, "favorite" | "played" | "positionTicks">
>

function patchBillboardItem(client: QueryClient, id: string, patch: BillboardUserDataPatch) {
  client.setQueryData<{ items: ItemSummary[] }>(queryKeys.billboard, (previous) => {
    if (!previous) return previous
    const index = previous.items.findIndex((item) => item.id === id)
    if (index < 0) return previous
    const items = [...previous.items]
    items[index] = { ...items[index], ...patch }
    return { ...previous, items }
  })
}

export function useSetPlayed() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ id, played }: UserDataMutation & { played: boolean }) =>
      api.setPlayed(id, played),
    onSuccess: (_result, { id, context, played }) => {
      patchBillboardItem(client, id, { played, positionTicks: 0 })
      invalidateMediaSurfaces(client, id, context)
    },
    onError: reportError(client),
  })
}

export function useSetFavorite() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ id, favorite }: UserDataMutation & { favorite: boolean }) =>
      api.setFavorite(id, favorite),
    onSuccess: (_result, { id, context, favorite }) => {
      patchBillboardItem(client, id, { favorite })
      invalidateMediaSurfaces(client, id, context)
    },
    onError: reportError(client),
  })
}
