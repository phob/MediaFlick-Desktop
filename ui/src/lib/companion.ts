import type { CompanionStatus } from "./api"
import { useCompanion } from "./integration-queries"

/**
 * The Companion install guide on flick.media: add the plugin repository to
 * Jellyfin, restart it, and install MediaFlick Companion.
 */
export const COMPANION_GUIDE_URL = "https://flick.media/companion/"

/**
 * The server answered that it has no Companion and could run one. A plugin
 * that could not be reached may still be installed, so it does not count as
 * missing; a server older than the Companion needs cannot install it.
 */
export function companionMissing(status: CompanionStatus | undefined) {
  return Boolean(status?.checked && !status.available && status.error === null && !status.serverTooOld)
}

/**
 * The Companion capabilities this app may use: none from a plugin it is not
 * compatible with. Every capability check reads this one probe-backed query.
 */
export function useCompanionCapabilities() {
  const companion = useCompanion()
  const capabilities = companion.data?.compatible ? companion.data.info?.capabilities ?? [] : []
  return {
    isPending: companion.isPending,
    has: (capability: string) => capabilities.includes(capability),
  }
}
