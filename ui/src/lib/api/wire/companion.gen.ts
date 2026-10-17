// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type CompanionInfo = {
  pluginVersion: string,
  apiVersion: number,
  capabilities: Array<string>,
  services: { [key in string]: boolean },
};

export type CompanionStatus = {
  available: boolean,
  compatible: boolean,
  checked: boolean,
  info: CompanionInfo | null,
  error: string | null,
  supportedApi: SupportedApi,
};

export type SupportedApi = {
  min: number,
  max: number,
};
