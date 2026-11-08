import { LoaderCircle, Music, Pause, Play, SkipBack, SkipForward, Square } from "lucide-react"
import { useState } from "react"
import { Link } from "react-router-dom"
import { MuteButton } from "@/components/player/MuteButton"
import { Timeline } from "@/components/player/Timeline"
import { formatClock } from "@/components/player/format"
import { Button } from "@/components/ui/button"
import { Slider } from "@/components/ui/slider"
import { imageUrl, TICKS_PER_MS, type AudioNowPlaying, type PlayerState } from "@/lib/api"
import { usePatchPlayerState, usePlayerCommand } from "@/lib/queries"

/**
 * The bar while an audio queue plays. It never hides and never covers the
 * page, so browsing goes on; it stays mounted across tracks.
 */
export function AudioPlayerBar({ player, audio }: { player: PlayerState; audio: AudioNowPlaying }) {
  const command = usePlayerCommand()
  const patchPlayerState = usePatchPlayerState()
  const [scrubMs, setScrubMs] = useState<number | null>(null)
  const [volumeDraft, setVolumeDraft] = useState<number | null>(null)
  const current = audio.current
  const loading = audio.phase !== "playing"
  const duration = player.durationMs ?? (current.runtimeTicks ?? 0) / TICKS_PER_MS
  const position = scrubMs ?? player.positionMs
  const paused = player.paused
  const volume = volumeDraft ?? player.volume ?? 100
  const muted = Boolean(player.mute)
  const artwork = current.artwork
  const artists = current.artists.join(", ")
  const upNext = audio.next ? `Up next: ${audio.next.title}` : null

  const togglePause = () => {
    patchPlayerState({ paused: !paused })
    command.mutate({ command: paused ? "resume" : "pause" })
  }
  const seek = (value: number) => {
    setScrubMs(null)
    patchPlayerState({ positionMs: value })
    command.mutate({ command: "seek", positionMs: value })
  }
  const toggleMute = () => {
    patchPlayerState({ mute: !muted })
    command.mutate({ command: "set-mute", mute: !muted })
  }
  const setVolume = (value: number) => {
    setVolumeDraft(null)
    patchPlayerState({ volume: value, mute: false })
    if (muted) command.mutate({ command: "set-mute", mute: false })
    command.mutate({ command: "set-volume", volume: value })
  }

  return (
    <footer
      aria-label="Now playing"
      className="player-bar relative z-20 flex shrink-0 items-center gap-2 border-t border-white/8 bg-card/92 px-3 py-2.5 shadow-[0_-16px_40px_rgba(0,0,0,0.28)] backdrop-blur-xl sm:gap-3 sm:px-4"
    >
      <div className="flex size-12 shrink-0 items-center justify-center overflow-hidden rounded bg-muted">
        {artwork ? (
          <img src={imageUrl({ id: artwork.itemId, primaryImageTag: artwork.tag }, "Primary", 96)} alt="" className="size-full object-cover" />
        ) : (
          <Music className="size-5 text-muted-foreground" aria-hidden />
        )}
      </div>
      <div className="min-w-0 basis-56" title={upNext ?? undefined}>
        <Link to={`/item/${encodeURIComponent(current.albumId ?? current.itemId)}`} className="block truncate font-medium">
          {current.title}
        </Link>
        <div className="truncate text-xs text-muted-foreground">
          {[artists, current.album, audio.length > 1 ? `${audio.index + 1} of ${audio.length}` : null].filter(Boolean).join(" · ")}
        </div>
      </div>

      <Button variant="ghost" size="icon" aria-label="Previous track" title="Previous track" onClick={() => command.mutate({ command: "previous-track" })}>
        <SkipBack className="size-4" />
      </Button>
      <Button variant="secondary" size="icon" aria-label={paused ? "Resume" : "Pause"} title={paused ? "Resume" : "Pause"} onClick={togglePause} disabled={loading}>
        {paused ? <Play className="size-4" /> : <Pause className="size-4" />}
      </Button>
      <Button
        variant="ghost"
        size="icon"
        aria-label="Next track"
        title={upNext ?? "Next track"}
        disabled={!audio.canNext}
        onClick={() => command.mutate({ command: "next-track" })}
      >
        <SkipForward className="size-4" />
      </Button>
      <Button variant="ghost" size="icon" aria-label="Stop" title="Stop" onClick={() => command.mutate({ command: "stop" })}>
        <Square className="size-4" />
      </Button>

      <Timeline
        position={position}
        duration={duration}
        chapters={player.chapters}
        segments={[]}
        bufferedUntil={player.diagnostics.bufferedUntilMs}
        onScrub={setScrubMs}
        onSeek={seek}
      />
      <span className="hidden shrink-0 text-xs tabular-nums text-muted-foreground sm:inline" aria-live="off">
        {loading ? <LoaderCircle className="size-4 animate-spin" aria-label="Loading" /> : `${formatClock(position)} / ${formatClock(duration)}`}
      </span>

      <div className="hidden w-32 shrink-0 items-center gap-2 lg:flex">
        <MuteButton muted={muted} volume={volume} onToggle={toggleMute} />
        <Slider
          value={[muted ? 0 : volume]}
          max={100}
          step={1}
          aria-label={`Volume ${Math.round(volume)}%`}
          onValueChange={([value]) => setVolumeDraft(value)}
          onValueCommit={([value]) => setVolume(value)}
          className="h-8 flex-1 cursor-pointer"
        />
      </div>
    </footer>
  )
}
