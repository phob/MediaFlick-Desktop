// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ItemSummary } from "./library.model.gen.ts"

export type ItemPage = {
  items: Array<ItemSummary>,
  total: number,
};

export type SortKey =
  | "name"
  | "year"
  | "added"
  | "episode-added"
  | "rating"
  | "played"
  | "plays"
  | "runtime"
  | "random";
