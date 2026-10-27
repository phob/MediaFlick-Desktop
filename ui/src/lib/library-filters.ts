import type {
  BrowseCapabilities,
  ItemQuery,
  LibraryFilterId,
  LibraryQuery,
  LibrarySortId,
  SeriesStatus,
  SortOrder,
  WatchFilter,
} from "./api"
import { isTitleKind } from "./kinds.ts"

export interface LibrarySort {
  id: LibrarySortId
  label: string
  showsLabel?: string
  natural: SortOrder
}

const LIBRARY_SORTS: Record<LibrarySortId, LibrarySort> = {
  name: { id: "name", label: "Name", natural: "asc" },
  year: { id: "year", label: "Release date", natural: "desc" },
  added: { id: "added", label: "Date added", showsLabel: "Date show added", natural: "desc" },
  "episode-added": { id: "episode-added", label: "Date episode added", natural: "desc" },
  rating: { id: "rating", label: "Rating", natural: "desc" },
  played: { id: "played", label: "Date played", natural: "desc" },
  plays: { id: "plays", label: "Play count", natural: "desc" },
  runtime: { id: "runtime", label: "Runtime", natural: "desc" },
  random: { id: "random", label: "Random", natural: "desc" },
}

export function sortsFor(capabilities: BrowseCapabilities): LibrarySort[] {
  return capabilities.sorts.map((id) => LIBRARY_SORTS[id])
}

export function librarySortLabel(sort: LibrarySort, tvShowsLibrary: boolean) {
  return tvShowsLibrary && sort.showsLabel ? sort.showsLabel : sort.label
}

function librarySort(id: string | null, capabilities: BrowseCapabilities) {
  const sorts = sortsFor(capabilities)
  return sorts.find((sort) => sort.id === id) ?? sorts[0] ?? LIBRARY_SORTS.name
}

export function offersFilter(capabilities: BrowseCapabilities, filter: LibraryFilterId) {
  return capabilities.filters.includes(filter)
}

export function newShuffleSeed() {
  return String(crypto.getRandomValues(new Uint32Array(1))[0])
}

export const WATCH_FILTERS: readonly { id: WatchFilter; label: string }[] = [
  { id: "unwatched", label: "Unwatched" },
  { id: "progress", label: "In progress" },
  { id: "watched", label: "Watched" },
]

export const SERIES_STATUSES: readonly SeriesStatus[] = ["Continuing", "Ended"]

export const REMEMBERED_LIBRARY_PARAMS = [
  "sort",
  "order",
  "seed",
  "genre",
  "decade",
  "watched",
  "parental",
  "seriesStatus",
  "favorite",
  "filters",
] as const

export interface LibraryFilterState {
  sort: LibrarySortId
  order: SortOrder
  seed: string
  genre: string
  decade: string
  watched: WatchFilter | ""
  parental: string
  seriesStatus: SeriesStatus | ""
  favorite: boolean
}

export interface ReleaseDecadeOption {
  value: number
  label: string
}

function releaseDecades(currentYear = new Date().getUTCFullYear()): ReleaseDecadeOption[] {
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

const LIBRARY_FILTERS: readonly LibraryFilterId[] = [
  "genre",
  "decade",
  "watched",
  "parental",
  "seriesStatus",
  "favorite",
]

export function readLibraryFilters(
  params: URLSearchParams,
  capabilities: BrowseCapabilities,
): LibraryFilterState {
  const offered = (filter: LibraryFilterId) => offersFilter(capabilities, filter)
  const sort = librarySort(params.get("sort"), capabilities)
  const order = params.get("order")
  const watched = WATCH_FILTERS.find((option) => option.id === params.get("watched"))
  const seriesStatus = SERIES_STATUSES.find((status) => status === params.get("seriesStatus"))
  const favorite = params.get("favorite") === "true" || params.get("favorite") === "1"
  return {
    sort: sort.id,
    order: order === "asc" || order === "desc" ? order : sort.natural,
    seed: sort.id === "random" ? validSeed(params.get("seed")) : "",
    genre: offered("genre") ? params.get("genre") ?? "" : "",
    decade: offered("decade") ? validDecade(params.get("decade")) : "",
    watched: offered("watched") ? watched?.id ?? "" : "",
    parental: offered("parental") ? params.get("parental")?.trim() ?? "" : "",
    seriesStatus: offered("seriesStatus") ? seriesStatus ?? "" : "",
    favorite: offered("favorite") && favorite,
  }
}

/** Applies a filter patch without discarding the route's search or media kind. */
export function writeLibraryFilters(
  previous: URLSearchParams,
  patch: Partial<LibraryFilterState>,
  capabilities: BrowseCapabilities,
) {
  const next = new URLSearchParams(previous)
  for (const [key, value] of Object.entries(patch)) {
    if (value === undefined) continue
    if (value === true) next.set(key, "true")
    else if (value === false || value === "") next.delete(key)
    else next.set(key, String(value))
  }
  if (patch.sort !== undefined && patch.order === undefined) next.delete("order")
  const sort = librarySort(next.get("sort"), capabilities)
  if (next.get("order") === sort.natural) next.delete("order")
  for (const filter of LIBRARY_FILTERS) {
    if (!offersFilter(capabilities, filter)) next.delete(filter)
  }
  if (sort.id !== "random") next.delete("seed")
  return next
}

export function libraryQuery(params: URLSearchParams, capabilities: BrowseCapabilities): LibraryQuery {
  const filters = readLibraryFilters(params, capabilities)
  return {
    favorite: filters.favorite || undefined,
    genre: filters.genre,
    decade: filters.decade ? Number(filters.decade) : undefined,
    sort: filters.sort,
    order: filters.order === librarySort(filters.sort, capabilities).natural
      ? undefined
      : filters.order,
    seed: filters.seed ? Number(filters.seed) : undefined,
    watched: filters.watched,
    parental: filters.parental,
    seriesStatus: filters.seriesStatus,
  }
}

export function libraryItemQuery(params: URLSearchParams, capabilities: BrowseCapabilities): ItemQuery {
  const kinds = (params.get("kind") ?? "")
    .split(",")
    .map((kind) => kind.trim())
    .filter(isTitleKind)
  return {
    search: params.get("search") ?? "",
    kind: [...new Set(kinds)],
    ...libraryQuery(params, capabilities),
  }
}

export function activeLibraryFilterCount(filters: LibraryFilterState) {
  return (
    Number(Boolean(filters.genre))
    + Number(Boolean(filters.decade))
    + Number(Boolean(filters.watched))
    + Number(Boolean(filters.parental))
    + Number(Boolean(filters.seriesStatus))
    + Number(filters.favorite)
  )
}

export const CLEARED_LIBRARY_FILTERS: Pick<
  LibraryFilterState,
  "genre" | "decade" | "watched" | "parental" | "seriesStatus" | "favorite"
> = {
  genre: "",
  decade: "",
  watched: "",
  parental: "",
  seriesStatus: "",
  favorite: false,
}
