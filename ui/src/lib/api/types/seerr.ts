import type { ItemSummary } from "./catalog.ts"

export type SeerrMediaType = "movie" | "tv"

export type SeerrStatus =
  | "unknown"
  | "pending"
  | "processing"
  | "partial"
  | "available"
  | "blacklisted"

/**
 * Why a `processing` title is not available yet, from Seerr's download queue
 * and the Companion's Radarr/Sonarr lookup. Absent or null when neither can
 * say, including from Companions that predate the field.
 */
export type SeerrActivity =
  | "downloading"
  | "searching"
  | "in-cinemas"
  | "unreleased"
  | "awaiting-episodes"

export type SeerrRequestStatus = "unknown" | "pending" | "approved" | "declined" | "failed"

/**
 * One Seerr result, already joined against the local cache by the shell.
 * `libraryItemId` is the whole point of the join: a result either plays or is
 * requested, and the card never has to guess which.
 */
export interface SeerrResult {
  mediaType: SeerrMediaType
  tmdbId: number
  title: string
  year: number | null
  overview: string | null
  posterPath: string | null
  backdropPath: string | null
  voteAverage: number | null
  status: SeerrStatus
  status4k: SeerrStatus
  activity?: SeerrActivity | null
  activity4k?: SeerrActivity | null
  libraryItemId: string | null
  /** Watch state from the local catalog join, for owned movies. */
  played?: boolean
}

export function discoveryRatingId(result: Pick<SeerrResult, "libraryItemId" | "mediaType" | "tmdbId">): string {
  return result.libraryItemId ?? `tmdb:${result.mediaType}:${result.tmdbId}`
}

export interface SeerrSeason {
  seasonNumber: number
  name: string | null
  episodeCount: number
  airDate: string | null
  status: SeerrStatus
  status4k: SeerrStatus
  activity?: SeerrActivity | null
  activity4k?: SeerrActivity | null
}

interface SeerrCastMember {
  id: number
  name: string
  character: string | null
  profilePath: string | null
}

interface SeerrCrewMember {
  id: number
  name: string
  /** Display labels in the Companion's key-job order, such as "Director". */
  jobs: string[]
  profilePath: string | null
}

export interface SeerrReleaseDate {
  region: string
  type: "premiere" | "limited-cinema" | "cinema" | "digital" | "physical" | "tv"
  date: string
  certification: string | null
}

interface SeerrContentRating {
  region: string
  rating: string
}

interface SeerrTrailer {
  name: string
  key: string
}

export interface SeerrMediaDetail extends SeerrResult {
  runtimeMinutes: number | null
  genres: string[]
  seasons: SeerrSeason[]
  tagline: string | null
  originalTitle: string | null
  voteCount: number | null
  releaseDate: string | null
  firstAirDate: string | null
  lastAirDate: string | null
  productionStatus: string | null
  inProduction: boolean
  seriesType: string | null
  numberOfSeasons: number | null
  numberOfEpisodes: number | null
  originalLanguage: string | null
  homepage: string | null
  /** Absent when an older compatible Companion serves the detail response. */
  externalIds?: { imdb: string | null; tvdb: number | null }
  budget: number | null
  revenue: number | null
  studios: string[]
  networks: string[]
  productionCountries: { code: string; name: string }[]
  spokenLanguages: { code: string; name: string }[]
  cast: SeerrCastMember[]
  /** Absent when an older compatible Companion serves the detail response. */
  crew?: SeerrCrewMember[]
  trailer: SeerrTrailer | null
  releaseDates: SeerrReleaseDate[]
  contentRatings: SeerrContentRating[]
  nextEpisode: {
    name: string
    airDate: string | null
    seasonNumber: number | null
    episodeNumber: number | null
  } | null
}

export interface SeerrPage<T> {
  page: number
  totalPages: number
  totalResults: number
  results: T[]
}

/**
 * Person credits plus the server titles the backend proved are on Jellyfin
 * even though its own cast relation never credited this person to them. They
 * are excluded from `results`' discoverable set by their `libraryItemId`.
 */
export interface SeerrPersonCreditsPage extends SeerrPage<SeerrResult> {
  libraryExtras?: ItemSummary[]
}

export interface SeerrRequest {
  id: number
  status: SeerrRequestStatus
  mediaType: SeerrMediaType
  tmdbId: number | null
  is4k: boolean
  createdAt: string | null
  updatedAt: string | null
  mediaStatus: SeerrStatus
  mediaActivity?: SeerrActivity | null
  seasons: number[]
  libraryItemId: string | null
}

export interface SeerrCapability {
  request: boolean
  autoApprove: boolean
}

export interface SeerrCapabilities {
  movie: SeerrCapability
  tv: SeerrCapability
  movie4k: SeerrCapability
  tv4k: SeerrCapability
  /** Seerr's REQUEST_ADVANCED bit: may choose a Radarr/Sonarr profile. */
  advancedRequest: boolean
}

interface SeerrQualityProfile {
  id: number
  name: string
  isDefault: boolean
}

interface SeerrRequestDestination {
  id: number
  name: string
  isDefault: boolean
  profiles: SeerrQualityProfile[]
}

export interface SeerrRequestOptions {
  destinations: SeerrRequestDestination[]
}

interface SeerrQuotaStatus {
  days: number | null
  limit: number | null
  used: number
  remaining: number | null
  restricted: boolean
}

export interface SeerrStatusInfo {
  linked: boolean
  instance: {
    movie4kEnabled: boolean
    series4kEnabled: boolean
    partialRequestsEnabled: boolean
  }
  user: { id: number; name: string; avatar: string | null; jellyfinUserId: string | null } | null
  capabilities: SeerrCapabilities | null
  quota: { movie: SeerrQuotaStatus; tv: SeerrQuotaStatus } | null
  mapped: boolean
}

export const SEERR_DISCOVER_ROWS = [
  {
    id: "trending",
    label: "Trending",
    title: "What everyone is watching",
    description: "The movies and series gaining momentum on TMDB right now.",
  },
  {
    id: "movies",
    label: "Popular movies",
    title: "Popular movies",
    description: "Shape Seerr’s movie catalogue by genre, score, and release date.",
  },
  {
    id: "tv",
    label: "Popular series",
    title: "Popular series",
    description: "Find the series people keep coming back to.",
  },
  {
    id: "upcoming-movies",
    label: "Upcoming movies",
    title: "Movies on the horizon",
    description: "Get requests in before the next wave of premieres lands.",
  },
  {
    id: "upcoming-tv",
    label: "Upcoming series",
    title: "Series on the horizon",
    description: "New and returning series with their first air dates ahead.",
  },
] as const

export type SeerrDiscoverRow = (typeof SEERR_DISCOVER_ROWS)[number]["id"]

type SeerrDiscoverSort = "popular" | "rating" | "newest"

export type SeerrReleaseDecade = number

type SeerrTrendingMediaType = "all" | SeerrMediaType

type SeerrTrendingWindow = "day" | "week"

export interface SeerrDiscoverFilters {
  genre?: number
  sort?: SeerrDiscoverSort
  minRating?: number
  decade?: SeerrReleaseDecade
  mediaType?: SeerrTrendingMediaType
  timeWindow?: SeerrTrendingWindow
}

export interface SeerrGenre {
  id: number
  name: string
  backdrops: string[]
}
