import { normalizeShortcut, shortcutFromEvent, shortcutLabel } from "@/lib/player-shortcuts"
import { DEFAULT_COMFORT, useSpoilerProtection } from "@/lib/viewing"
import {
  AudioLines,
  Captions,
  ChevronLeft,
  ChevronRight,
  FastForward,
  LoaderCircle,
  Maximize,
  MoreHorizontal,
  Pause,
  Play,
  Rewind,
  Settings,
  Square,
  Volume2,
  VolumeX,
} from "lucide-react"
import { useEffect, useEffectEvent, useRef, useState } from "react"
import { Link } from "react-router-dom"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Slider } from "@/components/ui/slider"
import { cn } from "@/lib/utils"
import {
  STREAMING_QUALITIES,
  TICKS_PER_MS,
  imageUrl,
  qualityLabel,
  type PlayerCommand,
  type PlayerSkipSegment,
  type PlayerState,
  type PlayerTrack,
  type StreamingQualityId,
} from "@/lib/api"
import {
  formatBitrate,
  formatCodec,
  formatFileSize,
  formatResolution,
  formatVideoRange,
} from "@/lib/format"
import {
  useChangePlaybackQuality,
  useItem,
  useMediaInfo,
  usePlaybackNeighbors,
  usePlayNeighbor,
  usePatchPlayerState,
  usePlayerCommand,
  usePlayerState,
  useSettings,
} from "@/lib/queries"
import { SUBTITLES_OFF, TrackMenu, TrackSubmenu, type TrackChoices } from "@/components/player/TrackMenu"
import { TuningMenu, type PlaybackTuning } from "@/components/player/TuningMenu"
import { Timeline } from "@/components/player/Timeline"
import { currentChapter, formatClock, trackLabel } from "@/components/player/format"

const TIME_DISPLAY_KEY = "mediaflick.player.time-display"

function savedTimeDisplay() {
  try {
    return window.localStorage.getItem(TIME_DISPLAY_KEY)
  } catch {
    return null
  }
}

function saveTimeDisplay(value: "elapsed" | "remaining") {
  try {
    window.localStorage.setItem(TIME_DISPLAY_KEY, value)
  } catch {
  }
}

function withShortcuts(label: string, fixed: string[], bindings: (string | null | undefined)[] = []) {
  const keys = [...fixed, ...bindings.filter((binding): binding is string => Boolean(binding)).map(shortcutLabel)]
  const unique = keys.filter((key, index) => keys.indexOf(key) === index)
  return unique.length ? `${label} (${unique.join(" or ")})` : label
}

function playMethodLabel(method: string | null | undefined) {
  switch (method) {
    case "DirectPlay":
      return "Direct play"
    case "DirectStream":
      return "Direct stream"
    case "Transcode":
      return "Transcoding"
    default:
      return method ?? "Playback method unavailable"
  }
}

function segmentLabel(segment: PlayerSkipSegment) {
  switch (segment.segmentType) {
    case "intro":
      return "Intro"
    case "outro":
      return "Credits"
    case "recap":
      return "Recap"
    case "commercial":
      return "Commercial"
  }
}

function MuteButton({ muted, volume, title, onToggle, className }: {
  muted: boolean
  volume: number
  title?: string
  onToggle: () => void
  className?: string
}) {
  return (
    <Button
      variant="ghost"
      size="icon"
      title={title}
      aria-label={muted ? "Unmute" : "Mute"}
      onClick={onToggle}
      className={cn("size-8", className)}
    >
      {muted || volume === 0 ? <VolumeX className="size-4" /> : <Volume2 className="size-4" />}
    </Button>
  )
}

const DEFAULT_TUNING: PlaybackTuning = {
  audioDelay: "0",
  subtitleDelay: "0",
  subtitleScale: "1",
  videoFit: "fit",
  videoAspect: "source",
  toneMapping: "auto",
  deinterlace: false,
}

function isStreamingQuality(value: string): value is StreamingQualityId {
  return STREAMING_QUALITIES.some((quality) => quality.id === value)
}

