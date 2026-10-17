// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type AppStatus = {
  library: LibraryStats,
  syncing: boolean,
  lastSync: string | null,
  bootstrap: Bootstrap,
  syncProgress: SyncProgress,
  companion: CompanionStatus,
  authenticated: boolean,
  expired: boolean,
  serverUrl: string | null,
  serverName: string | null,
  userId: string | null,
  userName: string | null,
  deviceId: string,
};

export type AppearanceAccent =
  | "signal"
  | "cobalt"
  | "amber"
  | "violet";

export type AppearanceDensity =
  | "compact"
  | "comfortable";

export type AppearanceView = {
  accent: AppearanceAccent,
  density: AppearanceDensity,
  artworkIntensity: number,
  backdropIntensity: number,
  reducedMotion: boolean,
  cardPreviews: boolean,
  showMediaInfo: boolean,
  /**
   * Canonical IDs from the fixed public MDBList source catalog.
   */
  ratingSources: Array<string>,
};

export type ApplicationView = {
  closeBehavior: CloseBehavior,
  showScrollbars: boolean,
  logLevel: string,
};

export type Billboard = {
  items: Array<ItemSummary>,
};

export type Bootstrap =
  | { "state": "loading" }
  | { "state": "retrying" } & Retry
  | { "state": "ready" };

export type BootstrapProgress = {
  complete: boolean,
  processed: number,
  total: number | null,
};

export type BrowseCapabilities = {
  sorts: Array<SortKey>,
  filters: Array<FilterKey>,
  letterIndex: boolean,
};

export type BrowsingRoutes = { [key in string]: string };

export type BrowsingSaved = {
  saved: boolean,
};

export type Capabilities = {
  platform: Platform,
  libmpv: boolean,
  integratedLibmpvOverlay: boolean,
  mpvInstaller: boolean,
};

export type ClassifiedTitle = {
  localItems: Array<LocalItem>,
  title: string,
  originalTitle: string | null,
  year: number | null,
  overview: string,
  releaseDate: string | null,
  sourceOrder: number,
  posterPath: string | null,
  backdropPath: string | null,
  adult: boolean,
  mediaType: MediaType,
  tmdbId: number,
};

export type ClientView = {
  player: PlayerView,
  playback: PlaybackView,
  application: ApplicationView,
};

export type CloseBehavior =
  | "exit_app"
  | "minimize_window";

export type CollectionMode =
  | "mediaFlick"
  | "jellyfin";

export type CollectionProfile = {
  id: string,
  revision: string,
  template: TemplateReference,
  title: string,
  description: string,
  customPosterId: string | null,
  source: CollectionSource,
  mediaType: MediaType,
  limit: ResultLimit,
  cadence: RefreshCadence,
  availableOnHome: boolean,
};

export type CollectionProfileDetail = {
  profile: CollectionProfile,
  status: ResultsStatus,
  owned: Array<ClassifiedTitle>,
  missing: Array<NormalizedTitle>,
  items: Array<NormalizedTitle>,
  libraryItems: Array<ItemSummary>,
  /**
   * Sent only once results are ready.
   */
  ownershipAvailable?: boolean,
  refresh: RefreshState,
  /**
   * Sent only once results are ready.
   */
  overdue?: boolean,
};

export type CollectionProfilesIndex = {
  profiles: Array<CollectionProfile>,
  errors: { [key in string]: string },
};

export type CollectionRecovery = {
  damagedPath: string,
  restoredBackup: boolean,
};

export type CollectionSettings = {
  effectiveMode: CollectionMode,
  mediaFlickAvailable: boolean,
  modeSelection: CollectionMode | null,
  franchises: FranchiseSettings,
  recovery: CollectionRecovery | null,
  readiness: ProviderReadiness,
};

export type CollectionSource =
  | { "kind": "tmdbDiscover", parameters: { [key in string]: JsonValue } }
  | { "kind": "tmdbCollection", collectionId: number, includeUnreleased: boolean }
  | { "kind": "mdbListPublicList", listId: string };

