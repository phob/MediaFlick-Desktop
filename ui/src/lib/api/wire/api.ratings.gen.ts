// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { RatingsIntegrationStatus } from "./integrations.mdblist.gen.ts"
import type { RatingsBatchResponse } from "./integrations.mdblist.cache.gen.ts"

export type RatingsBatchBody = {
  ids: Array<string>,
};

export type ApiRatingsRoutes = {
  "GET /api/integrations/ratings": { params: null, body: null, kind: "json", response: RatingsIntegrationStatus },
  "POST /api/ratings/batch": { params: null, body: RatingsBatchBody, kind: "json", response: RatingsBatchResponse },
};
