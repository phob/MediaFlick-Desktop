import type { CatalogKind, ItemQuery } from "./api/types.ts"

/** Where a windowed grid reads its pages from. */
export type BrowseSource =
  | { scope: "titles"; query: ItemQuery }
  | { scope: "children"; itemId: string }

const CATALOG_KINDS: ReadonlySet<string> = new Set<CatalogKind>(["Movie", "Series", "Season", "Episode"])

export function isCatalogKind(kind: string): kind is CatalogKind {
  return CATALOG_KINDS.has(kind)
}

/** The cross-library title view a catalog item belongs to: seasons and episodes are part of their series. */
export function titleBrowseKind(kind: CatalogKind): "Movie" | "Series" {
  switch (kind) {
    case "Movie":
      return "Movie"
    case "Series":
    case "Season":
    case "Episode":
      return "Series"
  }
}

export function genreRoute(genre: string, kind: CatalogKind) {
  return `/library?kind=${titleBrowseKind(kind)}&genre=${encodeURIComponent(genre)}`
}
