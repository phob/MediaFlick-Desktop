// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type DataFolderOpened = {
  folder: string,
};

export type LibraryRecreated = {
  /**
   * Where the old `library.db` went.
   */
  backup: string,
};

export type Refusal =
  | "newerSchema"
  | "unusable";

export type RefusedLibrary = {
  refusal: Refusal,
  /**
   * The folder holding `library.db`, where Recreate library leaves the old one.
   */
  folder: string,
  /**
   * The name Recreate library would give the old file now. The name
   * carries the second it moves, in UTC.
   */
  backupName: string,
};

export type StartupRecovery = {
  /**
   * Set while the services are down because `library.db` was refused.
   */
  library: RefusedLibrary | null,
};

export type ApiRecoveryRoutes = {
  "GET /api/recovery": { params: null, body: null, kind: "json", response: StartupRecovery },
  "POST /api/recovery/library/recreate": { params: null, body: null, kind: "json", response: LibraryRecreated },
  "POST /api/recovery/data-folder/open": { params: null, body: null, kind: "json", response: DataFolderOpened },
};
