// Typed client for the JSON API served by `src/shell/cef/api.rs` on the
// `mediaflick-desktop://app/` scheme. Shapes mirror `summary_row` / `detail_row`
// in `src/library/mod.rs` and the handlers in `api.rs`.

import { isJsonObject, jsonBoolean, jsonString, type JsonObject, type JsonValue } from "./json.ts"

const TICKS_PER_MS = 10_000
const POSTER_WIDTH = 400
/** Home progress cards are drawn wider and use 16:9 art. */
export const LANDSCAPE_WIDTH = 560
/** The detail hero renders edge to edge, so its backdrop needs a real width. */
export const BACKDROP_WIDTH = 1920
/** The detail hero's poster: a bigger slot than the library grid's. */
export const DETAIL_POSTER_WIDTH = 600
/** Episode still cards: wide grid slots, doubled to cover HiDPI. */
export const THUMBNAIL_WIDTH = 800
const HEADSHOT_WIDTH = 200
/** Title treatments are drawn at most ~28rem wide; twice that covers HiDPI. */
const LOGO_WIDTH = 800
export const PAGE_SIZE = 60

type ItemKind = "Movie" | "Series" | "Season" | "Episode" | (string & {})

export interface ItemSummary {
  id: string
  kind: ItemKind
  name: string
  year: number | null
  runtimeTicks: number | null
  communityRating: number | null
  officialRating: string | null
  seriesId: string | null
  seriesName: string | null
  indexNumber: number | null
  parentIndexNumber: number | null
  primaryImageTag: string | null
  thumbImageTag: string | null
  /** The title treatment — the wordmark on transparency, where the server has one. */
  logoImageTag: string | null
  backdropImageTag: string | null
  childCount: number | null
  premiereDate: string | null
  seasonId: string | null
  played: boolean
  playCount: number
  positionTicks: number
  favorite: boolean
  /**
   * Only `/api/item/{id}/children` carries this, passed through from its live
   * server reconcile; the grid queries do not, and offline rows have none.
   */
  overview?: string | null
}

export interface Person {
  id: string | null
  name: string | null
  role: string | null
  type: string | null
  imageTag: string | null
}

/** Stable identity shared by Jellyfin cast filtering and TMDB/Seerr credits. */
export interface PersonIdentity {
  jellyfinId: string
  tmdbId: number | null
  name: string
  imageTag: string | null
}

export interface PersonResolution {
  person: PersonIdentity | null
  candidates: PersonIdentity[]
  ambiguous: boolean
}

export interface PersonResolveQuery {
  jellyfinId?: string
  tmdbId?: number
  name?: string
}

/** The cached thin detail row: instant, but without prose or cast. */
export interface ItemDetail extends ItemSummary {
  genres: string[]
  originalTitle: string | null
  providerIds: { tmdb: string | null; imdb: string | null; tvdb: string | null }
  parentId: string | null
  dateCreated: string | null
}

/**
 * Rich metadata fetched live from Jellyfin by `/api/item/{id}/about` for the
 * detail page. Nothing here is cached locally; with the server unreachable
 * these sections stay in their loading/error states.
 */
export interface ItemAbout {
  overview: string | null
  criticRating: number | null
  people: Person[]
  tags: string[]
  studios: string[]
}

/** The billboard's purpose-built live prose response. */
export interface ItemSynopsis {
  overview: string | null
}

/** One card's live technical descriptors from `/api/technical/batch`. */
export interface ItemTechnical {
  id: string
  mediaStreams: MediaStream[]
}

/** One profile's newest rating and newest written review in the current movie RSS feed. */
export interface LetterboxdReview {
  profileId: string
  username: string
  displayName: string
  profileUrl: string
  /** Canonical Letterboxd film entry; absent if the feed supplied an unsafe URL. */
  entryUrl: string | null
  rating: number | null
  /** Native code strips provider HTML before this reaches React. */
  review: string | null
  reviewTruncated: boolean
  watchedDate: string | null
  /** True when a failed refresh fell back to an older in-memory feed. */
  stale: boolean
}

export interface LetterboxdReviewsResponse {
  reviews: LetterboxdReview[]
  configuredProfiles: number
  unavailableProfiles: number
}

/** One track of a media source, as `media_stream_json` in `api.rs` shapes it. */
export interface MediaStream {
  index: number
  /** Present on cached item summaries; detail sources already group by type. */
  type?: string | null
  codec: string | null
  profile?: string | null
  language: string | null
  title: string | null
  displayTitle: string | null
  width: number | null
  height: number | null
  channels: number | null
  audioSpatialFormat?: string | null
  videoRange: string | null
  videoRangeType: string | null
  bitDepth: number | null
  isDefault: boolean
  isForced: boolean
  isHearingImpaired: boolean
  isExternal: boolean
}

/** One playable file behind an item, from `/api/item/{id}/media`. */
export interface MediaSource {
  id: string | null
  name: string
  container: string | null
  fileName: string | null
  size: number | null
  bitrate: number | null
  defaultAudioStreamIndex: number | null
  defaultSubtitleStreamIndex: number | null
  video: MediaStream[]
  audio: MediaStream[]
  subtitles: MediaStream[]
}

/** A saved choice after the shell has validated it against current sources. */
export interface PlaybackTrackPreference {
  mediaSourceId: string | null
  mediaSourceIndex: number
  audioStreamIndex: number | null
  /** `null` means subtitles off. */
  subtitleStreamIndex: number | null
}

export interface MediaInfoResponse {
  sources: MediaSource[]
  playbackPreference: PlaybackTrackPreference | null
}

export type PlaybackTrackPreferenceWrite = PlaybackTrackPreference

export interface TrailerSummary {
  id: string | null
  name: string
  embedUrl: string | null
}

/**
 * External title pages the shell can open in the default browser. `source`
 * names the stable provider id each destination accepts. The kinds mirror the
 * native routes, so the UI never offers a link the shell would refuse to build.
 */
const EXTERNAL_PROVIDERS = [
  { id: "imdb", label: "IMDb", source: "imdb", kinds: ["Movie", "Series", "Episode"] },
  { id: "tmdb", label: "TMDB", source: "tmdb", kinds: ["Movie", "Series"] },
  { id: "tvdb", label: "TVDB", source: "tvdb", kinds: ["Movie", "Series", "Episode"] },
  { id: "letterboxd", label: "Letterboxd", source: "tmdb", kinds: ["Movie"] },
  { id: "trakt", label: "Trakt", source: "imdb", kinds: ["Movie", "Series"] },
] as const

