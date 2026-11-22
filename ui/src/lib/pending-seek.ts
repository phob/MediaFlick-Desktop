export interface SeekPlayerContext {
  readonly active: boolean
  readonly playbackId: number | null
  readonly itemId: string | null
  readonly nowPlaying:
    | { readonly kind: "video" }
    | { readonly kind: "audio"; readonly playlistItemId: string }
    | null
  readonly positionMs: number
  readonly durationMs: number | null
  readonly skipSegments: ReadonlyArray<{ readonly triggered: boolean }>
}

export interface PendingSeek {
  readonly sessionKey: string
  readonly originMs: number
  readonly targetMs: number
  readonly minSentMs: number
  readonly maxSentMs: number
  readonly sentAtMs: number
  readonly triggeredSegments: number
}

const CATCH_UP_TOLERANCE_MS = 2_500
const BURST_SPAN_MARGIN_MS = 5_000
export const PENDING_SEEK_TTL_MS = 4_000

function playbackSessionKey(player: SeekPlayerContext): string {
  const trackKey = player.nowPlaying?.kind === "audio" ? player.nowPlaying.playlistItemId : ""
  return `${player.playbackId ?? ""}:${player.itemId ?? ""}:${trackKey}`
}

function countTriggeredSegments(segments: ReadonlyArray<{ readonly triggered: boolean }>): number {
  let count = 0
  for (const segment of segments) {
    if (segment.triggered) count += 1
  }
  return count
}

export function clampSeekPosition(positionMs: number, durationMs: number | null): number {
  if (!Number.isFinite(positionMs)) return 0
  const upper = durationMs !== null && Number.isFinite(durationMs) && durationMs > 0 ? durationMs : Infinity
  return Math.max(0, Math.min(upper, positionMs))
}

export function reconcilePendingSeek(
  pending: PendingSeek | null,
  player: SeekPlayerContext | null | undefined,
  nowMs: number,
): PendingSeek | null {
  if (!pending || !player?.active) return null
  if (pending.sessionKey !== playbackSessionKey(player)) return null
  if (nowMs - pending.sentAtMs >= PENDING_SEEK_TTL_MS) return null
  if (countTriggeredSegments(player.skipSegments) > pending.triggeredSegments) return null

  const reportedMs = player.positionMs
  const spanMin = Math.min(pending.originMs, pending.minSentMs) - BURST_SPAN_MARGIN_MS
  const spanMax = Math.max(pending.originMs, pending.maxSentMs) + BURST_SPAN_MARGIN_MS
  if (reportedMs < spanMin || reportedMs > spanMax) return null

  const distance = Math.abs(pending.targetMs - pending.originMs)
  if (distance > 0) {
    const tolerance = Math.min(CATCH_UP_TOLERANCE_MS, distance * 0.4)
    if (Math.abs(reportedMs - pending.targetMs) <= tolerance) return null
  }

  return pending
}

export function stepPendingSeek(
  pending: PendingSeek | null,
  player: SeekPlayerContext,
  deltaMs: number,
  nowMs: number,
): { readonly targetMs: number; readonly pending: PendingSeek | null } {
  const current = reconcilePendingSeek(pending, player, nowMs)
  const baseMs = current ? current.targetMs : player.positionMs
  const targetMs = clampSeekPosition(baseMs + deltaMs, player.durationMs)

  if (current) {
    return {
      targetMs,
      pending: {
        ...current,
        targetMs,
        minSentMs: Math.min(current.minSentMs, targetMs),
        maxSentMs: Math.max(current.maxSentMs, targetMs),
        sentAtMs: nowMs,
      },
    }
  }

  if (targetMs === player.positionMs) {
    return { targetMs, pending: null }
  }

  return {
    targetMs,
    pending: {
      sessionKey: playbackSessionKey(player),
      originMs: player.positionMs,
      targetMs,
      minSentMs: targetMs,
      maxSentMs: targetMs,
      sentAtMs: nowMs,
      triggeredSegments: countTriggeredSegments(player.skipSegments),
    },
  }
}

export function setPendingSeekTarget(
  player: SeekPlayerContext,
  positionMs: number,
  nowMs: number,
): { readonly targetMs: number; readonly pending: PendingSeek | null } {
  const targetMs = clampSeekPosition(positionMs, player.durationMs)
  if (targetMs === player.positionMs) {
    return { targetMs, pending: null }
  }
  return {
    targetMs,
    pending: {
      sessionKey: playbackSessionKey(player),
      originMs: player.positionMs,
      targetMs,
      minSentMs: targetMs,
      maxSentMs: targetMs,
      sentAtMs: nowMs,
      triggeredSegments: countTriggeredSegments(player.skipSegments),
    },
  }
}
