export interface ItemTechnical {
  id: string
  mediaStreams: MediaStream[]
}

export interface MediaStream {
  index: number
  /** Present on cached item summaries; detail sources already group by type. */
  type?: string | null
  codec: string | null
  profile?: string | null
  language: string | null
  title: string | null
  displayTitle: string | null
  width: number | null
  height: number | null
  channels: number | null
  audioSpatialFormat?: string | null
  videoRange: string | null
  videoRangeType: string | null
  bitDepth: number | null
  isDefault: boolean
  isForced: boolean
  isHearingImpaired: boolean
  isExternal: boolean
}

export interface MediaSource {
  id: string | null
  name: string
  container: string | null
  fileName: string | null
  size: number | null
  bitrate: number | null
  defaultAudioStreamIndex: number | null
  defaultSubtitleStreamIndex: number | null
  video: MediaStream[]
  audio: MediaStream[]
  subtitles: MediaStream[]
}

export interface PlaybackTrackPreference {
  mediaSourceId: string | null
  mediaSourceIndex: number
  audioStreamIndex: number | null
  /** `null` means subtitles off. */
  subtitleStreamIndex: number | null
}

export interface MediaInfoResponse {
  sources: MediaSource[]
  playbackPreference: PlaybackTrackPreference | null
}

export type PlaybackTrackPreferenceWrite = PlaybackTrackPreference

export interface TrailerSummary {
  id: string | null
  name: string
  embedUrl: string | null
}
