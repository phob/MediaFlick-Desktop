// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

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
