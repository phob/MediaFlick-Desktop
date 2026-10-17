// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ItemSummary } from "./library.model.gen.ts"
import type { StreamingQuality } from "./preferences.model.gen.ts"

export type PlayBody = {
  itemId: string,
  resume?: boolean,
  startTicks?: number,
  mediaSourceId?: string,
  mediaSourceIndex?: number,
  audioStreamIndex?: number,
  subtitleStreamIndex?: number,
  quality?: StreamingQuality,
};

export type PlayStarted = {
  started: boolean,
  itemId?: string,
  playMethod?: string,
  mediaSource?: string,
  startTicks?: number,
};

export type PlaybackItemBody = {
  itemId: string,
};

export type PlaybackNeighbors = {
  previous: ItemSummary | null,
  next: ItemSummary | null,
};

export type PlayerCommandAccepted = {
  accepted: boolean,
};

export type PlayerCommandBody =
  | { "command": "pause" }
  | { "command": "resume" }
  | { "command": "seek", positionMs: number }
  | { "command": "set-volume", volume: number }
  | { "command": "set-mute", mute: boolean }
  | { "command": "set-audio-delay", delaySeconds: number }
  | { "command": "set-subtitle-delay", delaySeconds: number }
  | { "command": "set-subtitle-scale", scale: number }
  | { "command": "set-video-fit", fit: VideoFitBody }
  | { "command": "set-video-aspect", aspect: VideoAspectBody }
  | { "command": "set-deinterlace", enabled: boolean }
  | { "command": "set-tone-mapping", mode: ToneMappingBody }
  | { "command": "set-audio-track", audioTrack: number }
  | { "command": "set-subtitle-track", subtitleTrack?: number | null }
  | { "command": "toggle-subtitles" }
  | { "command": "toggle-fullscreen" }
  | { "command": "stop" }
  | { "command": "mark-watched-next" };

export type SyncRequested = {
  requested: boolean,
};

export type ToneMappingBody =
  | "auto"
  | "clip"
  | "mobius"
  | "reinhard"
  | "hable"
  | "bt.2390";

export type VideoAspectBody =
  | "source"
  | "4:3"
  | "16:9"
  | "21:9";

export type VideoFitBody =
  | "fit"
  | "fill";