export type ExternalProvider = (typeof EXTERNAL_PROVIDERS)[number]["id"]
type ExternalIdSource = (typeof EXTERNAL_PROVIDERS)[number]["source"]
type DiscoveryMediaKind = "Movie" | "Series"
type ExternalLinkId = ExternalProvider | "rotten-tomatoes-search"

export interface ExternalMediaLink {
  id: ExternalLinkId
  label: string
  href: string
  actionLabel?: string
}

export type ExternalMenuLink =
  | ExternalMediaLink
  | { id: ExternalProvider; label: string }

function validExternalId(source: ExternalIdSource, value: string | null | undefined) {
  if (!value || value.length > 32) return false
  if (source === "imdb") return /^tt\d+$/.test(value)
  return /^\d+$/.test(value) && Number(value) > 0
}

function rottenTomatoesSearchLink(title: string, year: number | null): ExternalMediaLink | null {
  const normalizedTitle = title.trim()
  if (!normalizedTitle) return null

  const normalizedYear = Number.isInteger(year) && year !== null && year > 1800
    ? ` ${year}`
    : ""
  const query = encodeURIComponent(`${normalizedTitle}${normalizedYear}`)
  return {
    id: "rotten-tomatoes-search",
    label: "Rotten Tomatoes",
    actionLabel: "Search Rotten Tomatoes",
    href: `https://www.rottentomatoes.com/search?search=${query}`,
  }
}

export function externalLinksFor(
  item: Pick<ItemDetail, "kind" | "name" | "year" | "providerIds">,
): ExternalMenuLink[] {
  const exactLinks = EXTERNAL_PROVIDERS.filter(
    (provider) =>
      validExternalId(provider.source, item.providerIds?.[provider.source]) &&
      provider.kinds.some((kind) => kind === item.kind),
  )
  const rottenTomatoes = item.kind === "Movie" || item.kind === "Series"
    ? rottenTomatoesSearchLink(item.name, item.year)
    : null
  return rottenTomatoes ? [...exactLinks, rottenTomatoes] : exactLinks
}

function externalMediaUrl(
  provider: ExternalProvider,
  id: string | null | undefined,
  kind: DiscoveryMediaKind,
) {
  const definition = EXTERNAL_PROVIDERS.find((candidate) => candidate.id === provider)
  if (!definition || !validExternalId(definition.source, id)) return null

  switch (provider) {
    case "imdb":
      return `https://www.imdb.com/title/${id}/`
    case "tmdb":
      return `https://www.themoviedb.org/${kind === "Movie" ? "movie" : "tv"}/${id}`
    case "tvdb":
      return `https://thetvdb.com/dereferrer/${kind === "Movie" ? "movie" : "series"}/${id}`
    case "letterboxd":
      return `https://letterboxd.com/tmdb/${id}`
    case "trakt":
      return `https://trakt.tv/${kind === "Movie" ? "movies" : "shows"}/${id}`
  }
}

export function discoveryExternalLinksFor(
  item: Pick<SeerrMediaDetail, "mediaType" | "tmdbId" | "title" | "year" | "externalIds">,
): ExternalMediaLink[] {
  const kind = item.mediaType === "movie" ? "Movie" : "Series"
  const externalIds = item.externalIds ?? { imdb: null, tvdb: null }
  const ids = {
    imdb: externalIds.imdb,
    tmdb: String(item.tmdbId),
    tvdb: externalIds.tvdb === null ? null : String(externalIds.tvdb),
  } satisfies Record<ExternalIdSource, string | null>

  const exactLinks = EXTERNAL_PROVIDERS.flatMap((provider) => {
    if (!provider.kinds.some((candidate) => candidate === kind)) return []
    const href = externalMediaUrl(provider.id, ids[provider.source], kind)
    return href ? [{ id: provider.id, label: provider.label, href }] : []
  })
  const rottenTomatoes = rottenTomatoesSearchLink(item.title, item.year)
  return rottenTomatoes ? [...exactLinks, rottenTomatoes] : exactLinks
}

export type HomeBuiltInId =
  | "watching"
  | "becauseYouWatched"
  | "recentlyAdded"
  | "upcoming"
  | "latestMovies"
  | "latestShows"
  | "myList"

export type HomeElement = {
  enabled: boolean
  label: string
  available: boolean
  category: "Built-in" | "Genre" | "My Collection"
} & (
  | { kind: "builtIn"; id: HomeBuiltInId }
  | { kind: "genre"; id: string }
  | { kind: "collection"; id: string }
)

export interface HomeConfiguration {
  billboard: boolean
  watching: {
    continueWatching: boolean
    nextUp: boolean
    combine: boolean
  }
  elements: HomeElement[]
}

export interface HomeSettingsResponse {
  settings: HomeConfiguration
  defaults: HomeConfiguration
  collectionMode: CollectionMode
}

export type HomeSettingsWrite = Omit<HomeConfiguration, "elements"> & {
  elements: Array<Pick<HomeElement, "kind" | "id" | "enabled">>
}

export function homeSettingsWrite(settings: HomeConfiguration): HomeSettingsWrite {
  return {
    billboard: settings.billboard,
    watching: settings.watching,
    elements: settings.elements.map(({ kind, id, enabled }) => ({ kind, id, enabled })),
  }
}

export interface HomeRow {
  kind: "builtIn" | "genre" | "collection"
  id: string
  title: string
  items: ItemSummary[]
}

export interface HomeResponse {
  configuration: HomeConfiguration
  continueWatching: ItemSummary[]
  rows: HomeRow[]
}

export interface HomeResumeResponse {
  continueWatching: ItemSummary[]
  nextUp: ItemSummary[]
}

interface LibraryStats {
  movies: number
  series: number
  seasons: number
  episodes: number
  total: number
}

interface BootstrapProgress {
  complete: boolean
  ready: boolean
  processed: number
  total: number | null
  initial: boolean
}

export interface Status {
  authenticated?: boolean
  serverUrl?: string | null
  userId?: string | null
  userName?: string | null
  library?: LibraryStats
  syncing?: boolean
  lastSync?: string | null
  bootstrapped?: boolean
  libraryReady?: boolean
  bootstrap?: BootstrapProgress
  syncProgress?: SyncProgress
  companion?: CompanionStatus
}

export type CompanionService = "sonarr" | "radarr" | "seerr" | "mdblist" | "tmdb"

