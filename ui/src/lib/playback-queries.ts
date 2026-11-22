import { type QueryClient, useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect, useSyncExternalStore } from "react"
import { toast } from "sonner"
import {
  api,
  type PlayerCommand,
  type PlayQueueBody,
  type PlayerState,
  type StreamingQualityId,
} from "./api"
import { reportError } from "./mutation-failure"
import {
  PENDING_SEEK_TTL_MS,
  reconcilePendingSeek,
  setPendingSeekTarget,
  stepPendingSeek,
  type PendingSeek,
} from "./pending-seek"
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

type SendSeek = (
  command: PlayerCommand,
  callbacks: { onSuccess: () => void; onError: () => void },
) => void

class SeekController {
  pending: PendingSeek | null = null
  private inFlight = false
  private queuedTargetMs: number | null = null
  private expiryTimer: ReturnType<typeof setTimeout> | undefined
  private readonly listeners = new Set<() => void>()

  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  snapshot = () => this.pending

  private notify() {
    for (const listener of this.listeners) listener()
  }

  private resetTimer() {
    if (this.expiryTimer !== undefined) {
      clearTimeout(this.expiryTimer)
      this.expiryTimer = undefined
    }
    if (this.pending) {
      this.expiryTimer = setTimeout(() => {
        this.expiryTimer = undefined
        if (this.pending) {
          this.pending = null
          this.notify()
        }
      }, PENDING_SEEK_TTL_MS)
    }
  }

  private setPending(next: PendingSeek | null) {
    if (next === this.pending) return
    this.pending = next
    this.resetTimer()
    this.notify()
  }

  clear() {
    this.queuedTargetMs = null
    this.setPending(null)
  }

  reconcile(player: PlayerState | null | undefined, nowMs = Date.now()) {
    if (!this.pending) return
    const next = reconcilePendingSeek(this.pending, player, nowMs)
    if (next === null) {
      this.queuedTargetMs = null
      this.setPending(null)
    }
  }

  private enqueue(targetMs: number, send: SendSeek) {
    if (this.inFlight) {
      this.queuedTargetMs = targetMs
      return
    }
    this.inFlight = true
    send(
      { command: "seek", positionMs: targetMs },
      {
        onSuccess: () => {
          this.inFlight = false
          const queued = this.queuedTargetMs
          this.queuedTargetMs = null
          if (queued !== null && this.pending !== null) {
            this.enqueue(queued, send)
          }
        },
        onError: () => {
          this.inFlight = false
          this.clear()
        },
      },
    )
  }

  step(client: QueryClient, fallback: PlayerState, deltaMs: number, send: SendSeek) {
    const latest = client.getQueryData<PlayerState>(queryKeys.playerState) ?? fallback
    const result = stepPendingSeek(this.pending, latest, deltaMs, Date.now())
    this.setPending(result.pending)
    this.enqueue(result.targetMs, send)
    return result.targetMs
  }

  seekTo(client: QueryClient, fallback: PlayerState, positionMs: number, send: SendSeek) {
    const latest = client.getQueryData<PlayerState>(queryKeys.playerState) ?? fallback
    const result = setPendingSeekTarget(latest, positionMs, Date.now())
    this.setPending(result.pending)
    this.enqueue(result.targetMs, send)
    return result.targetMs
  }
}

const seekControllers = new WeakMap<QueryClient, SeekController>()

function getSeekController(client: QueryClient): SeekController {
  let controller = seekControllers.get(client)
  if (!controller) {
    controller = new SeekController()
    seekControllers.set(client, controller)
  }
  return controller
}

export function reconcilePendingSeekForClient(client: QueryClient, player: PlayerState | null | undefined) {
  seekControllers.get(client)?.reconcile(player)
}

export function clearPendingSeek(client: QueryClient) {
  seekControllers.get(client)?.clear()
}

const RESET_SEEK_COMMANDS = new Set<PlayerCommand["command"]>([
  "stop",
  "mark-watched-next",
  "next-track",
  "previous-track",
])

export function usePlayerCommand() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (command: PlayerCommand) => {
      if (RESET_SEEK_COMMANDS.has(command.command)) clearPendingSeek(client)
      return api.playerCommand(command)
    },
    onSettled: () => {
      void client.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError(client),
  })
}

export function usePlayerSeek(player?: PlayerState) {
  const client = useQueryClient()
  const command = usePlayerCommand()
  const controller = getSeekController(client)
  const pending = useSyncExternalStore(controller.subscribe, controller.snapshot, controller.snapshot)

  useEffect(() => {
    if (player) controller.reconcile(player)
  }, [controller, player])

  const effectivePending =
    pending && player ? reconcilePendingSeek(pending, player, pending.sentAtMs) : pending

  const send: SendSeek = (next, callbacks) => {
    command.mutate(next, callbacks)
  }

  return {
    pendingPositionMs: effectivePending ? effectivePending.targetMs : null,
    seekTo: (current: PlayerState, positionMs: number) =>
      controller.seekTo(client, current, positionMs, send),
    seekBy: (current: PlayerState, deltaMs: number) =>
      controller.step(client, current, deltaMs, send),
  }
}

export function usePatchPlayerState() {
  const client = useQueryClient()
  return (patch: Partial<PlayerState>) => patchPlayerState(client, patch)
}
