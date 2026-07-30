import { useEffect } from "react"
import { toast } from "sonner"
import { api, type PlayerState } from "./api"
import { invalidateMediaSurfaces, queryClient, queryKeys } from "./query-client"

declare global {
  interface Window {
    /** Called by `dispatch_playback_event` in `src/shell/cef/mod.rs`. */
    __mediaFlickDesktopPlaybackStopped?: (payload: PlayerState) => void
    /** Called after the shell's post-playback Jellyfin/cache refresh settles. */
    __mediaFlickDesktopPlaybackCacheRefreshed?: (payload: {
      itemId: string
      status: "refreshed" | "deferred"
    }) => void
  }
}

/**
 * The push half of the playback loop. Polling alone would leave the bar up for
 * up to a second after the player quits, and — more importantly — the shell
 * only tells us *why* playback stopped here, which is what decides whether the
 * next episode should start.
 */
export function usePlaybackStoppedBridge() {
  useEffect(() => {
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
      delete window.__mediaFlickDesktopPlaybackStopped
      delete window.__mediaFlickDesktopPlaybackCacheRefreshed
    }
  }, [])
}
