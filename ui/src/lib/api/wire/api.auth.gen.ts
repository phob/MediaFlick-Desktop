// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { AppStatus } from "./api.gen.ts"
import type { QuickConnectPoll, QuickConnectStart, ServerInfo } from "./jellyfin.session.gen.ts"

export type LoginBody = {
  server: string,
  username: string,
  password: string,
};

export type LogoutBody = {
  forgetLibrary?: boolean,
};

export type QuickConnectPollBody = {
  server: string,
  secret: string,
};

export type ServerBody = {
  server: string,
};

export type ApiAuthRoutes = {
  "POST /api/auth/connect": { params: null, body: ServerBody, kind: "json", response: ServerInfo },
  "POST /api/auth/login": { params: null, body: LoginBody, kind: "json", response: AppStatus },
  "POST /api/auth/quickconnect/start": { params: null, body: ServerBody, kind: "json", response: QuickConnectStart },
  "POST /api/auth/quickconnect/poll": { params: null, body: QuickConnectPollBody, kind: "json", response: QuickConnectPoll },
  "POST /api/auth/logout": { params: null, body: LogoutBody, kind: "json", response: AppStatus },
};
