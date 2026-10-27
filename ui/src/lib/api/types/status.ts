import type * as Wire from "../wire.gen.ts"

export type Status = Wire.AppStatus

/**
 * `/api/startup`: what the first frame reads, in one request. A part that its
 * own route could not answer is `null`, and its query then asks for it.
 */
export type StartupResponse = Wire.StartupResponse

export type CompanionService = "sonarr" | "radarr" | "seerr" | "mdblist" | "tmdb"

export type CompanionStatus = Wire.CompanionStatus

export type SyncFailure = Wire.SyncFailure

/** A failed cycle and when the worker tries again, in Unix seconds. */
export type SyncRetry = Wire.Retry

/**
 * What the shell gates on after sign-in (`Bootstrap`, `src/library/sync.rs`):
 * Home needs a usable catalog. A retry outlives the next attempt, so it stays
 * while `Status.syncing` reports that attempt running.
 */
export type SyncProgress = Wire.SyncProgress
