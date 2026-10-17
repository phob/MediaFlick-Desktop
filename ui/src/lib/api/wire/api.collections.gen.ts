// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ItemSummary } from "./library.model.gen.ts"

export type JellyfinCollectionDetail = {
  id: string,
  name: string,
  primaryImageTag: string | null,
  backdropImageTag: string | null,
  items: Array<ItemSummary>,
  totalRecordCount: number,
};

export type JellyfinCollectionSummary = {
  id: string,
  name: string,
  primaryImageTag: string | null,
  backdropImageTag: string | null,
  itemCount: number | null,
};

export type JellyfinCollections = {
  collections: Array<JellyfinCollectionSummary>,
};

export type LocalAccountDeletionBody = {
  confirmed: boolean,
};

export type PublicListSearchBody = {
  query: string,
};

export type PublicListValidateBody = {
  selector: string,
};

export type UploadedArtwork = {
  id: string,
};
