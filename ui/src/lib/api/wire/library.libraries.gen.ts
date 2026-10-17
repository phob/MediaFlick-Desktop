// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { SortKey } from "./library.gen.ts"

export type BrowseCapabilities = {
  sorts: Array<SortKey>,
  filters: Array<FilterKey>,
  letterIndex: boolean,
};

export type FilterKey =
  | "genre"
  | "decade"
  | "watched"
  | "parental"
  | "seriesStatus"
  | "favorite";

export type LibraryId = string;

export type LibraryKind =
  | "movies"
  | "shows"
  | "music"
  | "musicVideos"
  | "homeVideos"
  | "photos"
  | "books"
  | "boxSets"
  | "playlists"
  | "liveTv"
  | "trailers"
  | "mixed";
