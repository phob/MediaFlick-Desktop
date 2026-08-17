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

/** The server-supplied community score, kept on its native ten-point scale. */
export function formatCommunityRating(communityRating: number | null | undefined) {
  if (communityRating == null || communityRating <= 0) return null
  return communityRating.toFixed(1)
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
  const known = new Map<string, string>([
    ["h264", "H.264"],
    ["hevc", "HEVC"],
    ["h265", "HEVC"],
    ["av1", "AV1"],
    ["vp9", "VP9"],
    ["mpeg2video", "MPEG-2"],
    ["vc1", "VC-1"],
    ["eac3", "E-AC-3"],
    ["ac3", "AC-3"],
    ["truehd", "TrueHD"],
    ["dts", "DTS"],
    ["dtshd", "DTS-HD"],
    ["dts_hd_ma", "DTS-HD MA"],
    ["dts_hd_hra", "DTS-HD HRA"],
    ["aac", "AAC"],
    ["opus", "Opus"],
    ["flac", "FLAC"],
    ["alac", "ALAC"],
    ["pcm_s16le", "PCM"],
    ["pcm_s24le", "PCM"],
    ["mp3", "MP3"],
    ["subrip", "SRT"],
    ["ass", "ASS"],
    ["ssa", "SSA"],
    ["pgssub", "PGS"],
    ["dvdsub", "VobSub"],
  ])
  const lower = codec.toLowerCase()
  return known.get(lower) ?? codec.toUpperCase()
}

/** Jellyfin often reports DTS-HD as codec `dts` plus a richer profile. */
export function formatAudioCodec(stream: Pick<MediaStream, "codec" | "profile">) {
  const codec = stream.codec?.trim()
  const profile = stream.profile?.trim() ?? ""
  if (codec?.toLowerCase() === "dts" && /dts[- ]?hd/i.test(profile)) {
    if (/\bma\b|master audio/i.test(profile)) return "DTS-HD MA"
    if (/\bhra\b|high resolution/i.test(profile)) return "DTS-HD HRA"
    return "DTS-HD"
  }
  return formatCodec(codec)
}

/**
 * Compact and expanded labels for Jellyfin's `VideoRangeType` values. The
 * expanded form keeps terse card labels understandable to assistive
 * technology and in native tooltips.
 */
const knownVideoRanges = new Map<string, { label: string; description?: string }>([
  ["hdr", { label: "HDR" }],
  ["hdr10", { label: "HDR10" }],
  ["hdr10plus", { label: "HDR10+" }],
  ["hlg", { label: "HLG" }],
  ["dovi", { label: "DV", description: "Dolby Vision" }],
  ["dovihdr10", { label: "DV/HDR10", description: "Dolby Vision / HDR10" }],
  ["doviwithhdr10", { label: "DV&HDR10", description: "Dolby Vision / HDR10" }],
  ["doviwithhdr10plus", { label: "DV&HDR10+", description: "Dolby Vision / HDR10+" }],
  ["doviwithhlg", { label: "DV&HLG", description: "Dolby Vision / HLG" }],
  ["doviwithsdr", { label: "DV", description: "Dolby Vision" }],
  ["doviwithel", { label: "DV", description: "Dolby Vision" }],
  ["doviinvalid", { label: "DV", description: "Dolby Vision" }],
])

function videoRangeLabels(stream: Pick<MediaStream, "videoRange" | "videoRangeType">) {
  const specific = stream.videoRangeType?.trim()
  const general = stream.videoRange?.trim()
  const value = specific && specific.toUpperCase() !== "SDR" ? specific : general
  if (!value || value.toUpperCase() === "SDR" || value.toUpperCase() === "UNKNOWN") return null
  // Anything Jellyfin adds later prints as the server spelled it rather than
  // as shouted camel case.
  const known = knownVideoRanges.get(value.toLowerCase())
  return known ?? { label: value }
}

/**
 * The dynamic range of a video track: "SDR" is the absence of news, so only
 * the HDR flavours are worth a label. `VideoRangeType` is the specific one
 * (HDR10, HLG, DOVI…), which is what decides whether a display tone-maps.
 */
export function formatVideoRange(stream: Pick<MediaStream, "videoRange" | "videoRangeType">) {
  return videoRangeLabels(stream)?.label ?? null
}

function describeVideoRange(stream: Pick<MediaStream, "videoRange" | "videoRangeType">) {
  const labels = videoRangeLabels(stream)
  return labels?.description ?? labels?.label ?? null
}

export function formatAudioSpatialFormat(
  stream: Pick<MediaStream, "audioSpatialFormat" | "profile" | "title" | "displayTitle" | "codec">,
) {
  const explicit = stream.audioSpatialFormat?.trim() ?? ""
  const evidence = [explicit, stream.profile, stream.title, stream.displayTitle, stream.codec]
    .filter(Boolean)
    .join(" ")
    .toLowerCase()
  const compact = evidence.replace(/[^a-z0-9+]+/g, "")

  if (compact.includes("atmos")) return "Atmos"
  if (compact.includes("dtsx")) return "DTS:X"
  if (!explicit || /^(none|unknown)$/i.test(explicit)) return null

  const known = new Map<string, string>([
    ["dolbyatmos", "Atmos"],
    ["dtsx", "DTS:X"],
  ])
  return known.get(explicit.toLowerCase().replace(/[^a-z0-9]+/g, "")) ?? explicit
}

