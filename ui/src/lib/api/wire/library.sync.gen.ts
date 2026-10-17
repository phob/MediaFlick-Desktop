// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type Bootstrap =
  | { "state": "loading" }
  | { "state": "retrying" } & Retry
  | { "state": "ready" };

export type BootstrapProgress = {
  complete: boolean,
  processed: number,
  total: number | null,
};

export type Retry = {
  reason: SyncFailure,
  retryAt: number,
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
