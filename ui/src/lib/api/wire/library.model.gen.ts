// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { LibraryId } from "./library.libraries.gen.ts"

export type DetailView =
  | { "layout": "title" } & TitleDetail
  | { "layout": "generic" } & GenericItemDetail;

export type GenericItemDetail = {
  genres: Array<string>,
  isFolder: boolean,
  dateCreated: string | null,
  id: string,
  kind: string,
  name: string,
  year: number | null,
  runtimeTicks: number | null,
  communityRating: number | null,
  officialRating: string | null,
  seriesId: string | null,
  seriesName: string | null,
  indexNumber: number | null,
  parentIndexNumber: number | null,
  primaryImageTag: string | null,
  thumbImageTag: string | null,
  logoImageTag: string | null,
  backdropImageTag: string | null,
  childCount: number | null,
  premiereDate: string | null,
  seasonId: string | null,
  played: boolean,
  playCount: number,
  positionTicks: number,
  favorite: boolean,
  unplayedCount: number | null,
  /**
   * Width over height of the primary image. Null for cached movies and
   * shows, whose art is a 2:3 poster.
   */
  primaryAspect: number | null,
  /**
   * `/api/item/{id}/children` carries this for a season's episodes, from
   * its live server reconcile, and a generic item's details carry their
   * own. The grid queries do not, and offline rows have none.
   */
  overview?: string,
  /**
   * The slot this item fills among a playlist's children, counted from
   * zero. Jellyfin's PlaylistItemId is the item's own id, so it cannot tell
   * two slots of one item apart.
   */
  playlistIndex?: number,
};

export type ItemSummary = {
  id: string,
  kind: string,
  name: string,
  year: number | null,
  runtimeTicks: number | null,
  communityRating: number | null,
  officialRating: string | null,
  seriesId: string | null,
  seriesName: string | null,
  indexNumber: number | null,
  parentIndexNumber: number | null,
  primaryImageTag: string | null,
  thumbImageTag: string | null,
  logoImageTag: string | null,
  backdropImageTag: string | null,
  childCount: number | null,
  premiereDate: string | null,
  seasonId: string | null,
  played: boolean,
  playCount: number,
  positionTicks: number,
  favorite: boolean,
  unplayedCount: number | null,
  /**
   * Width over height of the primary image. Null for cached movies and
   * shows, whose art is a 2:3 poster.
   */
  primaryAspect: number | null,
  /**
   * `/api/item/{id}/children` carries this for a season's episodes, from
   * its live server reconcile, and a generic item's details carry their
   * own. The grid queries do not, and offline rows have none.
   */
  overview?: string,
  /**
   * The slot this item fills among a playlist's children, counted from
   * zero. Jellyfin's PlaylistItemId is the item's own id, so it cannot tell
   * two slots of one item apart.
   */
  playlistIndex?: number,
};

export type LibraryStats = {
  movies: number,
  series: number,
  seasons: number,
  episodes: number,
  total: number,
};

export type ProviderIds = {
  tmdb: string | null,
  imdb: string | null,
  tvdb: string | null,
};

export type ResolvedPlaybackPreference = {
  mediaSourceId: string | null,
  mediaSourceIndex: number,
  audioStreamIndex: number | null,
  /**
   * `None` is subtitles off. Playback translates it to Jellyfin/mpv's off
   * sentinel only when an existing stored row made that choice explicit.
   */
  subtitleStreamIndex: number | null,
};

export type TitleDetail = {
  /**
   * Always a catalog library, whose profile offers the genre filter: only
   * catalog libraries hold memberships. The UI's genre links rely on it.
   */
  libraryId: LibraryId | null,
  genres: Array<string>,
  originalTitle: string | null,
  providerIds: ProviderIds,
  parentId: string | null,
  dateCreated: string | null,
  id: string,
  kind: string,
  name: string,
  year: number | null,
  runtimeTicks: number | null,
  communityRating: number | null,
  officialRating: string | null,
  seriesId: string | null,
  seriesName: string | null,
  indexNumber: number | null,
  parentIndexNumber: number | null,
  primaryImageTag: string | null,
  thumbImageTag: string | null,
  logoImageTag: string | null,
  backdropImageTag: string | null,
  childCount: number | null,
  premiereDate: string | null,
  seasonId: string | null,
  played: boolean,
  playCount: number,
  positionTicks: number,
  favorite: boolean,
  unplayedCount: number | null,
  /**
   * Width over height of the primary image. Null for cached movies and
   * shows, whose art is a 2:3 poster.
   */
  primaryAspect: number | null,
  /**
   * `/api/item/{id}/children` carries this for a season's episodes, from
   * its live server reconcile, and a generic item's details carry their
   * own. The grid queries do not, and offline rows have none.
   */
  overview?: string,
  /**
   * The slot this item fills among a playlist's children, counted from
   * zero. Jellyfin's PlaylistItemId is the item's own id, so it cannot tell
   * two slots of one item apart.
   */
  playlistIndex?: number,
};
