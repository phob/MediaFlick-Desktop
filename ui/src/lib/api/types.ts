import type { JsonObject } from "../json.ts"

export const CATALOG_KINDS = ["Movie", "Series", "Season", "Episode"] as const
export type CatalogKind = (typeof CATALOG_KINDS)[number]

type ItemKind = CatalogKind | (string & {})

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
  logoImageTag: string | null
  backdropImageTag: string | null
  childCount: number | null
  premiereDate: string | null
  seasonId: string | null
  played: boolean
  playCount: number
  positionTicks: number
  favorite: boolean
  unplayedCount: number | null
  /** Width over height of the primary image. Null for cached movies and shows, whose art is a 2:3 poster. */
  primaryAspect: number | null
  /**
   * `/api/item/{id}/children` carries this for a season's episodes, from its
   * live server reconcile, and a generic item's details carry their own. The
   * grid queries do not, and offline rows have none.
   */
  overview?: string | null
  /**
   * A playlist's children carry the slot each item fills, counted from zero.
   * The same item can fill two slots.
   */
  playlistIndex?: number
}

export interface Person {
  id: string | null
  name: string | null
  role: string | null
  type: string | null
  imageTag: string | null
}

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

export interface ItemDetail extends ItemSummary {
  layout: "title"
  kind: CatalogKind
  genres: string[]
  originalTitle: string | null
  providerIds: { tmdb: string | null; imdb: string | null; tvdb: string | null }
  parentId: string | null
  dateCreated: string | null
  /** The library holding the title, or its series for a season or episode. Null until library membership is known. */
  libraryId: string | null
}

export interface GenericItemDetail extends ItemSummary {
  layout: "generic"
  genres: string[]
  isFolder: boolean
  dateCreated: string | null
}

export type AnyItemDetail = ItemDetail | GenericItemDetail

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

export interface ItemSynopsis {
  overview: string | null
}

export interface ItemTechnical {
  id: string
  mediaStreams: MediaStream[]
}

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

export type HomeBuiltInId =
  | "watching"
  | "becauseYouWatched"
  | "recentlyAdded"
  | "recentlyAddedShows"
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

/**
 * `/api/startup`: what the first frame reads, in one request. A part that its
 * own route could not answer is `null`, and its query then asks for it.
 */
export interface StartupResponse {
  status: Status | null
  settings: ClientSettings | null
  viewing: ViewingSettings | null
  browsing: Record<string, string> | null
  home: HomeResponse | null
  billboard: { items: ItemSummary[] } | null
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
  processed: number
  total: number | null
}

export interface Status {
  authenticated: boolean
  expired: boolean
  serverUrl: string | null
  serverName: string | null
  userId: string | null
  userName: string | null
  deviceId: string
  library: LibraryStats
  syncing: boolean
  lastSync: string | null
  bootstrap: Bootstrap
  syncProgress: SyncProgress
  companion: CompanionStatus
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
  posterPath?: string | null
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
  comfort: PlayerComfort
  playerBackend: "libmpv" | "mpv"
  mpvPath: string | null
  defaultFullscreen: "fullscreen" | "windowed"
  markWatchedNext: string | null
  playerConfigured: boolean
}

export type PlayerSettingsWrite = Omit<PlayerSettings, "playerConfigured">

export function playerSettingsWrite(settings: PlayerSettings): PlayerSettingsWrite {
  return {
    playerBackend: settings.playerBackend,
    mpvPath: settings.mpvPath,
    defaultFullscreen: settings.defaultFullscreen,
    markWatchedNext: settings.markWatchedNext,
    comfort: settings.comfort,
  }
}

export interface ViewingSettings {
  spoilerProtection: boolean
  nextEpisode: "off" | "ask" | "auto"
  countdownSeconds: number
  episodeLimit: number
  audioLanguages: string[]
  subtitleLanguages: string[]
  preferOriginalAudio: boolean
  subtitleMode: "server" | "off" | "forced" | "always" | "foreignAudio"
  resumeRewindSeconds: number
  textScale: number
  posterSize: number
  previewDelayMs: number
  startupDestination: "home" | "movies" | "series" | "calendar" | "last"
  rememberFilters: boolean
  hideWatched: boolean
}

export interface PlayerComfort {
  subtitleSize: number
  subtitleOutline: number
  subtitleBackground: number
  subtitlePosition: number
  seekBackSeconds: number
  seekForwardSeconds: number
  pauseKey: string
  muteKey: string
  fullscreenKey: string
  seekBackKey: string
  seekForwardKey: string
  stopKey: string
  subtitlesKey: string
  seekBackThirtyKey: string
  seekForwardThirtyKey: string
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
    integratedLibmpvOverlay: boolean
    mpvInstaller: boolean
  }
  recoveries: { area: string; restoredBackup: boolean }[]
  serverUrl: string | null
}

