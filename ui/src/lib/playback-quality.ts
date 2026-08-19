import { useSyncExternalStore } from "react"
import type { StreamingQualityId } from "./api"

/**
 * The per-session streaming-quality override, carried over from the
 * hand-written UI's `state.quality`. The persisted default lives in Client
 * Settings (the native dialog); this only redirects the *next* playback, so it
 * is deliberately module state rather than something written back to disk.
 *
 * `null` means "use the saved Settings default".
 */
let override: StreamingQualityId | null = null
const listeners = new Set<() => void>()

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

export function setQualityOverride(quality: StreamingQualityId | null) {
  override = quality
  for (const listener of listeners) listener()
}

function qualityOverride() {
  return override
}

export function useQualityOverride() {
  return useSyncExternalStore(subscribe, qualityOverride, qualityOverride)
}
