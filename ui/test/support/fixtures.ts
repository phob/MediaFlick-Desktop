import type { QueryClient } from "@tanstack/react-query"
import type {
  BrowseCapabilities,
  CompanionStatus,
  ItemDetail,
  ItemSummary,
  LibrariesResponse,
  PlayerState,
  Status,
} from "../../src/lib/api"
import { queryKeys } from "../../src/lib/query-client"
import { DEFAULT_VIEWING } from "../../src/lib/viewing"
import { isJsonObject, type JsonObject, type JsonValue } from "../../src/lib/json"

export function itemSummary(
  overrides: Pick<ItemSummary, "id" | "kind" | "name"> & Partial<ItemSummary>,
): ItemSummary {
  const { id, kind, name, ...rest } = overrides
  return {
    id,
    kind,
    name,
    year: null,
    runtimeTicks: null,
    communityRating: null,
    officialRating: null,
    seriesId: null,
    seriesName: null,
    indexNumber: null,
    parentIndexNumber: null,
    primaryImageTag: null,
    thumbImageTag: null,
    logoImageTag: null,
    backdropImageTag: null,
    childCount: null,
    premiereDate: null,
    seasonId: null,
    played: false,
    playCount: 0,
    positionTicks: 0,
    favorite: false,
    unplayedCount: null,
    primaryAspect: null,
    ...rest,
  }
}

export function itemDetail(
  overrides: Pick<ItemDetail, "id" | "kind" | "name"> & Partial<ItemDetail>,
): ItemDetail {
  return {
    ...itemSummary(overrides),
    layout: "title",
    genres: [],
    originalTitle: null,
    providerIds: { tmdb: null, imdb: null, tvdb: null },
    parentId: null,
    dateCreated: null,
    libraryId: null,
    ...overrides,
  }
}

export function appStatus(overrides: Partial<Status> = {}): Status {
  const catalog = { complete: true, processed: 0, total: 0 }
  return {
    authenticated: false,
    expired: false,
    serverUrl: null,
    serverName: null,
    userId: null,
    userName: null,
    deviceId: "test-device",
    library: { movies: 0, series: 0, seasons: 0, episodes: 0, total: 0 },
    syncing: false,
    lastSync: null,
    bootstrap: { state: "ready" },
    syncProgress: { active: false, phase: "complete", catalog, retry: null },
    ...overrides,
  }
}

export function companionWithSeerr(): CompanionStatus {
  return {
    available: true,
    compatible: true,
    checked: true,
    info: {
      pluginVersion: "0.2.2",
      apiVersion: 1,
      capabilities: ["seerr", "seerr-person-discovery"],
      services: { sonarr: false, radarr: false, seerr: true, mdblist: false, tmdb: false },
    },
    error: null,
    supportedApi: { min: 1, max: 1 },
  }
}

export function playerSnapshot(overrides: Partial<PlayerState> = {}): PlayerState {
  return {
    active: false,
    playbackId: null,
    itemId: null,
    mediaSourceId: null,
    playSessionId: null,
    playMethod: null,
    positionMs: 0,
    durationMs: null,
    paused: false,
    volume: null,
    mute: null,
    tracks: [],
    chapters: [],
    skipSegments: [],
    diagnostics: { bufferedUntilMs: null, buffering: false, droppedFrames: null, frameRate: null },
    stopReason: null,
    ...overrides,
  }
}

export function requireElement<ElementType extends Element>(
  element: ElementType | null,
  description: string,
): ElementType {
  if (element === null) throw new Error(`Expected ${description}`)
  return element
}

export function parseJsonObject(text: string): JsonObject {
  const value: JsonValue = JSON.parse(text)
  if (!isJsonObject(value)) throw new Error("Expected a JSON object")
  return value
}

export const MOVIES_LIBRARY_ID = "f137a2dd21bbc1b99aa5c0f6bf02a805"
export const SHOWS_LIBRARY_ID = "a656b907eb3a73532e40e44b968d0225"
export const MUSIC_LIBRARY_ID = "7e64e319657a9516ec78490da03edccb"

export const MOVIE_BROWSE: BrowseCapabilities = {
  sorts: ["name", "year", "added", "rating", "played", "plays", "runtime", "random"],
  filters: ["genre", "decade", "watched", "parental", "favorite"],
  letterIndex: true,
}

export const CROSS_LIBRARY_BROWSE: BrowseCapabilities = {
  sorts: ["name", "year", "added", "rating", "played", "random"],
  filters: ["genre", "decade", "watched", "parental", "favorite"],
  letterIndex: true,
}

export function librariesResponse(): LibrariesResponse {
  return {
    known: true,
    libraries: [
      { id: MOVIES_LIBRARY_ID, name: "Films", kind: "movies", imageTag: null, membershipPending: false, browse: MOVIE_BROWSE },
      {
        id: SHOWS_LIBRARY_ID,
        name: "Shows",
        kind: "shows",
        imageTag: null,
        membershipPending: false,
        browse: {
          sorts: ["name", "year", "added", "episode-added", "rating", "played", "random"],
          filters: ["genre", "decade", "watched", "parental", "seriesStatus", "favorite"],
          letterIndex: true,
        },
      },
      {
        id: MUSIC_LIBRARY_ID,
        name: "Music",
        kind: "music",
        imageTag: null,
        membershipPending: false,
        browse: { sorts: ["name", "added", "year"], filters: ["favorite"], letterIndex: false },
      },
    ],
    crossLibrary: CROSS_LIBRARY_BROWSE,
  }
}

export function seedSignedInLibraries(client: QueryClient, browsing: Record<string, string> = {}) {
  client.setQueryData(queryKeys.status, appStatus({ authenticated: true, serverUrl: "server", userId: "user" }))
  client.setQueryData(queryKeys.viewing("server:user"), DEFAULT_VIEWING)
  client.setQueryData(queryKeys.browsing("server:user"), browsing)
  client.setQueryData(queryKeys.libraries, librariesResponse())
}
