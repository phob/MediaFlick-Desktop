import { useQueryClient } from "@tanstack/react-query"
import { useEffect, useEffectEvent } from "react"
import type { PlayerCommand, PlayerState } from "./api"
import { usePlayerCommand, usePlayNeighbor, useSeekCommands } from "./playback-queries"
import { queryKeys } from "./query-client"

const AUDIO_SEEK_BACK_MS = 10_000
const AUDIO_SEEK_FORWARD_MS = 30_000

export function usePlaybackKeys() {
  const queryClient = useQueryClient()
  const command = usePlayerCommand()
  const { seekBy } = useSeekCommands()
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
    const triggerSeek = (delta: number) => {
      event.preventDefault()
      seekBy(player, delta)
    }
    switch (event.key) {
      case " ":
        send({ command: "toggle-pause" })
        return
      case "ArrowLeft":
        triggerSeek(-AUDIO_SEEK_BACK_MS)
        return
      case "ArrowRight":
        triggerSeek(AUDIO_SEEK_FORWARD_MS)
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
