// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { CollectionMode } from "./collections.model.gen.ts"
import type { LibraryId } from "./library.libraries.gen.ts"
import type { HomeBuiltIn, HomeWatchingSettings } from "./preferences.accounts.gen.ts"

export type HomeConfiguration = {
  billboard: boolean,
  watching: HomeWatchingSettings,
  elements: Array<HomeElementView>,
};

export type HomeElementCategory =
  | "Built-in"
  | "Genre"
  | "My Collection"
  | "Library";

export type HomeElementView = { label: string, available: boolean, category: HomeElementCategory, enabled: boolean } & ({ "kind": "builtIn", id: HomeBuiltIn } | { "kind": "genre", id: string } | { "kind": "collection", id: string } | { "kind": "library", id: LibraryId });

export type HomeSettingsResponse = {
  settings: HomeConfiguration,
  defaults: HomeConfiguration,
  collectionMode: CollectionMode,
};