export interface CompanionInfo {
  pluginVersion: string
  apiVersion: number
  capabilities: string[]
  services: Record<CompanionService, boolean>
}

export interface CompanionStatus {
  available: boolean
  compatible: boolean
  checked: boolean
  info: CompanionInfo | null
  error: string | null
  supportedApi: { min: number; max: number }
}

type CalendarEntryKind = "episode" | "movie"
export type CalendarDateKind = "air" | "digital" | "physical" | "cinema"

export interface CalendarEntry {
  kind: CalendarEntryKind
  date: string
  dateKind: CalendarDateKind
  title: string
  seriesTitle: string | null
  season: number | null
  episode: number | null
  tmdbId: number | null
  tvdbId: number | null
  seriesTmdbId?: number | null
  seriesTvdbId?: number | null
  monitored: boolean
  hasFile: boolean
  posterUrl: string | null
  libraryItemId: string | null
  seriesLibraryItemId?: string | null
}

export interface CalendarSource {
  enabled: boolean
  available: boolean
  stale: boolean
  refreshedAt: string | null
  error: string | null
}

export interface ReleaseCalendar {
  entries: CalendarEntry[]
  refreshedAt: string | null
  sources: Record<string, CalendarSource>
  windowStart: string
  windowEnd: string
  provider: "plugin" | "metadata"
}

export interface PlayerSettings {
  playerBackend: "libmpv" | "mpv" | "mpchc"
  mpvPath: string | null
  mpchcPath: string | null
  defaultFullscreen: "fullscreen" | "windowed"
  markWatchedNext: string | null
  /** Computed by the shell from the selected backend/path; never writable. */
  playerConfigured: boolean
}

export type PlayerSettingsWrite = Omit<PlayerSettings, "playerConfigured">

export function playerSettingsWrite(settings: PlayerSettings): PlayerSettingsWrite {
  return {
    playerBackend: settings.playerBackend,
    mpvPath: settings.mpvPath,
    mpchcPath: settings.mpchcPath,
    defaultFullscreen: settings.defaultFullscreen,
    markWatchedNext: settings.markWatchedNext,
  }
}

export interface ClientSettings {
  client: {
    player: PlayerSettings
    playback: {
      streamingQuality: StreamingQualityId
      skipIntro: SegmentSkipMode
      skipCredits: SegmentSkipMode
      skipRecap: SegmentSkipMode
      skipCommercial: SegmentSkipMode
    }
    application: {
      closeBehavior: "exit_app" | "minimize_window"
      showScrollbars: boolean
      logLevel: "trace" | "debug" | "info" | "warn" | "error"
    }
  }
  appearance: AppearanceSettings
  capabilities: {
    platform: "windows" | "macos" | "linux" | "other"
    libmpv: boolean
    integratedLibmpvOverlay?: boolean
    mpchc: boolean
    mpvInstaller: boolean
  }
  recoveries?: { area: string; restoredBackup: boolean }[]
  serverUrl: string | null
}

type SegmentSkipMode = "disabled" | "prompt" | "always"

export interface AppearanceSettings {
  theme: "system" | "dark" | "light"
  accent: "signal" | "cobalt" | "amber" | "violet"
  density: "compact" | "comfortable"
  artworkIntensity: number
  backdropIntensity: number
  reducedMotion: boolean
  /** Whether resting the pointer on a media card opens the expanded panel. */
  cardPreviews: boolean
  /** Whether technical video/audio facts are drawn over library cards. */
  showMediaInfo: boolean
  /** Canonical IDs from the fixed public MDBList source catalog. */
  ratingSources: string[]
}

type RatingOrigin = "plugin"

export interface RatingSourceDefinition {
  id: string
  label: string
  shortLabel: string
  scaleMax: number
  format: "percent" | "decimal" | "integer" | "stars" | (string & {})
  known: boolean
}

export interface RatingsIntegrationStatus {
  boundaryVersion: 1
  effectiveOrigin: RatingOrigin | "none"
  available: boolean
  selectionEnabled: boolean
  plugin: {
    available: boolean
    capability: "ratings-v1"
    boundaryVersion: 1
    detail: string
  }
  sources: RatingSourceDefinition[]
  selectedSources: string[]
}

export interface NormalizedRating {
  sourceId: string
  rawSource: string
  value: number
  score: number | null
  votes: number | null
  scaleMax: number
}

export interface ItemRatings {
  id: string
  ratings: NormalizedRating[]
  origin: RatingOrigin
  fetchedAt: number
  sourceUpdatedAt: string | null
  stale: boolean
  schemaVersion: number
}

export interface RatingsBatchResponse {
  available: boolean
  effectiveOrigin: RatingOrigin | "none"
  items: ItemRatings[]
  retryAt: number | null
  quota?: {
    limit: number | null
    remaining: number | null
    resetAt: number | null
  }
  diagnostic: string | null
}

export interface LetterboxdProfile {
  id: string
  provider: "letterboxd"
  profileKey: string
  displayName: string
  canonicalUrl: string
  enabled: boolean
  verificationStatus: "verified" | "unverified"
  createdAt: number
  lastCheckedAt: number | null
}

/** Mirrors the JSON `Session::connect` answers. */
export interface ServerInfo {
  serverUrl: string
  serverName: string | null
  version: string | null
  quickConnect: boolean
}

export interface QuickConnectStart {
  serverUrl: string
  code: string
  secret: string
}

// ------------------------------------------------------------------- seerr

/** The two TMDB namespaces; `person` results never reach the UI. */
export type SeerrMediaType = "movie" | "tv"

/** Normalized media availability returned by Companion's Seerr contract. */
export type SeerrStatus =
  | "unknown"
  | "pending"
  | "processing"
  | "partial"
  | "available"
  | "blacklisted"

/** Normalized request state returned by Companion's Seerr contract. */
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
  libraryItemId: string | null
  /** Watch state from the local catalog join, for owned movies. */
  played?: boolean
}

export interface SeerrSeason {
  seasonNumber: number
  name: string | null
  episodeCount: number
  airDate: string | null
  status: SeerrStatus
  status4k: SeerrStatus
}

interface SeerrCastMember {
  id: number
  name: string
  character: string | null
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
  creators: string[]
  directors: string[]
  writers: string[]
  productionCountries: { code: string; name: string }[]
  spokenLanguages: { code: string; name: string }[]
  cast: SeerrCastMember[]
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
  seasons: number[]
  libraryItemId: string | null
}

