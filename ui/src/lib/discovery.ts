import type {
  SeerrDiscoverFilters,
  SeerrDiscoverRow,
  SeerrReleaseDecade,
} from "./api"

export const DISCOVERY_FILTER_KEYS = [
  "genre",
  "sort",
  "minRating",
  "decade",
  "mediaType",
  "timeWindow",
] as const

const LEGACY_DISCOVERY_FILTER_KEYS = ["century"] as const
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
      label:
        decade === CURRENT_DECADE
          ? `${decade}s (${decade}–present)`
          : `${decade}s (${decade}–${decade + 9})`,
    })
  }
  return options
}

export const RELEASE_DECADES = {
  // Preserve the historical catalogue coverage of the former broad filter.
  movie: releaseDecades(1800),
  tv: releaseDecades(1900),
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

/** Read only values represented by the controls; malformed deep links safely
 * fall back to that row's defaults instead of reaching the app API. */
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
  if (
    decadeDiscovery &&
    RELEASE_DECADES[mediaType].some((option) => option.value === decade)
  ) {
    filters.decade = decade as SeerrReleaseDecade
  }
  return filters
}

/** Replace one row's server-side filters while preserving search, tab, and
 * local-library state in the URL. Defaults are omitted to keep links concise. */
export function writeDiscoveryFilters(
  params: URLSearchParams,
  row: SeerrDiscoverRow,
  filters: SeerrDiscoverFilters,
) {
  const next = new URLSearchParams(params)
  for (const key of DISCOVERY_FILTER_KEYS) next.delete(key)
  // Intermediate builds briefly serialized the superseded century control.
  // Never reinterpret that much broader selection as one particular decade.
  for (const key of LEGACY_DISCOVERY_FILTER_KEYS) next.delete(key)

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