function ActivePlayerBar({
  player,
  onMenuOpenChange,
}: {
  player: PlayerState
  onMenuOpenChange?: (open: boolean) => void
}) {
  const command = usePlayerCommand()
  const patchPlayerState = usePatchPlayerState()
  const neighborPlayback = usePlayNeighbor()
  const qualityChange = useChangePlaybackQuality()
  const settings = useSettings()
  const builtIn = settings.data?.client.player.playerBackend === "libmpv"
  const comfort = builtIn ? settings.data?.client.player.comfort ?? DEFAULT_COMFORT : DEFAULT_COMFORT
  const seekBackMs = comfort.seekBackSeconds * 1000
  const seekForwardMs = comfort.seekForwardSeconds * 1000
  const [scrubMs, setScrubMs] = useState<number | null>(null)
  const [feedback, setFeedback] = useState<string | null>(null)
  const [showRemaining, setShowRemaining] = useState(
    () => savedTimeDisplay() === "remaining",
  )
  const [volumeDraft, setVolumeDraft] = useState<number | null>(null)
  const [qualityOverride, setQualityOverride] = useState<StreamingQualityId | null>(null)
  const [tuning, setTuning] = useState(() => ({...DEFAULT_TUNING, subtitleScale:String(comfort.subtitleSize / 100)}))
  const feedbackTimer = useRef<number | undefined>(undefined)
  const itemId = player.itemId ?? undefined
  const { data: item } = useItem(itemId)
  const media = useMediaInfo(itemId)
  const neighbors = usePlaybackNeighbors(itemId, item?.kind === "Episode")
  const hideNextTitle = useSpoilerProtection(neighbors.data?.next ?? undefined)

  useEffect(
    () => () => {
      if (feedbackTimer.current !== undefined) window.clearTimeout(feedbackTimer.current)
    },
    [],
  )

  const duration = player.durationMs ?? 0
  const position = scrubMs ?? player.positionMs ?? 0
  const paused = Boolean(player.paused)
  const volume = volumeDraft ?? player.volume ?? 100
  const muted = Boolean(player.mute)
  const chapters = player.chapters ?? []
  const segments = player.skipSegments ?? []
  const chapter = currentChapter(chapters, position)
  const activeSegment = segments.find(
    (segment) =>
      position >= segment.startTicks / TICKS_PER_MS && position < segment.endTicks / TICKS_PER_MS,
  )
  const audioTracks = player.tracks?.filter((track) => track.kind === "audio") ?? []
  const subtitleTracks = player.tracks?.filter((track) => track.kind === "subtitle") ?? []
  const selectedAudio = audioTracks.find((track) => track.selected)
  const selectedSubtitle = subtitleTracks.find((track) => track.selected)
  const episode =
    item?.kind === "Episode" && item.parentIndexNumber != null && item.indexNumber != null
      ? `S${item.parentIndexNumber}E${item.indexNumber}`
      : null
  const title = item?.kind === "Episode" ? (item.seriesName ?? item.name) : (item?.name ?? "Playing")
  const subtitle = item?.kind === "Episode" ? [episode, item.name].filter(Boolean).join(" · ") : null
  const source =
    media.data?.sources?.find((candidate) => candidate.id === player.mediaSourceId) ??
    media.data?.sources?.[0]
  const video = source?.video[0]
  const selectedQuality =
    qualityOverride ?? settings.data?.client?.playback?.streamingQuality ?? "auto"

  const showAction = (message: string) => {
    setFeedback(message)
    if (feedbackTimer.current !== undefined) window.clearTimeout(feedbackTimer.current)
    feedbackTimer.current = window.setTimeout(() => setFeedback(null), 1400)
  }

  const send = (nextCommand: PlayerCommand, message?: string) => {
    command.mutate(nextCommand)
    if (message) showAction(message)
  }

  const seek = (value: number) => {
    const target = Math.max(0, Math.min(duration || value, value))
    setScrubMs(null)
    patchPlayerState({ positionMs: target })
    send({ command: "seek", positionMs: target })
  }

  const relativeSeek = (deltaMs: number) => {
    const target = Math.max(0, Math.min(duration || position + deltaMs, position + deltaMs))
    seek(target)
    showAction(`${deltaMs < 0 ? "Back" : "Forward"} ${Math.abs(deltaMs) / 1000} seconds`)
  }

  const togglePause = () => {
    patchPlayerState({ paused: !paused })
    send({ command: paused ? "resume" : "pause" }, paused ? "Playing" : "Paused")
  }

  const toggleMute = () => {
    patchPlayerState({ mute: !muted })
    send({ command: "set-mute", mute: !muted }, muted ? `Volume ${Math.round(volume)}%` : "Muted")
  }

  const setVolume = (value: number) => {
    setVolumeDraft(null)
    patchPlayerState({ volume: value, mute: false })
    if (muted) command.mutate({ command: "set-mute", mute: false })
    send({ command: "set-volume", volume: value }, `Volume ${Math.round(value)}%`)
  }

  const selectTrack = (kind: PlayerTrack["kind"], id: number | null) => {
    const tracks = player.tracks?.map((track) =>
      track.kind === kind ? { ...track, selected: track.id === id } : track,
    )
    patchPlayerState({ tracks })
    if (kind === "audio" && id != null) {
      send({ command: "set-audio-track", audioTrack: id }, "Audio track changed")
      return
    }
    send(
      { command: "set-subtitle-track", subtitleTrack: id },
      id === null ? "Subtitles off" : "Subtitles changed",
    )
  }

  const tune = (patch: Partial<PlaybackTuning>, nextCommand: PlayerCommand, message: string) => {
    setTuning((current) => ({ ...current, ...patch }))
    send(nextCommand, message)
  }

  const toggleTimeDisplay = () => {
    const next = !showRemaining
    setShowRemaining(next)
    saveTimeDisplay(next ? "remaining" : "elapsed")
  }

  const handleKeyDown = useEffectEvent((event: KeyboardEvent) => {
    const target = event.target instanceof HTMLElement ? event.target : null
    if (
      target?.isContentEditable ||
      target?.matches("input, textarea, select, [role='slider'], [role='menuitemradio']")
    ) {
      return
    }
    if (event.repeat || event.isComposing || target?.closest("[role=dialog], [role=menu], [role=listbox], [data-shortcut-recorder]")) return
    const binding = shortcutFromEvent(event)
    if (!binding) return
    const matches = (key: string | null | undefined) => Boolean(key) && normalizeShortcut(key ?? "") === binding
    if (binding === "SPACE" && target?.closest("button, a")) return
    if (builtIn && matches(settings.data?.client.player.markWatchedNext)) {
      event.preventDefault()
      send({ command: "mark-watched-next" }, "Marked watched")
    } else if (binding === "SPACE" || matches(comfort.pauseKey)) {
      event.preventDefault()
      togglePause()
    } else if (binding === "LEFT" || matches(comfort.seekBackKey)) {
      event.preventDefault()
      relativeSeek(-seekBackMs)
    } else if (binding === "RIGHT" || matches(comfort.seekForwardKey)) {
      event.preventDefault()
      relativeSeek(seekForwardMs)
    } else if (matches(comfort.muteKey)) {
      event.preventDefault()
      toggleMute()
    } else if (matches(comfort.fullscreenKey)) {
      event.preventDefault()
      send({ command: "toggle-fullscreen" }, "Fullscreen toggled")
    } else if (builtIn && matches(comfort.stopKey)) {
      event.preventDefault()
      send({ command: "stop" }, "Playback stopped")
    } else if (builtIn && matches(comfort.subtitlesKey)) {
      event.preventDefault()
      send({ command: "toggle-subtitles" }, "Subtitles toggled")
    } else if (builtIn && matches(comfort.seekBackThirtyKey)) {
      event.preventDefault()
      relativeSeek(-30_000)
    } else if (builtIn && matches(comfort.seekForwardThirtyKey)) {
      event.preventDefault()
      relativeSeek(30_000)
    }
  })
  useEffect(() => {
    const listener = (event: KeyboardEvent) => handleKeyDown(event)
    window.addEventListener("keydown", listener)
    return () => window.removeEventListener("keydown", listener)
  }, [])

  const audioChoices: TrackChoices = {
    tracks: audioTracks,
    value: selectedAudio ? String(selectedAudio.id) : "",
    onValueChange: (value) => selectTrack("audio", Number(value)),
  }
  const subtitleChoices: TrackChoices = {
    tracks: subtitleTracks,
    value: selectedSubtitle ? String(selectedSubtitle.id) : SUBTITLES_OFF,
    allowOff: true,
    onValueChange: (value) => selectTrack("subtitle", value === SUBTITLES_OFF ? null : Number(value)),
  }

  const timeText = showRemaining
    ? `-${formatClock(Math.max(0, duration - position))}`
    : `${formatClock(position)} / ${formatClock(duration)}`

  return (
    <footer className="player-bar relative z-20 flex shrink-0 items-center gap-2 border-t border-white/8 bg-card/92 px-3 py-2.5 shadow-[0_-16px_40px_rgba(0,0,0,0.28)] backdrop-blur-xl sm:gap-3 sm:px-4">
      {player.diagnostics?.buffering && (
        <div
          role="status"
          className="pointer-events-none absolute bottom-full left-1/2 mb-8 flex -translate-x-1/2 items-center gap-2 rounded-md border border-edge bg-scrim/82 px-4 py-2 text-sm font-medium text-white shadow-xl"
        >
          <LoaderCircle className="size-4 animate-spin" />
          Buffering
        </div>
      )}
      {feedback && !player.diagnostics?.buffering && (
        <div
          role="status"
          className="pointer-events-none absolute bottom-full left-1/2 mb-8 -translate-x-1/2 rounded-md border border-edge bg-scrim/82 px-4 py-2 text-sm font-medium text-white shadow-xl"
        >
          {feedback}
        </div>
      )}

      {item?.primaryImageTag && (
        <img src={imageUrl(item)} alt="" className="hidden h-12 w-8 shrink-0 rounded object-cover 2xl:block" />
      )}

      <div className="hidden min-w-0 basis-52 xl:block">
        {itemId ? (
          <Link to={`/item/${encodeURIComponent(itemId)}`} className="block truncate font-medium">
            {title}
          </Link>
        ) : (
          <span className="block truncate font-medium">{title}</span>
        )}
        <div className="truncate text-xs text-muted-foreground">
          {[subtitle, chapter?.title].filter(Boolean).join(" · ")}
        </div>
      </div>

      <Button
        variant="ghost"
        size="icon"
        title={neighbors.data?.previous ? `Previous: ${neighbors.data.previous.name}` : "No previous episode"}
        aria-label="Previous episode"
        disabled={!neighbors.data?.previous || neighborPlayback.isPending}
        onClick={() => itemId && neighborPlayback.mutate({ itemId, direction: "previous" })}
        className="hidden lg:inline-flex"
      >
        <ChevronLeft className="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        title={withShortcuts(`Back ${comfort.seekBackSeconds} seconds`, ["←"], [comfort.seekBackKey])}
        aria-label={`Back ${comfort.seekBackSeconds} seconds`}
        onClick={() => relativeSeek(-seekBackMs)}
        className="hidden sm:inline-flex"
      >
        <Rewind className="size-4" />
      </Button>
      <Button
        variant="secondary"
        size="icon"
        title={withShortcuts(paused ? "Resume" : "Pause", ["Space"], [comfort.pauseKey])}
        aria-label={paused ? "Resume" : "Pause"}
        onClick={togglePause}
      >
        {paused ? <Play className="size-4" /> : <Pause className="size-4" />}
      </Button>
      <Button
        variant="ghost"
        size="icon"
        title={withShortcuts(`Forward ${comfort.seekForwardSeconds} seconds`, ["→"], [comfort.seekForwardKey])}
        aria-label={`Forward ${comfort.seekForwardSeconds} seconds`}
        onClick={() => relativeSeek(seekForwardMs)}
        className="hidden sm:inline-flex"
      >
        <FastForward className="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        title={neighbors.data?.next ? `Next: ${hideNextTitle ? "Unwatched episode" : neighbors.data.next.name}` : "No next episode"}
        aria-label="Next episode"
        disabled={!neighbors.data?.next || neighborPlayback.isPending}
        onClick={() => itemId && neighborPlayback.mutate({ itemId, direction: "next" })}
        className="hidden lg:inline-flex"
      >
        <ChevronRight className="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        title={withShortcuts("Stop", [], builtIn ? [comfort.stopKey] : [])}
        aria-label="Stop"
        onClick={() => send({ command: "stop" })}
        className="hidden 2xl:inline-flex"
      >
        <Square className="size-4" />
      </Button>

      <Timeline
        position={position}
        duration={duration}
        chapters={chapters}
        segments={segments}
        bufferedUntil={player.diagnostics?.bufferedUntilMs ?? null}
        onScrub={setScrubMs}
        onSeek={seek}
      />

      {activeSegment && (
        <Button
          variant="secondary"
          size="sm"
          onClick={() => {
            seek(activeSegment.endTicks / TICKS_PER_MS)
            showAction(`Skipped ${segmentLabel(activeSegment)}`)
          }}
          className="hidden shrink-0 md:inline-flex"
        >
          Skip {segmentLabel(activeSegment)}
        </Button>
      )}

      <Button
        variant="ghost"
        size="sm"
        className="hidden h-auto shrink-0 px-1 text-xs font-normal tabular-nums text-muted-foreground hover:text-foreground md:inline-flex"
        title="Toggle elapsed and remaining time"
        aria-label={`${timeText}. Toggle elapsed and remaining time`}
        onClick={toggleTimeDisplay}
      >
        {timeText}
      </Button>

      <div className="hidden w-32 shrink-0 items-center gap-2 xl:flex">
        <MuteButton
          muted={muted}
          volume={volume}
          title={withShortcuts(muted ? "Unmute" : "Mute", [], [comfort.muteKey])}
          onToggle={toggleMute}
        />
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

      {audioTracks.length > 1 && (
        <div className="hidden 2xl:block">
          <TrackMenu
            icon={<AudioLines className="size-4" />}
            label="Audio track"
            onOpenChange={onMenuOpenChange}
            {...audioChoices}
          />
        </div>
      )}
      {subtitleTracks.length > 0 && (
        <div className="hidden 2xl:block">
          <TrackMenu
            icon={<Captions className="size-4" />}
            label="Subtitles"
            onOpenChange={onMenuOpenChange}
            {...subtitleChoices}
          />
        </div>
      )}

      <DropdownMenu onOpenChange={onMenuOpenChange}>
        <DropdownMenuTrigger asChild>
          <Button variant="secondary" size="icon" title="Playback settings" aria-label="Playback settings">
            <Settings className="size-4" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent side="top" align="end" sideOffset={10} className="w-72">
          <DropdownMenuLabel>Playback settings</DropdownMenuLabel>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>Quality</DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="max-h-80 overflow-y-auto">
              <DropdownMenuRadioGroup
                value={selectedQuality}
                onValueChange={(value) => {
                  if (!itemId || !isStreamingQuality(value)) return
                  const quality = value
                  setQualityOverride(quality)
                  qualityChange.mutate({ itemId, positionMs: position, quality })
                  showAction(`Quality ${qualityLabel(quality)}`)
                }}
              >
                {STREAMING_QUALITIES.map((quality) => (
                  <DropdownMenuRadioItem key={quality.id} value={quality.id}>
                    {quality.label}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <TuningMenu tuning={tuning} onChange={tune} />
          {chapters.length > 0 && (
            <DropdownMenuSub>
              <DropdownMenuSubTrigger>Chapters</DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="max-h-80 overflow-y-auto">
                {chapters.map((entry, index) => (
                  <DropdownMenuItem key={`${entry.startMs}-${index}`} onSelect={() => seek(entry.startMs)}>
                    <span className="min-w-0 flex-1 truncate">{entry.title}</span>
                    <span className="text-xs tabular-nums text-muted-foreground">
                      {formatClock(entry.startMs)}
                    </span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuLabel>Playback information</DropdownMenuLabel>
          <div className="space-y-1 px-2 pb-2 text-xs text-muted-foreground">
            <div>{playMethodLabel(player.playMethod)}</div>
            <div>{qualityLabel(selectedQuality) ?? "Automatic quality"}</div>
            {video && (
              <div>
                {[formatResolution(video), formatVideoRange(video), formatCodec(video.codec)]
                  .filter(Boolean)
                  .join(" · ")}
              </div>
            )}
            {source && (
              <div>
                {[source.container?.toUpperCase(), formatBitrate(source.bitrate), formatFileSize(source.size)]
                  .filter(Boolean)
                  .join(" · ")}
              </div>
            )}
            {selectedAudio && <div>{trackLabel(selectedAudio, audioTracks.indexOf(selectedAudio))}</div>}
            {(player.diagnostics?.frameRate != null ||
              player.diagnostics?.droppedFrames != null) && (
              <div>
                {[
                  player.diagnostics.frameRate == null
                    ? null
                    : `${player.diagnostics.frameRate.toFixed(2)} fps`,
                  player.diagnostics.droppedFrames == null
                    ? null
                    : `${player.diagnostics.droppedFrames} dropped frames`,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </div>
            )}
            <div>{chapters.length} chapters · {segments.length} skip regions</div>
          </div>
        </DropdownMenuContent>
      </DropdownMenu>

      <Button
        variant="secondary"
        size="icon"
        title={withShortcuts("Toggle fullscreen", [], [comfort.fullscreenKey])}
        aria-label="Toggle fullscreen"
        onClick={() => send({ command: "toggle-fullscreen" }, "Fullscreen toggled")}
        className="hidden sm:inline-flex"
      >
        <Maximize className="size-4" />
      </Button>

      <DropdownMenu onOpenChange={onMenuOpenChange}>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon" title="More playback controls" aria-label="More playback controls" className="2xl:hidden">
            <MoreHorizontal className="size-4" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent side="top" align="end" sideOffset={10} className="w-64">
          <DropdownMenuItem onSelect={() => relativeSeek(-seekBackMs)}>Back {comfort.seekBackSeconds} seconds</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => relativeSeek(seekForwardMs)}>Forward {comfort.seekForwardSeconds} seconds</DropdownMenuItem>
          <DropdownMenuItem disabled={!neighbors.data?.previous} onSelect={() => itemId && neighborPlayback.mutate({ itemId, direction: "previous" })}>
            Previous episode
          </DropdownMenuItem>
          <DropdownMenuItem disabled={!neighbors.data?.next} onSelect={() => itemId && neighborPlayback.mutate({ itemId, direction: "next" })}>
            Next episode
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>Volume</DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-56 p-3">
              <div className="flex items-center gap-3">
                <MuteButton muted={muted} volume={volume} onToggle={toggleMute} className="shrink-0" />
                <Slider
                  value={[muted ? 0 : volume]}
                  max={100}
                  step={1}
                  aria-label={`Volume ${Math.round(volume)}%`}
                  onValueChange={([value]) => setVolumeDraft(value)}
                  onValueCommit={([value]) => setVolume(value)}
                />
                <span className="w-8 text-right text-xs tabular-nums">{Math.round(volume)}</span>
              </div>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          {audioTracks.length > 1 && <TrackSubmenu label="Audio track" {...audioChoices} />}
          {subtitleTracks.length > 0 && <TrackSubmenu label="Subtitles" {...subtitleChoices} />}
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onSelect={() => send({ command: "stop" })}>
            Stop playback
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </footer>
  )
}

export function PlayerBar({ onMenuOpenChange }: { onMenuOpenChange?: (open: boolean) => void }) {
  const { data: player } = usePlayerState()
  if (!player?.active) return null
  return <ActivePlayerBar key={player.playbackId ?? player.itemId} player={player} onMenuOpenChange={onMenuOpenChange} />
}
