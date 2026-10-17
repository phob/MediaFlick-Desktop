// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { RatingSourceDefinition } from "./integrations.mdblist.schema.gen.ts"

export type EffectiveOrigin =
  | "plugin"
  | "none";

export type RatingOrigin = "plugin";

export type RatingsIntegrationStatus = {
  boundaryVersion: number,
  effectiveOrigin: EffectiveOrigin,
  available: boolean,
  selectionEnabled: boolean,
  plugin: RatingsPluginStatus,
  sources: Array<RatingSourceDefinition>,
  selectedSources: Array<string>,
};

export type RatingsPluginStatus = {
  available: boolean,
  capability: string,
  boundaryVersion: number,
  detail: string,
};
