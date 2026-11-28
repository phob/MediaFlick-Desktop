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

export type TrickplaySpriteFrame = {
  tileIndex: number
  offsetX: number
  offsetY: number
  previewWidth: number
  previewHeight: number
  sheetWidth: number
  sheetHeight: number
}

export function trickplayFrameAt(
  info: {
    width: number
    height: number
    tileWidth: number
    tileHeight: number
    thumbnailCount: number
    intervalMs: number
  },
  positionMs: number,
  previewWidth = 192,
): TrickplaySpriteFrame | null {
  if (
    !Number.isFinite(positionMs) ||
    !Number.isFinite(previewWidth) ||
    previewWidth <= 0 ||
    info.width <= 0 ||
    info.height <= 0 ||
    info.tileWidth <= 0 ||
    info.tileHeight <= 0 ||
    info.thumbnailCount <= 0 ||
    info.intervalMs <= 0
  ) {
    return null
  }
  const perTile = info.tileWidth * info.tileHeight
  const frameIndex = Math.min(
    info.thumbnailCount - 1,
    Math.floor(Math.max(0, positionMs) / info.intervalMs),
  )
  const tileIndex = Math.floor(frameIndex / perTile)
  const offsetInTile = frameIndex % perTile
  const col = offsetInTile % info.tileWidth
  const row = Math.floor(offsetInTile / info.tileWidth)
  const previewHeight = Math.max(1, Math.round((previewWidth * info.height) / info.width))
  return {
    tileIndex,
    offsetX: col * previewWidth,
    offsetY: row * previewHeight,
    previewWidth,
    previewHeight,
    sheetWidth: info.tileWidth * previewWidth,
    sheetHeight: info.tileHeight * previewHeight,
  }
}

export function currentChapter(chapters: PlayerChapter[], positionMs: number) {
  return chapters.reduce<PlayerChapter | null>(
    (current, chapter) => (chapter.startMs <= positionMs ? chapter : current),
    null,
  )
}
