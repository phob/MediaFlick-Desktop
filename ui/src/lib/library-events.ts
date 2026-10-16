import { useQueryClient } from "@tanstack/react-query"
import { useEffect } from "react"
import {
  invalidateLibrariesChanged,
  invalidateLibraryChanged,
  invalidateLibraryMemberships,
  queryKeys,
} from "./query-client"
import { readShellEvent, shellEventIds, type ShellEvent } from "./shell-events"

interface LiveChange {
  all: boolean
  itemIds: string[]
  contextIds: string[]
}

function readLiveChange(event: ShellEvent): LiveChange | null {
  if (event.type !== "live-changed") return null
  return {
    all: event.payload.all === true,
    itemIds: shellEventIds(event.payload.itemIds),
    contextIds: shellEventIds(event.payload.contextIds),
  }
}

function createThrottle(flush: () => void) {
  let timer: ReturnType<typeof setTimeout> | undefined
  let pending = false
  const fire = () => {
    timer = undefined
    if (!pending) return
    pending = false
    flush()
    timer = setTimeout(fire, 1_000)
  }
  return {
    request() {
      pending = true
      if (!timer) fire()
    },
    cancel() {
      clearTimeout(timer)
      timer = undefined
      pending = false
    },
  }
}

export function useLibraryMetadataBridge() {
  const queryClient = useQueryClient()
  useEffect(() => {
    const catalogLibraryIds = new Set<string>()
    const catalog = createThrottle(() => {
      invalidateLibraryChanged(queryClient, [], [], "catalog", [...catalogLibraryIds])
      catalogLibraryIds.clear()
    })
    const livePending = { all: false, itemIds: new Set<string>(), contextIds: new Set<string>() }
    const live = createThrottle(() => {
      const scope = livePending.all ? "resync" : "live"
      invalidateLibraryChanged(queryClient, [...livePending.itemIds], [...livePending.contextIds], scope)
      livePending.all = false
      livePending.itemIds.clear()
      livePending.contextIds.clear()
    })
    const receive = (event: Event) => {
      const detail = readShellEvent(event)
      if (detail?.type === "jellyfin-session-expired") {
        void queryClient.invalidateQueries({ queryKey: queryKeys.status })
        return
      }
      if (detail?.type === "libraries-changed") {
        invalidateLibrariesChanged(queryClient)
        return
      }
      if (detail?.type === "library-memberships-settled") {
        invalidateLibraryMemberships(queryClient)
        return
      }
      if (detail?.type === "collections-changed") {
        void queryClient.invalidateQueries({ queryKey: ["collections"] })
        void queryClient.invalidateQueries({ queryKey: queryKeys.home, exact: true })
        void queryClient.invalidateQueries({ queryKey: queryKeys.homeSettings, exact: true })
        return
      }
      const change = detail && readLiveChange(detail)
      if (change) {
        if (change.all) livePending.all = true
        for (const id of change.itemIds) livePending.itemIds.add(id)
        for (const id of change.contextIds) livePending.contextIds.add(id)
        live.request()
        return
      }
      if (detail?.type !== "library-changed" && detail?.type !== "catalog-changed") return
      const itemIds = shellEventIds(detail.payload.itemIds)
      const contextIds = shellEventIds(detail.payload.contextIds)
      if (detail.type === "catalog-changed") {
        for (const id of shellEventIds(detail.payload.libraryIds)) catalogLibraryIds.add(id)
        invalidateLibraryChanged(queryClient, itemIds, contextIds, "items")
        catalog.request()
      } else {
        catalog.cancel()
        catalogLibraryIds.clear()
        invalidateLibraryChanged(queryClient, itemIds, contextIds)
      }
    }

    window.addEventListener("mediaflick-desktop-shell", receive)
    return () => {
      window.removeEventListener("mediaflick-desktop-shell", receive)
      catalog.cancel()
      live.cancel()
    }
  }, [queryClient])
}