export type CollectionTemplate = {
  title: string,
  description: string,
  category: TemplateCategory,
  pictogram: TemplatePictogram,
  source: CollectionSource,
  mediaType: MediaType,
  limit: ResultLimit,
  cadence: RefreshCadence,
  id: string,
};

export type CollectionTemplateOption = {
  template: CollectionTemplate,
  /**
   * The provider the template reads from is ready.
   */
  available: boolean,
};

export type CollectionTemplates = {
  categories: Array<TemplateCategory>,
  templates: Array<CollectionTemplateOption>,
  readiness: ProviderReadiness,
};

export type CollectionTitle = {
  item: NormalizedTitle,
};

export type CommittedProfile = {
  profile: CollectionProfile,
  total: number,
};

export type CompanionInfo = {
  pluginVersion: string,
  apiVersion: number,
  capabilities: Array<string>,
  services: { [key in string]: boolean },
};

export type CompanionStatus = {
  available: boolean,
  compatible: boolean,
  checked: boolean,
  info: CompanionInfo | null,
  error: string | null,
  supportedApi: SupportedApi,
};

export type DetailView =
  | { "layout": "title" } & TitleDetail
  | { "layout": "generic" } & GenericItemDetail;

export type EffectiveOrigin =
  | "plugin"
  | "none";

export type ErrorBody = {
  error: string,
  expired?: boolean,
};

export type ExternalLinkOpened = {
  opened: boolean,
  url: string,
};

export type ExternalProfile = {
  id: string,
  provider: string,
  profileKey: string,
  displayName: string,
  canonicalUrl: string,
  enabled: boolean,
  verificationStatus: string,
  createdAt: number,
  lastCheckedAt: number | null,
};

export type FavoriteState = {
  favorite: boolean,
};

export type FilterKey =
  | "genre"
  | "decade"
  | "watched"
  | "parental"
  | "seriesStatus"
  | "favorite";

export type FilterOptions = {
  genres: Array<string>,
  officialRatings: Array<string>,
};

export type FranchiseCollection = {
  collectionId: number,
  name: string,
  posterPath: string | null,
  backdropPath: string | null,
  owned: Array<ClassifiedTitle>,
  missing: Array<NormalizedTitle>,
  items: Array<NormalizedTitle>,
  libraryItems: Array<ItemSummary>,
  ownershipAvailable: boolean,
};

export type FranchiseCollectionSummary = {
  collectionId: number,
  name: string,
  posterPath: string | null,
  backdropPath: string | null,
  ownedCount: number,
  missingCount: number,
  ownershipAvailable: boolean,
};

export type FranchiseCollectionsIndex = {
  status: ResultsStatus,
  franchises: Array<FranchiseCollectionSummary>,
};

export type FranchiseSettings = {
  includeUnreleased: boolean,
};

export type FullscreenBehavior =
  | "fullscreen"
  | "windowed";

export type GenericItemDetail = {
  genres: Array<string>,
  isFolder: boolean,
  dateCreated: string | null,
  id: string,
  kind: string,
  name: string,
  year: number | null,
  runtimeTicks: number | null,
  communityRating: number | null,
  officialRating: string | null,
  seriesId: string | null,
  seriesName: string | null,
  indexNumber: number | null,
  parentIndexNumber: number | null,
  primaryImageTag: string | null,
  thumbImageTag: string | null,
  logoImageTag: string | null,
  backdropImageTag: string | null,
  childCount: number | null,
  premiereDate: string | null,
  seasonId: string | null,
  played: boolean,
  playCount: number,
  positionTicks: number,
  favorite: boolean,
  unplayedCount: number | null,
  /**
   * Width over height of the primary image. Null for cached movies and
   * shows, whose art is a 2:3 poster.
   */
  primaryAspect: number | null,
  /**
   * `/api/item/{id}/children` carries this for a season's episodes, from
   * its live server reconcile, and a generic item's details carry their
   * own. The grid queries do not, and offline rows have none.
   */
  overview?: string,
  /**
   * The slot this item fills among a playlist's children, counted from
   * zero. Jellyfin's PlaylistItemId is the item's own id, so it cannot tell
   * two slots of one item apart.
   */
  playlistIndex?: number,
};

