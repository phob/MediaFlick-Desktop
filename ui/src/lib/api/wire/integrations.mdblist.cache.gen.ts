// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { EffectiveOrigin, RatingOrigin } from "./integrations.mdblist.gen.ts"
import type { NormalizedRating } from "./integrations.mdblist.schema.gen.ts"

export type ItemRatings = {
  id: string,
  ratings: Array<NormalizedRating>,
  origin: RatingOrigin,
  fetchedAt: number,
  sourceUpdatedAt: string | null,
  stale: boolean,
  schemaVersion: number,
};

export type RatingsBatchResponse = {
  available: boolean,
  effectiveOrigin: EffectiveOrigin,
  items: Array<ItemRatings>,
  retryAt: number | null,
  diagnostic: string | null,
};
