import { CATALOG_KINDS, type CatalogKind, type ItemQuery } from "./api/types.ts"

export type BrowseSource =
  | { scope: "titles"; query: ItemQuery }
  | { scope: "children"; itemId: string }

const catalogKinds: ReadonlySet<string> = new Set(CATALOG_KINDS)

export function isCatalogKind(kind: string): kind is CatalogKind {
  return catalogKinds.has(kind)
}

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
