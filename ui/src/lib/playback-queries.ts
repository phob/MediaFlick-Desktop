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

interface PlayerStateSync {
  generation: number
  lastActive: boolean | null
  lastPlaybackId: number | null
  stoppedPlaybackIds: Set<number>
}

const MAX_STOPPED_PLAYBACK_IDS = 32
const playerStateSyncs = new WeakMap<QueryClient, PlayerStateSync>()

function getPlayerStateSync(client: QueryClient): PlayerStateSync {
  let sync = playerStateSyncs.get(client)
  if (!sync) {
    sync = {
      generation: 0,
      lastActive: null,
      lastPlaybackId: null,
      stoppedPlaybackIds: new Set(),
    }
    playerStateSyncs.set(client, sync)
  }
  return sync
}

function recordStoppedPlaybackId(sync: PlayerStateSync, playbackId: number | null | undefined) {
  if (playbackId == null) return
  sync.stoppedPlaybackIds.add(playbackId)
  if (sync.stoppedPlaybackIds.size > MAX_STOPPED_PLAYBACK_IDS) {
    const oldest = sync.stoppedPlaybackIds.values().next().value
    if (oldest !== undefined) sync.stoppedPlaybackIds.delete(oldest)
  }
}

export function notePlaybackStateEvent(client: QueryClient, payload: PlayerState) {
  const sync = getPlayerStateSync(client)
  const playbackId = payload.playbackId ?? null
  if (sync.lastActive !== payload.active || sync.lastPlaybackId !== playbackId) {
    sync.generation += 1
    sync.lastActive = payload.active
    sync.lastPlaybackId = playbackId
    if (!payload.active) {
      void client.cancelQueries({ queryKey: queryKeys.playerState })
    }
  }
}

export function notePlaybackStoppedEvent(
  client: QueryClient,
  payload: PlayerState,
  superseded = false,
) {
  const sync = getPlayerStateSync(client)
  recordStoppedPlaybackId(sync, payload.playbackId)
  if (superseded) return
  sync.generation += 1
  sync.lastActive = false
  sync.lastPlaybackId = payload.playbackId ?? null
  void client.cancelQueries({ queryKey: queryKeys.playerState })
}

export function usePlayerState() {
  const client = useQueryClient()
  return useQuery({
    queryKey: queryKeys.playerState,
    queryFn: async ({ signal }) => {
      const sync = getPlayerStateSync(client)
      const startedGeneration = sync.generation
      const fetched = await api.playerState(signal)
      const staleStopped =
        fetched.active &&
        fetched.playbackId != null &&
        sync.stoppedPlaybackIds.has(fetched.playbackId)
      if (sync.generation !== startedGeneration || staleStopped) {
        return client.getQueryData<PlayerState>(queryKeys.playerState) ?? { ...fetched, active: false }
      }
      return fetched
    },
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

class SeekController {
  pending: PendingSeek | null = null
  private inFlight = false
  private queuedTargetMs: number | null = null
  private generation = 0
  private expiryTimer: ReturnType<typeof setTimeout> | undefined
  private readonly listeners = new Set<() => void>()

  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  snapshot = () => this.pending

  isTargetSettled() {
    return !this.inFlight && this.queuedTargetMs === null
  }

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
        this.queuedTargetMs = null
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
    this.generation += 1
    this.queuedTargetMs = null
    this.setPending(null)
  }

  reconcile(player: PlayerState | null | undefined, nowMs = Date.now()) {
    if (!this.pending) return
    const next = reconcilePendingSeek(this.pending, player, nowMs, this.isTargetSettled())
    if (next === null) {
      this.queuedTargetMs = null
      this.setPending(null)
    }
  }

  private enqueue(client: QueryClient, targetMs: number) {
    if (this.inFlight) {
      this.queuedTargetMs = targetMs
      return
    }
    this.inFlight = true
    const generation = this.generation
    void api.playerCommand({ command: "seek", positionMs: targetMs }).then(
      () => {
        this.inFlight = false
        void client.invalidateQueries({ queryKey: queryKeys.playerState })
        const queued = this.queuedTargetMs
        this.queuedTargetMs = null
        if (queued !== null) {
          this.enqueue(client, queued)
          return
        }
        this.reconcile(client.getQueryData<PlayerState>(queryKeys.playerState))
      },
      (error: unknown) => {
        this.inFlight = false
        void client.invalidateQueries({ queryKey: queryKeys.playerState })
        reportError(client)(error instanceof Error ? error : new Error(String(error)))
        if (generation === this.generation) {
          this.clear()
          return
        }
        const queued = this.queuedTargetMs
        this.queuedTargetMs = null
        if (queued !== null) {
          this.enqueue(client, queued)
        }
      },
    )
  }

  step(client: QueryClient, fallback: PlayerState, deltaMs: number) {
    const latest = client.getQueryData<PlayerState>(queryKeys.playerState) ?? fallback
    const result = stepPendingSeek(this.pending, latest, deltaMs, Date.now(), this.isTargetSettled())
    this.setPending(result.pending)
    this.enqueue(client, result.targetMs)
    return result.targetMs
  }

  seekTo(client: QueryClient, fallback: PlayerState, positionMs: number) {
    const latest = client.getQueryData<PlayerState>(queryKeys.playerState) ?? fallback
    const result = setPendingSeekTarget(latest, positionMs, Date.now())
    this.setPending(result.pending)
    this.enqueue(client, result.targetMs)
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
    mutationFn: (command: PlayerCommand) => api.playerCommand(command),
    onMutate: (command) => {
      if (RESET_SEEK_COMMANDS.has(command.command)) clearPendingSeek(client)
    },
    onSettled: () => {
      void client.invalidateQueries({ queryKey: queryKeys.playerState })
    },
    onError: reportError(client),
  })
}

export function usePendingSeekPosition(player: PlayerState) {
  const client = useQueryClient()
  const controller = getSeekController(client)
  const pending = useSyncExternalStore(controller.subscribe, controller.snapshot, controller.snapshot)

  useEffect(() => {
    controller.reconcile(player)
  }, [controller, player])

  const effectivePending = pending
    ? reconcilePendingSeek(pending, player, pending.sentAtMs, controller.isTargetSettled())
    : null

  return effectivePending ? effectivePending.targetMs : null
}

export function useSeekCommands() {
  const client = useQueryClient()
  const controller = getSeekController(client)

  return {
    seekTo: (current: PlayerState, positionMs: number) =>
      controller.seekTo(client, current, positionMs),
    seekBy: (current: PlayerState, deltaMs: number) =>
      controller.step(client, current, deltaMs),
  }
}

export function usePatchPlayerState() {
  const client = useQueryClient()
  return (patch: Partial<PlayerState>) => patchPlayerState(client, patch)
}
