// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ServerTooOld, ServerVersion } from "./jellyfin.version.gen.ts"

export type QuickConnectPoll = {
  authenticated: boolean,
  session?: SessionStatus,
};

export type QuickConnectStart = {
  serverUrl: string,
  code: string,
  secret: string,
};

export type ServerInfo = {
  serverUrl: string,
  serverName: string,
  version: ServerVersion,
  quickConnect: boolean,
  tooOld: ServerTooOld | null,
};

export type SessionStatus = {
  authenticated: boolean,
  expired: boolean,
  serverUrl: string | null,
  serverName: string | null,
  userId: string | null,
  userName: string | null,
  deviceId: string,
  administrator: boolean,
};
