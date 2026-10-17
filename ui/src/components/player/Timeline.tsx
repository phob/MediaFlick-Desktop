import { useState, type PointerEvent } from "react"
import { Slider } from "@/components/ui/slider"
import { TICKS_PER_MS, type PlayerChapter, type PlayerSkipSegment } from "@/lib/api"
import { currentChapter, formatClock } from "@/components/player/format"

export function Timeline({
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
