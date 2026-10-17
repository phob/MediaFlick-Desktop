export interface PlayerTrack {
  id: number
  kind: "audio" | "subtitle"
  language: string | null
  title: string | null
  codec: string | null
  selected: boolean
  external: boolean
}

export interface PlayerChapter {
  title: string
  startMs: number
}

export interface PlayerSkipSegment {
  segmentType: "intro" | "outro" | "recap" | "commercial"
  startTicks: number
  endTicks: number
  triggered: boolean
}

export interface PlaybackDiagnostics {
  bufferedUntilMs: number | null
  buffering: boolean
  droppedFrames: number | null
  frameRate: number | null
}

export type StopReason =
  | "eof"
  | "watched-next"
  | "stop"
  | "quit"
  | "error"
  | "redirect"
  | "shutdown"
  | "unknown"

export interface PlayerState {
  active: boolean
  playbackId: number | null
  itemId: string | null
  mediaSourceId: string | null
  playSessionId: string | null
  playMethod: string | null
  positionMs: number
  durationMs: number | null
  paused: boolean
  volume: number | null
  mute: boolean | null
  tracks: PlayerTrack[]
  chapters: PlayerChapter[]
  skipSegments: PlayerSkipSegment[]
  diagnostics: PlaybackDiagnostics
  stopReason: StopReason | null
}

/** The `started: false` shape comes back when there is no next episode. */
export interface PlayStarted {
  started: boolean
  itemId?: string
  playMethod?: string
  mediaSource?: string
  startTicks?: number
}

export type PlayerCommand =
  | { command: "mark-watched-next" | "toggle-subtitles" }
  | { command: "pause" | "resume" | "stop" }
  | { command: "seek"; positionMs: number }
  | { command: "set-volume"; volume: number }
  | { command: "set-mute"; mute: boolean }
  | { command: "set-audio-delay"; delaySeconds: number }
  | { command: "set-subtitle-delay"; delaySeconds: number }
  | { command: "set-subtitle-scale"; scale: number }
  | { command: "set-video-fit"; fit: "fit" | "fill" }
  | { command: "set-video-aspect"; aspect: "source" | "4:3" | "16:9" | "21:9" }
  | { command: "set-deinterlace"; enabled: boolean }
  | {
      command: "set-tone-mapping"
      mode: "auto" | "clip" | "mobius" | "reinhard" | "hable" | "bt.2390"
    }
  | { command: "set-audio-track"; audioTrack: number }
  | { command: "set-subtitle-track"; subtitleTrack: number | null }
  | { command: "toggle-fullscreen" }
