// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { SkipSegment } from "./playback.segments.gen.ts"

export type ArtworkRef = {
  itemId: string,
  tag: string,
};

export type AudioDisplay = {
  itemId: string,
  title: string,
  artists: Array<string>,
  album: string | null,
  albumId: string | null,
  artwork: ArtworkRef | null,
  discNumber: number | null,
  trackNumber: number | null,
  runtimeTicks: number | null,
};

export type AudioKind =
  | "music"
  | "audiobook";

export type AudioNowPlaying = {
  mediaKind: AudioKind,
  current: AudioDisplay,
  next: AudioDisplay | null,
  playlistItemId: PlaylistItemId,
  index: number,
  length: number,
  canPrevious: boolean,
  canNext: boolean,
  phase: AudioPhase,
};

export type AudioPhase =
  | "preparing"
  | "loading"
  | "playing";

export type NowPlaying =
  | { "kind": "video" }
  | { "kind": "audio" } & AudioNowPlaying;

export type PlaybackDiagnostics = {
  bufferedUntilMs: number | null,
  buffering: boolean,
  droppedFrames: number | null,
  frameRate: number | null,
  decoderDroppedFrames: number | null,
  mistimedFrames: number | null,
  hwdecCurrent: string | null,
  hwdecInterop: string | null,
  currentVo: string | null,
  gpuContext: string | null,
  vsyncJitter: number | null,
  avsyncMs: number | null,
  videoPixelFormat: string | null,
  videoColormatrix: string | null,
  videoPrimaries: string | null,
  videoGamma: string | null,
  videoSigPeak: number | null,
  videoWidth: number | null,
  videoHeight: number | null,
  audioFormat: string | null,
  audioSampleRate: number | null,
  audioChannels: string | null,
};

export type PlayerChapter = {
  title: string,
  startMs: number,
};

export type PlayerSnapshot = {
  nowPlaying: NowPlaying | null,
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

export type PlaylistItemId = string;

export type StopReason =
  | "eof"
  | "watched-next"
  | "stop"
  | "quit"
  | "error"
  | "redirect"
  | "unknown";