export type Genres = {
  genres: Array<string>,
};

export type HomeBuiltIn =
  | "watching"
  | "becauseYouWatched"
  | "recentlyAdded"
  | "recentlyAddedShows"
  | "upcoming"
  | "latestMovies"
  | "latestShows"
  | "myList";

export type HomeConfiguration = {
  billboard: boolean,
  watching: HomeWatchingSettings,
  elements: Array<HomeElementView>,
};

export type HomeElementCategory =
  | "Built-in"
  | "Genre"
  | "My Collection"
  | "Library";

export type HomeElementView = { label: string, available: boolean, category: HomeElementCategory, enabled: boolean } & ({ "kind": "builtIn", id: HomeBuiltIn } | { "kind": "genre", id: string } | { "kind": "collection", id: string } | { "kind": "library", id: LibraryId });

export type HomeResponse = {
  configuration: HomeConfiguration,
  continueWatching: Array<ItemSummary>,
  rows: Array<HomeRow>,
};

export type HomeResumeResponse = {
  continueWatching: Array<ItemSummary>,
  nextUp: Array<ItemSummary>,
};

export type HomeRow = { title: string, items: Array<ItemSummary> } & ({ "kind": "builtIn", id: HomeBuiltIn } | { "kind": "genre", id: string } | { "kind": "collection", id: string });

export type HomeSettingsResponse = {
  settings: HomeConfiguration,
  defaults: HomeConfiguration,
  collectionMode: CollectionMode,
};

export type HomeWatchingSettings = {
  continueWatching: boolean,
  nextUp: boolean,
  combine: boolean,
};

export type ItemAbout = {
  overview: string | null,
  criticRating: number | null,
  people: Array<Person>,
  tags: Array<string>,
  studios: Array<string>,
};

export type ItemLetters = {
  letters: { [key in string]: number },
};

export type ItemPage = {
  items: Array<ItemSummary>,
  total: number,
};

export type ItemRatings = {
  id: string,
  ratings: Array<NormalizedRating>,
  origin: RatingOrigin,
  fetchedAt: number,
  sourceUpdatedAt: string | null,
  stale: boolean,
  schemaVersion: number,
};

export type ItemSummary = {
  id: string,
  kind: string,
  name: string,
  year: number | null,
  runtimeTicks: number | null,
  communityRating: number | null,
  officialRating: string | null,
  seriesId: string | null,
  seriesName: string | null,
  indexNumber: number | null,
  parentIndexNumber: number | null,
  primaryImageTag: string | null,
  thumbImageTag: string | null,
  logoImageTag: string | null,
  backdropImageTag: string | null,
  childCount: number | null,
  premiereDate: string | null,
  seasonId: string | null,
  played: boolean,
  playCount: number,
  positionTicks: number,
  favorite: boolean,
  unplayedCount: number | null,
  /**
   * Width over height of the primary image. Null for cached movies and
   * shows, whose art is a 2:3 poster.
   */
  primaryAspect: number | null,
  /**
   * `/api/item/{id}/children` carries this for a season's episodes, from
   * its live server reconcile, and a generic item's details carry their
   * own. The grid queries do not, and offline rows have none.
   */
  overview?: string,
  /**
   * The slot this item fills among a playlist's children, counted from
   * zero. Jellyfin's PlaylistItemId is the item's own id, so it cannot tell
   * two slots of one item apart.
   */
  playlistIndex?: number,
};

export type ItemSynopsis = {
  overview: string | null,
};

export type ItemTechnical = {
  id: string,
  mediaStreams: Array<MediaStreamView>,
};

export type JellyfinCollectionDetail = {
  id: string,
  name: string,
  primaryImageTag: string | null,
  backdropImageTag: string | null,
  items: Array<ItemSummary>,
  totalRecordCount: number,
};

export type JellyfinCollectionSummary = {
  id: string,
  name: string,
  primaryImageTag: string | null,
  backdropImageTag: string | null,
  itemCount: number | null,
};

export type JellyfinCollections = {
  collections: Array<JellyfinCollectionSummary>,
};

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | Array<JsonValue>
  | { [key in string]: JsonValue };

