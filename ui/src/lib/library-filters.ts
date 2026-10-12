import type { ItemQuery, SortOrder } from "./api"

export type LibraryKind = "Movie" | "Series"

const BOTH_KINDS: readonly LibraryKind[] = ["Movie", "Series"]

export type LibrarySortId =
  | "name"
  | "year"
  | "added"
  | "episode-added"
  | "rating"
  | "played"
  | "plays"
  | "runtime"
  | "random"

export interface LibrarySort {
  id: LibrarySortId
  label: string
  seriesLabel?: string
  appliesTo: readonly LibraryKind[]
  natural: SortOrder
}

const NAME_SORT: LibrarySort = { id: "name", label: "Name", appliesTo: BOTH_KINDS, natural: "asc" }

export const LIBRARY_SORTS: readonly LibrarySort[] = [
  NAME_SORT,
  { id: "year", label: "Release date", appliesTo: BOTH_KINDS, natural: "desc" },
  { id: "added", label: "Date added", seriesLabel: "Date show added", appliesTo: BOTH_KINDS, natural: "desc" },
  { id: "episode-added", label: "Date episode added", appliesTo: ["Series"], natural: "desc" },
  { id: "rating", label: "Rating", appliesTo: BOTH_KINDS, natural: "desc" },
  { id: "played", label: "Date played", appliesTo: BOTH_KINDS, natural: "desc" },
  { id: "plays", label: "Play count", appliesTo: ["Movie"], natural: "desc" },
  { id: "runtime", label: "Runtime", appliesTo: ["Movie"], natural: "desc" },
  { id: "random", label: "Random", appliesTo: BOTH_KINDS, natural: "desc" },
]

export function librarySortsFor(kinds: readonly LibraryKind[]) {
  return LIBRARY_SORTS.filter((sort) => kinds.every((kind) => sort.appliesTo.includes(kind)))
}

export function librarySortLabel(sort: LibrarySort, kinds: readonly LibraryKind[]) {
  return kinds.length === 1 && kinds[0] === "Series" && sort.seriesLabel ? sort.seriesLabel : sort.label
}

function librarySort(id: string | null, kinds: readonly LibraryKind[]) {
  return librarySortsFor(kinds).find((sort) => sort.id === id) ?? NAME_SORT
}

export function newShuffleSeed() {
  return String(crypto.getRandomValues(new Uint32Array(1))[0])
}

export const REMEMBERED_LIBRARY_PARAMS = [
  "sort",
  "order",
  "seed",
  "genre",
  "decade",
  "watched",
  "filters",
] as const

export interface LibraryFilterState {
  sort: LibrarySortId
  order: SortOrder
  seed: string
  genre: string
  decade: string
  watched: WatchedFilter
  favorite: boolean
}

type WatchedFilter = "true" | "false" | ""

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

const MAX_SEED = 2 ** 32 - 1

function validSeed(value: string | null) {
  return value && /^\d{1,10}$/.test(value) && Number(value) <= MAX_SEED ? String(Number(value)) : ""
}

export function libraryViewKinds(params: URLSearchParams): LibraryKind[] {
  const kinds = libraryKind(params)
    .split(",")
    .map((kind) => kind.trim())
    .filter((kind): kind is LibraryKind => kind === "Movie" || kind === "Series")
  return kinds.length ? [...new Set(kinds)] : [...BOTH_KINDS]
}

export function readLibraryFilters(params: URLSearchParams): LibraryFilterState {
  const sort = librarySort(params.get("sort"), libraryViewKinds(params))
  const order = params.get("order")
  const watched = params.get("watched")
  return {
    sort: sort.id,
    order: order === "asc" || order === "desc" ? order : sort.natural,
    seed: sort.id === "random" ? validSeed(params.get("seed")) : "",
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
  if (patch.sort !== undefined && patch.order === undefined) next.delete("order")
  const sort = librarySort(next.get("sort"), libraryViewKinds(next))
  if (next.get("order") === sort.natural) next.delete("order")
  if (sort.id !== "random") next.delete("seed")
  next.delete("offset")
  next.delete("page")
  return next
}

export function libraryKind(params: URLSearchParams) {
  // Person mode is a mixed movie/series/episode Search view, not a catalogue tab.
  if (params.get("mode") === "person") return ""
  // The existing API accepts comma-separated kinds, and Home links use
  // `Movie,Series` for mixed genre pages. An explicit blank also historically
  // meant all kinds, so only an absent value receives the Movies default.
  if (params.has("kind")) return params.get("kind") ?? ""
  const favorite = params.get("favorite") === "true" || params.get("favorite") === "1"
  return params.get("search") || favorite ? "" : "Movie"
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
    order: filters.order === librarySort(filters.sort, libraryViewKinds(params)).natural
      ? undefined
      : filters.order,
    seed: filters.seed ? Number(filters.seed) : undefined,
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
