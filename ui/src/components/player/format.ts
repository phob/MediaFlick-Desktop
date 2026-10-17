import type { PlayerChapter, PlayerTrack } from "@/lib/api"
import { formatCodec, formatLanguage } from "@/lib/format"

export function formatClock(ms: number) {
  const total = Math.max(0, Math.round(ms / 1000))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const seconds = total % 60
  const pad = (value: number) => String(value).padStart(2, "0")
  return hours ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`
}

export function trackLabel(track: PlayerTrack, position: number) {
  const parts = [track.title?.trim(), formatLanguage(track.language), formatCodec(track.codec)]
    .filter((part): part is string => Boolean(part))
    .filter((part, index, all) => all.indexOf(part) === index)
  if (parts.length) return parts.join(" · ")
  return `${track.kind === "audio" ? "Audio" : "Subtitle"} ${position + 1}`
}

export function currentChapter(chapters: PlayerChapter[], positionMs: number) {
  return chapters.reduce<PlayerChapter | null>(
    (current, chapter) => (chapter.startMs <= positionMs ? chapter : current),
    null,
  )
}
