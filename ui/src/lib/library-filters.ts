import type { ItemQuery } from "./api"

export const LIBRARY_SORTS = [
  { id: "name", label: "Name" },
  { id: "year", label: "Year" },
  { id: "added", label: "Recently added" },
  { id: "rating", label: "Rating" },
] as const

export type LibraryKind = "Movie" | "Series"
export type WatchedFilter = "true" | "false" | ""

export interface LibraryFilterState {
  sort: string
  genre: string
  decade: string
  watched: WatchedFilter
  favorite: boolean
}

export interface ReleaseDecadeOption {
  value: number
  label: string
}

/** Standard release decades, newest first, shared by both local catalogues. */
export function releaseDecades(currentYear = new Date().getUTCFullYear()): ReleaseDecadeOption[] {
  const currentDecade = Math.floor(currentYear / 10) * 10
  const decades: ReleaseDecadeOption[] = []
  for (let decade = currentDecade; decade >= 1900; decade -= 10) {
    decades.push({ value: decade, label: `${decade}s` })
  }
  return decades
}

export const RELEASE_DECADES = releaseDecades()

function validDecade(value: string | null) {
  return value && RELEASE_DECADES.some((option) => String(option.value) === value) ? value : ""
}

export function readLibraryFilters(params: URLSearchParams): LibraryFilterState {
  const sort = params.get("sort") ?? "name"
  const watched = params.get("watched")
  return {
    sort: LIBRARY_SORTS.some((option) => option.id === sort) ? sort : "name",
    genre: params.get("genre") ?? "",
    decade: validDecade(params.get("decade")),
    watched: watched === "true" || watched === "false" ? watched : "",
    favorite: params.get("favorite") === "true" || params.get("favorite") === "1",
  }
}

/**
 * Applies a filter patch without discarding the route's search or media kind.
 * Paging belongs to the virtual grid, but old/deep links can still carry these
 * values; a filter change always returns to the first result page.
 */
export function writeLibraryFilters(
  previous: URLSearchParams,
  patch: Partial<LibraryFilterState>,
) {
  const next = new URLSearchParams(previous)
  for (const [key, value] of Object.entries(patch)) {
    if (value === undefined) continue
    if (value === true) next.set(key, "true")
    else if (value === false || value === "") next.delete(key)
    else next.set(key, String(value))
  }
  next.delete("offset")
  next.delete("page")
  return next
}

export function libraryKind(params: URLSearchParams) {
  // The existing API accepts comma-separated kinds, and Home links use
  // `Movie,Series` for mixed genre pages. An explicit blank also historically
  // meant all kinds, so only an absent value receives the Movies default.
  if (params.has("kind")) return params.get("kind") ?? ""
  const filters = readLibraryFilters(params)
  return params.get("search") || filters.favorite ? "" : "Movie"
}

export function libraryKindPath(kind: LibraryKind) {
  return `/library?kind=${kind}`
}

export function libraryItemQuery(params: URLSearchParams): ItemQuery {
  const filters = readLibraryFilters(params)
  return {
    search: params.get("search") ?? "",
    kind: libraryKind(params),
    favorite: filters.favorite || undefined,
    genre: filters.genre,
    decade: filters.decade ? Number(filters.decade) : undefined,
    sort: filters.sort,
    watched: filters.watched,
  }
}

export function activeLibraryFilterCount(filters: LibraryFilterState) {
  return (
    Number(Boolean(filters.genre))
    + Number(Boolean(filters.decade))
    + Number(Boolean(filters.watched))
    + Number(filters.favorite)
  )
}

export const CLEARED_LIBRARY_FILTERS: Pick<
  LibraryFilterState,
  "genre" | "decade" | "watched" | "favorite"
> = {
  genre: "",
  decade: "",
  watched: "",
  favorite: false,
}
