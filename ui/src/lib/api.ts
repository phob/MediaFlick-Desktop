// Typed client for the JSON API served by `src/shell/cef/api.rs` on the
// `mediaflick-desktop://app/` scheme. Shapes mirror `summary_row` / `detail_row`
// in `src/library/mod.rs` and the handlers in `api.rs`.

export const TICKS_PER_MS = 10_000
export const POSTER_WIDTH = 400
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
  childCount: number | null
  premiereDate: string | null
  seasonId: string | null
  played: boolean
  playCount: number
  positionTicks: number
  favorite: boolean
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
  backdropImageTag: string | null
  criticRating: number | null
  originalTitle: string | null
  providerIds: { tmdb: string | null; imdb: string | null; tvdb: string | null }
  parentId: string | null
  dateCreated: string | null
}

export interface HomeRow {
  id: "resume" | "nextUp" | "recent"
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
  [key: string]: unknown
}

export interface ClientSettings {
  streamingQuality: string
  playerBackend: string
  playerConfigured: boolean
  serverUrl: string | null
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
    const envelope = payload as { error?: string; expired?: boolean } | null
    throw new ApiError(
      envelope?.error ?? `request failed (${response.status})`,
      response.status,
      Boolean(envelope?.expired),
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
  settings: () => request<ClientSettings>("/api/settings"),

  connect: (server: string) => request<unknown>("/api/auth/connect", { method: "POST", body: { server } }),
  login: (server: string, username: string, password: string) =>
    request<Status>("/api/auth/login", { method: "POST", body: { server, username, password } }),
  quickConnectStart: (server: string) =>
    request<{ code?: string; secret?: string }>("/api/auth/quickconnect/start", {
      method: "POST",
      body: { server },
    }),
  quickConnectPoll: (server: string, secret: string) =>
    request<{ authenticated?: boolean } & Status>("/api/auth/quickconnect/poll", {
      method: "POST",
      body: { server, secret },
    }),
  logout: () => request<Status>("/api/auth/logout", { method: "POST" }),

  home: () => request<{ rows: HomeRow[] }>("/api/home"),
  genres: () => request<{ genres: string[] }>("/api/genres"),
  items: (query: ItemQuery, signal?: AbortSignal) =>
    request<{ items: ItemSummary[]; total: number }>(`/api/items${queryString(query)}`, { signal }),
  item: (id: string) => request<ItemDetail>(`/api/item/${encodeURIComponent(id)}`),
  children: (id: string) =>
    request<{ items: ItemSummary[] }>(`/api/item/${encodeURIComponent(id)}/children`),
  setPlayed: (id: string, played: boolean) =>
    request<unknown>(`/api/item/${encodeURIComponent(id)}/played`, { method: "POST", body: { played } }),
  setFavorite: (id: string, favorite: boolean) =>
    request<unknown>(`/api/item/${encodeURIComponent(id)}/favorite`, {
      method: "POST",
      body: { favorite },
    }),

  play: (itemId: string, resume: boolean) =>
    request<unknown>("/api/play", { method: "POST", body: { itemId, resume } }),
  playNext: (itemId: string) => request<unknown>("/api/play/next", { method: "POST", body: { itemId } }),

  playerState: () => request<PlayerState>("/api/player/state"),
  playerCommand: (name: string, extra: Record<string, unknown> = {}) =>
    request<unknown>("/api/player/command", { method: "POST", body: { command: name, ...extra } }),

  sync: () => request<{ requested: boolean }>("/api/sync", { method: "POST" }),
}

/** Poster/backdrop URL through the Rust image proxy, which keeps the token out of the DOM. */
export function imageUrl(
  item: Pick<ItemSummary, "id" | "primaryImageTag">,
  type: "Primary" | "Backdrop" = "Primary",
  width = POSTER_WIDTH,
) {
  return `/api/image/${encodeURIComponent(item.id)}/${type}${queryString({ width, tag: item.primaryImageTag })}`
}

export function ticksToMs(ticks: number | null | undefined) {
  return (ticks ?? 0) / TICKS_PER_MS
}

export function progressFraction(item: Pick<ItemSummary, "positionTicks" | "runtimeTicks">) {
  if (!item.runtimeTicks || !item.positionTicks) return 0
  return Math.min(1, item.positionTicks / item.runtimeTicks)
}
