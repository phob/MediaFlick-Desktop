import { isJsonObject, jsonBoolean, jsonString, type JsonValue } from "../json.ts"
import type * as Wire from "./wire.gen.ts"
import type { ReleaseCalendar } from "./types/calendar.ts"
import type { ItemQuery, PersonResolveQuery } from "./types/catalog.ts"
import type { CollectionProfileDraft, PublicCollectionList } from "./types/collections.ts"
import type { HomeSettingsWrite } from "./types/home.ts"
import type { BrowsingPage, LibraryQuery } from "./types/libraries.ts"
import type { PlaybackTrackPreferenceWrite } from "./types/media.ts"
import type { PlayerCommand } from "./types/playback.ts"
import type {
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
} from "./types/seerr.ts"
import type {
  AppearanceSettingsWrite,
  ApplicationSettingsWrite,
  PlaybackSettingsWrite,
  PlayerSettingsWrite,
  StreamingQualityId,
  ViewingSettingsWrite,
} from "./types/settings.ts"
import { link, target, type CallArgs, type RouteOf } from "./routes.ts"
import type { ExternalProvider } from "./urls.ts"

export const PAGE_SIZE = 60

export class ApiError extends Error {
  readonly status: number
  readonly expired: boolean
  constructor(message: string, status: number, expired: boolean) {
    super(message)
    this.name = "ApiError"
    this.status = status
    this.expired = expired
  }
}

type Routes = Wire.Routes

/**
 * Companion JSON that the native app relays without a Rust type, so the
 * generated map marks these routes `relay` and the UI describes them here.
 * Indexing by every relay route makes a new one fail to compile until it is
 * described.
 */
interface RelayAnswerTypes {
  "GET /api/calendar": ReleaseCalendar
  "POST /api/collections/mdblist/search": { lists: PublicCollectionList[] }
  "POST /api/collections/mdblist/validate": PublicCollectionList
  "GET /api/seerr/status": SeerrStatusInfo
  "GET /api/seerr/search": SeerrPage<SeerrResult>
  "GET /api/seerr/person/{tmdbId}/credits": SeerrPersonCreditsPage
  "GET /api/seerr/discover/{kind}": SeerrPage<SeerrResult>
  "GET /api/seerr/genres/{mediaType}": SeerrGenre[]
  "GET /api/seerr/media/{mediaType}/{tmdbId}": SeerrMediaDetail
  "GET /api/seerr/request-options/{mediaType}": SeerrRequestOptions
  "POST /api/seerr/request": SeerrRequest
  "GET /api/seerr/requests": SeerrPage<SeerrRequest>
}
type RelayAnswers = { [K in RouteOf<"relay">]: RelayAnswerTypes[K] }

/** The routes the UI reads as JSON, and what each answers with. */
type Answers = { [K in RouteOf<"json">]: Extract<Routes[K], { kind: "json" }>["response"] } & RelayAnswers
type JsonRoute = keyof Answers

type ApiErrorEnvelope = Partial<Wire.ErrorBody>

function readApiErrorEnvelope(value: JsonValue): ApiErrorEnvelope | null {
  if (!isJsonObject(value)) return null
  const error = jsonString(value.error)
  const expired = jsonBoolean(value.expired)
  return {
    error: error ?? undefined,
    expired: expired ?? undefined,
  }
}

/** Sends a call to a JSON or relay route and reads its answer. */
async function request<K extends JsonRoute>(route: K, ...[call]: CallArgs<K>): Promise<Answers[K]> {
  const { method, url } = target(route, call ?? {})
  const init: RequestInit = { method, signal: call?.signal }
  const body = call?.body
  if (body instanceof ArrayBuffer) {
    init.body = body
  } else if (body !== undefined) {
    init.headers = { "Content-Type": "application/json" }
    init.body = JSON.stringify(body)
  }
  const response = await fetch(url, init)
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
  // SAFETY: The UI bundle ships inside the binary built from the same commit
  // as `wire.gen.ts`, whose route map is generated from the Rust route
  // declarations the compiler holds each handler to. A relay route's answer
  // is the Companion JSON that `RelayAnswerTypes` describes.
  return payload as Answers[K]
}

