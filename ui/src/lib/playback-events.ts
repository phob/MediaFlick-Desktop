import { useQueryClient } from "@tanstack/react-query"
import { useEffect } from "react"
import { toast } from "sonner"
import { toastFailure } from "./failure-toast"
import { useViewing } from "./viewing"
import { accountKey, useStatus } from "./queries"
import { api, type PlayerState } from "./api"
import type * as Wire from "./api/wire.gen.ts"
import { invalidateMediaSurfaces, queryKeys } from "./query-client"

declare global {
  interface Window {
    __mediaFlickDesktopPlaybackStateChanged?: (payload: PlayerState) => void
    __mediaFlickDesktopPlaybackStopped?: (payload: PlayerState) => void
    __mediaFlickDesktopPlaybackCacheRefreshed?: (payload: Wire.PlaybackCacheRefreshed) => void
  }
}

/**
 * Applies native player snapshots as they arrive. Stop events also carry the
 * completion reason used to decide whether the next episode should start.
 */
export function usePlaybackEventsBridge() {
  const queryClient = useQueryClient()
  const viewing = useViewing()
  const { data: status } = useStatus()
  const account = accountKey(status)
  useEffect(() => {
    let timer: ReturnType<typeof setInterval> | undefined
    let toastId: string | number | undefined
    let completed = 0
    let continuing = false
    let stoppedItem: string | undefined
    let disposed = false
    let promptGeneration = 0
    const cancel = () => {
      promptGeneration += 1
      clearInterval(timer)
      timer = undefined
      const dismissed = toastId
      toastId = undefined
      if (dismissed !== undefined) toast.dismiss(dismissed)
    }
    const manualPlay = () => { cancel(); completed = 0; continuing = false; stoppedItem = undefined }
    window.addEventListener("mediaflick-manual-play", manualPlay)

    window.__mediaFlickDesktopPlaybackStateChanged = (payload) => {
      if (payload.active) { cancel(); stoppedItem = undefined }

      queryClient.setQueryData<PlayerState>(queryKeys.playerState, (previous) => ({
        ...previous,
        ...payload,
      }))
    }

    window.__mediaFlickDesktopPlaybackStopped = (payload) => {
      queryClient.setQueryData<PlayerState>(queryKeys.playerState, { ...payload, active: false })

      if (!payload?.itemId) invalidateMediaSurfaces(queryClient)

      const finished = payload?.stopReason === "eof" || payload?.stopReason === "watched-next"
      if (!finished || !payload?.itemId) { manualPlay(); return }
      if (stoppedItem === payload.itemId) return
      stoppedItem = payload.itemId
      const settings = viewing.data
      if (!settings || !status?.authenticated) return
      completed += 1
      const explicitNext = payload.stopReason === "watched-next"
      if (!explicitNext && (settings.nextEpisode === "off" || (settings.episodeLimit > 0 && completed >= settings.episodeLimit))) {
        completed = 0
        return
      }
      const playNext = () => {
        cancel()
        if (disposed || continuing) return
        continuing = true
        void api.playNext(payload.itemId!).then((result) => {
          continuing = false
          if (disposed || !result.started) return
          toast.success("Playing the next episode")
          void queryClient.invalidateQueries({ queryKey: queryKeys.playerState })
        }).catch((error: Error) => { continuing = false; if (!disposed) toastFailure(error) })
      }
      if (explicitNext || settings.nextEpisode === "auto") { playNext(); return }
      let remaining = settings.countdownSeconds
      const prompt = () => {
        toastId = toast(`Next episode in ${remaining} seconds`, {
          id: toastId, duration: Infinity,
          action: {label:"Play now", onClick:playNext},
          cancel: {label:"Stop watching", onClick:manualPlay},
          onDismiss: cancel,
        })
      }
      const generation = promptGeneration
      void api.playbackNeighbors(payload.itemId).then(({next}) => {
        if (disposed || generation !== promptGeneration) return
        if (!next) { completed = 0; return }
        prompt()
        timer = setInterval(() => { remaining -= 1; if (remaining <= 0) playNext(); else prompt() }, 1000)
      }).catch((error: Error) => { if (!disposed && generation === promptGeneration) toastFailure(error) })
    }

    window.__mediaFlickDesktopPlaybackCacheRefreshed = (payload) => {
      invalidateMediaSurfaces(queryClient, payload?.itemId)
    }

    return () => {
      disposed = true
      cancel()
      window.removeEventListener("mediaflick-manual-play", manualPlay)
      delete window.__mediaFlickDesktopPlaybackStateChanged
      delete window.__mediaFlickDesktopPlaybackStopped
      delete window.__mediaFlickDesktopPlaybackCacheRefreshed
    }
  }, [account, queryClient, status?.authenticated, viewing.data])
}
