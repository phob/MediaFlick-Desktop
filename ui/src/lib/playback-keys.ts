import { useQueryClient } from "@tanstack/react-query"
import { useEffect, useEffectEvent } from "react"
import type { PlayerCommand, PlayerState } from "./api"
import { usePatchPlayerState, usePlayerCommand, usePlayNeighbor } from "./queries"
import { queryKeys } from "./query-client"

const AUDIO_SEEK_BACK_MS = 10_000
const AUDIO_SEEK_FORWARD_MS = 30_000

/**
 * The app's one owner of playback keys, mounted by the shell for every page.
 * Media keys act on whatever plays, wherever focus is, unless a page claimed
 * the key first (`preventDefault`): a reader may take Next and Previous for
 * chapters. While audio plays, Space and the arrows act only when nothing
 * that wants them has focus. The video bar keeps its own keys.
 */
export function usePlaybackKeys() {
  const queryClient = useQueryClient()
  const command = usePlayerCommand()
  const patchPlayerState = usePatchPlayerState()
  const neighbor = usePlayNeighbor()

  const handleKeyDown = useEffectEvent((event: KeyboardEvent) => {
    if (event.defaultPrevented || event.repeat || event.isComposing) return
    const player = queryClient.getQueryData<PlayerState>(queryKeys.playerState)
    if (!player?.active) return
    const audio = player.nowPlaying?.kind === "audio"
    const send = (next: PlayerCommand) => {
      event.preventDefault()
      command.mutate(next)
    }
    switch (event.key) {
      case "MediaPlayPause":
        send({ command: "toggle-pause" })
        return
      case "MediaStop":
        send({ command: "stop" })
        return
      case "MediaTrackNext":
      case "MediaTrackPrevious": {
        const forward = event.key === "MediaTrackNext"
        if (audio) {
          send({ command: forward ? "next-track" : "previous-track" })
        } else if (player.itemId) {
          event.preventDefault()
          neighbor.mutate({ itemId: player.itemId, direction: forward ? "next" : "previous" })
        }
        return
      }
    }
    if (!audio || !keysAreFree(event.target)) return
    const seekBy = (delta: number) => {
      const positionMs = Math.max(0, Math.min(player.durationMs ?? Infinity, player.positionMs + delta))
      patchPlayerState({ positionMs })
      send({ command: "seek", positionMs })
    }
    switch (event.key) {
      case " ":
        send({ command: "toggle-pause" })
        return
      case "ArrowLeft":
        seekBy(-AUDIO_SEEK_BACK_MS)
        return
      case "ArrowRight":
        seekBy(AUDIO_SEEK_FORWARD_MS)
        return
    }
  })

  useEffect(() => {
    const listener = (event: KeyboardEvent) => handleKeyDown(event)
    window.addEventListener("keydown", listener)
    return () => window.removeEventListener("keydown", listener)
  }, [])
}

function keysAreFree(target: EventTarget | null) {
  if (target === document.body || target === document.documentElement || target === null) return true
  return target instanceof Element
    && target.closest(".player-bar") !== null
    && !target.closest("button, a, input, textarea, select, [role='slider'], [contenteditable='true']")
}