export interface CardMediaSummary {
  video: string[]
  audio: string[]
  /** Complete, spoken/native-tooltip text, including facts omitted visually. */
  description: string
}

function uniqueLabels(parts: Array<string | null>) {
  const present = parts.filter((part): part is string => Boolean(part))
  return present.filter(
    (part, index) =>
      present.findIndex((candidate) => candidate.toLowerCase() === part.toLowerCase()) === index,
  )
}

function streamKind(stream: MediaStream, kind: "video" | "audio") {
  return stream.type?.toLowerCase() === kind
}

function videoQuality(stream: MediaStream) {
  // Width is the resolution formatter's primary signal too. Weight it first
  // so a scope transfer or a stream whose height is absent still beats a
  // complete but lower-resolution frame size.
  return (stream.width ?? 0) * 10_000 + (stream.height ?? 0) * 10 + (formatVideoRange(stream) ? 1 : 0)
}

function bestVideo(streams: MediaStream[]) {
  return streams.reduce<MediaStream | null>((best, stream) => {
    if (!streamKind(stream, "video")) return best
    return !best || videoQuality(stream) > videoQuality(best) ? stream : best
  }, null)
}

function audioQuality(stream: MediaStream) {
  const codec = formatAudioCodec(stream)?.toLowerCase() ?? ""
  const codecRank = new Map<string, number>([
    ["truehd", 700],
    ["dts-hd ma", 675],
    ["dts-hd hra", 665],
    ["dts-hd", 650],
    ["flac", 600],
    ["dts", 500],
    ["e-ac-3", 450],
    ["ac-3", 400],
    ["opus", 350],
    ["aac", 300],
    ["mp3", 200],
  ])
  return (
    (formatAudioSpatialFormat(stream) ? 10_000 : 0) +
    (codecRank.get(codec) ?? 0) +
    (stream.channels ?? 0) * 10 +
    (stream.isDefault ? 1 : 0)
  )
}

function bestAudio(streams: MediaStream[]) {
  return streams.reduce<MediaStream | null>((best, stream) => {
    if (!streamKind(stream, "audio")) return best
    return !best || audioQuality(stream) > audioQuality(best) ? stream : best
  }, null)
}

/**
 * Two restrained card readout lines: resolution/range for the best video
 * stream, codec/spatial format for the most capable audio stream. The tooltip
 * keeps codec, bit depth, and channels available without turning the card into
 * a wall of labels.
 */
export function summarizeCardMedia(streams: MediaStream[] | null | undefined): CardMediaSummary | null {
  if (!streams?.length) return null
  const video = bestVideo(streams)
  const audio = bestAudio(streams)

  const resolution = video ? formatResolution(video) : null
  const range = video ? formatVideoRange(video) : null
  const videoCodec = video ? formatCodec(video.codec) : null
  const spatial = audio ? formatAudioSpatialFormat(audio) : null
  const audioCodec = audio ? formatAudioCodec(audio) : null
  const channels = audio ? formatChannels(audio.channels) : null

  // Range is more useful than a codec at card size. For ordinary SDR files,
  // the codec takes that second slot instead.
  const visibleVideo = uniqueLabels([resolution, range ?? videoCodec])
  const visibleAudio = uniqueLabels([audioCodec, spatial ?? channels])
  const fullVideo = uniqueLabels([
    resolution,
    video ? describeVideoRange(video) : null,
    videoCodec,
    video?.bitDepth ? `${video.bitDepth}-bit` : null,
  ])
  const fullAudio = uniqueLabels([audioCodec, spatial, channels])
  const description = [
    fullVideo.length ? `Video: ${fullVideo.join(", ")}` : null,
    fullAudio.length ? `Audio: ${fullAudio.join(", ")}` : null,
  ]
    .filter(Boolean)
    .join("; ")

  return visibleVideo.length || visibleAudio.length
    ? { video: visibleVideo, audio: visibleAudio, description }
    : null
}

/** Compact one-line summary of a track, used in the media-info lists. */
export function describeStream(stream: MediaStream, kind: "video" | "audio" | "subtitle") {
  const suppliedName = stream.title?.trim() || stream.displayTitle?.trim() || null
  const parts = [
    kind === "video" ? formatResolution(stream) : formatLanguage(stream.language),
    suppliedName,
    kind === "audio" ? formatAudioCodec(stream) : formatCodec(stream.codec),
    kind === "video" ? formatVideoRange(stream) : null,
    kind === "video" && stream.bitDepth ? `${stream.bitDepth}-bit` : null,
    kind === "audio" ? formatAudioSpatialFormat(stream) : null,
    kind === "audio" ? formatChannels(stream.channels) : null,
    kind === "subtitle" && stream.isForced ? "Forced" : null,
    kind === "subtitle" && stream.isHearingImpaired ? "SDH" : null,
    kind === "subtitle" && stream.isExternal ? "External" : null,
  ].filter((part): part is string => Boolean(part))
  const unique = parts.filter(
    (part, index) => parts.findIndex((candidate) => candidate.toLowerCase() === part.toLowerCase()) === index,
  )
  // A track with nothing recognisable still has whatever the server called it.
  return unique.length ? unique.join(" · ") : "Unknown"
}

/** Full selector label, including the server's default marker when present. */
export function describeTrackChoice(stream: MediaStream, kind: "audio" | "subtitle") {
  const label = describeStream(stream, kind)
  return stream.isDefault ? `${label} · Default` : label
}
