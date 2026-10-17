// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

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
  version: string,
  quickConnect: boolean,
};

export type SessionStatus = {
  authenticated: boolean,
  expired: boolean,
  serverUrl: string | null,
  serverName: string | null,
  userId: string | null,
  userName: string | null,
  deviceId: string,
};
