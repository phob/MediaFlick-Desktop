// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { ServerTooOld, ServerVersion } from "./jellyfin.version.gen.ts"

export type QuickConnectPoll = {
  authenticated: boolean,
  /**
   * Sent once the code was approved and the session signed in.
   */
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
  /**
   * Set when the server is older than Desktop supports. Sign-in to it is
   * refused, so Quick Connect is not checked.
   */
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
  /**
   * The signed-in user administers the server, as the last policy read
   * showed. A restored session reads `false` until its policy is read.
   */
  administrator: boolean,
};
