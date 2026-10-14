import type {
  SeerrDiscoverFilters,
  SeerrDiscoverRow,
  SeerrResult,
  SeerrReleaseDecade,
} from "./api"

export type DiscoveryAvailability = "all" | "outside" | "library"

export const DISCOVERY_FILTER_KEYS = [
  "genre",
  "sort",
  "minRating",
  "decade",
  "mediaType",
  "timeWindow",
] as const

const CURRENT_YEAR = new Date().getUTCFullYear()
const CURRENT_DECADE = Math.floor(CURRENT_YEAR / 10) * 10

function releaseDecades(firstDecade: number): readonly {
  value: SeerrReleaseDecade
  label: string
}[] {
  const options = []
  for (let decade = CURRENT_DECADE; decade >= firstDecade; decade -= 10) {
    options.push({
      value: decade,
      label: `${decade}s`,
    })
  }
  return options
}

export const RELEASE_DECADES = {
  movie: releaseDecades(1900),
  tv: releaseDecades(1900),
}

/**
 * One immutable identity for everything that can change a discovery wall.
 *
 * TanStack hashes objects stably, but making this boundary a scalar also lets
 * React key the whole result subtree to the exact same identity. In
 * particular, the local library filter must be represented even though it is
 * applied after the Seerr response has been joined to the local library.
 */
export function discoveryResultSetKey(
  row: SeerrDiscoverRow,
  filters: SeerrDiscoverFilters,
  availability: DiscoveryAvailability = "all",
) {
  return [
    `row=${row}`,
    `genre=${filters.genre ?? ""}`,
    `decade=${filters.decade ?? ""}`,
    `sort=${filters.sort ?? ""}`,
    `rating=${filters.minRating ?? ""}`,
    `media=${filters.mediaType ?? ""}`,
    `window=${filters.timeWindow ?? ""}`,
    `library=${availability}`,
  ].join("&")
}

export function discoveryCardKey(resultSetKey: string, result: SeerrResult) {
  return `${resultSetKey}:${result.mediaType}-${result.tmdbId}`
}

/**
 * Flatten only pages fetched for the currently rendered result set. The query
 * key already isolates these in normal operation; retaining the identity on
 * each page makes that invariant explicit at the final infinite-page join and
 * also removes an upstream duplicate if a moving catalogue straddles pages.
 */
export function discoveryResultsForSet(
  resultSetKey: string,
  pages: readonly { resultSetKey: string; results: SeerrResult[] }[] | undefined,
) {
  if (!pages) return undefined

  const seen = new Set<string>()
  return pages.flatMap((page) => {
    if (page.resultSetKey !== resultSetKey) return []
    return page.results.filter((result) => {
      const key = `${result.mediaType}-${result.tmdbId}`
      if (seen.has(key)) return false
      seen.add(key)
      return true
    })
  })
}

export function defaultDiscoveryFilters(
  row: SeerrDiscoverRow,
  advancedDiscovery = true,
): SeerrDiscoverFilters {
  if (!advancedDiscovery) return {}
  if (row === "trending") return { mediaType: "all", timeWindow: "day" }
  if (row === "movies" || row === "tv") return { sort: "popular" }
  return {}
}

function positiveInteger(value: string | null) {
  if (!value) return undefined
  const parsed = Number(value)
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : undefined
}

export function readDiscoveryFilters(
  params: URLSearchParams,
  row: SeerrDiscoverRow,
  advancedDiscovery: boolean,
  decadeDiscovery: boolean,
): SeerrDiscoverFilters {
  const filters = defaultDiscoveryFilters(row, advancedDiscovery)
  if (!advancedDiscovery) return filters

  if (row === "trending") {
    const mediaType = params.get("mediaType")
    if (mediaType === "all" || mediaType === "movie" || mediaType === "tv") {
      filters.mediaType = mediaType
    }
    const timeWindow = params.get("timeWindow")
    if (timeWindow === "day" || timeWindow === "week") filters.timeWindow = timeWindow
    return filters
  }

  if (row !== "movies" && row !== "tv") return filters

  const genre = positiveInteger(params.get("genre"))
  if (genre) filters.genre = genre
  const sort = params.get("sort")
  if (sort === "popular" || sort === "rating" || sort === "newest") filters.sort = sort
  const minRating = positiveInteger(params.get("minRating"))
  if (minRating === 6 || minRating === 7 || minRating === 8) filters.minRating = minRating

  const decade = positiveInteger(params.get("decade"))
  const mediaType = row === "movies" ? "movie" : "tv"
  const selectedDecade = decadeDiscovery
    ? RELEASE_DECADES[mediaType].find((option) => option.value === decade)
    : undefined
  if (selectedDecade) filters.decade = selectedDecade.value
  return filters
}

export function writeDiscoveryFilters(
  params: URLSearchParams,
  row: SeerrDiscoverRow,
  filters: SeerrDiscoverFilters,
) {
  const next = new URLSearchParams(params)
  for (const key of DISCOVERY_FILTER_KEYS) next.delete(key)

  if (row === "trending") {
    if (filters.mediaType && filters.mediaType !== "all") {
      next.set("mediaType", filters.mediaType)
    }
    if (filters.timeWindow && filters.timeWindow !== "day") {
      next.set("timeWindow", filters.timeWindow)
    }
    return next
  }

  if (row !== "movies" && row !== "tv") return next
  if (filters.genre) next.set("genre", String(filters.genre))
  if (filters.sort && filters.sort !== "popular") next.set("sort", filters.sort)
  if (filters.minRating) next.set("minRating", String(filters.minRating))
  if (filters.decade) next.set("decade", String(filters.decade))
  return next
}
