// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ItemLetters } from "./api.catalog.gen.ts"
import type { ItemPage, SortKey } from "./library.gen.ts"
import type { FilterOptions } from "./library.browse.gen.ts"
import type { BrowseCapabilities, LibraryId, LibraryKind } from "./library.libraries.gen.ts"
import type { ItemSummary } from "./library.model.gen.ts"

export type LibrariesResponse = {
  /**
   * False until a sync cycle has read the server's library list.
   */
  known: boolean,
  /**
   * In the user's Jellyfin order.
   */
  libraries: Array<LibraryView>,
  crossLibrary: BrowseCapabilities,
};

export type LibraryLatest = {
  items: Array<ItemSummary>,
  sort: SortKey,
};

export type LibraryView = {
  id: LibraryId,
  name: string,
  kind: LibraryKind,
  imageTag: string | null,
  /**
   * True while a movies or TV shows library's titles are not known yet:
   * during the first catalog download, and until the library's first
   * membership walk lands. A later walk keeps the stored titles, so it
   * never sets this.
   */
  membershipPending: boolean,
  browse: BrowseCapabilities,
};

export type ApiLibrariesRoutes = {
  "GET /api/libraries": { params: null, body: null, kind: "json", response: LibrariesResponse },
  "GET /api/libraries/{id}/items": { params: { id: string | number }, body: null, kind: "json", response: ItemPage },
  "GET /api/libraries/{id}/letters": { params: { id: string | number }, body: null, kind: "json", response: ItemLetters },
  "GET /api/libraries/{id}/options": { params: { id: string | number }, body: null, kind: "json", response: FilterOptions },
  "GET /api/libraries/{id}/latest": { params: { id: string | number }, body: null, kind: "json", response: LibraryLatest },
};
