// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type ErrorBody = {
  error: string,
  expired?: boolean,
  /**
   * Set on the 409 that answers a request whose account was switched
   * before it finished. Other 409s on this API mean something else.
   */
  accountChanged?: boolean,
  /**
   * Set on the 503 that answers every request once the services failed
   * to open at startup. `error` says why; the failure is final for the
   * process, so the UI shows it instead of signing in.
   */
  startupFailed?: boolean,
};
