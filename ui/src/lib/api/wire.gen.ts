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

export type Bootstrap =
  | { "state": "loading" }
  | { "state": "retrying" } & Retry
  | { "state": "ready" };

export type BootstrapProgress = {
  complete: boolean,
  processed: number,
  total: number | null,
};

export type Capabilities = {
  platform: Platform,
  libmpv: boolean,
  integratedLibmpvOverlay: boolean,
  mpvInstaller: boolean,
};

export type ClientView = {
  player: PlayerView,
  playback: PlaybackView,
  application: ApplicationView,
};

export type CloseBehavior =
  | "exit_app"
  | "minimize_window";

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

export type ErrorBody = {
  error: string,
  expired?: boolean,
};

export type FilterOptions = {
  genres: Array<string>,
  officialRatings: Array<string>,
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

export type LibraryId = string;

export type LibraryStats = {
  movies: number,
  series: number,
  seasons: number,
  episodes: number,
  total: number,
};

export type Platform =
  | "windows"
  | "macos"
  | "linux"
  | "other";

export type PlaybackDiagnostics = {
  bufferedUntilMs: number | null,
  buffering: boolean,
  droppedFrames: number | null,
  frameRate: number | null,
};

export type PlaybackView = {
  streamingQuality: StreamingQuality,
  skipIntro: SegmentSkipMode,
  skipCredits: SegmentSkipMode,
  skipRecap: SegmentSkipMode,
  skipCommercial: SegmentSkipMode,
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

export type ProviderIds = {
  tmdb: string | null,
  imdb: string | null,
  tvdb: string | null,
};

export type Recovery = {
  area: string,
  restoredBackup: boolean,
};

export type Retry = {
  reason: SyncFailure,
  retryAt: number,
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

export type SettingsView = {
  client: ClientView,
  appearance: AppearanceView,
  capabilities: Capabilities,
  recoveries: Array<Recovery>,
  serverUrl: string | null,
};

export type SkipSegment = {
  segmentType: SegmentType,
  startTicks: number,
  endTicks: number,
  triggered: boolean,
};

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

export type VideoAspectBody =
  | "source"
  | "4:3"
  | "16:9"
  | "21:9";

export type VideoFitBody =
  | "fit"
  | "fill";
