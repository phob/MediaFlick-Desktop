// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { Billboard, HomeResponse } from "./api.catalog.home.gen.ts"
import type { BrowsingRoutes, SettingsView, ViewingView } from "./api.settings.gen.ts"
import type { CompanionStatus } from "./companion.gen.ts"
import type { LibraryStats } from "./library.model.gen.ts"
import type { Bootstrap, SyncProgress } from "./library.sync.gen.ts"

export type AppStatus = {
  library: LibraryStats,
  syncing: boolean,
  lastSync: string | null,
  bootstrap: Bootstrap,
  syncProgress: SyncProgress,
  companion: CompanionStatus,
  authenticated: boolean,
  expired: boolean,
  serverUrl: string | null,
  serverName: string | null,
  userId: string | null,
  userName: string | null,
  deviceId: string,
};

export type StartupResponse = {
  status: AppStatus,
  settings: SettingsView,
  viewing: ViewingView | null,
  browsing: BrowsingRoutes | null,
  home: HomeResponse | null,
  billboard: Billboard | null,
};

export type ApiRoutes = {
  "GET /api/status": { params: null, body: null, kind: "json", response: AppStatus },
  "GET /api/startup": { params: null, body: null, kind: "json", response: StartupResponse },
  "GET /api/companion/info": { params: null, body: null, kind: "json", response: CompanionStatus },
  "POST /api/companion/probe": { params: null, body: null, kind: "json", response: CompanionStatus },
};