export type LetterboxdProfileRemoved = {
  removed: boolean,
};

export type LetterboxdProfileSaved = {
  profile: ExternalProfile,
};

export type LetterboxdProfiles = {
  profiles: Array<ExternalProfile>,
};

export type LibrariesResponse = {
  /**
   * False until a sync cycle has read the server's library list.
   */
  known: boolean,
  /**
   * In the user's Jellyfin order.
   */
  libraries: Array<LibraryView>,
  crossLibrary: BrowseCapabilities,
};

export type LibraryId = string;

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
  | "mixed";

export type LibraryLatest = {
  items: Array<ItemSummary>,
  sort: SortKey,
};

export type LibraryStats = {
  movies: number,
  series: number,
  seasons: number,
  episodes: number,
  total: number,
};

export type LibraryView = {
  id: LibraryId,
  name: string,
  kind: LibraryKind,
  imageTag: string | null,
  /**
   * True while a movies or TV shows library's titles are not known yet:
   * during the first catalog download, and until the library's first
   * membership walk lands. A later walk keeps the stored titles, so it
   * never sets this.
   */
  membershipPending: boolean,
  browse: BrowseCapabilities,
};

export type LocalItem = {
  id: string,
  name: string,
  kind: string,
  played: boolean,
};

export type MediaInfoResponse = {
  sources: Array<MediaSourceView>,
  playbackPreference: ResolvedPlaybackPreference | null,
};

export type MediaSourceView = {
  id: string | null,
  name: string,
  container: string | null,
  fileName: string | null,
  size: number | null,
  bitrate: number | null,
  defaultAudioStreamIndex: number | null,
  defaultSubtitleStreamIndex: number | null,
  video: Array<MediaStreamView>,
  audio: Array<MediaStreamView>,
  subtitles: Array<MediaStreamView>,
};

export type MediaStreamView = {
  index: number,
  type: string | null,
  codec: string | null,
  profile: string | null,
  language: string | null,
  title: string | null,
  displayTitle: string | null,
  width: number | null,
  height: number | null,
  channels: number | null,
  audioSpatialFormat: string | null,
  videoRange: string | null,
  videoRangeType: string | null,
  bitDepth: number | null,
  isDefault: boolean,
  isForced: boolean,
  isHearingImpaired: boolean,
  isExternal: boolean,
};

export type MediaType =
  | "movie"
  | "series"
  | "mixed";

export type MemberReview = {
  profileId: string,
  username: string,
  displayName: string,
  profileUrl: string,
  /**
   * Canonical Letterboxd film entry; absent if the feed supplied an unsafe URL.
   */
  entryUrl: string | null,
  rating: number | null,
  /**
   * Native code strips provider HTML before this reaches React.
   */
  review: string | null,
  reviewTruncated: boolean,
  watchedDate: string | null,
  /**
   * True when a failed refresh fell back to an older in-memory feed.
   */
  stale: boolean,
};

export type MovieCollection = {
  tmdbId: number,
  collection: MovieFranchise | null,
};

export type MovieFranchise = {
  id: number,
  name: string,
};

export type MpvHelpOpened = {
  opened: boolean,
};

export type NextEpisode =
  | "off"
  | "ask"
  | "auto";

export type NextUpResponse = {
  item: ItemSummary | null,
};

export type NormalizedRating = {
  sourceId: string,
  value: number,
  /**
   * The source's 0 to 100 score, when it reports one.
   */
  score: number | null,
  votes: number | null,
  scaleMax: number,
};

export type NormalizedTitle = {
  title: string,
  originalTitle: string | null,
  year: number | null,
  overview: string,
  releaseDate: string | null,
  sourceOrder: number,
  posterPath: string | null,
  backdropPath: string | null,
  adult: boolean,
  mediaType: MediaType,
  tmdbId: number,
};

export type OfficialRatings = {
  officialRatings: Array<string>,
};

export type Person = {
  id: string | null,
  name: string | null,
  role: string | null,
  type: string | null,
  imageTag: string | null,
};

