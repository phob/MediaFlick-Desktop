import { useEffect } from "react"
import { invalidateLibraryMetadata, queryClient, queryKeys } from "./query-client"

type ShellEvent = {
  type?: string
  payload?: { itemIds?: unknown }
}

/** Relays native background metadata commits into React Query's active views. */
export function useLibraryMetadataBridge() {
  useEffect(() => {
    const receive = (event: Event) => {
      const detail = (event as CustomEvent<ShellEvent>).detail
      if (detail?.type === "jellyfin-session-expired") {
        void queryClient.invalidateQueries({ queryKey: queryKeys.status })
        return
      }
      if (detail?.type !== "library-metadata-repaired") return
      const itemIds = Array.isArray(detail.payload?.itemIds)
        ? detail.payload.itemIds.filter((id): id is string => typeof id === "string")
        : []
      invalidateLibraryMetadata(...itemIds)
    }

    window.addEventListener("mediaflick-desktop-shell", receive)
    return () => window.removeEventListener("mediaflick-desktop-shell", receive)
  }, [])
}
