// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type SeerrRequestBody = {
  mediaType: string,
  tmdbId: number,
  /**
   * An explicit list means "these seasons"; its absence means "the whole
   * show", which Seerr expands to whatever it does not already have.
   */
  seasons?: Array<number>,
  is4k?: boolean,
  serverId?: number,
  profileId?: number,
};

export type SeerrRequestCancelled = {
  cancelled: boolean,
  id: number,
};
