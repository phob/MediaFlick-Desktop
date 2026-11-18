import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import {
  api,
  type PlayerCommand,
  type PlayQueueBody,
  type PlayerState,
  type StreamingQualityId,
} from "./api"
import { reportError } from "./mutation-failure"
import { optionalIdQuery, patchPlayerState, queryKeys } from "./query-client"

export function usePlaybackNeighbors(itemId: string | undefined, enabled = true) {
  return useQuery({
    ...optionalIdQuery(
      itemId,
      queryKeys.playbackNeighbors(itemId ?? ""),
      (itemId) => api.playbackNeighbors(itemId),
      enabled,
    ),
    staleTime: 30_000,
    retry: false,
  })
}

export function usePlayerState() {
  return useQuery({
    queryKey: queryKeys.playerState,
    queryFn: api.playerState,
    refetchInterval: (query) => (query.state.data?.active ? 10_000 : false),
    staleTime: 0,
  })
}

export function usePlay() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({
      id,
      resume,
      quality,
    }: {
      id: string
      resume: boolean
      quality?: StreamingQualityId
    }) => api.play(id, resume, quality),
    onSuccess: (started) => {
      toast.success(started.playMethod ? `Playing (${started.playMethod})` : "Playing")
      void client.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError(client),
  })
}

export function usePlayQueue() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (body: PlayQueueBody) => api.playQueue(body),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError(client),
  })
}

export function useChangePlaybackQuality() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({
      itemId,
      positionMs,
      quality,
    }: {
      itemId: string
      positionMs: number
      quality: StreamingQualityId
    }) => api.changePlaybackQuality(itemId, Math.round(positionMs * 10_000), quality),
    onSuccess: (started) => {
      toast.success(started.playMethod ? `Playback restarted (${started.playMethod})` : "Playback restarted")
      void client.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError(client),
  })
}

export function usePlayNeighbor() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ itemId, direction }: { itemId: string; direction: "previous" | "next" }) =>
      direction === "previous" ? api.playPrevious(itemId) : api.playNext(itemId),
    onSuccess: (started, { direction }) => {
      if (!started.started) {
        toast.info(direction === "previous" ? "No previous episode" : "No next episode")
        return
      }
      toast.success(direction === "previous" ? "Playing the previous episode" : "Playing the next episode")
      void client.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError(client),
  })
}

export function usePlayerCommand() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (command: PlayerCommand) => api.playerCommand(command),
    onSettled: () => {
      void client.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError(client),
  })
}

export function usePatchPlayerState() {
  const client = useQueryClient()
  return (patch: Partial<PlayerState>) => patchPlayerState(client, patch)
}
