// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type HomeBuiltIn =
  | "watching"
  | "becauseYouWatched"
  | "recentlyAdded"
  | "recentlyAddedShows"
  | "upcoming"
  | "latestMovies"
  | "latestShows"
  | "myList";

export type HomeWatchingSettings = {
  continueWatching: boolean,
  nextUp: boolean,
  combine: boolean,
};
