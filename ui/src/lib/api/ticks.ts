// Jellyfin positions and runtimes are in 100-nanosecond ticks.

import type { ItemSummary } from "./types.ts"

export const TICKS_PER_MS = 10_000

export function ticksToMs(ticks: number | null | undefined) {
  return (ticks ?? 0) / TICKS_PER_MS
}

export function progressFraction(item: Pick<ItemSummary, "positionTicks" | "runtimeTicks">) {
  if (!item.runtimeTicks || !item.positionTicks) return 0
  return Math.min(1, item.positionTicks / item.runtimeTicks)
}
