// Display formatting for the detail page. Everything returns `null` for values
// worth omitting entirely, so callers can drop a row instead of printing a dash.

import { ticksToMs, type MediaStream } from "./api"

export function formatRuntime(ticks: number | null | undefined) {
  const minutes = Math.round(ticksToMs(ticks) / 60_000)
  if (!minutes) return null
  const hours = Math.floor(minutes / 60)
  return hours ? `${hours}h ${minutes % 60}m` : `${minutes}m`
}

/** "24m left" for the Resume button — the part still ahead, not behind. */
export function formatRemaining(positionTicks: number, runtimeTicks: number | null) {
  if (!runtimeTicks || positionTicks <= 0) return null
  const minutes = Math.round(ticksToMs(runtimeTicks - positionTicks) / 60_000)
  if (minutes <= 0) return null
  const hours = Math.floor(minutes / 60)
  return hours ? `${hours}h ${minutes % 60}m left` : `${minutes}m left`
}

/**
 * The community score restated as the "% Match" the browsing surfaces lead
 * with. It is the same ten-point number the detail page prints beside a star —
 * not a personalised prediction — so anything without a score gets no badge at
 * all rather than an invented one.
 */
export function formatMatch(communityRating: number | null | undefined) {
  if (communityRating == null || communityRating <= 0) return null
  return `${Math.min(99, Math.round(communityRating * 10))}% match`
}

/** Jellyfin dates are ISO 8601; an unparsable one is not worth a row. */
export function formatDate(value: string | null | undefined) {
  if (!value) return null
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return null
  return date.toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" })
}

export function formatYearOnly(value: string | null | undefined) {
  if (!value) return null
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? null : String(date.getFullYear())
}

export function formatFileSize(bytes: number | null | undefined) {
  if (!bytes || bytes <= 0) return null
  const gib = bytes / 1024 ** 3
  if (gib >= 1) return `${gib.toFixed(gib >= 10 ? 1 : 2)} GiB`
  return `${Math.round(bytes / 1024 ** 2)} MiB`
}

export function formatBitrate(bitsPerSecond: number | null | undefined) {
  if (!bitsPerSecond || bitsPerSecond <= 0) return null
  const mbps = bitsPerSecond / 1_000_000
  return mbps >= 10 ? `${Math.round(mbps)} Mbps` : `${mbps.toFixed(1)} Mbps`
}

/**
 * The marketing name for a frame size. Sources are rarely exactly 1920x1080 —
 * a 2.39:1 scope transfer is 1920x804 — so the label keys on width, with height
 * as the fallback for anamorphic or pillarboxed oddities.
 */
export function formatResolution(stream: Pick<MediaStream, "width" | "height">) {
  const { width, height } = stream
  if (!width && !height) return null
  if (width) {
    if (width >= 3400) return "4K"
    if (width >= 2400) return "1440p"
    if (width >= 1800) return "1080p"
    if (width >= 1200) return "720p"
    if (width >= 900) return "576p"
    if (width >= 600) return "480p"
  }
  return height ? `${height}p` : null
}

export function formatChannels(channels: number | null | undefined) {
  switch (channels) {
    case null:
    case undefined:
    case 0:
      return null
    case 1:
      return "Mono"
    case 2:
      return "Stereo"
    case 6:
      return "5.1"
    case 8:
      return "7.1"
    default:
      return `${channels}.0`
  }
}

const languageNames = (() => {
  try {
    return new Intl.DisplayNames(undefined, { type: "language" })
  } catch {
    return null
  }
})()

/**
 * "eng" reads as "English" where the platform knows the code. Jellyfin also
 * emits things like "und" and free-text values, so anything unresolved falls
 * back to the raw code rather than disappearing.
 */
export function formatLanguage(code: string | null | undefined) {
  if (!code) return null
  const trimmed = code.trim()
  if (!trimmed || trimmed.toLowerCase() === "und") return null
  try {
    const resolved = languageNames?.of(trimmed)
    if (resolved && resolved.toLowerCase() !== trimmed.toLowerCase()) return resolved
  } catch {
    // A code Intl cannot parse at all throws; the raw value still says more
    // than nothing.
  }
  return trimmed.toUpperCase()
}

export function formatCodec(codec: string | null | undefined) {
  if (!codec) return null
  const known: Record<string, string> = {
    h264: "H.264",
    hevc: "HEVC",
    h265: "HEVC",
    av1: "AV1",
    vp9: "VP9",
    mpeg2video: "MPEG-2",
    vc1: "VC-1",
    eac3: "E-AC-3",
    ac3: "AC-3",
    truehd: "TrueHD",
    dts: "DTS",
    dtshd: "DTS-HD",
    aac: "AAC",
    opus: "Opus",
    flac: "FLAC",
    mp3: "MP3",
    subrip: "SRT",
    ass: "ASS",
    ssa: "SSA",
    pgssub: "PGS",
    dvdsub: "VobSub",
  }
  const lower = codec.toLowerCase()
  return known[lower] ?? codec.toUpperCase()
}

/**
 * The dynamic range of a video track: "SDR" is the absence of news, so only
 * the HDR flavours are worth a label. `VideoRangeType` is the specific one
 * (HDR10, HLG, DOVI…), which is what decides whether a display tone-maps.
 */
export function formatVideoRange(stream: Pick<MediaStream, "videoRange" | "videoRangeType">) {
  const specific = stream.videoRangeType?.trim()
  const general = stream.videoRange?.trim()
  const value = specific && specific.toUpperCase() !== "SDR" ? specific : general
  if (!value || value.toUpperCase() === "SDR" || value.toUpperCase() === "UNKNOWN") return null
  // Keys are Jellyfin's `VideoRangeType` values, lowercased. Anything it adds
  // later prints as the server spelled it rather than as shouted camel case.
  const known: Record<string, string> = {
    hdr: "HDR",
    hdr10: "HDR10",
    hdr10plus: "HDR10+",
    hlg: "HLG",
    dovi: "Dolby Vision",
    dovihdr10: "Dolby Vision / HDR10",
    doviwithhdr10: "Dolby Vision / HDR10",
    doviwithhlg: "Dolby Vision / HLG",
    doviwithsdr: "Dolby Vision",
    doviwithel: "Dolby Vision",
    doviinvalid: "Dolby Vision",
  }
  return known[value.toLowerCase()] ?? value
}

/** Compact one-line summary of a track, used in the media-info lists. */
export function describeStream(stream: MediaStream, kind: "video" | "audio" | "subtitle") {
  const parts = [
    kind === "video" ? formatResolution(stream) : formatLanguage(stream.language),
    formatCodec(stream.codec),
    kind === "video" ? formatVideoRange(stream) : null,
    kind === "video" && stream.bitDepth ? `${stream.bitDepth}-bit` : null,
    kind === "audio" ? formatChannels(stream.channels) : null,
    kind === "subtitle" && stream.isForced ? "Forced" : null,
    kind === "subtitle" && stream.isExternal ? "External" : null,
  ].filter(Boolean)
  // A track with nothing recognisable still has whatever the server called it.
  return parts.length ? parts.join(" · ") : (stream.displayTitle ?? stream.title ?? "Unknown")
}
