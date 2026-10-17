// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { HomeConfiguration } from "./api.catalog.home_settings.gen.ts"
import type { ItemSummary } from "./library.model.gen.ts"
import type { HomeBuiltIn } from "./preferences.accounts.gen.ts"

export type Billboard = {
  items: Array<ItemSummary>,
};

export type HomeResponse = {
  configuration: HomeConfiguration,
  continueWatching: Array<ItemSummary>,
  rows: Array<HomeRow>,
};

export type HomeResumeResponse = {
  continueWatching: Array<ItemSummary>,
  nextUp: Array<ItemSummary>,
};

export type HomeRow = { title: string, items: Array<ItemSummary> } & ({ "kind": "builtIn", id: HomeBuiltIn } | { "kind": "genre", id: string } | { "kind": "collection", id: string });
