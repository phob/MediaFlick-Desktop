// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ItemSummary, ResolvedPlaybackPreference } from "./library.model.gen.ts"

export type ExternalLinkBody = {
  provider: string,
};

export type ExternalLinkOpened = {
  opened: boolean,
  url: string,
};

export type FavoriteBody = {
  favorite: boolean,
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

export type PlaybackTrackPreferenceWrite = {
  mediaSourceIndex: number,
  mediaSourceId: string | null,
  audioStreamIndex: number | null,
  subtitleStreamIndex: number | null,
};

export type PlayedBody = {
  played: boolean,
};

export type PlayedState = {
  played: boolean,
};

export type TechnicalBatch = {
  items: Array<ItemTechnical>,
};

export type TechnicalBatchBody = {
  ids: Array<string>,
};

export type TrailerResponse = {
  trailer: TrailerSummary | null,
};

export type TrailerSummary = {
  id: string | null,
  name: string,
  embedUrl: string | null,
};

export type ApiMediaRoutes = {
  "POST /api/technical/batch": { params: null, body: TechnicalBatchBody, kind: "json", response: TechnicalBatch },
  "GET /api/item/{id}/media": { params: { id: string | number }, body: null, kind: "json", response: MediaInfoResponse },
  "PATCH /api/item/{id}/playback-preference": { params: { id: string | number }, body: PlaybackTrackPreferenceWrite, kind: "json", response: PlaybackPreferenceSaved },
  "GET /api/item/{id}/trailer": { params: { id: string | number }, body: null, kind: "json", response: TrailerResponse },
  "GET /api/item/{id}/nextup": { params: { id: string | number }, body: null, kind: "json", response: NextUpResponse },
  "POST /api/item/{id}/external": { params: { id: string | number }, body: ExternalLinkBody, kind: "json", response: ExternalLinkOpened },
  "POST /api/item/{id}/played": { params: { id: string | number }, body: PlayedBody, kind: "json", response: PlayedState },
  "POST /api/item/{id}/favorite": { params: { id: string | number }, body: FavoriteBody, kind: "json", response: FavoriteState },
  "GET /api/trailer/{id}/stream": { params: { id: string | number }, body: null, kind: "binary" },
};
