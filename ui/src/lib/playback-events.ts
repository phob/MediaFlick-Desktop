import { useEffect } from "react"
import { toast } from "sonner"
import { api, type PlayerState } from "./api"
import { invalidateMediaSurfaces, queryClient, queryKeys } from "./query-client"

declare global {
  interface Window {
    /** Called by `dispatch_playback_event` in `src/shell/cef/events.rs`. */
    __mediaFlickDesktopPlaybackStateChanged?: (payload: PlayerState) => void
    /** Called by `dispatch_playback_event` in `src/shell/cef/events.rs`. */
    __mediaFlickDesktopPlaybackStopped?: (payload: PlayerState) => void
    /** Called after the shell's post-playback Jellyfin/cache refresh settles. */
    __mediaFlickDesktopPlaybackCacheRefreshed?: (payload: {
      itemId: string
      status: "refreshed" | "deferred"
    }) => void
  }
}

/**
 * Applies native player snapshots as they arrive. Stop events also carry the
 * completion reason used to decide whether the next episode should start.
 */
export function usePlaybackEventsBridge() {
  useEffect(() => {
    window.__mediaFlickDesktopPlaybackStateChanged = (payload) => {
      queryClient.setQueryData(queryKeys.playerState, (previous?: PlayerState) => ({
        ...previous,
        ...payload,
      }))
    }

    window.__mediaFlickDesktopPlaybackStopped = (payload) => {
      queryClient.setQueryData(queryKeys.playerState, { active: false })

      // An item-bearing stop is invalidated by the completion callback below,
      // after Jellyfin's final playstate has actually reached SQLite. Stops
      // without an item cannot start that refresh, so clear their surfaces now.
      if (!payload?.itemId) invalidateMediaSurfaces()

      const finished = payload?.stopReason === "eof" || payload?.stopReason === "watched-next"
      if (!finished || !payload?.itemId) return

      void api
        .playNext(payload.itemId)
        .then((result) => {
          if (!result.started) return
          toast.success("Playing the next episode")
          void queryClient.invalidateQueries({ queryKey: queryKeys.playerState })
        })
        .catch((error: Error) => toast.error(error.message))
    }

    window.__mediaFlickDesktopPlaybackCacheRefreshed = (payload) => {
      // Deferred means the focused refresh failed and the shell queued a full
      // sync. Invalidating here still avoids treating the pre-stop cache as
      // fresh; the ordinary query lifecycle can pick up that sync afterward.
      invalidateMediaSurfaces(payload?.itemId)
    }

    return () => {
      delete window.__mediaFlickDesktopPlaybackStateChanged
      delete window.__mediaFlickDesktopPlaybackStopped
      delete window.__mediaFlickDesktopPlaybackCacheRefreshed
    }
  }, [])
}
