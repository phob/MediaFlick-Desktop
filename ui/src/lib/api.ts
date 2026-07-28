// Typed client for the JSON API served by `src/shell/cef/api.rs` on the
// `mediaflick-desktop://app/` scheme. Shapes mirror `summary_row` / `detail_row`
// in `src/library/mod.rs` and the handlers in `api.rs`.

export const TICKS_PER_MS = 10_000
export const POSTER_WIDTH = 400
/** Home progress cards are drawn wider and use 16:9 art. */
export const LANDSCAPE_WIDTH = 560
/** The detail hero renders edge to edge, so its backdrop needs a real width. */
export const BACKDROP_WIDTH = 1920
/** Detail poster and episode thumbnails: bigger slots than the grid's. */
export const DETAIL_POSTER_WIDTH = 600
export const THUMBNAIL_WIDTH = 480
export const HEADSHOT_WIDTH = 200
/** Title treatments are drawn at most ~28rem wide; twice that covers HiDPI. */
export const LOGO_WIDTH = 800
export const PAGE_SIZE = 60

export type ItemKind = "Movie" | "Series" | "Season" | "Episode" | (string & {})

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
  /** Only `/api/item/{id}/children` carries this; the grid queries do not. */
  overview?: string | null
}

export interface Person {
  id: string | null
  name: string | null
  role: string | null
  type: string | null
  imageTag: string | null
}

export interface ItemDetail extends ItemSummary {
  overview: string | null
  genres: string[]
  tags: string[]
  studios: string[]
  people: Person[]
  criticRating: number | null
  originalTitle: string | null
  providerIds: { tmdb: string | null; imdb: string | null; tvdb: string | null }
  parentId: string | null
  dateCreated: string | null
}

/** One track of a media source, as `media_stream_json` in `api.rs` shapes it. */
export interface MediaStream {
  index: number
  codec: string | null
  language: string | null
  title: string | null
  displayTitle: string | null
  width: number | null
  height: number | null
  channels: number | null
  videoRange: string | null
  videoRangeType: string | null
  bitDepth: number | null
  isDefault: boolean
  isForced: boolean
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
  video: MediaStream[]
  audio: MediaStream[]
  subtitles: MediaStream[]
}

export interface TrailerSummary {
  id: string | null
  name: string
  embedUrl: string | null
}

/**
 * External databases the shell can open in the default browser. The UI only
 * names the provider — `external_url` in `api.rs` owns the address, and the
 * kinds listed here mirror the routes it knows, so a button never appears for a
 * link the shell would refuse to build.
 */
export const EXTERNAL_PROVIDERS = [
  { id: "imdb", label: "IMDb", kinds: ["Movie", "Series", "Episode"] },
  { id: "tmdb", label: "TMDb", kinds: ["Movie", "Series"] },
  { id: "tvdb", label: "TVDB", kinds: ["Movie", "Series", "Episode"] },
] as const

export type ExternalProvider = (typeof EXTERNAL_PROVIDERS)[number]["id"]

export function externalLinksFor(item: ItemDetail) {
  return EXTERNAL_PROVIDERS.filter(
    (provider) =>
      Boolean(item.providerIds?.[provider.id]) &&
      (provider.kinds as readonly string[]).includes(item.kind),
  )
}

/**
 * `resume` is Continue Watching and Next Up in one row — `home` in
 * `src/shell/cef/api.rs` merges them, half-watched items first. `favorites` is
 * assembled by the UI from the regular items endpoint for the My List shelf.
 */
export interface HomeRow {
  id: "resume" | "recent" | "favorites"
  title: string
  items: ItemSummary[]
}

export interface LibraryStats {
  [key: string]: unknown
}

export interface Status {
  authenticated?: boolean
  serverUrl?: string | null
  userName?: string | null
  library?: LibraryStats
  syncing?: boolean
  lastSync?: string | null
  bootstrapped?: boolean
  companion?: CompanionStatus
  [key: string]: unknown
}

export interface CompanionInfo {
  pluginVersion: string
  apiVersion: number
  capabilities: string[]
  services: Record<"sonarr" | "radarr" | "seerr", boolean>
}

export interface CompanionStatus {
  available: boolean
  compatible: boolean
  checked: boolean
  info: CompanionInfo | null
  error: string | null
  supportedApi: { min: number; max: number }
}

