// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type ApiImagesRoutes = {
  "GET /api/image/{id}/{imageType}": { params: { id: string | number, imageType: string | number }, body: null, kind: "binary" },
  "GET /api/trickplay/{id}/{width}/{tile}": { params: { id: string | number, width: string | number, tile: string | number }, body: null, kind: "binary" },
};