export const api = {
  viewing: () => request("GET /api/settings/viewing"),
  saveViewing: (value: ViewingSettingsWrite) => request("PATCH /api/settings/viewing", { body: value }),
  browsing: () => request("GET /api/settings/browsing"),
  saveBrowsing: (page: BrowsingPage, route: string) => request("PATCH /api/settings/browsing", { body: { page, route } }),
  status: () => request("GET /api/status"),
  startup: (home: boolean) => request("GET /api/startup", { query: { home: home ? 1 : undefined } }),
  companion: {
    info: () => request("GET /api/companion/info"),
    probe: () => request("POST /api/companion/probe"),
  },
  settings: () => request("GET /api/settings"),
  homeSettings: () => request("GET /api/settings/home"),
  saveHomeSettings: (body: HomeSettingsWrite) => request("PATCH /api/settings/home", { body }),
  settingsPatch: {
    player: (body: PlayerSettingsWrite) => request("PATCH /api/settings/client/player", { body }),
    playback: (body: PlaybackSettingsWrite) => request("PATCH /api/settings/client/playback", { body }),
    application: (body: ApplicationSettingsWrite) => request("PATCH /api/settings/client/application", { body }),
    appearance: (body: AppearanceSettingsWrite) => request("PATCH /api/settings/appearance", { body }),
  },
  shell: {
    windowReady: () => request("POST /api/shell/window/ready"),
    filePicker: (requestId: string) => request("POST /api/shell/file-picker", { body: { requestId } }),
    installMpv: (requestId: string) => request("POST /api/shell/mpv/install", { body: { requestId } }),
    mpvHelp: () => request("POST /api/shell/mpv/help"),
  },
  ratings: {
    status: () => request("GET /api/integrations/ratings"),
    batch: (ids: string[], signal?: AbortSignal) => request("POST /api/ratings/batch", { body: { ids }, signal }),
  },
  technical: {
    batch: (ids: string[], signal?: AbortSignal) => request("POST /api/technical/batch", { body: { ids }, signal }),
  },
  letterboxd: {
    profiles: () => request("GET /api/integrations/letterboxd"),
    add: (profile: string) => request("POST /api/integrations/letterboxd", { body: { profile } }),
    setEnabled: (id: string, enabled: boolean) =>
      request("PATCH /api/integrations/letterboxd/{id}", { params: { id }, body: { enabled } }),
    refresh: (id: string) => request("POST /api/integrations/letterboxd/{id}/refresh", { params: { id } }),
    remove: (id: string) => request("DELETE /api/integrations/letterboxd/{id}", { params: { id } }),
    open: (id: string) => request("POST /api/integrations/letterboxd/{id}/open", { params: { id } }),
  },

  connect: (server: string, signal?: AbortSignal) => request("POST /api/auth/connect", { body: { server }, signal }),
  login: (server: string, username: string, password: string) =>
    request("POST /api/auth/login", { body: { server, username, password } }),
  quickConnectStart: (server: string) => request("POST /api/auth/quickconnect/start", { body: { server } }),
  quickConnectPoll: (server: string, secret: string, signal?: AbortSignal) =>
    request("POST /api/auth/quickconnect/poll", { body: { server, secret }, signal }),
  logout: () => request("POST /api/auth/logout"),

  home: () => request("GET /api/home"),
  homeResume: () => request("GET /api/home/resume"),
  billboard: () => request("GET /api/billboard"),
  genres: () => request("GET /api/genres"),
  officialRatings: (kind: string) => request("GET /api/official-ratings", { query: { kind } }),
  resolvePerson: (query: PersonResolveQuery, signal?: AbortSignal) =>
    request("GET /api/person/resolve", { query: { ...query }, signal }),
  items: (query: ItemQuery, signal?: AbortSignal) => request("GET /api/items", { query: { ...query }, signal }),
  itemLetters: (query: ItemQuery, signal?: AbortSignal) =>
    request("GET /api/items/letters", { query: { ...query }, signal }),
  libraries: () => request("GET /api/libraries"),
  libraryItems: (id: string, query: LibraryQuery, offset: number, limit: number, signal?: AbortSignal) =>
    request("GET /api/libraries/{id}/items", { params: { id }, query: { ...query, offset, limit }, signal }),
  libraryLetters: (id: string, query: LibraryQuery, signal?: AbortSignal) =>
    request("GET /api/libraries/{id}/letters", { params: { id }, query: { ...query }, signal }),
  libraryOptions: (id: string) => request("GET /api/libraries/{id}/options", { params: { id } }),
  libraryLatest: (id: string, limit?: number) =>
    request("GET /api/libraries/{id}/latest", { params: { id }, query: { limit } }),
  item: (id: string) => request("GET /api/item/{id}", { params: { id } }),
  itemSynopsis: (id: string) => request("GET /api/item/{id}/synopsis", { params: { id } }),
  itemAbout: (id: string) => request("GET /api/item/{id}/about", { params: { id } }),
  itemLetterboxd: (id: string) => request("GET /api/item/{id}/letterboxd", { params: { id } }),
  movieLetterboxd: (tmdbId: number) => request("GET /api/letterboxd/movie/{tmdbId}", { params: { tmdbId } }),
  /** Without a page, a cached series or season answers with every child. */
  children: (id: string, page?: { offset: number; limit: number }, signal?: AbortSignal) =>
    request("GET /api/item/{id}/children", { params: { id }, query: { ...page }, signal }),
  media: (id: string) => request("GET /api/item/{id}/media", { params: { id } }),
  setPlaybackPreference: (id: string, body: PlaybackTrackPreferenceWrite) =>
    request("PATCH /api/item/{id}/playback-preference", { params: { id }, body }),
  trailer: (id: string) => request("GET /api/item/{id}/trailer", { params: { id } }),
  trailerStreamUrl: (id: string) => link("GET /api/trailer/{id}/stream", { params: { id } }),
  nextUp: (id: string) => request("GET /api/item/{id}/nextup", { params: { id } }),
  openExternal: (id: string, provider: ExternalProvider) =>
    request("POST /api/item/{id}/external", { params: { id }, body: { provider } }),
  setPlayed: (id: string, played: boolean) => request("POST /api/item/{id}/played", { params: { id }, body: { played } }),
  setFavorite: (id: string, favorite: boolean) =>
    request("POST /api/item/{id}/favorite", { params: { id }, body: { favorite } }),

  /** `quality` overrides the saved Settings default for this play only. */
  play: (itemId: string, resume: boolean, quality?: StreamingQualityId) => {
    window.dispatchEvent(new Event("mediaflick-manual-play"))
    return request("POST /api/play", { body: { itemId, resume, quality } })
  },
  changePlaybackQuality: (itemId: string, startTicks: number, quality: StreamingQualityId) =>
    request("POST /api/play", { body: { itemId, startTicks, quality } }),
  playNext: (itemId: string) => request("POST /api/play/next", { body: { itemId } }),
  playPrevious: (itemId: string) => request("POST /api/play/previous", { body: { itemId } }),
  playbackNeighbors: (itemId: string) => request("POST /api/play/neighbors", { body: { itemId } }),

  playerState: () => request("GET /api/player/state"),
  playerCommand: (command: PlayerCommand) => request("POST /api/player/command", { body: command }),

  sync: () => request("POST /api/sync"),
  calendar: (start: string, end: string, signal?: AbortSignal) =>
    request("GET /api/calendar", { query: { start, end }, signal }),

  seerr: {
    status: () => request("GET /api/seerr/status"),
    search: (q: string, page = 1, signal?: AbortSignal) => request("GET /api/seerr/search", { query: { q, page }, signal }),
    personCredits: (tmdbId: number, jellyfinId?: string | null, signal?: AbortSignal) =>
      request("GET /api/seerr/person/{tmdbId}/credits", { params: { tmdbId }, query: { personId: jellyfinId }, signal }),
    discover: (
      row: SeerrDiscoverRow,
      filters: SeerrDiscoverFilters = {},
      page = 1,
      signal?: AbortSignal,
    ) => request("GET /api/seerr/discover/{kind}", { params: { kind: row }, query: { page, ...filters }, signal }),
    genres: (mediaType: SeerrMediaType, signal?: AbortSignal) =>
      request("GET /api/seerr/genres/{mediaType}", { params: { mediaType }, signal }),
    media: (mediaType: SeerrMediaType, tmdbId: number) =>
      request("GET /api/seerr/media/{mediaType}/{tmdbId}", { params: { mediaType, tmdbId } }),
    requestOptions: (mediaType: SeerrMediaType, is4k = false) =>
      request("GET /api/seerr/request-options/{mediaType}", { params: { mediaType }, query: { is4k } }),

    requests: (filter = "all", take = 40, signal?: AbortSignal) =>
      request("GET /api/seerr/requests", { query: { filter, take }, signal }),
    /** Omitting `seasons` on a series asks for everything Seerr lacks. */
    request: (body: Wire.SeerrRequestBody) => request("POST /api/seerr/request", { body }),
    cancelRequest: (id: number) => request("DELETE /api/seerr/request/{id}", { params: { id } }),
  },

  collections: {
    settings: (reprobe = false) =>
      reprobe ? request("POST /api/collections/settings/reprobe") : request("GET /api/collections/settings"),
    patchSettings: (body: Wire.CollectionSettingsPatch) => request("PATCH /api/collections/settings", { body }),
    templates: (signal?: AbortSignal) => request("GET /api/collections/templates", { signal }),
    searchPublicLists: (query: string, signal?: AbortSignal) =>
      request("POST /api/collections/mdblist/search", { body: { query }, signal }),
    validatePublicList: (selector: string, signal?: AbortSignal) =>
      request("POST /api/collections/mdblist/validate", { body: { selector }, signal }),
    preview: (body: CollectionProfileDraft, signal?: AbortSignal) =>
      request("POST /api/collections/preview", { body, signal }),
    profiles: (signal?: AbortSignal) => request("GET /api/collections/profiles", { signal }),
    createProfile: (body: CollectionProfileDraft) => request("POST /api/collections/profiles", { body }),
    updateProfile: (id: string, body: CollectionProfileDraft) =>
      request("PATCH /api/collections/profiles/{profileId}", { params: { profileId: id }, body }),
    deleteProfile: (id: string) => request("DELETE /api/collections/profiles/{profileId}", { params: { profileId: id } }),
    reorderProfiles: (profileIds: string[]) => request("PUT /api/collections/profiles/order", { body: { profileIds } }),
    refreshProfile: (id: string) =>
      request("POST /api/collections/profiles/{profileId}/refresh", { params: { profileId: id } }),
    mine: (signal?: AbortSignal) => request("GET /api/collections/mine", { signal }),
    mineDetail: (id: string, signal?: AbortSignal) =>
      request("GET /api/collections/mine/{profileId}", { params: { profileId: id }, signal }),
    franchises: (localDate: string, signal?: AbortSignal) =>
      request("GET /api/collections/franchises", { query: { localDate }, signal }),
    franchise: (id: number, localDate: string, signal?: AbortSignal) =>
      request("GET /api/collections/franchises/{collectionId}", { params: { collectionId: id }, query: { localDate }, signal }),
    jellyfin: (signal?: AbortSignal) => request("GET /api/collections/jellyfin", { signal }),
    jellyfinDetail: (id: string, signal?: AbortSignal) =>
      request("GET /api/collections/jellyfin/{boxsetId}", { params: { boxsetId: id }, signal }),
    uploadArtwork: (body: ArrayBuffer, signal?: AbortSignal) => request("POST /api/collections/artwork", { body, signal }),
    artworkUrl: (id: string) => link("GET /api/collections/artwork/{artworkId}", { params: { artworkId: id } }),
    providerArtworkUrl: (path: string | null | undefined, size = "w342") =>
      path ? link("GET /api/collections/provider-artwork", { query: { path, size } }) : null,
    title: (mediaType: SeerrMediaType, tmdbId: number, signal?: AbortSignal) =>
      request("GET /api/collections/title/{mediaType}/{tmdbId}", { params: { mediaType, tmdbId }, signal }),
    forMovie: (tmdbId: number) => request("GET /api/collections/movie/{tmdbId}", { params: { tmdbId } }),
    deleteLocalAccount: () => request("DELETE /api/collections/local-account", { body: { confirmed: true } }),
  },
}