export type CalendarEntryKind = "episode" | "movie"
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
  monitored: boolean
  hasFile: boolean
  posterUrl: string | null
  libraryItemId: string | null
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

export interface ClientSettings {
  streamingQuality: string
  playerBackend: string
  playerConfigured: boolean
  serverUrl: string | null
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

/** `status_name` in `src/seerr/api/model.rs`. */
export type SeerrStatus =
  | "unknown"
  | "pending"
  | "processing"
  | "partial"
  | "available"
  | "blacklisted"

/** `request_status_name` in `src/seerr/api/model.rs`. */
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
}

export interface SeerrSeason {
  seasonNumber: number
  name: string | null
  episodeCount: number
  airDate: string | null
  status: SeerrStatus
}

export interface SeerrMediaDetail extends SeerrResult {
  runtimeMinutes: number | null
  genres: string[]
  seasons: SeerrSeason[]
}

export interface SeerrPage<T> {
  page: number
  totalPages: number
  totalResults: number
  results: T[]
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
}

export interface SeerrQuotaStatus {
  days: number | null
  limit: number | null
  used: number
  remaining: number | null
  restricted: boolean
}

/** Mirrors `SeerrSession::status` in `src/seerr/mod.rs`. */
export interface SeerrStatusInfo {
  configured: boolean
  linked: boolean
  expired: boolean
  serverUrl: string | null
  instance: {
    movie4kEnabled: boolean
    series4kEnabled: boolean
    partialRequestsEnabled: boolean
  }
  user: { id: number; name: string; avatar: string | null; jellyfinUserId: string | null } | null
  capabilities: SeerrCapabilities | null
  quota: { movie: SeerrQuotaStatus; tv: SeerrQuotaStatus } | null
  mapped?: boolean
}

/** What `POST /api/seerr/connect` reports about the instance it probed. */
export interface SeerrServerInfo {
  serverUrl: string
  version: string
  applicationTitle: string | null
  localLogin: boolean
  mediaServerLogin: boolean
  newSignInAllowed: boolean
  movie4kEnabled: boolean
  series4kEnabled: boolean
  partialRequestsEnabled: boolean
  linked: boolean
}

/**
 * `link` answers `{ method: "password" }` whenever Quick Connect is unavailable
 * on either side — that is the expected answer on every stable Seerr, not an
 * error, so the dialog just shows the password form.
 */
export interface SeerrLinkResult {
  method: "password" | "quickconnect"
  linked: boolean
  secret?: string
  status?: SeerrStatusInfo
}

export const SEERR_DISCOVER_ROWS = [
  { id: "trending", label: "Trending" },
  { id: "movies", label: "Movies" },
  { id: "tv", label: "Series" },
] as const

export type SeerrDiscoverRow = (typeof SEERR_DISCOVER_ROWS)[number]["id"]

/**
 * TMDB art through the Rust proxy. The size is a name from the allowlist in
 * `tmdb_image_url` (`src/seerr/api/client.rs`) — the UI never composes an
 * address, exactly as with the Jellyfin image proxy and the external links.
 */
