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

export type ApiSeerrRoutes = {
  "GET /api/seerr/status": { params: null, body: null, kind: "relay" },
  "GET /api/seerr/search": { params: null, body: null, kind: "relay" },
  "GET /api/seerr/person/{tmdbId}/credits": { params: { tmdbId: string | number }, body: null, kind: "relay" },
  "GET /api/seerr/discover/{kind}": { params: { kind: string | number }, body: null, kind: "relay" },
  "GET /api/seerr/genres/{mediaType}": { params: { mediaType: string | number }, body: null, kind: "relay" },
  "GET /api/seerr/media/{mediaType}/{tmdbId}": { params: { mediaType: string | number, tmdbId: string | number }, body: null, kind: "relay" },
  "GET /api/seerr/request-options/{mediaType}": { params: { mediaType: string | number }, body: null, kind: "relay" },
  "POST /api/seerr/request": { params: null, body: SeerrRequestBody, kind: "relay" },
  "GET /api/seerr/requests": { params: null, body: null, kind: "relay" },
  "DELETE /api/seerr/request/{id}": { params: { id: string | number }, body: null, kind: "json", response: SeerrRequestCancelled },
  "GET /api/seerr/image/{size}/{file}": { params: { size: string | number, file: string | number }, body: null, kind: "binary" },
};
