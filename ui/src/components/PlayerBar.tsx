import { Pause, Play, Square } from "lucide-react"
import { useState } from "react"
import { Link } from "react-router-dom"
import { Button } from "@/components/ui/button"
import { Slider } from "@/components/ui/slider"
import { imageUrl } from "@/lib/api"
import { useItem, usePlayerCommand, usePlayerState } from "@/lib/queries"
import { patchPlayerState } from "@/lib/query-client"

function formatClock(ms: number) {
  const total = Math.max(0, Math.round(ms / 1000))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const seconds = total % 60
  const pad = (value: number) => String(value).padStart(2, "0")
  return hours ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`
}

/**
 * The in-app playback controls. The selected backend owns video rendering;
 * this owns "what is playing, where am I in it, stop it" without leaving the
 * app.
 */
export function PlayerBar() {
  const { data: player } = usePlayerState()
  const command = usePlayerCommand()
  // While the thumb is held, the 1 s poll must not yank it back.
  const [scrubMs, setScrubMs] = useState<number | null>(null)
  const itemId = player?.active ? (player.itemId ?? undefined) : undefined
  const { data: item } = useItem(itemId)

  if (!player?.active) return null

  const duration = player.durationMs ?? 0
  const position = scrubMs ?? player.positionMs ?? 0
  const paused = Boolean(player.paused)
  const episode =
    item?.kind === "Episode" && item.parentIndexNumber != null && item.indexNumber != null
      ? `S${item.parentIndexNumber}E${item.indexNumber}`
      : null
  const title = item?.kind === "Episode" ? (item.seriesName ?? item.name) : (item?.name ?? "Playing")
  const subtitle = item?.kind === "Episode" ? [episode, item.name].filter(Boolean).join(" · ") : null

  const seek = (value: number) => {
    setScrubMs(null)
    // The poll is a second behind the seek; show the target immediately so the
    // thumb does not snap back to where it was released from.
    patchPlayerState({ positionMs: value })
    command.mutate({ command: "seek", positionMs: value })
  }

  return (
    <footer className="player-bar relative z-20 flex shrink-0 items-center gap-4 border-t border-white/8 bg-card/92 px-4 py-2.5 shadow-[0_-16px_40px_rgba(0,0,0,0.28)] backdrop-blur-xl">
      {item?.primaryImageTag && (
        <img src={imageUrl(item)} alt="" className="h-12 w-8 shrink-0 rounded object-cover" />
      )}

      <div className="min-w-0 basis-56">
        {itemId ? (
          <Link to={`/item/${encodeURIComponent(itemId)}`} className="block truncate font-medium">
            {title}
          </Link>
        ) : (
          <span className="block truncate font-medium">{title}</span>
        )}
        {subtitle && <div className="truncate text-xs text-muted-foreground">{subtitle}</div>}
      </div>

      <Button
        variant="secondary"
        size="icon"
        title={paused ? "Resume" : "Pause"}
        aria-label={paused ? "Resume" : "Pause"}
        onClick={() => {
          patchPlayerState({ paused: !paused })
          command.mutate({ command: paused ? "resume" : "pause" })
        }}
      >
        {paused ? <Play className="size-4" /> : <Pause className="size-4" />}
      </Button>
      <Button
        variant="secondary"
        size="icon"
        title="Stop"
        aria-label="Stop"
        onClick={() => command.mutate({ command: "stop" })}
      >
        <Square className="size-4" />
      </Button>

      <Slider
        value={[Math.min(position, duration || position)]}
        max={duration || 1}
        step={1000}
        disabled={!duration}
        aria-label="Playback position"
        onValueChange={([value]) => setScrubMs(value)}
        onValueCommit={([value]) => seek(value)}
        className="min-w-24 flex-1"
      />

      <div className="shrink-0 text-xs tabular-nums text-muted-foreground">
        {formatClock(position)} / {formatClock(duration)}
      </div>
    </footer>
  )
}
