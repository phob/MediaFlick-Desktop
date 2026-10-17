// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { CollectionMode, CollectionProfile, ProviderReadiness } from "./collections.model.gen.ts"
import type { CollectionTemplate, TemplateCategory } from "./collections.templates.gen.ts"
import type { FranchiseSettings } from "./preferences.collections.gen.ts"

export type CollectionProfilesIndex = {
  profiles: Array<CollectionProfile>,
  errors: { [key in string]: string },
};

export type CollectionRecovery = {
  damagedPath: string,
  restoredBackup: boolean,
};

export type CollectionSettings = {
  effectiveMode: CollectionMode,
  mediaFlickAvailable: boolean,
  modeSelection: CollectionMode | null,
  franchises: FranchiseSettings,
  recovery: CollectionRecovery | null,
  readiness: ProviderReadiness,
};

export type CollectionTemplateOption = {
  template: CollectionTemplate,
  /**
   * The provider the template reads from is ready.
   */
  available: boolean,
};

export type CollectionTemplates = {
  categories: Array<TemplateCategory>,
  templates: Array<CollectionTemplateOption>,
  readiness: ProviderReadiness,
};

export type CommittedProfile = {
  profile: CollectionProfile,
  total: number,
};

export type ProfileDeleted = {
  deleted: boolean,
};
