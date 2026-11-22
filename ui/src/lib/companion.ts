import type { CompanionStatus } from "./api"
import { useCompanion } from "./integration-queries"

export const COMPANION_GUIDE_URL = "https://flick.media/companion/"

export function companionMissing(status: CompanionStatus | undefined) {
  return Boolean(status?.checked && !status.available && status.error === null && !status.serverTooOld)
}

export function useCompanionCapabilities() {
  const companion = useCompanion()
  const capabilities = companion.data?.compatible ? companion.data.info?.capabilities ?? [] : []
  return {
    isPending: companion.isPending,
    has: (capability: string) => capabilities.includes(capability),
  }
}
