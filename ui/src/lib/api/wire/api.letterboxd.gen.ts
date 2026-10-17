// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ExternalProfile } from "./integrations.letterboxd.gen.ts"

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