/** What one media kind may do, from the user's Seerr permission mask. */
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

/** The signed-in user's Seerr state reported by MediaFlick Companion. */
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

// ------------------------------------------------------------- collections

export type CollectionMode = "mediaFlick" | "jellyfin"
export type CollectionMediaType = "movie" | "series" | "mixed"
export type RefreshCadence = "manual" | "daily" | "weekly" | "monthly"
export type CollectionCategory =
  | "trending"
  | "popular"
  | "streamingServices"
  | "topRated"
  | "inTheaters"
  | "upcoming"
  | "onAir"
  | "editorial"
  | "custom"

export type CollectionTemplatePictogram =
  | "award"
  | "binary"
  | "blocks"
  | "bone"
  | "bookOpen"
  | "briefcase"
  | "bug"
  | "calendarClock"
  | "calendarDays"
  | "circle"
  | "compass"
  | "crosshair"
  | "drama"
  | "film"
  | "flame"
  | "ghost"
  | "heart"
  | "landmark"
  | "languages"
  | "laugh"
  | "listVideo"
  | "monitorPlay"
  | "mountain"
  | "music"
  | "orbit"
  | "palette"
  | "pawPrint"
  | "popcorn"
  | "rocket"
  | "search"
  | "slidersHorizontal"
  | "sparkles"
  | "star"
  | "swords"
  | "telescope"
  | "trendingUp"
  | "trophy"
  | "tv"
  | "usersRound"
  | "wandSparkles"
  | "zap"

export interface ProviderReadiness {
  tmdb: boolean
  mdblist: boolean
}

export interface CollectionSettings {
  effectiveMode: CollectionMode
  mediaFlickAvailable: boolean
  modeSelection: CollectionMode | null
  franchises: { includeUnreleased: boolean }
  readiness: ProviderReadiness
  recovery: { damagedPath: string; restoredBackup: boolean } | null
}

export type CollectionSource =
  | {
      kind: "tmdbDiscover"
      parameters: JsonObject
    }
  | {
      kind: "tmdbCollection"
      collectionId: number
      includeUnreleased: boolean
    }
  | {
      kind: "mdbListPublicList"
      listId: string
    }

export type CollectionResultLimit =
  | { kind: "all" }
  | { kind: "maximum"; count: number }

export interface CollectionTemplateReference {
  id: string
}

export interface CollectionProfileDraft {
  template: CollectionTemplateReference
  title: string
  description: string
  customPosterId: string | null
  source: CollectionSource
  mediaType: CollectionMediaType
  limit: CollectionResultLimit
  cadence: RefreshCadence
  availableOnHome: boolean
}

export interface CollectionProfile extends CollectionProfileDraft {
  id: string
  revision: string
}

export interface CollectionTemplate extends Omit<CollectionProfileDraft, "template" | "customPosterId" | "availableOnHome"> {
  id: string
  category: CollectionCategory
  pictogram: CollectionTemplatePictogram
}

export interface CollectionTemplates {
  categories: CollectionCategory[]
  templates: { template: CollectionTemplate; available: boolean }[]
  readiness: ProviderReadiness
}

export interface NormalizedCollectionTitle {
  mediaType: CollectionMediaType
  tmdbId: number
  title: string
  originalTitle?: string | null
  year?: number | null
  overview: string
  releaseDate?: string | null
  sourceOrder: number
  posterPath?: string | null
  backdropPath?: string | null
  adult: boolean
}

export interface ClassifiedCollectionTitle extends NormalizedCollectionTitle {
  localItems: { id: string; name: string; kind: string; played: boolean }[]
}

export interface CollectionPreview {
  items: NormalizedCollectionTitle[]
  total: number
  movies: number
  series: number
  sourceIdentity?: string | null
}

export interface PublicCollectionList {
  id: string
  name: string
  owner: string | null
}

export interface CollectionProfilesIndex {
  profiles: CollectionProfile[]
  errors?: Record<string, string>
}

export interface CollectionProfileDetail {
  profile: CollectionProfile
  status: "updating" | "resultsUnavailable" | "ready"
  owned: ClassifiedCollectionTitle[]
  missing: NormalizedCollectionTitle[]
  items: NormalizedCollectionTitle[]
  libraryItems: ItemSummary[]
  ownershipAvailable?: boolean
  refresh?: {
    lastAttempt: number | null
    lastSuccess: number | null
    latestFailure: string | null
    nextDue: number | null
    initialized: boolean
  }
  overdue?: boolean
}

export interface FranchiseCollection {
  collectionId: number
  name: string
  posterPath: string | null
  backdropPath: string | null
  owned: ClassifiedCollectionTitle[]
  missing: NormalizedCollectionTitle[]
  items?: NormalizedCollectionTitle[]
  libraryItems: ItemSummary[]
  ownershipAvailable?: boolean
}

export interface FranchiseCollectionSummary {
  collectionId: number
  name: string
  posterPath: string | null
  backdropPath: string | null
  ownedCount: number
  missingCount: number
  ownershipAvailable: boolean
}

export interface FranchiseCollectionsIndex {
  status: "updating" | "resultsUnavailable" | "ready"
  franchises: FranchiseCollectionSummary[]
}

export interface JellyfinCollectionSummary {
  id: string
  name: string
  primaryImageTag: string | null
  backdropImageTag: string | null
  itemCount: number | null
}

export interface JellyfinCollectionDetail {
  id: string
  name: string
  primaryImageTag: string | null
  backdropImageTag: string | null
  items: ItemSummary[]
  totalRecordCount: number
}