export function seerrImageUrl(path: string | null | undefined, size = "w300") {
  if (!path) return null
  return `/api/seerr/image/${size}/${encodeURIComponent(path.replace(/^\//, ""))}`
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

export interface PlayerCapabilities {
  chapterMarkers: boolean
  externalSubtitles: boolean
  injectedHotkeys: boolean
  absoluteVolume: boolean
  pushesPosition: boolean
}

export interface PlayerState {
  active: boolean
  playbackId?: string | null
  itemId?: string | null
  mediaSourceId?: string | null
  playSessionId?: string | null
  positionMs?: number
  durationMs?: number
  paused?: boolean
  volume?: number | null
  mute?: boolean
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

export interface ItemQuery {
  search?: string
  kind?: string
  genre?: string
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
  /**
   * The *Seerr* session lapsed, which `from_seerr_error` reports under its own
   * name. Signing out of the app over it would be wrong: the Jellyfin session
   * is untouched, and only the Seerr link needs establishing again.
   */
  readonly seerrExpired: boolean

  constructor(message: string, status: number, expired: boolean, seerrExpired = false) {
    super(message)
    this.name = "ApiError"
    this.status = status
    this.expired = expired
    this.seerrExpired = seerrExpired
  }
}

interface RequestOptions {
  method?: string
  body?: unknown
  signal?: AbortSignal
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const init: RequestInit = { method: options.method ?? "GET", signal: options.signal }
  if (options.body !== undefined) {
    init.headers = { "Content-Type": "application/json" }
    init.body = JSON.stringify(options.body)
  }

  const response = await fetch(path, init)
  let payload: unknown = null
  try {
    payload = await response.json()
  } catch {
    payload = null
  }

  if (!response.ok) {
    const envelope = payload as {
      error?: string
      expired?: boolean
      seerrExpired?: boolean
    } | null
    throw new ApiError(
      envelope?.error ?? `request failed (${response.status})`,
      response.status,
      Boolean(envelope?.expired),
      Boolean(envelope?.seerrExpired),
    )
  }
  return payload as T
}

function queryString(params: object) {
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

  home: () => request<{ rows: HomeRow[] }>("/api/home"),
  billboard: () => request<{ items: ItemSummary[] }>("/api/billboard"),
  genres: () => request<{ genres: string[] }>("/api/genres"),
  items: (query: ItemQuery, signal?: AbortSignal) =>
    request<{ items: ItemSummary[]; total: number }>(`/api/items${queryString(query)}`, { signal }),
  item: (id: string) => request<ItemDetail>(`/api/item/${encodeURIComponent(id)}`),
  children: (id: string) =>
    request<{ items: ItemSummary[] }>(`/api/item/${encodeURIComponent(id)}/children`),
  media: (id: string) =>
    request<{ sources: MediaSource[] }>(`/api/item/${encodeURIComponent(id)}/media`),
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

  /** `quality` overrides the saved Client Settings default for this play only. */
  play: (itemId: string, resume: boolean, quality?: StreamingQualityId) =>
    request<PlayStarted>("/api/play", { method: "POST", body: { itemId, resume, quality } }),
  playNext: (itemId: string) =>
    request<PlayStarted>("/api/play/next", { method: "POST", body: { itemId } }),

  playerState: () => request<PlayerState>("/api/player/state"),
  playerCommand: (command: PlayerCommand) =>
    request<unknown>("/api/player/command", { method: "POST", body: command }),

  sync: () => request<{ requested: boolean }>("/api/sync", { method: "POST" }),
  calendar: (start: string, end: string, signal?: AbortSignal) =>
    request<ReleaseCalendar>(`/api/calendar${queryString({ start, end })}`, { signal }),

  seerr: {
    status: () => request<SeerrStatusInfo>("/api/seerr/status"),
    connect: (server: string) =>
      request<SeerrServerInfo>("/api/seerr/connect", { method: "POST", body: { server } }),
    /** Tries the password-less path; answers `method: "password"` if unavailable. */
    link: () => request<SeerrLinkResult>("/api/seerr/link", { method: "POST" }),
    linkPoll: (secret: string) =>
      request<SeerrLinkResult>("/api/seerr/link/poll", { method: "POST", body: { secret } }),
    linkPassword: (username: string, password: string) =>
      request<SeerrLinkResult>("/api/seerr/link/password", {
        method: "POST",
        body: { username, password },
      }),
    unlink: () => request<SeerrStatusInfo>("/api/seerr/unlink", { method: "POST" }),

    search: (q: string, page = 1, signal?: AbortSignal) =>
      request<SeerrPage<SeerrResult>>(`/api/seerr/search${queryString({ q, page })}`, { signal }),
    discover: (row: SeerrDiscoverRow, page = 1, signal?: AbortSignal) =>
      request<SeerrPage<SeerrResult>>(`/api/seerr/discover/${row}${queryString({ page })}`, {
        signal,
      }),
    media: (mediaType: SeerrMediaType, tmdbId: number) =>
      request<SeerrMediaDetail>(`/api/seerr/media/${mediaType}/${tmdbId}`),

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
    }) => request<SeerrRequest>("/api/seerr/request", { method: "POST", body }),
    cancelRequest: (id: number) =>
      request<{ cancelled: boolean }>(`/api/seerr/request/${id}`, { method: "DELETE" }),
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
  return imageUrl(item, "Backdrop", maxWidth, item.backdropImageTag)
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