export type PersonIdentity = {
  jellyfinId: string,
  tmdbId: number | null,
  name: string,
  imageTag: string | null,
};

export type PersonResolution = {
  person: PersonIdentity | null,
  candidates: Array<PersonIdentity>,
  ambiguous: boolean,
};

export type Platform =
  | "windows"
  | "macos"
  | "linux"
  | "other";

export type PlayStarted = {
  started: boolean,
  itemId?: string,
  playMethod?: string,
  mediaSource?: string,
  startTicks?: number,
};

export type PlaybackDiagnostics = {
  bufferedUntilMs: number | null,
  buffering: boolean,
  droppedFrames: number | null,
  frameRate: number | null,
};

export type PlaybackNeighbors = {
  previous: ItemSummary | null,
  next: ItemSummary | null,
};

export type PlaybackPreferenceSaved = {
  playbackPreference: ResolvedPlaybackPreference | null,
};

export type PlaybackView = {
  streamingQuality: StreamingQuality,
  skipIntro: SegmentSkipMode,
  skipCredits: SegmentSkipMode,
  skipRecap: SegmentSkipMode,
  skipCommercial: SegmentSkipMode,
};

export type PlayedState = {
  played: boolean,
};

export type PlayerBackend =
  | "libmpv"
  | "mpv";

export type PlayerChapter = {
  title: string,
  startMs: number,
};

export type PlayerComfort = {
  subtitleSize: number,
  subtitleOutline: number,
  subtitleBackground: number,
  subtitlePosition: number,
  seekBackSeconds: number,
  seekForwardSeconds: number,
  pauseKey: string,
  muteKey: string,
  fullscreenKey: string,
  seekBackKey: string,
  seekForwardKey: string,
  stopKey: string,
  subtitlesKey: string,
  seekBackThirtyKey: string,
  seekForwardThirtyKey: string,
};

export type PlayerCommandAccepted = {
  accepted: boolean,
};

export type PlayerCommandBody =
  | { "command": "pause" }
  | { "command": "resume" }
  | { "command": "seek", positionMs: number }
  | { "command": "set-volume", volume: number }
  | { "command": "set-mute", mute: boolean }
  | { "command": "set-audio-delay", delaySeconds: number }
  | { "command": "set-subtitle-delay", delaySeconds: number }
  | { "command": "set-subtitle-scale", scale: number }
  | { "command": "set-video-fit", fit: VideoFitBody }
  | { "command": "set-video-aspect", aspect: VideoAspectBody }
  | { "command": "set-deinterlace", enabled: boolean }
  | { "command": "set-tone-mapping", mode: ToneMappingBody }
  | { "command": "set-audio-track", audioTrack: number }
  | { "command": "set-subtitle-track", subtitleTrack: number | null }
  | { "command": "toggle-subtitles" }
  | { "command": "toggle-fullscreen" }
  | { "command": "stop" }
  | { "command": "mark-watched-next" };

export type PlayerSnapshot = {
  active: boolean,
  playbackId: number | null,
  itemId: string | null,
  mediaSourceId: string | null,
  playSessionId: string | null,
  playMethod: string | null,
  positionMs: number,
  durationMs: number | null,
  paused: boolean,
  volume: number | null,
  mute: boolean | null,
  tracks: Array<PlayerTrack>,
  chapters: Array<PlayerChapter>,
  skipSegments: Array<SkipSegment>,
  diagnostics: PlaybackDiagnostics,
  stopReason: StopReason | null,
};

export type PlayerTrack = {
  id: number,
  kind: PlayerTrackKind,
  language: string | null,
  title: string | null,
  codec: string | null,
  selected: boolean,
  external: boolean,
};

export type PlayerTrackKind =
  | "audio"
  | "subtitle";

export type PlayerView = {
  playerBackend: PlayerBackend,
  mpvPath: string | null,
  defaultFullscreen: FullscreenBehavior,
  markWatchedNext: string | null,
  comfort: PlayerComfort,
  playerConfigured: boolean,
};

export type ProfileDeleted = {
  deleted: boolean,
};