/** The single exact TMDB franchise a movie belongs to, or none. */
export interface MovieCollection {
  tmdbId: number
  collection: { id: number; name: string } | null
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

type SyncPhase = "catalog" | "reconciling" | "retrying" | "complete"

export interface SyncProgress {
  active: boolean
  phase: SyncPhase
  catalog: BootstrapProgress
  error: string | null
  retryAt: number | null
}

export interface SeerrGenre {
  id: number
  name: string
  backdrops: string[]
}

// The first Companion-backed image proxy cached error JSON as immutable art.
// Keep the same disk key while forcing CEF to ask the repaired proxy once.
const SEERR_IMAGE_CACHE_VERSION = 2

/**
 * Provider art through the Desktop and Companion proxies. The UI supplies
 * only an allowlisted rendition name and provider-issued image path.
 */
export function seerrImageUrl(path: string | null | undefined, size = "w300") {
  if (!path) return null
  return `/api/seerr/image/${size}/${encodeURIComponent(path.replace(/^\//, ""))}?v=${SEERR_IMAGE_CACHE_VERSION}`
}

/** Ids accepted by `StreamingQuality::from_id` (`src/preferences/model.rs`). */
export type StreamingQualityId =
  | "original"
  | "auto"
  | "120_mbps"
  | "80_mbps"
  | "60_mbps"
  | "40_mbps"
  | "20_mbps"
  | "10_mbps"
  | "5_mbps"
  | "3_mbps"
  | "1_5_mbps"

export const STREAMING_QUALITIES: { id: StreamingQualityId; label: string }[] = [
  { id: "original", label: "Original file" },
  { id: "auto", label: "Auto" },
  { id: "120_mbps", label: "120 Mbps" },
  { id: "80_mbps", label: "80 Mbps" },
  { id: "60_mbps", label: "60 Mbps" },
  { id: "40_mbps", label: "40 Mbps" },
  { id: "20_mbps", label: "20 Mbps" },
  { id: "10_mbps", label: "10 Mbps" },
  { id: "5_mbps", label: "5 Mbps" },
  { id: "3_mbps", label: "3 Mbps" },
  { id: "1_5_mbps", label: "1.5 Mbps" },
]

export function qualityLabel(id: string | null | undefined) {
  return STREAMING_QUALITIES.find((quality) => quality.id === id)?.label ?? null
}

interface PlayerCapabilities {
  chapterMarkers: boolean
  externalSubtitles: boolean
  injectedHotkeys: boolean
  absoluteVolume: boolean
  pushesPosition: boolean
  fullscreen: boolean
  playbackTuning: boolean
}

export interface PlayerTrack {
  id: number
  kind: "audio" | "subtitle"
  language: string | null
  title: string | null
  codec: string | null
  selected: boolean
  external: boolean
}

export interface PlayerChapter {
  title: string
  startMs: number
}

export interface PlayerSkipSegment {
  segmentType: "intro" | "outro" | "recap" | "commercial"
  startTicks: number
  endTicks: number
  triggered: boolean
}

export interface PlaybackDiagnostics {
  bufferedUntilMs: number | null
  buffering: boolean
  droppedFrames: number | null
  frameRate: number | null
}

export interface PlayerState {
  active: boolean
  playbackId?: string | null
  itemId?: string | null
  mediaSourceId?: string | null
  playSessionId?: string | null
  playMethod?: string | null
  positionMs?: number
  durationMs?: number
  paused?: boolean
  volume?: number | null
  mute?: boolean
  tracks?: PlayerTrack[]
  chapters?: PlayerChapter[]
  skipSegments?: PlayerSkipSegment[]
  diagnostics?: PlaybackDiagnostics
  stopReason?: string | null
  capabilities?: PlayerCapabilities
}

/** The `started: false` shape comes back when there is no next episode. */
export interface PlayStarted {
  started: boolean
  itemId?: string
  playMethod?: string
  mediaSource?: string
  startTicks?: number
}

/** Mirrors the command arm in `player_command` (`src/shell/cef/api.rs`). */
export type PlayerCommand =
  | { command: "pause" | "resume" | "stop" }
  | { command: "seek"; positionMs: number }
  | { command: "set-volume"; volume: number }
  | { command: "set-mute"; mute: boolean }
  | { command: "set-audio-delay"; delaySeconds: number }
  | { command: "set-subtitle-delay"; delaySeconds: number }
  | { command: "set-subtitle-scale"; scale: number }
  | { command: "set-video-fit"; fit: "fit" | "fill" }
  | { command: "set-video-aspect"; aspect: "source" | "4:3" | "16:9" | "21:9" }
  | { command: "set-deinterlace"; enabled: boolean }
  | {
      command: "set-tone-mapping"
      mode: "auto" | "clip" | "mobius" | "reinhard" | "hable" | "bt.2390"
    }
  | { command: "set-audio-track"; audioTrack: number }
  | { command: "set-subtitle-track"; subtitleTrack: number | null }
  | { command: "toggle-fullscreen" }

export interface ItemQuery {
  search?: string
  /** Exact Jellyfin person id; switches `/api/items` to a live `PersonIds` query. */
  personId?: string
  kind?: string
  genre?: string
  /** Inclusive first year of a standard release decade (for example 1990). */
  decade?: number
  parentId?: string
  seriesId?: string
  watched?: "true" | "false" | ""
  favorite?: boolean
  sort?: string
  offset?: number
  limit?: number
}

/** Mirrors the `{ error, expired }` envelope `ApiResponse::error` produces. */
export class ApiError extends Error {
  readonly status: number
  /** The server rejected the stored token — the shell must return to sign-in. */
  readonly expired: boolean
  constructor(message: string, status: number, expired: boolean) {
    super(message)
    this.name = "ApiError"
    this.status = status
    this.expired = expired
  }
}

interface RequestOptions {
  method?: string
  body?: unknown
  signal?: AbortSignal
}

interface ApiErrorEnvelope {
  error?: string
  expired?: boolean
}

function readApiErrorEnvelope(value: JsonValue): ApiErrorEnvelope | null {
  if (!isJsonObject(value)) return null
  const error = jsonString(value.error)
  const expired = jsonBoolean(value.expired)
  return {
    error: error ?? undefined,
    expired: expired ?? undefined,
  }
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const init: RequestInit = { method: options.method ?? "GET", signal: options.signal }
  if (options.body !== undefined) {
    init.headers = { "Content-Type": "application/json" }
    init.body = JSON.stringify(options.body)
  }

  const response = await fetch(path, init)
  let payload: JsonValue = null
  try {
    payload = await response.json()
  } catch {
    payload = null
  }

  if (!response.ok) {
    const envelope = readApiErrorEnvelope(payload)
    throw new ApiError(
      envelope?.error ?? `request failed (${response.status})`,
      response.status,
      Boolean(envelope?.expired),
    )
  }
  // SAFETY: The embedded UI and Rust shell ship together, and every request<T>
  // call names the response type owned by that same-version local API route.
  return payload as T
}

async function upload<T>(path: string, body: ArrayBuffer): Promise<T> {
  const response = await fetch(path, { method: "POST", body })
  let payload: JsonValue = null
  try {
    payload = await response.json()
  } catch {
    payload = null
  }
  if (!response.ok) {
    const envelope = readApiErrorEnvelope(payload)
    throw new ApiError(
      envelope?.error ?? `request failed (${response.status})`,
      response.status,
      Boolean(envelope?.expired),
    )
  }
  // SAFETY: Every upload call names the response type owned by the
  // same-version local Rust route.
  return payload as T
}

type QueryParameterValue = string | number | boolean | null | undefined

function queryString<T>(params: { [K in keyof T]: QueryParameterValue }) {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null || value === "") continue
    search.set(key, String(value))
  }
  const encoded = search.toString()
  return encoded ? `?${encoded}` : ""
}

