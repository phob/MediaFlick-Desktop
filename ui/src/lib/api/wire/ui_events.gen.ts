// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type AboutInfo = {
  appName: string,
  version: string,
  gitVersion: string,
  createdBy: string,
};

export type CatalogChanges = {
  itemIds: Array<string>,
  contextIds: Array<string>,
  libraryIds: Array<string>,
};

export type DownloadProgress = {
  downloaded: number,
  total: number | null,
};

export type ErrorToast = {
  title: string,
  body: string,
};

export type FilePickerCompleted = {
  requestId: string,
  path: string | null,
  error: string | null,
};

export type ItemChanges = {
  itemIds: Array<string>,
  contextIds: Array<string>,
};

export type LiveChanges = {
  itemIds: Array<string>,
  contextIds: Array<string>,
  /**
   * Any page Jellyfin serves may have changed: the event stream was lost.
   */
  all?: boolean,
};

export type MpvInstallProgress = { requestId: string } & ({ "state": "downloading", downloaded: number, total: number | null } | { "state": "extracting" } | { "state": "completed", path: string } | { "state": "failed", message: string });

export type PlaybackCacheRefreshOutcome =
  | "refreshed"
  | "deferred";

export type PlaybackCacheRefreshed = {
  itemId: string,
  status: PlaybackCacheRefreshOutcome,
};

export type ShellEvent =
  | { "type": "library-changed", "payload": ItemChanges }
  | { "type": "live-changed", "payload": LiveChanges }
  | { "type": "catalog-changed", "payload": CatalogChanges }
  | { "type": "collections-changed", "payload": Record<symbol, never> }
  | { "type": "libraries-changed", "payload": Record<symbol, never> }
  | { "type": "library-memberships-settled", "payload": Record<symbol, never> }
  | { "type": "jellyfin-session-expired", "payload": Record<symbol, never> }
  | { "type": "window-close-blocked", "payload": Record<symbol, never> }
  | { "type": "mpv-install-progress", "payload": MpvInstallProgress }
  | { "type": "file-picker-completed", "payload": FilePickerCompleted };

export type UpdateProgress =
  | { "state": "downloading", "payload": DownloadProgress }
  | { "state": "installing", "payload": DownloadProgress }
  | { "state": "error", "payload": { message: string } };
