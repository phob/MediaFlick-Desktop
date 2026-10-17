// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { SkipSegment } from "./playback.segments.gen.ts"

export type PlaybackDiagnostics = {
  bufferedUntilMs: number | null,
  buffering: boolean,
  droppedFrames: number | null,
  frameRate: number | null,
};

export type PlayerChapter = {
  title: string,
  startMs: number,
};

export type PlayerSnapshot = {
  active: boolean,
  playbackId: number | null,
  itemId: string | null,
  mediaSourceId: string | null,
  playSessionId: string | null,
  playMethod: string | null,
  positionMs: number,
  durationMs: number | null,
  paused: boolean,
  volume: number | null,
  mute: boolean | null,
  tracks: Array<PlayerTrack>,
  chapters: Array<PlayerChapter>,
  skipSegments: Array<SkipSegment>,
  diagnostics: PlaybackDiagnostics,
  stopReason: StopReason | null,
};

export type PlayerTrack = {
  id: number,
  kind: PlayerTrackKind,
  language: string | null,
  title: string | null,
  codec: string | null,
  selected: boolean,
  external: boolean,
};

export type PlayerTrackKind =
  | "audio"
  | "subtitle";

export type StopReason =
  | "eof"
  | "watched-next"
  | "stop"
  | "quit"
  | "error"
  | "redirect"
  | "shutdown"
  | "unknown";
