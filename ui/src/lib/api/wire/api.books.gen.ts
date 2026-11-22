// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { BookFormat, BookResume, ReadingLocation } from "./books.gen.ts"
import type { ReaderAppearance } from "./books.appearance.gen.ts"

export type BookManifest = {
  title: string,
  format: BookFormat,
  resume: BookResume,
  appearance: ReaderAppearance,
};

export type BookPositionSaved = {
  synced: boolean,
};

export type BookPositionWrite = {
  location: ReadingLocation,
  finished: boolean,
};

export type ApiBooksRoutes = {
  "GET /api/item/{id}/book": { params: { id: string | number }, body: null, kind: "json", response: BookManifest },
  "GET /api/item/{id}/book/file": { params: { id: string | number }, body: null, kind: "binary" },
  "PUT /api/item/{id}/book/position": { params: { id: string | number }, body: BookPositionWrite, kind: "json", response: BookPositionSaved },
  "PUT /api/reader/appearance": { params: null, body: ReaderAppearance, kind: "json", response: ReaderAppearance },
};
