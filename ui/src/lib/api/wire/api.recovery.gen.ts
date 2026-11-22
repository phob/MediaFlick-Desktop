// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type DataFolderOpened = {
  folder: string,
};

export type LibraryRecreated = {
  backup: string,
};

export type Refusal =
  | "newerSchema"
  | "unusable";

export type RefusedLibrary = {
  refusal: Refusal,
  folder: string,
  backupName: string,
};

export type StartupRecovery = {
  library: RefusedLibrary | null,
};

export type ApiRecoveryRoutes = {
  "GET /api/recovery": { params: null, body: null, kind: "json", response: StartupRecovery },
  "POST /api/recovery/library/recreate": { params: null, body: null, kind: "json", response: LibraryRecreated },
  "POST /api/recovery/data-folder/open": { params: null, body: null, kind: "json", response: DataFolderOpened },
};
