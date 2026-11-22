import { useSyncExternalStore } from "react"
import type { StreamingQualityId } from "./api"

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
