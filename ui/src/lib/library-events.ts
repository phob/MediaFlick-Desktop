import { useEffect } from "react"
import { invalidateLibraryChanged, queryClient, queryKeys } from "./query-client"
import { readShellEvent, shellEventIds } from "./shell-events"

/** Relays native background metadata commits into React Query's active views. */
export function useLibraryMetadataBridge() {
  useEffect(() => {
    const receive = (event: Event) => {
      const detail = readShellEvent(event)
      if (detail?.type === "jellyfin-session-expired") {
        void queryClient.invalidateQueries({ queryKey: queryKeys.status })
        return
      }
      if (detail?.type === "collections-changed") {
        void queryClient.invalidateQueries({ queryKey: ["collections"] })
        return
      }
      if (detail?.type !== "library-changed") return
      const itemIds = shellEventIds(detail.payload.itemIds)
      const contextIds = shellEventIds(detail.payload.contextIds)
      invalidateLibraryChanged(itemIds, contextIds)
    }

    window.addEventListener("mediaflick-desktop-shell", receive)
    return () => window.removeEventListener("mediaflick-desktop-shell", receive)
  }, [])
}