export const api = {
  status: () => request<Status>("/api/status"),
  companion: {
    info: () => request<CompanionStatus>("/api/companion/info"),
    probe: () => request<CompanionStatus>("/api/companion/probe", { method: "POST" }),
  },
  settings: () => request<ClientSettings>("/api/settings"),
  homeSettings: () => request<HomeSettingsResponse>("/api/settings/home"),
  saveHomeSettings: (body: HomeSettingsWrite) =>
    request<HomeSettingsResponse>("/api/settings/home", { method: "PATCH", body }),
  settingsPatch: {
    player: (body: PlayerSettingsWrite) =>
      request<ClientSettings>("/api/settings/client/player", { method: "PATCH", body }),
    playback: (body: ClientSettings["client"]["playback"]) =>
      request<ClientSettings>("/api/settings/client/playback", { method: "PATCH", body }),
    application: (body: ClientSettings["client"]["application"]) =>
      request<ClientSettings>("/api/settings/client/application", { method: "PATCH", body }),
    appearance: (body: AppearanceSettings) =>
      request<ClientSettings>("/api/settings/appearance", { method: "PATCH", body }),
  },
  shell: {
    windowReady: () =>
      request<{ queued: boolean }>("/api/shell/window/ready", { method: "POST" }),
    filePicker: (requestId: string, target: "mpv" | "mpchc") =>
      request<{ requestId: string; queued: boolean }>("/api/shell/file-picker", {
        method: "POST",
        body: { requestId, target },
      }),
    installMpv: (requestId: string) =>
      request<{ requestId: string; queued: boolean }>("/api/shell/mpv/install", {
        method: "POST",
        body: { requestId },
      }),
    mpvHelp: () => request<{ opened: boolean }>("/api/shell/mpv/help", { method: "POST" }),
  },
  ratings: {
    status: () => request<RatingsIntegrationStatus>("/api/integrations/ratings"),
    batch: (ids: string[], signal?: AbortSignal) =>
      request<RatingsBatchResponse>("/api/ratings/batch", {
        method: "POST",
        body: { ids },
        signal,
      }),
  },
  technical: {
    batch: (ids: string[], signal?: AbortSignal) =>
      request<{ items: ItemTechnical[] }>("/api/technical/batch", {
        method: "POST",
        body: { ids },
        signal,
      }),
  },
  letterboxd: {
    profiles: () => request<{ profiles: LetterboxdProfile[] }>("/api/integrations/letterboxd"),
    add: (profile: string) =>
      request<{ profile: LetterboxdProfile }>("/api/integrations/letterboxd", {
        method: "POST",
        body: { profile },
      }),
    setEnabled: (id: string, enabled: boolean) =>
      request<{ profile: LetterboxdProfile }>(`/api/integrations/letterboxd/${encodeURIComponent(id)}`, {
        method: "PATCH",
        body: { enabled },
      }),
    refresh: (id: string) =>
      request<{ profile: LetterboxdProfile }>(
        `/api/integrations/letterboxd/${encodeURIComponent(id)}/refresh`,
        { method: "POST" },
      ),
    remove: (id: string) =>
      request<{ removed: boolean }>(`/api/integrations/letterboxd/${encodeURIComponent(id)}`, {
        method: "DELETE",
      }),
    open: (id: string) =>
      request<{ opened: boolean; url: string }>(
        `/api/integrations/letterboxd/${encodeURIComponent(id)}/open`,
        { method: "POST" },
      ),
  },

  connect: (server: string, signal?: AbortSignal) =>
    request<ServerInfo>("/api/auth/connect", { method: "POST", body: { server }, signal }),
  login: (server: string, username: string, password: string) =>
    request<Status>("/api/auth/login", { method: "POST", body: { server, username, password } }),
  quickConnectStart: (server: string) =>
    request<QuickConnectStart>("/api/auth/quickconnect/start", {
      method: "POST",
      body: { server },
    }),
  quickConnectPoll: (server: string, secret: string, signal?: AbortSignal) =>
    request<{ authenticated: boolean }>("/api/auth/quickconnect/poll", {
      method: "POST",
      body: { server, secret },
      signal,
    }),
  logout: () => request<Status>("/api/auth/logout", { method: "POST" }),

  home: () => request<HomeResponse>("/api/home"),
  homeResume: () => request<HomeResumeResponse>("/api/home/resume"),
  billboard: () => request<{ items: ItemSummary[] }>("/api/billboard"),
  genres: () => request<{ genres: string[] }>("/api/genres"),
  resolvePerson: (query: PersonResolveQuery, signal?: AbortSignal) =>
    request<PersonResolution>(`/api/person/resolve${queryString(query)}`, { signal }),
  items: (query: ItemQuery, signal?: AbortSignal) =>
    request<{ items: ItemSummary[]; total: number }>(`/api/items${queryString(query)}`, { signal }),
  item: (id: string) => request<ItemDetail>(`/api/item/${encodeURIComponent(id)}`),
  itemSynopsis: (id: string) =>
    request<ItemSynopsis>(`/api/item/${encodeURIComponent(id)}/synopsis`),
  itemAbout: (id: string) => request<ItemAbout>(`/api/item/${encodeURIComponent(id)}/about`),
  itemLetterboxd: (id: string) =>
    request<LetterboxdReviewsResponse>(`/api/item/${encodeURIComponent(id)}/letterboxd`),
  movieLetterboxd: (tmdbId: number) =>
    request<LetterboxdReviewsResponse>(`/api/letterboxd/movie/${tmdbId}`),
  children: (id: string) =>
    request<{ items: ItemSummary[] }>(`/api/item/${encodeURIComponent(id)}/children`),
  media: (id: string) =>
    request<MediaInfoResponse>(`/api/item/${encodeURIComponent(id)}/media`),
  setPlaybackPreference: (id: string, body: PlaybackTrackPreferenceWrite) =>
    request<{ playbackPreference: PlaybackTrackPreference }>(
      `/api/item/${encodeURIComponent(id)}/playback-preference`,
      { method: "PATCH", body },
    ),
  trailer: (id: string) =>
    request<{ trailer: TrailerSummary | null }>(`/api/item/${encodeURIComponent(id)}/trailer`),
  trailerStreamUrl: (id: string) => `/api/trailer/${encodeURIComponent(id)}/stream`,
  nextUp: (id: string) =>
    request<{ item: ItemSummary | null }>(`/api/item/${encodeURIComponent(id)}/nextup`),
  openExternal: (id: string, provider: ExternalProvider) =>
    request<{ opened: boolean; url: string }>(`/api/item/${encodeURIComponent(id)}/external`, {
      method: "POST",
      body: { provider },
    }),
  setPlayed: (id: string, played: boolean) =>
    request<unknown>(`/api/item/${encodeURIComponent(id)}/played`, { method: "POST", body: { played } }),
  setFavorite: (id: string, favorite: boolean) =>
    request<unknown>(`/api/item/${encodeURIComponent(id)}/favorite`, {
      method: "POST",
      body: { favorite },
    }),

  /** `quality` overrides the saved Settings default for this play only. */
  play: (itemId: string, resume: boolean, quality?: StreamingQualityId) =>
    request<PlayStarted>("/api/play", { method: "POST", body: { itemId, resume, quality } }),
  changePlaybackQuality: (itemId: string, startTicks: number, quality: StreamingQualityId) =>
    request<PlayStarted>("/api/play", {
      method: "POST",
      body: { itemId, startTicks, quality },
    }),
  playNext: (itemId: string) =>
    request<PlayStarted>("/api/play/next", { method: "POST", body: { itemId } }),
  playPrevious: (itemId: string) =>
    request<PlayStarted>("/api/play/previous", { method: "POST", body: { itemId } }),
  playbackNeighbors: (itemId: string) =>
    request<{ previous: ItemSummary | null; next: ItemSummary | null }>("/api/play/neighbors", {
      method: "POST",
      body: { itemId },
    }),

  playerState: () => request<PlayerState>("/api/player/state"),
  playerCommand: (command: PlayerCommand) =>
    request<unknown>("/api/player/command", { method: "POST", body: command }),

  sync: () => request<{ requested: boolean }>("/api/sync", { method: "POST" }),
  calendar: (start: string, end: string, signal?: AbortSignal) =>
    request<ReleaseCalendar>(`/api/calendar${queryString({ start, end })}`, { signal }),

  seerr: {
    status: () => request<SeerrStatusInfo>("/api/seerr/status"),
    search: (q: string, page = 1, signal?: AbortSignal) =>
      request<SeerrPage<SeerrResult>>(`/api/seerr/search${queryString({ q, page })}`, { signal }),
    personCredits: (tmdbId: number, jellyfinId?: string | null, signal?: AbortSignal) =>
      request<SeerrPersonCreditsPage>(
        `/api/seerr/person/${tmdbId}/credits${queryString({ personId: jellyfinId })}`,
        { signal },
      ),
    discover: (
      row: SeerrDiscoverRow,
      filters: SeerrDiscoverFilters = {},
      page = 1,
      signal?: AbortSignal,
    ) =>
      request<SeerrPage<SeerrResult>>(
        `/api/seerr/discover/${row}${queryString({ page, ...filters })}`,
        { signal },
      ),
    genres: (mediaType: SeerrMediaType, signal?: AbortSignal) =>
      request<SeerrGenre[]>(`/api/seerr/genres/${mediaType}`, { signal }),
    media: (mediaType: SeerrMediaType, tmdbId: number) =>
      request<SeerrMediaDetail>(`/api/seerr/media/${mediaType}/${tmdbId}`),
    requestOptions: (mediaType: SeerrMediaType, is4k = false) =>
      request<SeerrRequestOptions>(
        `/api/seerr/request-options/${mediaType}${queryString({ is4k })}`,
      ),

    requests: (filter = "all", take = 40, signal?: AbortSignal) =>
      request<SeerrPage<SeerrRequest>>(`/api/seerr/requests${queryString({ filter, take })}`, {
        signal,
      }),
    /** Omitting `seasons` on a series asks for everything Seerr lacks. */
    request: (body: {
      mediaType: SeerrMediaType
      tmdbId: number
      seasons?: number[]
      is4k?: boolean
      serverId?: number
      profileId?: number
    }) => request<SeerrRequest>("/api/seerr/request", { method: "POST", body }),
    cancelRequest: (id: number) =>
      request<{ cancelled: boolean }>(`/api/seerr/request/${id}`, { method: "DELETE" }),
  },

  // ------------------------------------------------------------ collections

  collections: {
    settings: (reprobe = false) =>
      request<CollectionSettings>(
        reprobe ? "/api/collections/settings/reprobe" : "/api/collections/settings",
        reprobe ? { method: "POST" } : undefined,
      ),
    patchSettings: (body: { modeSelection?: CollectionMode; includeUnreleased?: boolean }) =>
      request<CollectionSettings>("/api/collections/settings", { method: "PATCH", body }),
    templates: (signal?: AbortSignal) =>
      request<CollectionTemplates>("/api/collections/templates", { signal }),
    searchPublicLists: (query: string, signal?: AbortSignal) =>
      request<{ lists: PublicCollectionList[] }>("/api/collections/mdblist/search", {
        method: "POST",
        body: { query },
        signal,
      }),
    validatePublicList: (selector: string, signal?: AbortSignal) =>
      request<PublicCollectionList>("/api/collections/mdblist/validate", {
        method: "POST",
        body: { selector },
        signal,
      }),
    preview: (body: CollectionProfileDraft, signal?: AbortSignal) =>
      request<CollectionPreview>("/api/collections/preview", { method: "POST", body, signal }),
    profiles: (signal?: AbortSignal) =>
      request<CollectionProfilesIndex>("/api/collections/profiles", { signal }),
    createProfile: (body: CollectionProfileDraft) =>
      request<{ profile: CollectionProfile; total: number }>("/api/collections/profiles", {
        method: "POST",
        body,
      }),
    updateProfile: (id: string, body: CollectionProfileDraft) =>
      request<CollectionProfile | { profile: CollectionProfile; total: number }>(
        `/api/collections/profiles/${encodeURIComponent(id)}`,
        { method: "PATCH", body },
      ),
    deleteProfile: (id: string) =>
      request<{ deleted: boolean }>(`/api/collections/profiles/${encodeURIComponent(id)}`, {
        method: "DELETE",
      }),
    reorderProfiles: (profileIds: string[]) =>
      request<CollectionProfilesIndex>("/api/collections/profiles/order", {
        method: "PUT",
        body: { profileIds },
      }),
    refreshProfile: (id: string) =>
      request<{ profile: CollectionProfile; total: number }>(
        `/api/collections/profiles/${encodeURIComponent(id)}/refresh`,
        { method: "POST" },
      ),
    mine: (signal?: AbortSignal) =>
      request<CollectionProfilesIndex>("/api/collections/mine", { signal }),
    mineDetail: (id: string, signal?: AbortSignal) =>
      request<CollectionProfileDetail>(`/api/collections/mine/${encodeURIComponent(id)}`, {
        signal,
      }),
    franchises: (localDate: string, signal?: AbortSignal) =>
      request<FranchiseCollectionsIndex>(
        `/api/collections/franchises${queryString({ localDate })}`,
        { signal },
      ),
    franchise: (id: number, localDate: string, signal?: AbortSignal) =>
      request<FranchiseCollection>(
        `/api/collections/franchises/${id}${queryString({ localDate })}`,
        { signal },
      ),
    jellyfin: (signal?: AbortSignal) =>
      request<{ collections: JellyfinCollectionSummary[] }>("/api/collections/jellyfin", {
        signal,
      }),
    jellyfinDetail: (id: string, signal?: AbortSignal) =>
      request<JellyfinCollectionDetail>(`/api/collections/jellyfin/${encodeURIComponent(id)}`, {
        signal,
      }),
    uploadArtwork: (body: ArrayBuffer) =>
      upload<{ id: string }>("/api/collections/artwork", body),
    artworkUrl: (id: string) => `/api/collections/artwork/${encodeURIComponent(id)}`,
    providerArtworkUrl: (path: string | null | undefined, size = "w342") =>
      path ? `/api/collections/provider-artwork${queryString({ path, size })}` : null,
    title: (mediaType: SeerrMediaType, tmdbId: number, signal?: AbortSignal) =>
      request<{ item: NormalizedCollectionTitle }>(
        `/api/collections/title/${mediaType}/${tmdbId}`,
        { signal },
      ),
    forMovie: (tmdbId: number) =>
      request<MovieCollection>(`/api/collections/movie/${tmdbId}`),
    deleteLocalAccount: () =>
      request<Status>("/api/collections/local-account", {
        method: "DELETE",
        body: { confirmed: true },
      }),
  },
}

