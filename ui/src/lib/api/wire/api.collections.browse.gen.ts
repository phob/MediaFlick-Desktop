// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ClassifiedTitle, CollectionProfile, NormalizedTitle } from "./collections.model.gen.ts"
import type { RefreshState } from "./collections.snapshots.gen.ts"
import type { ItemSummary } from "./library.model.gen.ts"

export type CollectionProfileDetail = {
  profile: CollectionProfile,
  status: ResultsStatus,
  owned: Array<ClassifiedTitle>,
  missing: Array<NormalizedTitle>,
  items: Array<NormalizedTitle>,
  libraryItems: Array<ItemSummary>,
  /**
   * Sent only once results are ready.
   */
  ownershipAvailable?: boolean,
  refresh: RefreshState,
  /**
   * Sent only once results are ready.
   */
  overdue?: boolean,
};

export type CollectionTitle = {
  item: NormalizedTitle,
};

export type FranchiseCollection = {
  collectionId: number,
  name: string,
  posterPath: string | null,
  backdropPath: string | null,
  owned: Array<ClassifiedTitle>,
  missing: Array<NormalizedTitle>,
  items: Array<NormalizedTitle>,
  libraryItems: Array<ItemSummary>,
  ownershipAvailable: boolean,
};

export type FranchiseCollectionSummary = {
  collectionId: number,
  name: string,
  posterPath: string | null,
  backdropPath: string | null,
  ownedCount: number,
  missingCount: number,
  ownershipAvailable: boolean,
};

export type FranchiseCollectionsIndex = {
  status: ResultsStatus,
  franchises: Array<FranchiseCollectionSummary>,
};

export type MovieCollection = {
  tmdbId: number,
  collection: MovieFranchise | null,
};

export type MovieFranchise = {
  id: number,
  name: string,
};

export type ResultsStatus =
  | "updating"
  | "resultsUnavailable"
  | "ready";