type SegmentSkipMode = "hidden" | "disabled" | "prompt" | "always"

export interface AppearanceSettings {
  accent: "signal" | "cobalt" | "amber" | "violet"
  density: "compact" | "comfortable"
  artworkIntensity: number
  backdropIntensity: number
  reducedMotion: boolean
  cardPreviews: boolean
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

type SyncPhase = "catalog" | "reconciling" | "complete"

export type SyncFailure = "unreachable" | "server" | "storage"

/** A failed cycle and when the worker tries again, in Unix seconds. */
export interface SyncRetry {
  reason: SyncFailure
  retryAt: number
}

/**
 * What the shell gates on after sign-in (`Bootstrap`, `src/library/sync.rs`):
 * Home needs a usable catalog. A retry outlives the next attempt, so it stays
 * while `Status.syncing` reports that attempt running.
 */
export type Bootstrap =
  | { state: "loading" }
  | ({ state: "retrying" } & SyncRetry)
  | { state: "ready" }

export interface SyncProgress {
  active: boolean
  phase: SyncPhase
  catalog: BootstrapProgress
  retry: SyncRetry | null
}

export interface SeerrGenre {
  id: number
  name: string
  backdrops: string[]
}

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

export type StopReason =
  | "eof"
  | "watched-next"
  | "stop"
  | "quit"
  | "error"
  | "redirect"
  | "shutdown"
  | "unknown"

export interface PlayerState {
  active: boolean
  playbackId: number | null
  itemId: string | null
  mediaSourceId: string | null
  playSessionId: string | null
  playMethod: string | null
  positionMs: number
  durationMs: number | null
  paused: boolean
  volume: number | null
  mute: boolean | null
  tracks: PlayerTrack[]
  chapters: PlayerChapter[]
  skipSegments: PlayerSkipSegment[]
  diagnostics: PlaybackDiagnostics
  stopReason: StopReason | null
}

/** The `started: false` shape comes back when there is no next episode. */
export interface PlayStarted {
  started: boolean
  itemId?: string
  playMethod?: string
  mediaSource?: string
  startTicks?: number
}

export type PlayerCommand =
  | { command: "mark-watched-next" | "toggle-subtitles" }
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

export type SortOrder = "asc" | "desc"

export type LibraryLetter =
  | "#" | "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H" | "I" | "J" | "K" | "L" | "M"
  | "N" | "O" | "P" | "Q" | "R" | "S" | "T" | "U" | "V" | "W" | "X" | "Y" | "Z"

export type LetterIndex = Partial<Record<LibraryLetter, number>>

export type WatchFilter = "unwatched" | "progress" | "watched"

export type SeriesStatus = "Continuing" | "Ended"

export interface ItemQuery {
  search?: string
  personId?: string
  kind?: string
  genre?: string
  /** Inclusive first year of a standard release decade (for example 1990). */
  decade?: number
  watched?: WatchFilter | ""
  favorite?: boolean
  parental?: string
  seriesStatus?: SeriesStatus | ""
  sort?: string
  order?: SortOrder
  seed?: number
  offset?: number
  limit?: number
}

export type LibraryKind =
  | "movies"
  | "shows"
  | "music"
  | "musicVideos"
  | "homeVideos"
  | "photos"
  | "books"
  | "boxSets"
  | "playlists"
  | "liveTv"
  | "trailers"
  | "mixed"

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

export type LibraryFilterId = "genre" | "decade" | "watched" | "parental" | "seriesStatus" | "favorite"

/** The sorts and filters a grid can run. A request for anything else answers 400. */
export interface BrowseCapabilities {
  sorts: LibrarySortId[]
  filters: LibraryFilterId[]
  letterIndex: boolean
}

export interface Library {
  id: string
  name: string
  kind: LibraryKind
  imageTag: string | null
  /** True while a movies or TV shows library owes a membership walk or one is running, so its grid may lack titles. A full walk (a refresh, new titles, a deletion sweep, the first fill) sets it for every catalog library until that library's walk lands. */
  membershipPending: boolean
  browse: BrowseCapabilities
}

export interface LibrariesResponse {
  /** False until a sync cycle has read the server's library list. */
  known: boolean
  /** In the user's Jellyfin order. */
  libraries: Library[]
  crossLibrary: BrowseCapabilities
}

export interface LibraryQuery {
  genre?: string
  decade?: number
  watched?: WatchFilter | ""
  favorite?: boolean
  parental?: string
  seriesStatus?: SeriesStatus | ""
  sort?: LibrarySortId
  order?: SortOrder
  seed?: number
}

export interface LibraryFilterOptions {
  genres: string[]
  officialRatings: string[]
}
