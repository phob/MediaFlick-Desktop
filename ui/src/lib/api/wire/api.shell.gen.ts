// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type MpvHelpOpened = {
  opened: boolean,
};

export type ShellQueued = {
  queued: boolean,
};

export type ShellRequestBody = {
  requestId: string,
};

export type ShellRequestQueued = {
  requestId: string,
  queued: boolean,
};

export type ApiShellRoutes = {
  "POST /api/shell/window/ready": { params: null, body: null, kind: "json", response: ShellQueued },
  "POST /api/shell/file-picker": { params: null, body: ShellRequestBody, kind: "json", response: ShellRequestQueued },
  "POST /api/shell/mpv/install": { params: null, body: ShellRequestBody, kind: "json", response: ShellRequestQueued },
  "POST /api/shell/mpv/help": { params: null, body: null, kind: "json", response: MpvHelpOpened },
};
