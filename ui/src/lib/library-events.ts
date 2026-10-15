import { useQueryClient } from "@tanstack/react-query"
import { useEffect } from "react"
import { invalidateLibraryChanged, invalidateLiveChanged, queryKeys } from "./query-client"
import { readShellEvent, shellEventIds, type ShellEvent } from "./shell-events"

/** The native `live-changed` event: ids the server changed that the cache does not hold. */
export interface LiveChange {
  itemIds: string[]
  contextIds: string[]
}

export function readLiveChange(event: ShellEvent): LiveChange | null {
  if (event.type !== "live-changed") return null
  return {
    itemIds: shellEventIds(event.payload.itemIds),
    contextIds: shellEventIds(event.payload.contextIds),
  }
}

export function useLibraryMetadataBridge() {
  const queryClient = useQueryClient()
  useEffect(() => {
    let catalogTimer: ReturnType<typeof setTimeout> | undefined
    let catalogPending = false
    const flushCatalog = () => {
      catalogTimer = undefined
      if (catalogPending) {
        catalogPending = false
        invalidateLibraryChanged(queryClient, [], [], "catalog")
        catalogTimer = setTimeout(flushCatalog, 1_000)
      }
    }
    let liveTimer: ReturnType<typeof setTimeout> | undefined
    const livePending = { itemIds: new Set<string>(), contextIds: new Set<string>() }
    const flushLive = () => {
      liveTimer = undefined
      if (livePending.itemIds.size === 0 && livePending.contextIds.size === 0) return
      invalidateLiveChanged(queryClient, [...livePending.itemIds], [...livePending.contextIds])
      livePending.itemIds.clear()
      livePending.contextIds.clear()
      liveTimer = setTimeout(flushLive, 1_000)
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
      const live = detail && readLiveChange(detail)
      if (live) {
        for (const id of live.itemIds) livePending.itemIds.add(id)
        for (const id of live.contextIds) livePending.contextIds.add(id)
        if (!liveTimer) flushLive()
        return
      }
      if (detail?.type !== "library-changed" && detail?.type !== "catalog-changed") return
      const itemIds = shellEventIds(detail.payload.itemIds)
      const contextIds = shellEventIds(detail.payload.contextIds)
      if (detail.type === "catalog-changed") {
        invalidateLibraryChanged(queryClient, itemIds, contextIds, catalogTimer ? "items" : "catalog")
        if (catalogTimer) catalogPending = true
        else catalogTimer = setTimeout(flushCatalog, 1_000)
      } else {
        clearTimeout(catalogTimer)
        catalogTimer = undefined
        catalogPending = false
        invalidateLibraryChanged(queryClient, itemIds, contextIds)
      }
    }

    window.addEventListener("mediaflick-desktop-shell", receive)
    return () => {
      window.removeEventListener("mediaflick-desktop-shell", receive)
      clearTimeout(catalogTimer)
      clearTimeout(liveTimer)
    }
  }, [queryClient])
}
