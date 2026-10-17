// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ExternalLinkOpened } from "./api.media.gen.ts"
import type { ExternalProfile, ReviewLookup } from "./integrations.letterboxd.gen.ts"

export type LetterboxdEnabledBody = {
  enabled: boolean,
};

export type LetterboxdProfileBody = {
  profile: string,
};

export type LetterboxdProfileRemoved = {
  removed: boolean,
};

export type LetterboxdProfileSaved = {
  profile: ExternalProfile,
};

export type LetterboxdProfiles = {
  profiles: Array<ExternalProfile>,
};

export type ApiLetterboxdRoutes = {
  "GET /api/integrations/letterboxd": { params: null, body: null, kind: "json", response: LetterboxdProfiles },
  "POST /api/integrations/letterboxd": { params: null, body: LetterboxdProfileBody, kind: "json", response: LetterboxdProfileSaved },
  "PATCH /api/integrations/letterboxd/{id}": { params: { id: string | number }, body: LetterboxdEnabledBody, kind: "json", response: LetterboxdProfileSaved },
  "DELETE /api/integrations/letterboxd/{id}": { params: { id: string | number }, body: null, kind: "json", response: LetterboxdProfileRemoved },
  "POST /api/integrations/letterboxd/{id}/refresh": { params: { id: string | number }, body: null, kind: "json", response: LetterboxdProfileSaved },
  "POST /api/integrations/letterboxd/{id}/open": { params: { id: string | number }, body: null, kind: "json", response: ExternalLinkOpened },
  "GET /api/letterboxd/movie/{tmdbId}": { params: { tmdbId: string | number }, body: null, kind: "json", response: ReviewLookup },
  "GET /api/item/{id}/letterboxd": { params: { id: string | number }, body: null, kind: "json", response: ReviewLookup },
};
