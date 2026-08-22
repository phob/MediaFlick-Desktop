/**
 * Sort and watch-state controls for a collection page. Release order stays
 * the default — TMDB's part ordering is the canonical way a franchise reads —
 * and the watched filter only meaningfully applies to owned entries.
 */
export const COLLECTION_SORTS = [
  { id: "release", label: "Release order" },
  { id: "name", label: "Name" },
  { id: "rating", label: "Rating" },
] as const

export interface CollectionFilterState {
  sort: (typeof COLLECTION_SORTS)[number]["id"]
  /** "" means any; only URL-normalized values reach the state. */
  watched: string
}

export function readCollectionFilters(params: URLSearchParams): CollectionFilterState {
  const sort = params.get("sort")
  const watched = params.get("watched")
  return {
    sort: COLLECTION_SORTS.find((option) => option.id === sort)?.id ?? "release",
    watched: watched === "true" || watched === "false" ? watched : "",
  }
}

/** Applies a patch without discarding the rest of the route's query. */
export function writeCollectionFilters(
  previous: URLSearchParams,
  patch: Partial<CollectionFilterState>,
) {
  const next = new URLSearchParams(previous)
  for (const [key, value] of Object.entries(patch)) {
    if (value === undefined) continue
    if (value === "") next.delete(key)
    else next.set(key, String(value))
  }
  return next
}

/** One comparable row shared by native BoxSet children and derived parts. */
export interface CollectionEntry {
  name: string
  /** Release year, when either source knows one. */
  year: number | null
  rating: number | null
}

export function compareCollectionEntries(
  sort: CollectionFilterState["sort"],
): (a: CollectionEntry, b: CollectionEntry) => number {
  switch (sort) {
    case "name":
      return (a, b) => a.name.localeCompare(b.name)
    case "rating":
      // Unrated titles sink below rated ones instead of sorting as zero.
      return (a, b) => (b.rating ?? -1) - (a.rating ?? -1)
    case "release":
      // Ascending year, unknown years last. Derived TMDB parts skip this
      // comparator entirely: TMDB already delivers canonical release order.
      return (a, b) =>
        (a.year ?? Number.MAX_SAFE_INTEGER) - (b.year ?? Number.MAX_SAFE_INTEGER)
  }
}