/**
 * Poster/backdrop URL through the Rust image proxy, which keeps the token out
 * of the DOM. The parameter has to be spelled `maxWidth`: that is what the
 * proxy in `src/shell/cef/api.rs` reads and what it forwards to Jellyfin. Under
 * any other name the proxy sees no width at all and serves the untouched
 * original — 2000x3000 posters decoded into a 168px slot, which is what made
 * the grid stutter.
 */
export function imageUrl(
  item: Pick<ItemSummary, "id" | "primaryImageTag">,
  type: "Primary" | "Backdrop" | "Thumb" | "Logo" = "Primary",
  maxWidth = POSTER_WIDTH,
  // Every image type has its own tag. Passing the poster's tag along with
  // `Backdrop` asks Jellyfin for an image that does not exist under it, so the
  // caller has to say which tag goes with the type it asked for.
  tag: string | null = item.primaryImageTag,
) {
  return `/api/image/${encodeURIComponent(item.id)}/${type}${queryString({ maxWidth, tag })}`
}

/** Null when the item has no backdrop, so callers can lay out without one. */
export function backdropUrl(item: ItemDetail, maxWidth = BACKDROP_WIDTH) {
  if (!item.backdropImageTag) return null
  // Jellyfin supplies a series' backdrop tag as ParentBackdropImageTags on
  // seasons and episodes. That image still belongs to the series item: asking
  // the child id for the inherited tag produces a 404 from the image endpoint.
  const imageOwner =
    (item.kind === "Season" || item.kind === "Episode") && item.seriesId
      ? { id: item.seriesId, primaryImageTag: null }
      : item
  return imageUrl(imageOwner, "Backdrop", maxWidth, item.backdropImageTag)
}

