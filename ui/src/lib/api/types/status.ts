import type { ItemSummary } from "./catalog.ts"
import type { HomeResponse } from "./home.ts"
import type { ClientSettings, ViewingSettings } from "./settings.ts"

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
