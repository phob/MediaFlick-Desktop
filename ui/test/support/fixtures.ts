import type { ItemDetail, ItemSummary, PlayerState, Status } from "../../src/lib/api"
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
    ...rest,
  }
}

export function itemDetail(
  overrides: Pick<ItemDetail, "id" | "kind" | "name"> & Partial<ItemDetail>,
): ItemDetail {
  return {
    ...itemSummary(overrides),
    genres: [],
    originalTitle: null,
    providerIds: { tmdb: null, imdb: null, tvdb: null },
    parentId: null,
    dateCreated: null,
    ...overrides,
  }
}

/** An idle player snapshot, as the shell sends when nothing is playing. */
/** A signed-out `/api/status` with an empty, fully synced library and no Companion. */
export function appStatus(overrides: Partial<Status> = {}): Status {
  const catalog = { complete: true, ready: true, processed: 0, total: 0, initial: false }
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
    bootstrapped: true,
    libraryReady: true,
    bootstrap: catalog,
    syncProgress: { active: false, phase: "complete", catalog, error: null, retryAt: null },
    companion: {
      available: false,
      compatible: false,
      checked: true,
      info: null,
      error: null,
      supportedApi: { min: 1, max: 1 },
    },
    ...overrides,
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
