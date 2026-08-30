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
import { useEffect, useRef, useState, type PointerEvent, type ReactNode } from "react"
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
import {
  STREAMING_QUALITIES,
  imageUrl,
  qualityLabel,
  type PlayerChapter,
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
  formatLanguage,
  formatResolution,
  formatVideoRange,
} from "@/lib/format"
import {
  useChangePlaybackQuality,
  useItem,
  useMediaInfo,
  usePlaybackNeighbors,
  usePlayNeighbor,
  usePlayerCommand,
  usePlayerState,
  useSettings,
} from "@/lib/queries"
import { patchPlayerState } from "@/lib/query-client"

const SUBTITLES_OFF = "__off__"
const TIME_DISPLAY_KEY = "mediaflick.player.time-display"
const TICKS_PER_MS = 10_000
const SEEK_BACK_MS = 10_000
const SEEK_FORWARD_MS = 30_000

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
    // Playback controls still work when browser storage is unavailable.
  }
}

function formatClock(ms: number) {
  const total = Math.max(0, Math.round(ms / 1000))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const seconds = total % 60
  const pad = (value: number) => String(value).padStart(2, "0")
  return hours ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`
}

function trackLabel(track: PlayerTrack, position: number) {
  const parts = [track.title?.trim(), formatLanguage(track.language), formatCodec(track.codec)]
    .filter((part): part is string => Boolean(part))
    .filter((part, index, all) => all.indexOf(part) === index)
  if (parts.length) return parts.join(" · ")
  return `${track.kind === "audio" ? "Audio" : "Subtitle"} ${position + 1}`
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

function currentChapter(chapters: PlayerChapter[], positionMs: number) {
  return chapters.reduce<PlayerChapter | null>(
    (current, chapter) => (chapter.startMs <= positionMs ? chapter : current),
    null,
  )
}

function TrackMenu({
  icon,
  label,
  tracks,
  value,
  allowOff = false,
  onValueChange,
  onOpenChange,
}: {
  icon: ReactNode
  label: string
  tracks: PlayerTrack[]
  value: string
  allowOff?: boolean
  onValueChange: (value: string) => void
  onOpenChange?: (open: boolean) => void
}) {
  const selected = tracks.find((track) => String(track.id) === value)
  const title = selected ? `${label}: ${trackLabel(selected, tracks.indexOf(selected))}` : label

  return (
    <DropdownMenu onOpenChange={onOpenChange}>
      <DropdownMenuTrigger asChild>
        <Button variant="secondary" size="icon" title={title} aria-label={label}>
          {icon}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="end" sideOffset={10} className="w-72">
        <DropdownMenuLabel>{label}</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={value} onValueChange={onValueChange}>
          {allowOff && <DropdownMenuRadioItem value={SUBTITLES_OFF}>Off</DropdownMenuRadioItem>}
          {tracks.map((track, index) => (
            <DropdownMenuRadioItem key={track.id} value={String(track.id)}>
              <span className="min-w-0 truncate">{trackLabel(track, index)}</span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function Timeline({
  position,
  duration,
  chapters,
  segments,
  bufferedUntil,
  onScrub,
  onSeek,
}: {
  position: number
  duration: number
  chapters: PlayerChapter[]
  segments: PlayerSkipSegment[]
  bufferedUntil: number | null
  onScrub: (value: number | null) => void
  onSeek: (value: number) => void
}) {
  const [hoverMs, setHoverMs] = useState<number | null>(null)
  const previewChapter = hoverMs === null ? null : currentChapter(chapters, hoverMs)
  const updateHover = (event: PointerEvent<HTMLDivElement>) => {
    if (!duration) return
    const bounds = event.currentTarget.getBoundingClientRect()
    const fraction = Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width))
    const value = fraction * duration
    setHoverMs(value)
  }
  const clearHover = () => setHoverMs(null)

  return (
    <div
      className="relative h-10 min-w-24 flex-1"
      onPointerMove={updateHover}
      onPointerLeave={clearHover}
    >
      {hoverMs !== null && duration > 0 && (
        <div
          className="pointer-events-none absolute bottom-full z-30 mb-2 -translate-x-1/2 rounded border bg-popover px-2 py-1 text-center text-xs shadow-lg"
          style={{ left: `${Math.max(4, Math.min(96, (hoverMs / duration) * 100))}%` }}
        >
          <div className="font-medium tabular-nums">{formatClock(hoverMs)}</div>
          {previewChapter && (
            <div className="max-w-48 truncate text-muted-foreground">{previewChapter.title}</div>
          )}
        </div>
      )}
      <div className="pointer-events-none absolute inset-x-2 top-1/2 z-10 h-1.5 -translate-y-1/2 overflow-hidden rounded-full">
        {duration > 0 && bufferedUntil !== null && (
          <span
            className="absolute inset-y-0 left-0 bg-white/20"
            style={{ width: `${Math.max(0, Math.min(100, (bufferedUntil / duration) * 100))}%` }}
          />
        )}
        {duration > 0 &&
          segments.map((segment, index) => {
            const startMs = segment.startTicks / TICKS_PER_MS
            const endMs = segment.endTicks / TICKS_PER_MS
            const left = Math.max(0, Math.min(100, (startMs / duration) * 100))
            const width = Math.max(0, Math.min(100 - left, ((endMs - startMs) / duration) * 100))
            return (
              <span
                key={`${segment.segmentType}-${segment.startTicks}-${index}`}
                className="absolute inset-y-0 bg-amber-400/55"
                style={{ left: `${left}%`, width: `${width}%` }}
              />
            )
          })}
        {duration > 0 &&
          chapters.map((chapter, index) => (
            <span
              key={`${chapter.startMs}-${index}`}
              className="absolute inset-y-0 w-px bg-white/70"
              style={{ left: `${Math.max(0, Math.min(100, (chapter.startMs / duration) * 100))}%` }}
            />
          ))}
      </div>
      <Slider
        value={[Math.min(position, duration || position)]}
        max={duration || 1}
        step={1000}
        disabled={!duration}
        aria-label="Playback position"
        onValueChange={([value]) => onScrub(value)}
        onValueCommit={([value]) => onSeek(value)}
        className="player-seek-target h-10 cursor-pointer"
      />
    </div>
  )
}

interface PlaybackTuning {
  audioDelay: string
  subtitleDelay: string
  subtitleScale: string
  videoFit: "fit" | "fill"
  videoAspect: "source" | "4:3" | "16:9" | "21:9"
  toneMapping: "auto" | "clip" | "mobius" | "reinhard" | "hable" | "bt.2390"
  deinterlace: boolean
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

function isVideoFit(value: string): value is PlaybackTuning["videoFit"] {
  return value === "fit" || value === "fill"
}

function isVideoAspect(value: string): value is PlaybackTuning["videoAspect"] {
  return value === "source" || value === "4:3" || value === "16:9" || value === "21:9"
}

function isToneMapping(value: string): value is PlaybackTuning["toneMapping"] {
  return ["auto", "clip", "mobius", "reinhard", "hable", "bt.2390"].includes(value)
}

function isStreamingQuality(value: string): value is StreamingQualityId {
  return STREAMING_QUALITIES.some((quality) => quality.id === value)
}

function TuningMenu({
  tuning,
  onChange,
}: {
  tuning: PlaybackTuning
  onChange: (patch: Partial<PlaybackTuning>, command: PlayerCommand, feedback: string) => void
}) {
  const delays = ["-2", "-1", "-0.5", "0", "0.5", "1", "2"]
  return (
    <>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>Audio delay</DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            value={tuning.audioDelay}
            onValueChange={(value) =>
              onChange(
                { audioDelay: value },
                { command: "set-audio-delay", delaySeconds: Number(value) },
                `Audio delay ${Number(value) >= 0 ? "+" : ""}${value}s`,
              )
            }
          >
            {delays.map((delay) => (
              <DropdownMenuRadioItem key={delay} value={delay}>
                {Number(delay) === 0 ? "No delay" : `${Number(delay) > 0 ? "+" : ""}${delay}s`}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>Subtitle delay</DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            value={tuning.subtitleDelay}
            onValueChange={(value) =>
              onChange(
                { subtitleDelay: value },
                { command: "set-subtitle-delay", delaySeconds: Number(value) },
                `Subtitle delay ${Number(value) >= 0 ? "+" : ""}${value}s`,
              )
            }
          >
            {delays.map((delay) => (
              <DropdownMenuRadioItem key={delay} value={delay}>
                {Number(delay) === 0 ? "No delay" : `${Number(delay) > 0 ? "+" : ""}${delay}s`}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>Subtitle size</DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            value={tuning.subtitleScale}
            onValueChange={(value) =>
              onChange(
                { subtitleScale: value },
                { command: "set-subtitle-scale", scale: Number(value) },
                `Subtitle size ${Math.round(Number(value) * 100)}%`,
              )
            }
          >
            {[
              ["0.75", "Small"],
              ["1", "Normal"],
              ["1.25", "Large"],
              ["1.5", "Extra large"],
            ].map(([value, label]) => (
              <DropdownMenuRadioItem key={value} value={value}>
                {label}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>Video fit</DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            value={tuning.videoFit}
            onValueChange={(value) => {
              if (!isVideoFit(value)) return
              onChange(
                { videoFit: value },
                { command: "set-video-fit", fit: value },
                value === "fill" ? "Video fills the window" : "Video fits the window",
              )
            }}
          >
            <DropdownMenuRadioItem value="fit">Fit</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="fill">Fill and crop</DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>Aspect ratio</DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            value={tuning.videoAspect}
            onValueChange={(value) => {
              if (!isVideoAspect(value)) return
              onChange(
                { videoAspect: value },
                { command: "set-video-aspect", aspect: value },
                `Aspect ratio ${value === "source" ? "from source" : value}`,
              )
            }}
          >
            <DropdownMenuRadioItem value="source">From source</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="4:3">4:3</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="16:9">16:9</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="21:9">21:9</DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>HDR tone mapping</DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            value={tuning.toneMapping}
            onValueChange={(value) => {
              if (!isToneMapping(value)) return
              onChange(
                { toneMapping: value },
                { command: "set-tone-mapping", mode: value },
                `Tone mapping ${value}`,
              )
            }}
          >
            {[
              ["auto", "Automatic"],
              ["bt.2390", "BT.2390"],
              ["mobius", "Mobius"],
              ["reinhard", "Reinhard"],
              ["hable", "Hable"],
              ["clip", "Clip"],
            ].map(([value, label]) => (
              <DropdownMenuRadioItem key={value} value={value}>
                {label}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      <DropdownMenuItem
        onSelect={() =>
          onChange(
            { deinterlace: !tuning.deinterlace },
            { command: "set-deinterlace", enabled: !tuning.deinterlace },
            `Deinterlacing ${tuning.deinterlace ? "off" : "on"}`,
          )
        }
      >
        Deinterlacing
        <span className="ml-auto text-xs text-muted-foreground">
          {tuning.deinterlace ? "On" : "Off"}
        </span>
      </DropdownMenuItem>
    </>
  )
}

function ActivePlayerBar({
  player,
  onMenuOpenChange,
}: {
  player: PlayerState
  onMenuOpenChange?: (open: boolean) => void
}) {
  const command = usePlayerCommand()
  const neighborPlayback = usePlayNeighbor()
  const qualityChange = useChangePlaybackQuality()
  const settings = useSettings()
  const [scrubMs, setScrubMs] = useState<number | null>(null)
  const [feedback, setFeedback] = useState<string | null>(null)
  const [showRemaining, setShowRemaining] = useState(
    () => savedTimeDisplay() === "remaining",
  )
  const [volumeDraft, setVolumeDraft] = useState<number | null>(null)
  const [qualityOverride, setQualityOverride] = useState<StreamingQualityId | null>(null)
  const [tuning, setTuning] = useState(DEFAULT_TUNING)
  const feedbackTimer = useRef<number | undefined>(undefined)
  const itemId = player.itemId ?? undefined
  const { data: item } = useItem(itemId)
  const media = useMediaInfo(itemId)
  const neighbors = usePlaybackNeighbors(itemId, item?.kind === "Episode")

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
    qualityOverride ??
    settings.data?.client?.playback?.streamingQuality ??
    settings.data?.streamingQuality ??
    "auto"

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
    showAction(deltaMs < 0 ? "Back 10 seconds" : "Forward 30 seconds")
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

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target instanceof HTMLElement ? event.target : null
      if (
        target?.isContentEditable ||
        target?.matches("input, textarea, select, [role='slider'], [role='menuitemradio']")
      ) {
        return
      }
      const key = event.key.toLowerCase()
      if (key === " " || key === "k") {
        event.preventDefault()
        togglePause()
      } else if (key === "arrowleft" || key === "j") {
        event.preventDefault()
        relativeSeek(-SEEK_BACK_MS)
      } else if (key === "arrowright" || key === "l") {
        event.preventDefault()
        relativeSeek(SEEK_FORWARD_MS)
      } else if (key === "m") {
        event.preventDefault()
        toggleMute()
      } else if (key === "f" && player.capabilities?.fullscreen !== false) {
        event.preventDefault()
        send({ command: "toggle-fullscreen" }, "Fullscreen toggled")
      }
    }
    window.addEventListener("keydown", handleKeyDown)
    return () => window.removeEventListener("keydown", handleKeyDown)
  })

  const timeText = showRemaining
    ? `-${formatClock(Math.max(0, duration - position))}`
    : `${formatClock(position)} / ${formatClock(duration)}`

  return (
    <footer className="player-bar relative z-20 flex shrink-0 items-center gap-2 border-t border-white/8 bg-card/92 px-3 py-2.5 shadow-[0_-16px_40px_rgba(0,0,0,0.28)] backdrop-blur-xl sm:gap-3 sm:px-4">
      {player.diagnostics?.buffering && (
        <div
          role="status"
          className="pointer-events-none absolute bottom-full left-1/2 mb-8 flex -translate-x-1/2 items-center gap-2 rounded-md border border-white/10 bg-black/82 px-4 py-2 text-sm font-medium text-white shadow-xl"
        >
          <LoaderCircle className="size-4 animate-spin" />
          Buffering
        </div>
      )}
      {feedback && !player.diagnostics?.buffering && (
        <div
          role="status"
          className="pointer-events-none absolute bottom-full left-1/2 mb-8 -translate-x-1/2 rounded-md border border-white/10 bg-black/82 px-4 py-2 text-sm font-medium text-white shadow-xl"
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
        title="Back 10 seconds (Left or J)"
        aria-label="Back 10 seconds"
        onClick={() => relativeSeek(-SEEK_BACK_MS)}
        className="hidden sm:inline-flex"
      >
        <Rewind className="size-4" />
      </Button>
      <Button
        variant="secondary"
        size="icon"
        title={paused ? "Resume (Space or K)" : "Pause (Space or K)"}
        aria-label={paused ? "Resume" : "Pause"}
        onClick={togglePause}
      >
        {paused ? <Play className="size-4" /> : <Pause className="size-4" />}
      </Button>
      <Button
        variant="ghost"
        size="icon"
        title="Forward 30 seconds (Right or L)"
        aria-label="Forward 30 seconds"
        onClick={() => relativeSeek(SEEK_FORWARD_MS)}
        className="hidden sm:inline-flex"
      >
        <FastForward className="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        title={neighbors.data?.next ? `Next: ${neighbors.data.next.name}` : "No next episode"}
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
        title="Stop"
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

      <button
        type="button"
        className="hidden shrink-0 text-xs tabular-nums text-muted-foreground hover:text-foreground md:block"
        title="Toggle elapsed and remaining time"
        aria-label={`${timeText}. Toggle elapsed and remaining time`}
        onClick={toggleTimeDisplay}
      >
        {timeText}
      </button>

      <div className="hidden w-32 shrink-0 items-center gap-2 xl:flex">
        <Button
          variant="ghost"
          size="icon"
          title={muted ? "Unmute (M)" : "Mute (M)"}
          aria-label={muted ? "Unmute" : "Mute"}
          onClick={toggleMute}
          className="size-8"
        >
          {muted || volume === 0 ? <VolumeX className="size-4" /> : <Volume2 className="size-4" />}
        </Button>
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
            tracks={audioTracks}
            value={selectedAudio ? String(selectedAudio.id) : ""}
            onValueChange={(value) => selectTrack("audio", Number(value))}
            onOpenChange={onMenuOpenChange}
          />
        </div>
      )}
      {subtitleTracks.length > 0 && (
        <div className="hidden 2xl:block">
          <TrackMenu
            icon={<Captions className="size-4" />}
            label="Subtitles"
            tracks={subtitleTracks}
            value={selectedSubtitle ? String(selectedSubtitle.id) : SUBTITLES_OFF}
            allowOff
            onValueChange={(value) =>
              selectTrack("subtitle", value === SUBTITLES_OFF ? null : Number(value))
            }
            onOpenChange={onMenuOpenChange}
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
          {player.capabilities?.playbackTuning !== false && (
            <TuningMenu tuning={tuning} onChange={tune} />
          )}
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

      {player.capabilities?.fullscreen !== false && (
        <Button
          variant="secondary"
          size="icon"
          title="Toggle fullscreen (F)"
          aria-label="Toggle fullscreen"
          onClick={() => send({ command: "toggle-fullscreen" }, "Fullscreen toggled")}
          className="hidden sm:inline-flex"
        >
          <Maximize className="size-4" />
        </Button>
      )}

      <DropdownMenu onOpenChange={onMenuOpenChange}>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon" title="More playback controls" aria-label="More playback controls" className="2xl:hidden">
            <MoreHorizontal className="size-4" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent side="top" align="end" sideOffset={10} className="w-64">
          <DropdownMenuItem onSelect={() => relativeSeek(-SEEK_BACK_MS)}>Back 10 seconds</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => relativeSeek(SEEK_FORWARD_MS)}>Forward 30 seconds</DropdownMenuItem>
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
                <button type="button" aria-label={muted ? "Unmute" : "Mute"} onClick={toggleMute}>
                  {muted || volume === 0 ? <VolumeX className="size-4" /> : <Volume2 className="size-4" />}
                </button>
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
          {audioTracks.length > 1 && (
            <DropdownMenuSub>
              <DropdownMenuSubTrigger>Audio track</DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="w-72">
                <DropdownMenuRadioGroup
                  value={selectedAudio ? String(selectedAudio.id) : ""}
                  onValueChange={(value) => selectTrack("audio", Number(value))}
                >
                  {audioTracks.map((track, index) => (
                    <DropdownMenuRadioItem key={track.id} value={String(track.id)}>
                      <span className="truncate">{trackLabel(track, index)}</span>
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          )}
          {subtitleTracks.length > 0 && (
            <DropdownMenuSub>
              <DropdownMenuSubTrigger>Subtitles</DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="w-72">
                <DropdownMenuRadioGroup
                  value={selectedSubtitle ? String(selectedSubtitle.id) : SUBTITLES_OFF}
                  onValueChange={(value) =>
                    selectTrack("subtitle", value === SUBTITLES_OFF ? null : Number(value))
                  }
                >
                  <DropdownMenuRadioItem value={SUBTITLES_OFF}>Off</DropdownMenuRadioItem>
                  {subtitleTracks.map((track, index) => (
                    <DropdownMenuRadioItem key={track.id} value={String(track.id)}>
                      <span className="truncate">{trackLabel(track, index)}</span>
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onSelect={() => send({ command: "stop" })}>
            Stop playback
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </footer>
  )
}

/** Controls for the active player while the selected backend owns video rendering. */
export function PlayerBar({ onMenuOpenChange }: { onMenuOpenChange?: (open: boolean) => void }) {
  const { data: player } = usePlayerState()
  if (!player?.active) return null
  return <ActivePlayerBar player={player} onMenuOpenChange={onMenuOpenChange} />
}
