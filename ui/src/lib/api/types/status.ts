import type * as Wire from "../wire.gen.ts"

export type Status = Wire.AppStatus

export type StartupResponse = Wire.StartupResponse

export type CompanionService = "sonarr" | "radarr" | "seerr" | "mdblist" | "tmdb"

export type CompanionStatus = Wire.CompanionStatus

export type SyncFailure = Wire.SyncFailure

export type SyncRetry = Wire.Retry

export type SyncProgress = Wire.SyncProgress
