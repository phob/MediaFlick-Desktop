import { useEffect } from "react"
import type { QueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import type { ClientSettings } from "@/lib/api"
import { sameJson } from "@/lib/json"
import { queryKeys } from "@/lib/query-client"
import { readShellEvent, type ShellEvent } from "@/lib/shell-events"

/** Structural equality for small settings drafts. */
export function same<T>(left: T, right: T) {
  return sameJson(left, right)
}

/** Stores the device settings the shell answered with and confirms the save. */
export function saveSettings(cache: QueryClient, saved: ClientSettings, message = "Settings saved") {
  cache.setQueryData(queryKeys.settings, saved)
  toast.success(message)
}

export function useShellEvents(listener: (event: ShellEvent) => void) {
  useEffect(() => {
    const receive = (event: Event) => {
      const shellEvent = readShellEvent(event)
      if (shellEvent) listener(shellEvent)
    }
    window.addEventListener("mediaflick-desktop-shell", receive)
    return () => window.removeEventListener("mediaflick-desktop-shell", receive)
  }, [listener])
}

/** A URL-safe id that pairs a native shell request with its completion event. */
export function requestId() {
  return crypto.randomUUID?.().replaceAll("-", "") ?? `${Date.now()}${Math.random().toString(16).slice(2)}`
}
