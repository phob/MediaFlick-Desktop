// Typed client for the JSON API served by `src/shell/cef/api.rs` on the
// `mediaflick-desktop://app/` scheme.

import { isJsonObject, jsonBoolean, jsonString, type JsonValue } from "../json.ts"
import type {
  AppearanceSettings,
  ClientSettings,
  CollectionMode,
  CollectionPreview,
  CollectionProfile,
  CollectionProfileDetail,
  CollectionProfileDraft,
  CollectionProfilesIndex,
  CollectionSettings,
  CollectionTemplates,
  CompanionStatus,
  FranchiseCollection,
  FranchiseCollectionsIndex,
  HomeResponse,
  HomeResumeResponse,
  HomeSettingsResponse,
  HomeSettingsWrite,
  ItemAbout,
  ItemDetail,
  ItemQuery,
  ItemSummary,
  ItemSynopsis,
  ItemTechnical,
  JellyfinCollectionDetail,
  JellyfinCollectionSummary,
  LetterboxdProfile,
  LetterboxdReviewsResponse,
  MediaInfoResponse,
  MovieCollection,
  NormalizedCollectionTitle,
  PersonResolution,
  PersonResolveQuery,
  PlayStarted,
  PlaybackTrackPreference,
  PlaybackTrackPreferenceWrite,
  PlayerCommand,
  PlayerSettingsWrite,
  PlayerState,
  PublicCollectionList,
  QuickConnectStart,
  RatingsBatchResponse,
  RatingsIntegrationStatus,
  ReleaseCalendar,
  SeerrDiscoverFilters,
  SeerrDiscoverRow,
  SeerrGenre,
  SeerrMediaDetail,
  SeerrMediaType,
  SeerrPage,
  SeerrPersonCreditsPage,
  SeerrRequest,
  SeerrRequestOptions,
  SeerrResult,
  SeerrStatusInfo,
  ServerInfo,
  StartupResponse,
  Status,
  StreamingQualityId,
  TrailerSummary,
  ViewingSettings,
} from "./types.ts"
import { queryString, type ExternalProvider } from "./urls.ts"

export const PAGE_SIZE = 60

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

/** Every route answers JSON; a failure carries the `{ error, expired }` envelope. */
async function readResponse<T>(response: Response): Promise<T> {
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
  // SAFETY: The embedded UI and Rust shell ship together, and every call names
  // the response type owned by that same-version local API route.
  return payload as T
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const init: RequestInit = { method: options.method ?? "GET", signal: options.signal }
  if (options.body !== undefined) {
    init.headers = { "Content-Type": "application/json" }
    init.body = JSON.stringify(options.body)
  }
  return readResponse<T>(await fetch(path, init))
}

async function upload<T>(path: string, body: ArrayBuffer, signal?: AbortSignal): Promise<T> {
  return readResponse<T>(await fetch(path, { method: "POST", body, signal }))
}

export const api = {
  viewing: () => request<ViewingSettings>("/api/settings/viewing"),
  saveViewing: (value: ViewingSettings) => request<ViewingSettings>("/api/settings/viewing", { method: "PATCH", body: value }),
  browsing: () => request<Record<string, string>>("/api/settings/browsing"),
  saveBrowsing: (page: string, route: string) => request<{ saved: boolean }>("/api/settings/browsing", { method: "PATCH", body: { page, route } }),
  status: () => request<Status>("/api/status"),
  startup: (home: boolean) => request<StartupResponse>(`/api/startup${home ? "?home=1" : ""}`),
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
    filePicker: (requestId: string) =>
      request<{ requestId: string; queued: boolean }>("/api/shell/file-picker", {
        method: "POST",
        body: { requestId },
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
  play: (itemId: string, resume: boolean, quality?: StreamingQualityId) => {
    window.dispatchEvent(new Event("mediaflick-manual-play"))
    return request<PlayStarted>("/api/play", { method: "POST", body: { itemId, resume, quality } })
  },
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
    uploadArtwork: (body: ArrayBuffer, signal?: AbortSignal) =>
      upload<{ id: string }>("/api/collections/artwork", body, signal),
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
