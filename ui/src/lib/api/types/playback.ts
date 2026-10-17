import type * as Wire from "../wire.gen.ts"

export type PlayerTrack = Wire.PlayerTrack

export type PlayerChapter = Wire.PlayerChapter

export type PlayerSkipSegment = Wire.SkipSegment

export type PlaybackDiagnostics = Wire.PlaybackDiagnostics

export type StopReason = Wire.StopReason

export type PlayerState = Wire.PlayerSnapshot

/** The `started: false` shape comes back when there is no next episode. */
export interface PlayStarted {
  started: boolean
  itemId?: string
  playMethod?: string
  mediaSource?: string
  startTicks?: number
}

export type PlayerCommand = Wire.PlayerCommandBody
