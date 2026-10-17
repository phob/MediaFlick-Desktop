// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ItemSummary, ResolvedPlaybackPreference } from "./library.model.gen.ts"

export type ExternalLinkOpened = {
  opened: boolean,
  url: string,
};

export type FavoriteState = {
  favorite: boolean,
};

export type ItemTechnical = {
  id: string,
  mediaStreams: Array<MediaStreamView>,
};

export type MediaInfoResponse = {
  sources: Array<MediaSourceView>,
  playbackPreference: ResolvedPlaybackPreference | null,
};

export type MediaSourceView = {
  id: string | null,
  name: string,
  container: string | null,
  fileName: string | null,
  size: number | null,
  bitrate: number | null,
  defaultAudioStreamIndex: number | null,
  defaultSubtitleStreamIndex: number | null,
  video: Array<MediaStreamView>,
  audio: Array<MediaStreamView>,
  subtitles: Array<MediaStreamView>,
};

export type MediaStreamView = {
  index: number,
  type: string | null,
  codec: string | null,
  profile: string | null,
  language: string | null,
  title: string | null,
  displayTitle: string | null,
  width: number | null,
  height: number | null,
  channels: number | null,
  audioSpatialFormat: string | null,
  videoRange: string | null,
  videoRangeType: string | null,
  bitDepth: number | null,
  isDefault: boolean,
  isForced: boolean,
  isHearingImpaired: boolean,
  isExternal: boolean,
};

export type NextUpResponse = {
  item: ItemSummary | null,
};

export type PlaybackPreferenceSaved = {
  playbackPreference: ResolvedPlaybackPreference | null,
};

export type PlayedState = {
  played: boolean,
};

export type TechnicalBatch = {
  items: Array<ItemTechnical>,
};

export type TrailerResponse = {
  trailer: TrailerSummary | null,
};

export type TrailerSummary = {
  id: string | null,
  name: string,
  embedUrl: string | null,
};
