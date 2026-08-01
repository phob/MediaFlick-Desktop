import type {
  SeerrDiscoverFilters,
  SeerrDiscoverRow,
  SeerrReleaseCentury,
} from "./api"

export const DISCOVERY_FILTER_KEYS = [
  "genre",
  "sort",
  "minRating",
  "century",
  "mediaType",
  "timeWindow",
] as const

export const RELEASE_CENTURIES: Record<"movie" | "tv", readonly {
  value: SeerrReleaseCentury
  label: string
}[]> = {
  // TMDB's film catalogue reaches into the nineteenth century.
  movie: [
    { value: 21, label: "21st century (2001–2100)" },
    { value: 20, label: "20th century (1901–2000)" },
    { value: 19, label: "19th century (1801–1900)" },
  ],
  // Television release records begin in the twentieth century.
  tv: [
    { value: 21, label: "21st century (2001–2100)" },
    { value: 20, label: "20th century (1901–2000)" },
  ],
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
  centuryDiscovery: boolean,
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

  const century = positiveInteger(params.get("century"))
  const mediaType = row === "movies" ? "movie" : "tv"
  if (
    centuryDiscovery &&
    RELEASE_CENTURIES[mediaType].some((option) => option.value === century)
  ) {
    filters.century = century as SeerrReleaseCentury
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
  if (filters.century) next.set("century", String(filters.century))
  return next
}
