import { useEffect } from "react"
import { invalidateLibraryChanged, queryClient, queryKeys } from "./query-client"
import { readShellEvent, shellEventIds } from "./shell-events"

/** Relays native background metadata commits into React Query's active views. */
export function useLibraryMetadataBridge() {
  useEffect(() => {
    let catalogTimer: ReturnType<typeof setTimeout> | undefined
    let catalogPending = false
    const flushCatalog = () => {
      catalogTimer = undefined
      if (catalogPending) {
        catalogPending = false
        invalidateLibraryChanged([], [], "catalog")
        catalogTimer = setTimeout(flushCatalog, 1_000)
      }
    }
    const receive = (event: Event) => {
      const detail = readShellEvent(event)
      if (detail?.type === "jellyfin-session-expired") {
        void queryClient.invalidateQueries({ queryKey: queryKeys.status })
        return
      }
      if (detail?.type === "collections-changed") {
        void queryClient.invalidateQueries({ queryKey: ["collections"] })
        void queryClient.invalidateQueries({ queryKey: queryKeys.home, exact: true })
        void queryClient.invalidateQueries({ queryKey: queryKeys.homeSettings, exact: true })
        return
      }
      if (detail?.type !== "library-changed" && detail?.type !== "catalog-changed") return
      const itemIds = shellEventIds(detail.payload.itemIds)
      const contextIds = shellEventIds(detail.payload.contextIds)
      if (detail.type === "catalog-changed") {
        // Show the first page immediately, then bound aggregate work during a burst.
        invalidateLibraryChanged(itemIds, contextIds, catalogTimer ? "items" : "catalog")
        if (catalogTimer) catalogPending = true
        else catalogTimer = setTimeout(flushCatalog, 1_000)
      } else {
        clearTimeout(catalogTimer)
        catalogTimer = undefined
        catalogPending = false
        invalidateLibraryChanged(itemIds, contextIds)
      }
    }

    window.addEventListener("mediaflick-desktop-shell", receive)
    return () => {
      window.removeEventListener("mediaflick-desktop-shell", receive)
      clearTimeout(catalogTimer)
    }
  }, [])
}