export type ProviderIds = {
  tmdb: string | null,
  imdb: string | null,
  tvdb: string | null,
};

export type ProviderReadiness = {
  tmdb: boolean,
  mdblist: boolean,
};

export type ProviderResult = {
  items: Array<NormalizedTitle>,
  total: number,
  movies: number,
  series: number,
  sourceIdentity: string | null,
};

export type QuickConnectPoll = {
  authenticated: boolean,
  /**
   * Sent once the code was approved and the session signed in.
   */
  session?: SessionStatus,
};

export type QuickConnectStart = {
  serverUrl: string,
  code: string,
  secret: string,
};

export type RatingFormat =
  | "percent"
  | "decimal"
  | "integer"
  | "stars";

export type RatingOrigin = "plugin";

export type RatingSourceDefinition = {
  id: string,
  label: string,
  shortLabel: string,
  scaleMax: number,
  format: RatingFormat,
  known: boolean,
};

export type RatingsBatchResponse = {
  available: boolean,
  effectiveOrigin: EffectiveOrigin,
  items: Array<ItemRatings>,
  retryAt: number | null,
  diagnostic: string | null,
};

export type RatingsIntegrationStatus = {
  boundaryVersion: number,
  effectiveOrigin: EffectiveOrigin,
  available: boolean,
  selectionEnabled: boolean,
  plugin: RatingsPluginStatus,
  sources: Array<RatingSourceDefinition>,
  selectedSources: Array<string>,
};

export type RatingsPluginStatus = {
  available: boolean,
  capability: string,
  boundaryVersion: number,
  detail: string,
};

export type Recovery = {
  area: string,
  restoredBackup: boolean,
};

export type RefreshCadence =
  | "manual"
  | "daily"
  | "weekly"
  | "monthly";

export type RefreshState = {
  lastAttempt: number | null,
  lastSuccess: number | null,
  latestFailure: string | null,
  nextDue: number | null,
  initialized: boolean,
};

export type ResolvedPlaybackPreference = {
  mediaSourceId: string | null,
  mediaSourceIndex: number,
  audioStreamIndex: number | null,
  /**
   * `None` is subtitles off. Playback translates it to Jellyfin/mpv's off
   * sentinel only when an existing stored row made that choice explicit.
   */
  subtitleStreamIndex: number | null,
};

export type ResultLimit =
  | { "kind": "all" }
  | { "kind": "maximum", count: number };

export type ResultsStatus =
  | "updating"
  | "resultsUnavailable"
  | "ready";

export type Retry = {
  reason: SyncFailure,
  retryAt: number,
};

export type ReviewLookup = {
  reviews: Array<MemberReview>,
  configuredProfiles: number,
  unavailableProfiles: number,
};

export type SeerrRequestCancelled = {
  cancelled: boolean,
  id: number,
};

export type SegmentSkipMode =
  | "hidden"
  | "disabled"
  | "prompt"
  | "always";

export type SegmentType =
  | "intro"
  | "outro"
  | "recap"
  | "commercial";

export type ServerInfo = {
  serverUrl: string,
  serverName: string,
  version: string,
  quickConnect: boolean,
};

export type SessionStatus = {
  authenticated: boolean,
  expired: boolean,
  serverUrl: string | null,
  serverName: string | null,
  userId: string | null,
  userName: string | null,
  deviceId: string,
};

export type SettingsView = {
  client: ClientView,
  appearance: AppearanceView,
  capabilities: Capabilities,
  recoveries: Array<Recovery>,
  serverUrl: string | null,
};

export type ShellQueued = {
  queued: boolean,
};

export type ShellRequestQueued = {
  requestId: string,
  queued: boolean,
};

export type SkipSegment = {
  segmentType: SegmentType,
  startTicks: number,
  endTicks: number,
  triggered: boolean,
};

export type SortKey =
  | "name"
  | "year"
  | "added"
  | "episode-added"
  | "rating"
  | "played"
  | "plays"
  | "runtime"
  | "random";

export type StartupResponse = {
  status: AppStatus,
  settings: SettingsView,
  viewing: ViewingView | null,
  browsing: BrowsingRoutes | null,
  home: HomeResponse | null,
  billboard: Billboard | null,
};

