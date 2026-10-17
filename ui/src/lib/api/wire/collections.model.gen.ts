// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type ClassifiedTitle = {
  localItems: Array<LocalItem>,
  title: string,
  originalTitle: string | null,
  year: number | null,
  overview: string,
  releaseDate: string | null,
  sourceOrder: number,
  posterPath: string | null,
  backdropPath: string | null,
  adult: boolean,
  mediaType: MediaType,
  tmdbId: number,
};

export type CollectionMode =
  | "mediaFlick"
  | "jellyfin";

export type CollectionProfile = {
  id: string,
  revision: string,
  template: TemplateReference,
  title: string,
  description: string,
  customPosterId: string | null,
  source: CollectionSource,
  mediaType: MediaType,
  limit: ResultLimit,
  cadence: RefreshCadence,
  availableOnHome: boolean,
};

export type CollectionSource =
  | { "kind": "tmdbDiscover", parameters: { [key in string]: JsonValue } }
  | { "kind": "tmdbCollection", collectionId: number, includeUnreleased: boolean }
  | { "kind": "mdbListPublicList", listId: string };

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | Array<JsonValue>
  | { [key in string]: JsonValue };

export type LocalItem = {
  id: string,
  name: string,
  kind: string,
  played: boolean,
};

export type MediaType =
  | "movie"
  | "series"
  | "mixed";

export type NormalizedTitle = {
  title: string,
  originalTitle: string | null,
  year: number | null,
  overview: string,
  releaseDate: string | null,
  sourceOrder: number,
  posterPath: string | null,
  backdropPath: string | null,
  adult: boolean,
  mediaType: MediaType,
  tmdbId: number,
};

export type ProviderReadiness = {
  tmdb: boolean,
  mdblist: boolean,
};

export type ProviderResult = {
  items: Array<NormalizedTitle>,
  total: number,
  movies: number,
  series: number,
  sourceIdentity: string | null,
};

export type RefreshCadence =
  | "manual"
  | "daily"
  | "weekly"
  | "monthly";

export type ResultLimit =
  | { "kind": "all" }
  | { "kind": "maximum", count: number };

export type TemplateReference = {
  id: string,
};