/**
 * Landscape cards prefer an episode still, then Jellyfin's purpose-built
 * Thumb art, then a backdrop. A poster is the last-resort fallback so a title
 * with incomplete metadata still has something to show.
 */
export function landscapeImageCandidates(
  item: Pick<
    ItemSummary,
    "id" | "kind" | "primaryImageTag" | "thumbImageTag" | "backdropImageTag"
  >,
  maxWidth = LANDSCAPE_WIDTH,
) {
  const candidates: string[] = []
  const add = (type: "Primary" | "Backdrop" | "Thumb", tag: string | null) => {
    if (tag) candidates.push(imageUrl(item, type, maxWidth, tag))
  }

  if (item.kind === "Episode") add("Primary", item.primaryImageTag)
  add("Thumb", item.thumbImageTag)
  add("Backdrop", item.backdropImageTag)
  if (item.kind !== "Episode") add("Primary", item.primaryImageTag)
  return [...new Set(candidates)]
}

/**
 * The item's own title treatment, or null where the server has none — which is
 * most of a typical library, so every caller has to keep its typeset heading as
 * the fallback rather than treating this as the primary path.
 *
 * An episode borrows nothing here on purpose: the logo that matters over an
 * episode still is the *show's*, and the caller knows its id.
 */
export function logoUrl(
  item: Pick<ItemSummary, "id" | "logoImageTag">,
  maxWidth = LOGO_WIDTH,
) {
  if (!item.logoImageTag) return null
  return imageUrl(
    { id: item.id, primaryImageTag: null },
    "Logo",
    maxWidth,
    item.logoImageTag,
  )
}

/** Cast headshots go through the same proxy: people are items in Jellyfin. */
export function personImageUrl(person: Person, maxWidth = HEADSHOT_WIDTH) {
  if (!person.id || !person.imageTag) return null
  return imageUrl({ id: person.id, primaryImageTag: person.imageTag }, "Primary", maxWidth)
}

export function ticksToMs(ticks: number | null | undefined) {
  return (ticks ?? 0) / TICKS_PER_MS
}

export function progressFraction(item: Pick<ItemSummary, "positionTicks" | "runtimeTicks">) {
  if (!item.runtimeTicks || !item.positionTicks) return 0
  return Math.min(1, item.positionTicks / item.runtimeTicks)
}