export type StartupView =
  | "home"
  | "calendar"
  | "last"
  | { "library": LibraryId };

export type StopReason =
  | "eof"
  | "watched-next"
  | "stop"
  | "quit"
  | "error"
  | "redirect"
  | "shutdown"
  | "unknown";

export type StreamingQuality =
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
  | "1_5_mbps";

export type SubtitleMode =
  | "server"
  | "off"
  | "forced"
  | "always"
  | "foreignAudio";

export type SupportedApi = {
  min: number,
  max: number,
};

export type SyncFailure =
  | "unreachable"
  | "server"
  | "storage";

export type SyncPhase =
  | "catalog"
  | "reconciling"
  | "complete";

export type SyncProgress = {
  active: boolean,
  phase: SyncPhase,
  catalog: BootstrapProgress,
  retry: Retry | null,
};

export type SyncRequested = {
  requested: boolean,
};

export type TechnicalBatch = {
  items: Array<ItemTechnical>,
};

export type TemplateCategory =
  | "trending"
  | "popular"
  | "streamingServices"
  | "topRated"
  | "inTheaters"
  | "upcoming"
  | "onAir"
  | "editorial"
  | "custom";

export type TemplatePictogram =
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
  | "zap";

export type TemplateReference = {
  id: string,
};

export type TitleDetail = {
  /**
   * Always a catalog library, whose profile offers the genre filter: only
   * catalog libraries hold memberships. The UI's genre links rely on it.
   */
  libraryId: LibraryId | null,
  genres: Array<string>,
  originalTitle: string | null,
  providerIds: ProviderIds,
  parentId: string | null,
  dateCreated: string | null,
  id: string,
  kind: string,
  name: string,
  year: number | null,
  runtimeTicks: number | null,
  communityRating: number | null,
  officialRating: string | null,
  seriesId: string | null,
  seriesName: string | null,
  indexNumber: number | null,
  parentIndexNumber: number | null,
  primaryImageTag: string | null,
  thumbImageTag: string | null,
  logoImageTag: string | null,
  backdropImageTag: string | null,
  childCount: number | null,
  premiereDate: string | null,
  seasonId: string | null,
  played: boolean,
  playCount: number,
  positionTicks: number,
  favorite: boolean,
  unplayedCount: number | null,
  /**
   * Width over height of the primary image. Null for cached movies and
   * shows, whose art is a 2:3 poster.
   */
  primaryAspect: number | null,
  /**
   * `/api/item/{id}/children` carries this for a season's episodes, from
   * its live server reconcile, and a generic item's details carry their
   * own. The grid queries do not, and offline rows have none.
   */
  overview?: string,
  /**
   * The slot this item fills among a playlist's children, counted from
   * zero. Jellyfin's PlaylistItemId is the item's own id, so it cannot tell
   * two slots of one item apart.
   */
  playlistIndex?: number,
};

export type ToneMappingBody =
  | "auto"
  | "clip"
  | "mobius"
  | "reinhard"
  | "hable"
  | "bt.2390";

export type TrailerResponse = {
  trailer: TrailerSummary | null,
};

export type TrailerSummary = {
  id: string | null,
  name: string,
  embedUrl: string | null,
};

export type UploadedArtwork = {
  id: string,
};

export type VideoAspectBody =
  | "source"
  | "4:3"
  | "16:9"
  | "21:9";

export type VideoFitBody =
  | "fit"
  | "fill";

export type ViewingView = {
  spoilerProtection: boolean,
  nextEpisode: NextEpisode,
  countdownSeconds: number,
  /**
   * Zero leaves continuous playback unlimited.
   */
  episodeLimit: number,
  audioLanguages: Array<string>,
  subtitleLanguages: Array<string>,
  preferOriginalAudio: boolean,
  subtitleMode: SubtitleMode,
  resumeRewindSeconds: number,
  textScale: number,
  posterSize: number,
  previewDelayMs: number,
  startupDestination: StartupView,
  rememberFilters: boolean,
  hideWatched: boolean,
};
