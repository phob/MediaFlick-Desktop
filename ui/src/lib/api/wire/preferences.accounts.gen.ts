// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { LibraryId } from "./library.libraries.gen.ts"

export type HomeBuiltIn =
  | "watching"
  | "becauseYouWatched"
  | "recentlyAdded"
  | "recentlyAddedShows"
  | "upcoming"
  | "latestMovies"
  | "latestShows"
  | "myList";

export type HomeElement = { enabled: boolean } & ({ "kind": "builtIn", id: HomeBuiltIn } | { "kind": "genre", id: string } | { "kind": "collection", id: string } | { "kind": "library", id: LibraryId });

export type HomeSettings = {
  billboard: boolean,
  watching: HomeWatchingSettings,
  elements: Array<HomeElement>,
};

export type HomeWatchingSettings = {
  continueWatching: boolean,
  nextUp: boolean,
  combine: boolean,
};
