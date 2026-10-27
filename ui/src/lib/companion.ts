import { useCompanion } from "./queries"

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
