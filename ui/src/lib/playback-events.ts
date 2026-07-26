import { useEffect } from "react"
import { toast } from "sonner"
import { api, type PlayerState } from "./api"
import { invalidateMediaSurfaces, queryClient, queryKeys } from "./query-client"

declare global {
  interface Window {
    /** Called by `dispatch_playback_event` in `src/shell/cef/mod.rs`. */
    __mediaFlickDesktopPlaybackStopped?: (payload: PlayerState) => void
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
    let pending: ReturnType<typeof setTimeout> | undefined

    window.__mediaFlickDesktopPlaybackStopped = (payload) => {
      queryClient.setQueryData(queryKeys.playerState, { active: false })

      // The shell mirrors the final position into the cache on a background
      // thread, so an immediate refetch can still read the pre-stop row and
      // leave Continue Watching stale.
      clearTimeout(pending)
      pending = setTimeout(() => invalidateMediaSurfaces(payload?.itemId ?? undefined), 500)

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

    return () => {
      clearTimeout(pending)
      delete window.__mediaFlickDesktopPlaybackStopped
    }
  }, [])
}
